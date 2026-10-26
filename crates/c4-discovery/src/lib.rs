// <wtl> project: c4 | file: crates/c4-discovery/src/lib.rs | author: watchthelight </wtl>
//! Finds Claude Code sessions and turns what it finds into [`c4_core::Observation`]s.
//!
//! Four readers, in the order a snapshot runs them:
//!
//! 1. [`registry`]: `~/.claude/sessions/<pid>.json`, one small file per running process. The `.key`
//!    siblings are never opened.
//! 2. [`agents`]: `claude agents --json`, which also lists background sessions.
//! 3. [`daemon`]: the background supervisor's `daemon/roster.json` and `jobs/<short>/state.json`,
//!    with every auth field dropped before anything is kept.
//! 4. [`process`]: the process table, which decides whether a pid is still the process the files
//!    describe, and [`host`], which reads the terminal host from the process environment.
//!
//! [`snapshot`] runs them together and folds the result into a [`c4_core::SessionTable`].
//! [`Discovery::fold`] takes observations from elsewhere (the hook events and status line payloads
//! the ingest server parses, transcript events) into the same table, in stamp order. Nothing here
//! writes to the Claude home, and nothing reads a transcript; that is the transcript crate's job.
//!
//! Every reader is tolerant: a file that cannot be parsed becomes a [`Problem`] in the report, never
//! an error that stops the scan, because one broken file must not hide the other sessions.

pub mod agents;
pub mod daemon;
pub mod home;
pub mod host;
pub mod process;
pub mod registry;
pub mod snapshot;

pub use agents::{
    AGENTS_CMD_ENV, AgentsCommand, AgentsEntry, AgentsError, Captured, parse_agents_json,
    resolve_on_path,
};
pub use daemon::{BackgroundJob, DaemonScan, JobState, Roster, RosterWorker, scan_daemon};
pub use home::{CONFIG_DIR_ENV, ClaudeHome, HOME_ENV, encode_cwd};
pub use host::{HOST_ENV_KEYS, HostHint, identify_host};
pub use process::{
    FakeProbe, Liveness, ProcessInfo, ProcessProbe, START_TOLERANCE_SECS, SysinfoProbe,
    check_liveness, filetime_to_unix_secs, start_matches, unix_secs_to_filetime,
};
pub use registry::{RegistryEntry, RegistryScan, scan_registry};
pub use snapshot::{
    Discovery, DiscoveryOptions, FoldResult, KnownProcess, RefreshResult, Snapshot, SnapshotReport,
    take_snapshot,
};

use serde::Serialize;
use std::path::PathBuf;

/// A file or entry a reader could not use. Reported, never raised, so one bad file cannot hide the
/// sessions around it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Problem {
    /// The file concerned.
    pub path: PathBuf,
    /// What was wrong with it.
    pub what: String,
}

impl Problem {
    /// Build a problem report.
    pub fn new(path: impl Into<PathBuf>, what: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            what: what.into(),
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.what)
    }
}
