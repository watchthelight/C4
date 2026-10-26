// <wtl> project: c4 | file: crates/c4-testkit/src/fake.rs | author: watchthelight </wtl>
//! A stand-in for a running Claude Code process. It is a child this crate spawns (`cmd.exe /d /c
//! pause` on Windows, `cat` elsewhere, both blocked on a pipe that stays open) and registers in a
//! scratch home the way Claude Code registers itself: `sessions/<pid>.json` with the child's real
//! pid and creation time. The discovery crate's liveness ladder compares that time with the process
//! table, so the stand-in counts as alive for exactly as long as the child runs. Dropping it ends
//! the child and, unless it was told to crash, removes the record, which is what Claude Code does on
//! exit. Nothing else is ever signaled.
//!
//! The child's environment has every terminal host variable removed, so discovery reports an
//! unknown host unless the options add one back. (`cmd.exe` puts `ComSpec` back by itself; the
//! host identification does not read it.)

use c4_core::{ProcessRef, SessionId, Timestamp};
use c4_discovery::{ClaudeHome, HOST_ENV_KEYS, ProcessProbe, SysinfoProbe, unix_secs_to_filetime};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The Claude Code version the stand-in claims. The fixtures use the same one.
pub const FAKE_VERSION: &str = "2.1.299";

/// What the record says. Built with the setters, then passed to [`FakeClaude::start`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FakeClaudeOptions {
    /// The session id. A fresh v4 uuid unless set.
    pub session: SessionId,
    /// The working directory the record names. It need not exist.
    pub cwd: String,
    /// The display name and where it came from (`user`, `auto`, `derived`).
    pub name: Option<(String, String)>,
    /// `busy`, `idle` or `waiting`.
    pub status: String,
    /// Why it waits, for `waiting`.
    pub waiting_for: Option<String>,
    /// The version the record claims.
    pub version: String,
    /// Environment variables for the child, on top of an environment with every terminal host
    /// variable removed. `("WEZTERM_PANE", "7")` makes discovery see a `WezTerm` pane.
    pub env: Vec<(String, String)>,
}

impl FakeClaudeOptions {
    /// Idle, unnamed, in `cwd`, with a fresh session id.
    #[must_use]
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            session: SessionId::new(uuid::Uuid::new_v4().to_string()),
            cwd: cwd.into(),
            name: None,
            status: "idle".into(),
            waiting_for: None,
            version: FAKE_VERSION.into(),
            env: Vec::new(),
        }
    }

    /// Use this session id.
    #[must_use]
    pub fn session(mut self, id: impl Into<String>) -> Self {
        self.session = SessionId::new(id);
        self
    }

    /// Start with this status and, for `waiting`, this reason.
    #[must_use]
    pub fn status(mut self, status: &str, waiting_for: Option<&str>) -> Self {
        self.status = status.to_string();
        self.waiting_for = waiting_for.map(str::to_string);
        self
    }

    /// Carry a display name.
    #[must_use]
    pub fn name(mut self, name: &str, source: &str) -> Self {
        self.name = Some((name.to_string(), source.to_string()));
        self
    }

    /// Set one variable in the child's environment.
    #[must_use]
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }
}

/// A running stand-in. See the module documentation.
#[derive(Debug)]
pub struct FakeClaude {
    child: Child,
    pid: u32,
    proc_start: u64,
    opts: FakeClaudeOptions,
    started_at: Timestamp,
    home: ClaudeHome,
    record_path: PathBuf,
    keep_record: bool,
}

impl FakeClaude {
    /// Spawn the child and write its record under `home`.
    pub fn start(home: &ClaudeHome, opts: FakeClaudeOptions) -> io::Result<Self> {
        std::fs::create_dir_all(home.sessions_dir())?;
        let mut child = stand_in(&opts.env).spawn()?;
        let pid = child.id();
        let start_secs = SysinfoProbe::new()
            .probe(pid)
            .and_then(|p| p.start_time_secs);
        let Some(start_secs) = start_secs else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(format!(
                "the process table shows no start time for pid {pid}"
            )));
        };
        // Claude Code writes the creation FILETIME on Windows (verified). What it writes on macOS
        // and Linux is not verified, so the stand-in writes plain Unix seconds there, which the
        // liveness ladder also accepts.
        let proc_start = if cfg!(windows) {
            unix_secs_to_filetime(start_secs)
        } else {
            start_secs
        };
        let started_at = Timestamp::now();
        let record_path = home.sessions_dir().join(format!("{pid}.json"));
        let mut fake = Self {
            child,
            pid,
            proc_start,
            opts,
            started_at,
            home: home.clone(),
            record_path,
            keep_record: false,
        };
        fake.write_record()?;
        if !fake.is_running() {
            return Err(io::Error::other(format!(
                "the stand-in process {pid} ended before it was registered"
            )));
        }
        Ok(fake)
    }

    /// The child's pid, the one in the record.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The process reference the record describes.
    #[must_use]
    pub fn process(&self) -> ProcessRef {
        ProcessRef::new(self.pid, self.proc_start)
    }

    /// The session id.
    #[must_use]
    pub fn session(&self) -> &SessionId {
        &self.opts.session
    }

    /// The working directory the record names.
    #[must_use]
    pub fn cwd(&self) -> &str {
        &self.opts.cwd
    }

    /// The options as they stand now.
    #[must_use]
    pub fn options(&self) -> &FakeClaudeOptions {
        &self.opts
    }

    /// `sessions/<pid>.json`.
    #[must_use]
    pub fn record_path(&self) -> &Path {
        &self.record_path
    }

    /// Where the session's main transcript would be.
    #[must_use]
    pub fn transcript_path(&self) -> PathBuf {
        self.home
            .transcript_path(&self.opts.cwd, &self.opts.session)
    }

    /// The record as Claude Code would write it now.
    #[must_use]
    pub fn record(&self) -> Value {
        let now = Timestamp::now().millis();
        let proc_start = if cfg!(windows) {
            Value::String(self.proc_start.to_string())
        } else {
            Value::from(self.proc_start)
        };
        let mut record = json!({
            "pid": self.pid,
            "sessionId": self.opts.session.as_str(),
            "cwd": self.opts.cwd,
            "startedAt": self.started_at.millis(),
            "procStart": proc_start,
            "version": self.opts.version,
            "peerProtocol": 1,
            "peerFeatures": ["notify_idle", "artifact_yield"],
            "kind": "interactive",
            "entrypoint": "cli",
            "updatedAt": now,
            "status": self.opts.status,
            "statusUpdatedAt": now,
        });
        if let Some((name, source)) = &self.opts.name {
            record["name"] = json!(name);
            record["nameSource"] = json!(source);
            record["nameSince"] = json!(self.started_at.millis());
        }
        if let Some(waiting_for) = &self.opts.waiting_for {
            record["waitingFor"] = json!(waiting_for);
        }
        record
    }

    /// Change the status and rewrite the record, as Claude Code does on every status change.
    pub fn set_status(&mut self, status: &str, waiting_for: Option<&str>) -> io::Result<()> {
        self.opts.status = status.to_string();
        self.opts.waiting_for = waiting_for.map(str::to_string);
        self.write_record()
    }

    /// Change the name and rewrite the record.
    pub fn set_name(&mut self, name: &str, source: &str) -> io::Result<()> {
        self.opts.name = Some((name.to_string(), source.to_string()));
        self.write_record()
    }

    /// Append one line to the session's main transcript.
    pub fn append_transcript(&self, line: &str) -> io::Result<PathBuf> {
        let path = self.transcript_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(line.trim_end_matches(['\r', '\n']).as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(path)
    }

    /// True while the child runs.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// End the stand-in the way Claude Code ends: the process goes away and so does the record.
    /// The child's stdin is closed, which ends it by itself; one that lingers is killed.
    pub fn exit(mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && self.is_running() {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.keep_record = false;
    }

    /// End the stand-in the way a crashed Claude Code ends: the process goes away and the record
    /// stays behind, pointing at a dead pid.
    pub fn crash(mut self) -> io::Result<()> {
        self.keep_record = true;
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }

    fn write_record(&mut self) -> io::Result<()> {
        let text = serde_json::to_string_pretty(&self.record()).map_err(io::Error::other)?;
        crate::home::write_atomic(&self.record_path, text.as_bytes())
    }
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.keep_record {
            let _ = std::fs::remove_file(&self.record_path);
        }
    }
}

/// The command that stays alive on a pipe. Its environment carries no terminal host variable.
fn stand_in(env: &[(String, String)]) -> Command {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/c", "pause"]);
        c
    } else {
        Command::new("cat")
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for key in HOST_ENV_KEYS {
        cmd.env_remove(key);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
