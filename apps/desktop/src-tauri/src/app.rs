// <wtl> project: c4 | file: apps/desktop/src-tauri/src/app.rs | author: watchthelight </wtl>
//! The window's side of discovery: one shared [`Discovery`] behind a mutex, a refresh thread that
//! emits a snapshot after every pass, and the two commands the window calls.
//!
//! Two cadences. The files under the Claude home and the process table are cheap, so they are
//! read every period (`C4_REFRESH_MS`, two seconds by default). The session list command
//! (`claude agents --json`) starts a Node process and, on macOS and Linux, can revive Claude
//! Code's background supervisor as a side effect, so it runs on every [`AGENTS_EVERY`]th pass only.
//! The document the window receives is the one `c4 sessions --json` prints, plus the transitions
//! of the pass, the pass counter and the last run of the session list command.

use c4_cli::HomeArgs;
use c4_cli::sessions::snapshot_json;
use c4_core::{SessionId, Timestamp, Transition};
use c4_discovery::{
    AgentsCommand, ClaudeHome, Discovery, DiscoveryOptions, SnapshotReport, SysinfoProbe,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::process::ExitCode;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

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
}

/// What the window and the refresh thread share.
pub struct AppState {
    home: ClaudeHome,
    home_source: &'static str,
    refresh: Duration,
    inner: Mutex<Inner>,
}

impl AppState {
    /// Discovery over the home the environment names, with the session list command from the
    /// environment (`C4_AGENTS_CMD`, else `claude agents --json`).
    #[must_use]
    pub fn from_env() -> Self {
        let args = HomeArgs { home: None };
        let home = args.resolve();
        let opts =
            DiscoveryOptions::files_only(home.clone()).with_agents(AgentsCommand::from_env());
        Self {
            home,
            home_source: args.source(),
            refresh: refresh_period(),
            inner: Mutex::new(Inner {
                discovery: Discovery::new(opts),
                probe: SysinfoProbe::new(),
                passes: 0,
                last_agents: None,
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
        document(&self.home, inner, &res.report, &res.transitions, now)
    }

    /// The static facts.
    #[must_use]
    pub fn about(&self) -> About {
        About {
            version: c4_core::VERSION,
            home: self.home.root().display().to_string(),
            home_source: self.home_source,
            refresh_ms: self.refresh.as_millis() as u64,
            agents_every: AGENTS_EVERY,
            os: std::env::consts::OS,
        }
    }
}

fn document(
    home: &ClaudeHome,
    inner: &Inner,
    report: &SnapshotReport,
    transitions: &[(SessionId, Transition)],
    now: Timestamp,
) -> Value {
    let mut value = snapshot_json(home, &inner.discovery, report, now);
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
fn list_sessions(state: State<'_, AppState>) -> Value {
    state.refresh(false)
}

/// The static facts for the status strip.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn about(state: State<'_, AppState>) -> About {
    state.about()
}

/// Open the window and run until it closes. Returns the process exit code.
#[must_use]
pub fn run() -> ExitCode {
    let state = AppState::from_env();
    let period = state.refresh;
    let result = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![list_sessions, about])
        .setup(move |app| {
            spawn_refresh(app.handle().clone(), period);
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("c4: {e}");
            ExitCode::from(1)
        }
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
                let document = handle.state::<AppState>().refresh(with_agents);
                if let Err(e) = handle.emit(SESSIONS_EVENT, document) {
                    tracing::warn!(error = %e, "could not emit the sessions event");
                }
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
    use c4_testkit::ScratchHome;

    #[test]
    fn the_document_carries_the_cli_shape_plus_the_pass() {
        let scratch = ScratchHome::seeded().unwrap();
        let state = AppState {
            home: scratch.home().clone(),
            home_source: "test",
            refresh: Duration::from_millis(DEFAULT_REFRESH_MS),
            inner: Mutex::new(Inner {
                discovery: Discovery::new(scratch.files_only()),
                probe: SysinfoProbe::new(),
                passes: 0,
                last_agents: None,
            }),
        };
        let doc = state.refresh(false);
        assert_eq!(doc["schema"], c4_cli::OUTPUT_SCHEMA);
        assert_eq!(doc["tick"], 1);
        assert!(doc["agents"].is_null(), "no command ran");
        assert!(doc["sessions"].as_array().is_some_and(|s| !s.is_empty()));
        assert!(doc["transitions"].is_array());
        assert_eq!(doc["home"], scratch.home().root().display().to_string());
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
    }

    #[test]
    fn the_refresh_period_has_a_floor() {
        assert!(refresh_period() >= Duration::from_millis(MIN_REFRESH_MS));
    }
}
