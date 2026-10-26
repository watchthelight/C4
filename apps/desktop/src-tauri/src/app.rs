// <wtl> project: c4 | file: apps/desktop/src-tauri/src/app.rs | author: watchthelight </wtl>
//! The window's side of discovery: one shared [`Discovery`] behind a mutex, a refresh thread that
//! emits a snapshot after every pass, the ingest server's batches folded into the same table, and
//! the two commands the window calls.
//!
//! Two cadences for the files. The files under the Claude home and the process table are cheap, so
//! they are read every period (`C4_REFRESH_MS`, two seconds by default). The session list command
//! (`claude agents --json`) starts a Node process and, on macOS and Linux, can revive Claude Code's
//! background supervisor as a side effect, so it runs on every [`AGENTS_EVERY`]th pass only. Hook
//! events and status line payloads have no cadence: the ingest worker (`ingest.rs`) folds each batch
//! as it arrives and emits the same event. The document the window receives is the one
//! `c4 sessions --json` prints, plus the transitions of the pass or batch, the pass counter, the
//! last run of the session list command and the ingest report. Two more commands read and write
//! the preferences file under the C4 home (`prefs.rs`).

use c4_cli::sessions::snapshot_json;
use c4_cli::{C4HomeArgs, HomeArgs};
use c4_core::{ObservationBody, SessionId, Timestamp, Transition};
use c4_discovery::{
    AgentsCommand, ClaudeHome, Discovery, DiscoveryOptions, SnapshotReport, SysinfoProbe,
};
use c4_ingest::Ingested;
use serde::Serialize;
use serde_json::{Value, json};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ingest::{Ingest, IngestReport, Worker};
use crate::prefs::Prefs;

/// The environment variable that sets the refresh period in milliseconds.
pub const REFRESH_MS_ENV: &str = "C4_REFRESH_MS";
/// The refresh period when the environment does not set one.
pub const DEFAULT_REFRESH_MS: u64 = 2000;
/// The shortest period accepted, so a typo cannot turn the app into a busy loop.
pub const MIN_REFRESH_MS: u64 = 250;
/// The session list command runs on every pass whose number is a multiple of this.
pub const AGENTS_EVERY: u64 = 8;
/// The event the window listens for; its payload is the snapshot document.
pub const SESSIONS_EVENT: &str = "sessions";

/// The last run of the session list command.
#[derive(Clone, Debug, Serialize)]
pub struct AgentsRun {
    /// When it ran.
    pub at: Timestamp,
    /// How many entries it returned, when it succeeded.
    pub entries: Option<usize>,
    /// Why it gave nothing, when it failed.
    pub error: Option<String>,
}

/// Static facts for the status strip.
#[derive(Clone, Debug, Serialize)]
pub struct About {
    /// The C4 version.
    pub version: &'static str,
    /// The Claude home being read.
    pub home: String,
    /// What chose the home (`C4_CLAUDE_HOME`, `CLAUDE_CONFIG_DIR` or `~/.claude`).
    pub home_source: &'static str,
    /// The C4 home the ingest server serves.
    pub c4_home: String,
    /// What chose it (`C4_HOME` or `~/.c4`).
    pub c4_home_source: &'static str,
    /// The ingest endpoint: the pipe name or socket path the shim writes to.
    pub endpoint: String,
    /// The preferences file under the C4 home.
    pub prefs_path: String,
    /// What was wrong with the preferences file when the window opened, if anything.
    pub prefs_problem: Option<String>,
    /// The refresh period in milliseconds.
    pub refresh_ms: u64,
    /// The session list command runs every this many passes.
    pub agents_every: u64,
    /// The operating system.
    pub os: &'static str,
}

struct Inner {
    discovery: Discovery,
    probe: SysinfoProbe,
    passes: u64,
    last_agents: Option<AgentsRun>,
    last_report: SnapshotReport,
}

/// What the window, the refresh thread and the ingest worker share.
pub struct AppState {
    home: ClaudeHome,
    home_source: &'static str,
    refresh: Duration,
    ingest: Ingest,
    prefs: Prefs,
    inner: Mutex<Inner>,
}

impl AppState {
    /// Discovery over the home the environment names, with the session list command from the
    /// environment (`C4_AGENTS_CMD`, else `claude agents --json`), and the ingest server and the
    /// preferences file on the C4 home the environment names (`C4_HOME`, else `~/.c4`).
    #[must_use]
    pub fn from_env() -> Self {
        let args = HomeArgs { home: None };
        let home = args.resolve();
        let opts =
            DiscoveryOptions::files_only(home.clone()).with_agents(AgentsCommand::from_env());
        let c4 = C4HomeArgs { c4_home: None };
        let c4_home = c4.resolve();
        Self::new(
            home,
            args.source(),
            opts,
            refresh_period(),
            Ingest::new(c4_home.clone(), c4.source()),
            Prefs::open(&c4_home),
        )
    }

    /// Discovery over `home` with `opts`, refreshed every `refresh`, the ingest handle and the
    /// preferences.
    #[must_use]
    pub fn new(
        home: ClaudeHome,
        home_source: &'static str,
        opts: DiscoveryOptions,
        refresh: Duration,
        ingest: Ingest,
        prefs: Prefs,
    ) -> Self {
        Self {
            home,
            home_source,
            refresh,
            ingest,
            prefs,
            inner: Mutex::new(Inner {
                discovery: Discovery::new(opts),
                probe: SysinfoProbe::new(),
                passes: 0,
                last_agents: None,
                last_report: SnapshotReport::default(),
            }),
        }
    }

    /// One pass; `with_agents` also runs the session list command. Returns the document the window
    /// renders.
    pub fn refresh(&self, with_agents: bool) -> Value {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let inner = &mut *guard;
        let now = Timestamp::now();
        let res = if with_agents {
            inner.discovery.refresh(&mut inner.probe, now)
        } else {
            inner.discovery.refresh_files_only(&mut inner.probe, now)
        };
        inner.passes += 1;
        if with_agents {
            inner.last_agents = Some(AgentsRun {
                at: now,
                entries: res.report.agents_entries,
                error: res.report.agents_error.clone(),
            });
        }
        inner.last_report = res.report;
        document(
            &self.home,
            inner,
            &res.transitions,
            now,
            &self.ingest.report(),
        )
    }

    /// Fold a batch of lines from the ingest server and return the document for the window: the
    /// table after the fold, the transitions the batch caused, and the report of the last pass. A
    /// line from inside a subagent (`agent_id` set) contributes its identity fields alone.
    pub fn fold(&self, items: Vec<Ingested>) -> Value {
        let now = Timestamp::now();
        let mut observations = Vec::with_capacity(items.len() * 2);
        let mut subagent_lines = 0u64;
        let mut last_line_at: Option<Timestamp> = None;
        for item in items {
            last_line_at = Some(last_line_at.map_or(item.recv_at, |t| t.max(item.recv_at)));
            if item.agent_id.is_some() {
                subagent_lines += 1;
                observations.extend(
                    item.observations
                        .into_iter()
                        .filter(|o| matches!(o.body, ObservationBody::Identity { .. })),
                );
            } else {
                observations.extend(item.observations);
            }
        }
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let inner = &mut *guard;
        let res = inner.discovery.fold(observations, now);
        self.ingest
            .note(res.applied as u64, subagent_lines, last_line_at);
        document(
            &self.home,
            inner,
            &res.transitions,
            now,
            &self.ingest.report(),
        )
    }

    /// The ingest handle: the C4 home, the inbox and the report.
    #[must_use]
    pub fn ingest(&self) -> &Ingest {
        &self.ingest
    }

    /// The preferences file.
    #[must_use]
    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    /// The static facts.
    #[must_use]
    pub fn about(&self) -> About {
        About {
            version: c4_core::VERSION,
            home: self.home.root().display().to_string(),
            home_source: self.home_source,
            c4_home: self.ingest.home().root().display().to_string(),
            c4_home_source: self.ingest.report().home_source,
            endpoint: self.ingest.home().endpoint(),
            prefs_path: self.prefs.path().display().to_string(),
            prefs_problem: self.prefs.problem().map(str::to_string),
            refresh_ms: self.refresh.as_millis() as u64,
            agents_every: AGENTS_EVERY,
            os: std::env::consts::OS,
        }
    }
}

fn document(
    home: &ClaudeHome,
    inner: &Inner,
    transitions: &[(SessionId, Transition)],
    now: Timestamp,
    ingest: &IngestReport,
) -> Value {
    let mut value = snapshot_json(home, &inner.discovery, &inner.last_report, now);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("tick".into(), json!(inner.passes));
        obj.insert("agents".into(), json!(inner.last_agents));
        let changes: Vec<Value> = transitions
            .iter()
            .map(|(id, t)| {
                json!({
                    "session": id,
                    "from": t.from,
                    "to": t.to,
                    "describe": t.to.describe(),
                    "evidence": t.evidence,
                })
            })
            .collect();
        obj.insert("transitions".into(), Value::Array(changes));
        obj.insert("ingest".into(), json!(ingest));
    }
    value
}

/// The refresh period from the environment, bounded below by [`MIN_REFRESH_MS`].
#[must_use]
pub fn refresh_period() -> Duration {
    let ms = std::env::var(REFRESH_MS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_REFRESH_MS);
    Duration::from_millis(ms.max(MIN_REFRESH_MS))
}

/// The window's first request: a files-only pass, then the document.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value
fn list_sessions(state: State<'_, Arc<AppState>>) -> Value {
    state.refresh(false)
}

/// The static facts for the status strip.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn about(state: State<'_, Arc<AppState>>) -> About {
    state.about()
}

/// The preferences document as it is now.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn prefs_get(state: State<'_, Arc<AppState>>) -> Value {
    state.prefs().document()
}

/// Replace one section of the preferences and write the file. Returns the document as written.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn prefs_set(
    state: State<'_, Arc<AppState>>,
    section: String,
    value: Value,
) -> Result<Value, String> {
    state
        .prefs()
        .set_section(&section, value)
        .map_err(|e| e.to_string())
}

/// Open the window and run until it closes. Returns the process exit code.
///
/// The ingest worker starts once the app is built and stops on the exit event, before the process
/// ends, so the socket file is removed and open connections get their moment. A worker that cannot
/// start (no runtime) is reported on stderr and the window runs without it.
#[must_use]
pub fn run() -> ExitCode {
    let state = Arc::new(AppState::from_env());
    let period = state.refresh;
    let built = tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            list_sessions,
            about,
            prefs_get,
            prefs_set
        ])
        .setup(move |app| {
            spawn_refresh(app.handle().clone(), period);
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            eprintln!("c4: {e}");
            return ExitCode::from(1);
        }
    };
    let handle = app.handle().clone();
    let mut worker = match Worker::spawn(state, move |document| emit(&handle, document)) {
        Ok(worker) => Some(worker),
        Err(e) => {
            eprintln!("c4: the ingest server could not get a runtime: {e}");
            None
        }
    };
    app.run(move |_, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(worker) = worker.as_mut()
        {
            worker.stop();
        }
    });
    ExitCode::SUCCESS
}

fn emit(handle: &AppHandle, document: Value) {
    if let Err(e) = handle.emit(SESSIONS_EVENT, document) {
        tracing::warn!(error = %e, "could not emit the sessions event");
    }
}

/// The refresh loop on its own thread: a pass, an event, a sleep. The thread ends with the process.
fn spawn_refresh(handle: AppHandle, period: Duration) {
    let spawned = std::thread::Builder::new()
        .name("c4-refresh".into())
        .spawn(move || {
            let mut pass: u64 = 0;
            loop {
                let with_agents = pass.is_multiple_of(AGENTS_EVERY);
                let document = handle.state::<Arc<AppState>>().refresh(with_agents);
                emit(&handle, document);
                pass += 1;
                std::thread::sleep(period);
            }
        });
    if let Err(e) = spawned {
        eprintln!("c4: could not start the refresh thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use c4_core::C4Home;
    use c4_testkit::ScratchHome;

    #[test]
    fn the_document_carries_the_cli_shape_plus_the_pass() {
        let scratch = ScratchHome::seeded().unwrap();
        let c4_home = C4Home::at(scratch.scratch_dir().join("c4"));
        let state = AppState::new(
            scratch.home().clone(),
            "test",
            scratch.files_only(),
            Duration::from_millis(DEFAULT_REFRESH_MS),
            Ingest::new(c4_home.clone(), "test"),
            Prefs::open(&c4_home),
        );
        let doc = state.refresh(false);
        assert_eq!(doc["schema"], c4_cli::OUTPUT_SCHEMA);
        assert_eq!(doc["tick"], 1);
        assert!(doc["agents"].is_null(), "no command ran");
        assert!(doc["sessions"].as_array().is_some_and(|s| !s.is_empty()));
        assert!(doc["transitions"].is_array());
        assert_eq!(doc["home"], scratch.home().root().display().to_string());
        assert_eq!(doc["ingest"]["state"], "starting", "no worker runs here");
        assert_eq!(doc["ingest"]["endpoint"], c4_home.endpoint());
        assert_eq!(doc["ingest"]["lines"], 0);
        assert_eq!(doc["ingest"]["folded"], 0);
        let doc = state.refresh(true);
        assert_eq!(doc["tick"], 2);
        assert!(
            doc["agents"].is_object(),
            "the run is recorded even though these options have no command: {doc}"
        );
        assert!(doc["agents"]["entries"].is_null());
        let about = state.about();
        assert_eq!(about.refresh_ms, DEFAULT_REFRESH_MS);
        assert_eq!(about.home_source, "test");
        assert_eq!(about.c4_home, c4_home.root().display().to_string());
        assert_eq!(about.c4_home_source, "test");
        assert_eq!(about.endpoint, c4_home.endpoint());
        assert_eq!(about.prefs_path, c4_home.prefs_path().display().to_string());
        assert!(about.prefs_problem.is_none());
        assert_eq!(state.prefs().document(), json!({ "schema": 1 }));
    }

    #[test]
    fn the_refresh_period_has_a_floor() {
        assert!(refresh_period() >= Duration::from_millis(MIN_REFRESH_MS));
    }
}
