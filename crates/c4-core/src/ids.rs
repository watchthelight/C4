// <wtl> project: c4 | file: crates/c4-core/src/ids.rs | author: watchthelight </wtl>
//! Identifiers: the durable session id, the process reference that defeats pid reuse, agent ids and
//! the kinds of terminal host a session can live in.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Claude Code's session identifier, a UUID string. It is the durable key for everything C4 stores:
/// a process can change its session id on `/clear` or `/resume`, and a session id can be resumed in
/// a new process days later.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    /// Wrap a session id string. No validation beyond trimming; see [`SessionId::looks_like_uuid`].
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into().trim().to_string())
    }

    /// The id as written by Claude Code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The 8-character prefix that the status line and the ledger use. Prefixes can collide, so this
    /// is for display and for joining with those files only.
    #[must_use]
    pub fn short(&self) -> &str {
        let end = self
            .0
            .char_indices()
            .nth(8)
            .map_or(self.0.len(), |(i, _)| i);
        &self.0[..end]
    }

    /// True when the id has the 8-4-4-4-12 hexadecimal shape Claude Code uses.
    #[must_use]
    pub fn looks_like_uuid(&self) -> bool {
        let parts: Vec<&str> = self.0.split('-').collect();
        parts.len() == 5
            && [8, 4, 4, 4, 12]
                .iter()
                .zip(&parts)
                .all(|(len, p)| p.len() == *len && p.chars().all(|c| c.is_ascii_hexdigit()))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionId({})", self.0)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// A subagent identifier as written in `subagents/agent-<id>.jsonl`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    /// Wrap an agent id.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The id as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A process together with its creation time, so that a pid handed to a new process after the
/// original exited is never mistaken for the original. On Windows the creation time is the process
/// creation `FILETIME`, which is what Claude Code writes as `procStart` in its session registry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct ProcessRef {
    /// The operating system process id.
    pub pid: u32,
    /// The creation time in the platform's native encoding, when known.
    pub proc_start: Option<u64>,
}

impl ProcessRef {
    /// A reference with a known creation time.
    #[must_use]
    pub fn new(pid: u32, proc_start: u64) -> Self {
        Self {
            pid,
            proc_start: Some(proc_start),
        }
    }

    /// A reference with an unknown creation time (older registries, foreign hosts).
    #[must_use]
    pub fn pid_only(pid: u32) -> Self {
        Self {
            pid,
            proc_start: None,
        }
    }

    /// True when both refer to the same process: equal pids, and equal creation times when both
    /// sides know theirs.
    #[must_use]
    pub fn same_process(&self, other: &ProcessRef) -> bool {
        self.pid == other.pid
            && match (self.proc_start, other.proc_start) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

impl fmt::Display for ProcessRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.proc_start {
            Some(t) => write!(f, "pid {} (started {t})", self.pid),
            None => write!(f, "pid {}", self.pid),
        }
    }
}

/// Whether a session is an interactive REPL or a background worker started with `claude --bg`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// An interactive session in a terminal.
    #[default]
    Interactive,
    /// A supervisor-owned background session.
    Background,
}

/// The terminal host a session was found in. Decides the attachment mode: hosts with a verified
/// control surface can be attached, the rest are observed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    /// A terminal pane that C4 owns.
    C4,
    /// `WezTerm`, controlled through `wezterm cli`.
    WezTerm,
    /// tmux, controlled through `tmux send-keys` and `select-pane`.
    Tmux,
    /// iTerm2, controlled through its Python API.
    ITerm2,
    /// kitty, controlled through `kitty @` when remote control is enabled.
    Kitty,
    /// Windows Terminal: window focus only, there is no public tab API.
    WindowsTerminal,
    /// Terminal.app.
    TerminalApp,
    /// The VS Code integrated terminal.
    VsCode,
    /// GNOME Terminal and other VTE terminals.
    GnomeTerminal,
    /// Alacritty.
    Alacritty,
    /// A bare console host (conhost or cmd window).
    Conhost,
    /// Nothing recognizable in the process environment.
    #[default]
    Unknown,
}

impl HostKind {
    /// The name shown in the sidebar.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            HostKind::C4 => "C4",
            HostKind::WezTerm => "WezTerm",
            HostKind::Tmux => "tmux",
            HostKind::ITerm2 => "iTerm2",
            HostKind::Kitty => "kitty",
            HostKind::WindowsTerminal => "Windows Terminal",
            HostKind::TerminalApp => "Terminal.app",
            HostKind::VsCode => "VS Code",
            HostKind::GnomeTerminal => "GNOME Terminal",
            HostKind::Alacritty => "Alacritty",
            HostKind::Conhost => "console",
            HostKind::Unknown => "unknown host",
        }
    }

    /// True for hosts whose adapter can deliver text to a verified pane.
    #[must_use]
    pub fn can_send(self) -> bool {
        matches!(
            self,
            HostKind::C4 | HostKind::WezTerm | HostKind::Tmux | HostKind::ITerm2 | HostKind::Kitty
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_shape() {
        let id = SessionId::new("a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d");
        assert!(id.looks_like_uuid());
        assert_eq!(id.short(), "a1b2c3d4");
        assert!(!SessionId::new("nope").looks_like_uuid());
        assert_eq!(SessionId::new("abc").short(), "abc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d\"");
    }

    #[test]
    fn process_ref_matching() {
        let a = ProcessRef::new(100, 5);
        assert!(a.same_process(&ProcessRef::new(100, 5)));
        assert!(!a.same_process(&ProcessRef::new(100, 6)));
        assert!(a.same_process(&ProcessRef::pid_only(100)));
        assert!(!a.same_process(&ProcessRef::pid_only(101)));
    }

    #[test]
    fn host_send_capability() {
        assert!(HostKind::WezTerm.can_send());
        assert!(!HostKind::WindowsTerminal.can_send());
        assert_eq!(
            serde_json::to_string(&HostKind::WindowsTerminal).unwrap(),
            "\"windows_terminal\""
        );
    }
}
