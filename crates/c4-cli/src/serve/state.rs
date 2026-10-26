// <wtl> project: c4 | file: crates/c4-cli/src/serve/state.rs | author: watchthelight </wtl>
//! The core's table: one shared [`Discovery`] behind a mutex, the pass the refresh thread runs,
//! the ingest server's batches folded into the same table, and the facts the window asks for.
//! Nothing here knows how the window is reached: the parent module carries the documents over
//! stdout, and the shell forwards them to the page as events.
//!
//! Two cadences for the files. The files under the Claude home and the process table are cheap, so
//! they are read every period (`C4_REFRESH_MS`, two seconds by default). The session list command
//! (`claude agents --json`) starts a Node process and, on macOS and Linux, can revive Claude Code's
//! background supervisor as a side effect, so it runs on every [`AGENTS_EVERY`]th pass only. Hook
//! events and status line payloads have no cadence: the ingest worker (`ingest.rs`) folds each batch
//! as it arrives and emits the same event. The document the window receives is the one
//! `c4 sessions --json` prints, plus the transitions of the pass or batch, the pass counter, the
//! last run of the session list command and the ingest report. The preferences file under the C4
//! home (`prefs.rs`) is read and written through the same state.

use crate::ServeArgs;
use crate::sessions::snapshot_json;
use c4_core::{ObservationBody, SessionId, Timestamp, Transition};
use c4_discovery::{
    AgentsCommand, ClaudeHome, Discovery, DiscoveryOptions, SnapshotReport, SysinfoProbe,
};
use c4_ingest::Ingested;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::ingest::{Ingest, IngestReport};
use super::prefs::Prefs;

/// The environment variable that sets the refresh period in milliseconds.
pub const REFRESH_MS_ENV: &str = "C4_REFRESH_MS";
/// The refresh period when the environment does not set one.
pub const DEFAULT_REFRESH_MS: u64 = 2000;
/// The shortest period accepted, so a typo cannot turn the core into a busy loop.
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
    /// What chose the home (`--home`, `C4_CLAUDE_HOME`, `CLAUDE_CONFIG_DIR` or `~/.claude`).
    pub home_source: &'static str,
    /// The C4 home the ingest server serves.
    pub c4_home: String,
    /// What chose it (`--c4-home`, `C4_HOME` or `~/.c4`).
    pub c4_home_source: &'static str,
    /// The ingest endpoint: the pipe name or socket path the shim writes to.
    pub endpoint: String,
    /// The preferences file under the C4 home.
    pub prefs_path: String,
    /// What was wrong with the preferences file when the core started, if anything.
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

/// What the window's requests, the refresh thread and the ingest worker share.
pub struct AppState {
    home: ClaudeHome,
    home_source: &'static str,
    refresh: Duration,
    ingest: Ingest,
    prefs: Prefs,
    inner: Mutex<Inner>,
}

impl AppState {
    /// Discovery over the home the arguments or the environment name, with the session list
    /// command from the environment (`C4_AGENTS_CMD`, else `claude agents --json`) unless
    /// `--files-only`, the refresh period from the environment, and the ingest server and the
    /// preferences file on the C4 home the arguments or the environment name (`C4_HOME`, else
    /// `~/.c4`).
    #[must_use]
    pub fn from_args(args: &ServeArgs) -> Self {
        let home = args.home.resolve();
        let mut opts = DiscoveryOptions::files_only(home.clone());
        if !args.files_only {
            opts = opts.with_agents(AgentsCommand::from_env());
        }
        let c4_home = args.c4_home.resolve();
        Self::new(
            home,
            args.home.source(),
            opts,
            refresh_period(),
            Ingest::new(c4_home.clone(), args.c4_home.source()),
            Prefs::open(&c4_home),
        )
    }

    /// [`Self::from_args`] with nothing on the command line: the environment decides everything.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_args(&ServeArgs::default())
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

    /// The refresh period.
    #[must_use]
    pub fn period(&self) -> Duration {
        self.refresh
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
        assert_eq!(doc["schema"], crate::OUTPUT_SCHEMA);
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
        assert_eq!(state.period(), Duration::from_millis(DEFAULT_REFRESH_MS));
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

    #[test]
    fn the_arguments_choose_the_homes_and_whether_the_command_runs() {
        let scratch = ScratchHome::seeded().unwrap();
        let c4_home = C4Home::at(scratch.scratch_dir().join("c4"));
        let args = ServeArgs {
            home: crate::HomeArgs {
                home: Some(scratch.root().to_path_buf()),
            },
            c4_home: crate::C4HomeArgs {
                c4_home: Some(c4_home.root().to_path_buf()),
            },
            files_only: true,
        };
        let state = AppState::from_args(&args);
        let about = state.about();
        assert_eq!(about.home, scratch.root().display().to_string());
        assert_eq!(about.home_source, "--home");
        assert_eq!(about.c4_home, c4_home.root().display().to_string());
        assert_eq!(about.c4_home_source, "--c4-home");
        let doc = state.refresh(true);
        assert!(
            doc["agents"]["entries"].is_null() && doc["agents"]["error"].is_null(),
            "files only: no command ran: {}",
            doc["agents"]
        );
    }
}
