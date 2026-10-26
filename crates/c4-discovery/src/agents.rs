// <wtl> project: c4 | file: crates/c4-discovery/src/agents.rs | author: watchthelight </wtl>
//! `claude agents --json`: Claude Code's own scriptable list of sessions. Interactive entries carry
//! `pid`, `status` and `waitingFor`; background entries carry the short `id` and a `state`. The
//! list comes from state files, so a background entry can outlive its process: the process table
//! decides liveness, not this list.

use c4_core::{
    BgState, Observation, ObservationBody, ProcessRef, RegistryStatus, SessionId, SessionKind,
    Timestamp,
};
use serde::Deserialize;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Replaces the command that lists sessions. Tests point it at a script that prints a fixture.
pub const AGENTS_CMD_ENV: &str = "C4_AGENTS_CMD";

/// One element of the list. Everything is optional; unknown fields are ignored.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsEntry {
    /// The short job id of a background session.
    #[serde(default)]
    pub id: Option<String>,
    /// The process id, while the process lives.
    #[serde(default)]
    pub pid: Option<u32>,
    /// The working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// `interactive` or `background`.
    #[serde(default)]
    pub kind: Option<String>,
    /// When the session started, milliseconds since the epoch.
    #[serde(default)]
    pub started_at: Option<i64>,
    /// The session id.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The display name.
    #[serde(default)]
    pub name: Option<String>,
    /// `busy`, `idle` or `waiting`, for interactive entries.
    #[serde(default)]
    pub status: Option<RegistryStatus>,
    /// Why the session waits, when it does.
    #[serde(default)]
    pub waiting_for: Option<String>,
    /// `working`, `blocked`, `done`, `failed` or `stopped`, for background entries.
    #[serde(default)]
    pub state: Option<BgState>,
}

impl AgentsEntry {
    /// The session id, when the entry has one.
    #[must_use]
    pub fn session(&self) -> Option<SessionId> {
        self.session_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(SessionId::new)
    }

    /// Background when the entry says so or carries a job id and no pid; interactive otherwise.
    #[must_use]
    pub fn session_kind(&self) -> SessionKind {
        match self.kind.as_deref() {
            Some("background") => SessionKind::Background,
            Some("interactive") => SessionKind::Interactive,
            _ if self.pid.is_none() && self.id.is_some() => SessionKind::Background,
            _ => SessionKind::Interactive,
        }
    }

    /// The observation this entry stands for, dated `now` because the list was produced now.
    /// Entries without a session id cannot be keyed and yield nothing.
    #[must_use]
    pub fn observation(&self, now: Timestamp) -> Option<Observation> {
        let session = self.session()?;
        Some(Observation::new(
            session,
            now,
            ObservationBody::Agents {
                session_kind: self.session_kind(),
                process: self.pid.map(ProcessRef::pid_only),
                status: self.status,
                waiting_for: self.waiting_for.clone(),
                bg_state: self.state,
                cwd: self.cwd.clone(),
                name: self.name.clone(),
            },
        ))
    }
}

/// Parse the command output: a JSON array, or an object holding the array under `sessions`,
/// `agents` or `items`.
pub fn parse_agents_json(text: &str) -> Result<Vec<AgentsEntry>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let array = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut map) => ["sessions", "agents", "items"]
            .iter()
            .find_map(|k| map.remove(*k))
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    array.into_iter().map(serde_json::from_value).collect()
}

/// Why the list could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum AgentsError {
    /// The program did not start.
    #[error("could not start {program}: {source}")]
    Spawn {
        /// The program.
        program: String,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// The program ran too long and was killed.
    #[error("{program} did not finish within {} ms", timeout.as_millis())]
    Timeout {
        /// The program.
        program: String,
        /// The budget it missed.
        timeout: Duration,
    },
    /// The program exited with a failure status.
    #[error("{program} failed ({status}): {stderr}")]
    Failed {
        /// The program.
        program: String,
        /// The exit status as text.
        status: String,
        /// The last part of its stderr.
        stderr: String,
    },
    /// The output was not the expected JSON.
    #[error("could not parse the output of {program}: {source}")]
    Parse {
        /// The program.
        program: String,
        /// The error.
        #[source]
        source: serde_json::Error,
    },
}

/// The command that lists sessions: `claude agents --json` unless [`AGENTS_CMD_ENV`] replaces it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AgentsCommand {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

impl Default for AgentsCommand {
    fn default() -> Self {
        Self {
            program: "claude".into(),
            args: vec!["agents".into(), "--json".into()],
        }
    }
}

impl AgentsCommand {
    /// A command from a program and its arguments.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// The command from the environment: [`AGENTS_CMD_ENV`] split on whitespace, else the default.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var(AGENTS_CMD_ENV)
            .ok()
            .and_then(|spec| Self::parse(&spec))
            .unwrap_or_default()
    }

    /// Parse a whitespace-separated command line. Quoting is not supported; a program path with
    /// spaces needs a wrapper script.
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let mut parts = spec.split_whitespace();
        let program = parts.next()?;
        Some(Self::new(program, parts))
    }

    /// Run the command with a time budget and parse what it prints.
    pub fn run(&self, timeout: Duration) -> Result<Vec<AgentsEntry>, AgentsError> {
        let output = self.capture(timeout)?;
        if !output.success {
            return Err(AgentsError::Failed {
                program: self.program.clone(),
                status: output.status,
                stderr: tail(&output.stderr, 400),
            });
        }
        parse_agents_json(&output.stdout).map_err(|source| AgentsError::Parse {
            program: self.program.clone(),
            source,
        })
    }

    fn command(&self, program: &OsStr) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// Run the command with a time budget and collect whatever it printed. [`Self::run`] is this
    /// plus the JSON parse. On Windows a bare program name is also looked up with PATHEXT, so an
    /// npm `.cmd` shim is found.
    pub fn capture(&self, timeout: Duration) -> Result<Captured, AgentsError> {
        let spawn_error = |source| AgentsError::Spawn {
            program: self.program.clone(),
            source,
        };
        let mut child = match self.command(self.program.as_ref()).spawn() {
            Ok(child) => child,
            // npm installs `claude` as a `.cmd` shim on Windows. The bare name only resolves to
            // `.exe`, so look along PATH with PATHEXT; the standard library runs a `.cmd` or `.bat`
            // file through cmd.exe itself, with the quoting that needs.
            Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound => {
                match resolve_on_path(&self.program) {
                    Some(found) => self
                        .command(found.as_os_str())
                        .spawn()
                        .map_err(spawn_error)?,
                    None => return Err(spawn_error(e)),
                }
            }
            Err(e) => return Err(spawn_error(e)),
        };
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AgentsError::Timeout {
                        program: self.program.clone(),
                        timeout,
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(spawn_error(e)),
            }
        };
        let join = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
            h.and_then(|h| h.join().ok())
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default()
        };
        Ok(Captured {
            success: status.success(),
            status: status.to_string(),
            stdout: join(stdout),
            stderr: join(stderr),
            elapsed: started.elapsed(),
        })
    }
}

/// Find `program` on PATH. On Windows a name without an extension is tried with each PATHEXT
/// extension, the way cmd.exe does, so an npm `.cmd` shim is found; elsewhere the name is the
/// executable. A program given with a directory is returned only when it exists as written.
#[must_use]
pub fn resolve_on_path(program: &str) -> Option<PathBuf> {
    let given = Path::new(program);
    if given.components().count() > 1 {
        return given.is_file().then(|| given.to_path_buf());
    }
    let candidates: Vec<String> = if cfg!(windows) && given.extension().is_none() {
        pathext()
            .iter()
            .map(|ext| format!("{program}{ext}"))
            .collect()
    } else {
        vec![program.to_string()]
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| candidates.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// The PATHEXT list, lower-cased, or cmd.exe's own default when it is unset.
fn pathext() -> Vec<String> {
    std::env::var("PATHEXT")
        .ok()
        .map(|p| {
            p.split(';')
                .filter(|e| e.starts_with('.'))
                .map(str::to_ascii_lowercase)
                .collect::<Vec<String>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            [".com", ".exe", ".bat", ".cmd"]
                .iter()
                .map(|ext| (*ext).to_string())
                .collect()
        })
}

/// What a command printed and how it ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Captured {
    /// True when the exit status was success.
    pub success: bool,
    /// The exit status as text.
    pub status: String,
    /// Everything it wrote to stdout, with invalid UTF-8 replaced.
    pub stdout: String,
    /// Everything it wrote to stderr, the same way.
    pub stderr: String,
    /// How long it ran.
    pub elapsed: Duration,
}

fn drain(mut reader: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    })
}

fn tail(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        return s.to_string();
    }
    let start = s
        .char_indices()
        .map(|(i, _)| i)
        .rev()
        .take(max)
        .last()
        .unwrap_or(0);
    format!("...{}", &s[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The fixture path with native separators and no `..`, since cmd.exe reads `/` as a switch.
    fn fixture() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root")
            .join("fixtures")
            .join("agents-json")
            .join("active.json")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn parses_the_fixture_including_unknown_kinds() {
        let text = std::fs::read_to_string(fixture()).unwrap();
        let entries = parse_agents_json(&text).unwrap();
        assert_eq!(entries.len(), 5);
        let bg: Vec<&AgentsEntry> = entries
            .iter()
            .filter(|e| e.session_kind() == SessionKind::Background)
            .collect();
        assert_eq!(bg.len(), 2);
        assert_eq!(bg[0].state, Some(BgState::Working));
        assert_eq!(bg[0].pid, Some(41011));
        assert_eq!(bg[1].state, Some(BgState::Blocked));
        assert_eq!(bg[1].pid, None);
        let odd = entries.iter().find(|e| e.pid == Some(41099)).unwrap();
        assert_eq!(odd.status, Some(RegistryStatus::Unknown));
        assert_eq!(odd.session_kind(), SessionKind::Interactive);
        let wrapped = parse_agents_json(r#"{"sessions":[{"pid":1,"sessionId":"x"}]}"#).unwrap();
        assert_eq!(wrapped.len(), 1);
        assert!(parse_agents_json("not json").is_err());
    }

    #[test]
    fn observation_needs_a_session_id() {
        let none = AgentsEntry::default();
        assert!(none.observation(Timestamp::from_millis(1)).is_none());
        let some = AgentsEntry {
            session_id: Some("0a1b2c3d-0001-4000-8000-000000000001".into()),
            pid: Some(41001),
            status: Some(RegistryStatus::Busy),
            ..AgentsEntry::default()
        };
        let obs = some.observation(Timestamp::from_millis(1)).unwrap();
        assert!(matches!(
            obs.body,
            ObservationBody::Agents {
                process: Some(p),
                status: Some(RegistryStatus::Busy),
                ..
            } if p == ProcessRef::pid_only(41001)
        ));
    }

    #[test]
    fn parses_command_specs() {
        let c = AgentsCommand::parse("python -I scripts/list.py --json").unwrap();
        assert_eq!(c.program, "python");
        assert_eq!(c.args, ["-I", "scripts/list.py", "--json"]);
        assert!(AgentsCommand::parse("   ").is_none());
        assert_eq!(AgentsCommand::default().args, ["agents", "--json"]);
    }

    #[test]
    fn runs_a_command_that_prints_the_fixture() {
        let cmd = if cfg!(windows) {
            AgentsCommand::new("cmd.exe", ["/d", "/c", "type", &fixture()])
        } else {
            AgentsCommand::new("cat", [fixture()])
        };
        let entries = cmd.run(Duration::from_secs(10)).unwrap();
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn reports_missing_programs_and_failures() {
        let missing = AgentsCommand::new("c4-no-such-program-0000", Vec::<String>::new());
        assert!(matches!(
            missing.run(Duration::from_secs(5)),
            Err(AgentsError::Spawn { .. })
        ));
        let failing = if cfg!(windows) {
            AgentsCommand::new("cmd.exe", ["/d", "/c", "exit", "3"])
        } else {
            AgentsCommand::new("sh", ["-c", "exit 3"])
        };
        assert!(matches!(
            failing.run(Duration::from_secs(5)),
            Err(AgentsError::Failed { .. })
        ));
    }

    #[test]
    fn resolves_programs_on_path() {
        assert!(resolve_on_path("c4-no-such-program-0000").is_none());
        assert!(resolve_on_path("c4-no-such-dir/x.exe").is_none());
        if cfg!(windows) {
            let cmd = resolve_on_path("cmd").expect("cmd.exe on PATH");
            assert!(
                cmd.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe")),
                "{}",
                cmd.display()
            );
        }
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("abcdef", 3), "...def");
        assert_eq!(tail("ab", 3), "ab");
    }
}
