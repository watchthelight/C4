// <wtl> project: c4 | file: crates/c4-discovery/src/home.rs | author: watchthelight </wtl>
//! The Claude home: where Claude Code keeps its registry, supervisor files and transcripts.

use c4_core::SessionId;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Overrides the Claude home for C4 only. Tests point it at a scratch directory.
pub const HOME_ENV: &str = "C4_CLAUDE_HOME";
/// Claude Code's own override of its config root.
pub const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

/// The root Claude Code writes under: `C4_CLAUDE_HOME`, else `CLAUDE_CONFIG_DIR`, else `~/.claude`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClaudeHome {
    root: PathBuf,
}

impl ClaudeHome {
    /// Resolve from the process environment.
    #[must_use]
    pub fn resolve() -> Self {
        Self::resolve_from(
            std::env::var_os(HOME_ENV).as_deref(),
            std::env::var_os(CONFIG_DIR_ENV).as_deref(),
            &user_home(),
        )
    }

    /// The resolution rule with its inputs made explicit: the C4 override wins, then Claude Code's
    /// own override, then `.claude` under the user's home. Empty values count as unset.
    #[must_use]
    pub fn resolve_from(
        c4_override: Option<&OsStr>,
        claude_override: Option<&OsStr>,
        user_home: &Path,
    ) -> Self {
        if let Some(p) = c4_override.filter(|p| !p.is_empty()) {
            return Self::at(p);
        }
        if let Some(p) = claude_override.filter(|p| !p.is_empty()) {
            return Self::at(p);
        }
        Self::at(user_home.join(".claude"))
    }

    /// A home at a known path.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// True when the root exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    /// `sessions/`: one `<pid>.json` per running process.
    #[must_use]
    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    /// `daemon/`: the background supervisor's files.
    #[must_use]
    pub fn daemon_dir(&self) -> PathBuf {
        self.root.join("daemon")
    }

    /// `daemon/roster.json`.
    #[must_use]
    pub fn roster_path(&self) -> PathBuf {
        self.daemon_dir().join("roster.json")
    }

    /// `jobs/`: one directory per background job.
    #[must_use]
    pub fn jobs_dir(&self) -> PathBuf {
        self.root.join("jobs")
    }

    /// `projects/`: transcripts, grouped by encoded working directory.
    #[must_use]
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// The transcript directory for a working directory.
    #[must_use]
    pub fn project_dir(&self, cwd: &str) -> PathBuf {
        self.projects_dir().join(encode_cwd(cwd))
    }

    /// The main transcript path for a session that ran in `cwd`.
    #[must_use]
    pub fn transcript_path(&self, cwd: &str, session: &SessionId) -> PathBuf {
        self.project_dir(cwd).join(format!("{session}.jsonl"))
    }
}

pub use c4_core::home::user_home;

/// Claude Code's project directory name for a working directory: every character that is not an
/// ASCII letter or digit becomes a dash, so `C:\Users\example\proj` is `C--Users-example-proj` and
/// `/home/example/proj` is `-home-example-proj`. Verified against Claude Code 2.1.299 on Windows for
/// ASCII paths; what it does with characters outside ASCII is not verified.
#[must_use]
pub fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_claude_code() {
        assert_eq!(
            encode_cwd("C:\\Users\\example\\proj-gamma"),
            "C--Users-example-proj-gamma"
        );
        assert_eq!(
            encode_cwd("/home/example/proj-alpha"),
            "-home-example-proj-alpha"
        );
        assert_eq!(
            encode_cwd("/tmp/All the Mods 10 - ATM10 (1)"),
            "-tmp-All-the-Mods-10---ATM10--1-"
        );
    }

    #[test]
    fn resolution_order() {
        let home = Path::new("/home/example");
        let a = ClaudeHome::resolve_from(
            Some(OsStr::new("/tmp/scratch")),
            Some(OsStr::new("/tmp/cfg")),
            home,
        );
        assert_eq!(a.root(), Path::new("/tmp/scratch"));
        let b = ClaudeHome::resolve_from(Some(OsStr::new("")), Some(OsStr::new("/tmp/cfg")), home);
        assert_eq!(b.root(), Path::new("/tmp/cfg"));
        let c = ClaudeHome::resolve_from(None, None, home);
        assert_eq!(c.root(), Path::new("/home/example/.claude"));
    }

    #[test]
    fn derived_paths() {
        let h = ClaudeHome::at("/tmp/scratch");
        assert_eq!(h.sessions_dir(), Path::new("/tmp/scratch/sessions"));
        assert_eq!(
            h.roster_path(),
            Path::new("/tmp/scratch/daemon/roster.json")
        );
        let sid = SessionId::new("0a1b2c3d-0001-4000-8000-000000000001");
        assert_eq!(
            h.transcript_path("/home/example/proj-alpha", &sid),
            Path::new(
                "/tmp/scratch/projects/-home-example-proj-alpha/0a1b2c3d-0001-4000-8000-000000000001.jsonl"
            )
        );
    }
}
