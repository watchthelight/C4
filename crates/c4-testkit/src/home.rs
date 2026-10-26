// <wtl> project: c4 | file: crates/c4-testkit/src/home.rs | author: watchthelight </wtl>
//! A Claude home in a temporary directory, laid out like `~/.claude`: `sessions/`, `projects/`
//! and whatever the fixtures add. Removed when dropped.

use c4_core::SessionId;
use c4_discovery::{ClaudeHome, DiscoveryOptions, HOME_ENV};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The workspace `fixtures/` directory.
#[must_use]
pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .join("fixtures")
}

/// A Claude home under a temporary directory. The home is `<tmp>/claude`; `<tmp>/work/<name>`
/// directories stand in for projects, so a stand-in session has a working directory that exists.
#[derive(Debug)]
pub struct ScratchHome {
    dir: tempfile::TempDir,
    home: ClaudeHome,
}

impl ScratchHome {
    /// An empty home: `sessions/` and `projects/` exist and hold nothing.
    pub fn empty() -> io::Result<Self> {
        let dir = tempfile::Builder::new().prefix("c4-scratch-").tempdir()?;
        let root = dir.path().join("claude");
        std::fs::create_dir_all(root.join("sessions"))?;
        std::fs::create_dir_all(root.join("projects"))?;
        Ok(Self {
            dir,
            home: ClaudeHome::at(root),
        })
    }

    /// A home holding a copy of `fixtures/home`: four registry records, a supervisor roster, two
    /// jobs and the alpha to delta transcripts.
    pub fn seeded() -> io::Result<Self> {
        let scratch = Self::empty()?;
        copy_dir(&fixtures_root().join("home"), scratch.root())?;
        Ok(scratch)
    }

    /// The home.
    #[must_use]
    pub fn home(&self) -> &ClaudeHome {
        &self.home
    }

    /// The home's root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.home.root()
    }

    /// The temporary directory holding the home and the work directories.
    #[must_use]
    pub fn scratch_dir(&self) -> &Path {
        self.dir.path()
    }

    /// A working directory for a stand-in session, `<tmp>/work/<name>`, created on demand and
    /// returned as the string the registry and the transcripts carry.
    pub fn project_dir(&self, name: &str) -> io::Result<String> {
        let dir = self.dir.path().join("work").join(name);
        std::fs::create_dir_all(&dir)?;
        Ok(dir.to_string_lossy().into_owned())
    }

    /// The environment variable that points C4 at this home.
    #[must_use]
    pub fn env_var(&self) -> (&'static str, OsString) {
        (HOME_ENV, self.root().as_os_str().to_os_string())
    }

    /// Discovery options that read this home and run no command.
    #[must_use]
    pub fn files_only(&self) -> DiscoveryOptions {
        DiscoveryOptions::files_only(self.home.clone())
    }

    /// Write a registry record for `pid` through a temporary file and a rename, so a scan never
    /// reads half of it.
    pub fn write_record(&self, pid: u32, record: &serde_json::Value) -> io::Result<PathBuf> {
        let dir = self.home.sessions_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{pid}.json"));
        let text = serde_json::to_string_pretty(record).map_err(io::Error::other)?;
        write_atomic(&path, text.as_bytes())?;
        Ok(path)
    }

    /// Remove the registry record for `pid`, as Claude Code does when its process exits. A record
    /// that is already gone is fine.
    pub fn remove_record(&self, pid: u32) -> io::Result<()> {
        match std::fs::remove_file(self.home.sessions_dir().join(format!("{pid}.json"))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Append one line to the main transcript of `session`, creating the project directory and the
    /// file on first use.
    pub fn append_transcript(
        &self,
        cwd: &str,
        session: &SessionId,
        line: &str,
    ) -> io::Result<PathBuf> {
        let path = self.home.transcript_path(cwd, session);
        append_line(&path, line)?;
        Ok(path)
    }

    /// Create or extend an agent transcript under `<session>/subagents/`, writing its `.meta.json`
    /// beside it when `meta` is given.
    pub fn write_agent_file(
        &self,
        cwd: &str,
        session: &SessionId,
        agent_id: &str,
        lines: &[&str],
        meta: Option<&serde_json::Value>,
    ) -> io::Result<PathBuf> {
        let dir = self
            .home
            .project_dir(cwd)
            .join(session.as_str())
            .join("subagents");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("agent-{agent_id}.jsonl"));
        if lines.is_empty() {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
        }
        for line in lines {
            append_line(&path, line)?;
        }
        if let Some(meta) = meta {
            let text = serde_json::to_string_pretty(meta).map_err(io::Error::other)?;
            write_atomic(
                &dir.join(format!("agent-{agent_id}.meta.json")),
                text.as_bytes(),
            )?;
        }
        Ok(path)
    }
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.trim_end_matches(['\r', '\n']).as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

/// Write `bytes` to a sibling temporary file and rename it over `path`.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_home_has_the_two_directories_and_nothing_else() {
        let scratch = ScratchHome::empty().unwrap();
        assert!(scratch.root().join("sessions").is_dir());
        assert!(scratch.root().join("projects").is_dir());
        assert_eq!(std::fs::read_dir(scratch.root()).unwrap().count(), 2);
        assert!(scratch.root().starts_with(scratch.scratch_dir()));
        let (key, value) = scratch.env_var();
        assert_eq!(key, "C4_CLAUDE_HOME");
        assert_eq!(Path::new(&value), scratch.root());
        assert_eq!(scratch.files_only().home.root(), scratch.root());
    }

    #[test]
    fn records_transcripts_and_agent_files_land_where_claude_code_puts_them() {
        let scratch = ScratchHome::empty().unwrap();
        let cwd = scratch.project_dir("proj").unwrap();
        assert!(Path::new(&cwd).is_dir());
        let sid = SessionId::new("0a1b2c3d-0001-4000-8000-000000000001");

        let record = scratch
            .write_record(
                4242,
                &serde_json::json!({"pid": 4242, "sessionId": sid.as_str()}),
            )
            .unwrap();
        assert_eq!(record, scratch.root().join("sessions").join("4242.json"));
        assert!(!record.with_extension("json.tmp").exists());
        scratch.remove_record(4242).unwrap();
        assert!(!record.exists());
        scratch.remove_record(4242).unwrap();

        let transcript = scratch
            .append_transcript(&cwd, &sid, r#"{"type":"user","uuid":"u1"}"#)
            .unwrap();
        scratch
            .append_transcript(&cwd, &sid, "{\"type\":\"assistant\",\"uuid\":\"a1\"}\r\n")
            .unwrap();
        assert_eq!(transcript, scratch.home().transcript_path(&cwd, &sid));
        let text = std::fs::read_to_string(&transcript).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(
            text.ends_with("\"a1\"}\n"),
            "one newline per line: {text:?}"
        );

        let agent = scratch
            .write_agent_file(
                &cwd,
                &sid,
                "a0000001",
                &[r#"{"type":"user","isSidechain":true}"#],
                Some(&serde_json::json!({"agentType": "Explore"})),
            )
            .unwrap();
        assert_eq!(
            agent,
            transcript
                .with_extension("")
                .join("subagents")
                .join("agent-a0000001.jsonl")
        );
        assert!(agent.with_extension("meta.json").is_file());
        let empty = scratch
            .write_agent_file(&cwd, &sid, "a0000002", &[], None)
            .unwrap();
        assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);
    }

    #[test]
    fn a_seeded_home_is_a_copy_of_the_fixtures() {
        let scratch = ScratchHome::seeded().unwrap();
        for rel in [
            "sessions/41001.json",
            "daemon/roster.json",
            "jobs/0b2c3d4e/state.json",
            "projects/-home-example-proj-alpha/0a1b2c3d-0001-4000-8000-000000000001.jsonl",
        ] {
            assert!(scratch.root().join(rel).is_file(), "{rel}");
        }
        let fixture = fixtures_root()
            .join("home")
            .join("sessions")
            .join("41001.json");
        assert_eq!(
            std::fs::read(fixture).unwrap(),
            std::fs::read(scratch.root().join("sessions").join("41001.json")).unwrap()
        );
    }
}
