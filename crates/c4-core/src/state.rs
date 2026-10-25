// <wtl> project: c4 | file: crates/c4-core/src/state.rs | author: watchthelight </wtl>
//! The nine states the sidebar shows, with their labels, sort weights and attention rules.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Why a session waits for the user rather than for a permission decision.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum InputReason {
    /// The assistant asked a question (the `AskUserQuestion` tool, or a `question` kind over OSC 7501).
    Question,
    /// An MCP elicitation dialog is open.
    Elicitation,
    /// A background agent needs input.
    AgentNeedsInput,
    /// An authentication step is waiting (OSC 7501 `auth`).
    Auth,
    /// A dialog is open (`waitingFor: dialog open`).
    Dialog,
    /// Blocked on the user for a reason the source did not name.
    Other,
}

impl InputReason {
    /// The label shown next to the state.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            InputReason::Question => "question",
            InputReason::Elicitation => "elicitation",
            InputReason::AgentNeedsInput => "agent needs input",
            InputReason::Auth => "sign-in",
            InputReason::Dialog => "dialog",
            InputReason::Other => "input",
        }
    }
}

/// Why a session is shown as disconnected.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectReason {
    /// The process is gone.
    ProcessGone,
    /// The pid is alive but belongs to another process now.
    PidReused,
    /// The registry file vanished and no process could be matched.
    RegistryGone,
    /// The terminal host pane disappeared.
    HostGone,
    /// The managed host process is gone.
    HostExited,
}

impl DisconnectReason {
    /// A short explanation for the sidebar.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            DisconnectReason::ProcessGone => "process exited",
            DisconnectReason::PidReused => "pid reused",
            DisconnectReason::RegistryGone => "registry entry gone",
            DisconnectReason::HostGone => "terminal pane gone",
            DisconnectReason::HostExited => "session host exited",
        }
    }
}

/// The state of a session as the sidebar shows it.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SessionState {
    /// A turn is in progress.
    Working,
    /// A tool call is in progress.
    RunningTool {
        /// The tool name as Claude Code reports it.
        tool: String,
    },
    /// A permission prompt is open.
    WaitingPermission {
        /// What the prompt is about, when known (a tool name, "sandbox request").
        what: Option<String>,
    },
    /// The session needs the user for something other than a permission decision.
    WaitingUserInput {
        /// Why.
        reason: InputReason,
        /// Free text from the source, when it gave one.
        detail: Option<String>,
    },
    /// The turn ended but background tasks or subagents are still running.
    BackgroundAgentsActive {
        /// How many are known to be running.
        count: u32,
    },
    /// Nothing is happening.
    #[default]
    Idle,
    /// Claude Code reported a failure (an API error, a failed background job, a crashed managed child).
    Failed {
        /// The error as reported.
        error: String,
    },
    /// The process or its host is gone without a clean end.
    Disconnected {
        /// Why.
        reason: DisconnectReason,
    },
    /// The session ended cleanly.
    Completed {
        /// The end reason Claude Code gave (`clear`, `resume`, `logout`, `prompt_input_exit`, `other`).
        reason: String,
    },
}

/// The nine states without their payloads, for matching and sorting.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    /// See [`SessionState::WaitingPermission`].
    WaitingPermission,
    /// See [`SessionState::WaitingUserInput`].
    WaitingUserInput,
    /// See [`SessionState::Failed`].
    Failed,
    /// See [`SessionState::Disconnected`].
    Disconnected,
    /// See [`SessionState::RunningTool`].
    RunningTool,
    /// See [`SessionState::BackgroundAgentsActive`].
    BackgroundAgentsActive,
    /// See [`SessionState::Working`].
    Working,
    /// See [`SessionState::Idle`].
    Idle,
    /// See [`SessionState::Completed`].
    Completed,
}

impl StateKind {
    /// All kinds in default attention order, highest first.
    pub const ALL: [StateKind; 9] = [
        StateKind::WaitingPermission,
        StateKind::WaitingUserInput,
        StateKind::Failed,
        StateKind::Disconnected,
        StateKind::RunningTool,
        StateKind::BackgroundAgentsActive,
        StateKind::Working,
        StateKind::Idle,
        StateKind::Completed,
    ];

    /// The label shown in the sidebar.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            StateKind::WaitingPermission => "waiting for permission",
            StateKind::WaitingUserInput => "waiting for input",
            StateKind::Failed => "failed",
            StateKind::Disconnected => "disconnected",
            StateKind::RunningTool => "running a tool",
            StateKind::BackgroundAgentsActive => "background agents active",
            StateKind::Working => "working",
            StateKind::Idle => "idle",
            StateKind::Completed => "completed",
        }
    }

    /// The default sort weight; higher sorts first.
    #[must_use]
    pub fn sort_weight(self) -> u8 {
        match self {
            StateKind::WaitingPermission => 100,
            StateKind::WaitingUserInput => 90,
            StateKind::Failed => 80,
            StateKind::Disconnected => 70,
            StateKind::RunningTool => 40,
            StateKind::BackgroundAgentsActive => 35,
            StateKind::Working => 30,
            StateKind::Idle => 10,
            StateKind::Completed => 0,
        }
    }

    /// True for the states that get the attention stripe and a notification.
    #[must_use]
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            StateKind::WaitingPermission | StateKind::WaitingUserInput | StateKind::Failed
        )
    }

    /// True while the session is still a live process doing or awaiting something.
    #[must_use]
    pub fn is_live(self) -> bool {
        !matches!(self, StateKind::Disconnected | StateKind::Completed)
    }

    /// True for the two states that mean a turn is in progress.
    #[must_use]
    pub fn is_busy(self) -> bool {
        matches!(self, StateKind::Working | StateKind::RunningTool)
    }
}

impl SessionState {
    /// The kind of this state.
    #[must_use]
    pub fn kind(&self) -> StateKind {
        match self {
            SessionState::Working => StateKind::Working,
            SessionState::RunningTool { .. } => StateKind::RunningTool,
            SessionState::WaitingPermission { .. } => StateKind::WaitingPermission,
            SessionState::WaitingUserInput { .. } => StateKind::WaitingUserInput,
            SessionState::BackgroundAgentsActive { .. } => StateKind::BackgroundAgentsActive,
            SessionState::Idle => StateKind::Idle,
            SessionState::Failed { .. } => StateKind::Failed,
            SessionState::Disconnected { .. } => StateKind::Disconnected,
            SessionState::Completed { .. } => StateKind::Completed,
        }
    }

    /// The sidebar label with the payload folded in ("running Bash", "waiting for input: question").
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            SessionState::RunningTool { tool } => format!("running {tool}"),
            SessionState::WaitingPermission { what: Some(what) } => {
                format!("waiting for permission: {what}")
            }
            SessionState::WaitingUserInput { reason, detail } => match detail {
                Some(d) if !d.is_empty() => format!("waiting for input: {} ({d})", reason.label()),
                _ => format!("waiting for input: {}", reason.label()),
            },
            SessionState::BackgroundAgentsActive { count } => {
                format!(
                    "{count} background agent{} active",
                    if *count == 1 { "" } else { "s" }
                )
            }
            SessionState::Failed { error } => format!("failed: {error}"),
            SessionState::Disconnected { reason } => format!("disconnected: {}", reason.label()),
            SessionState::Completed { reason } => format!("completed ({reason})"),
            other => other.kind().label().to_string(),
        }
    }

    /// Shorthand for [`StateKind::sort_weight`].
    #[must_use]
    pub fn sort_weight(&self) -> u8 {
        self.kind().sort_weight()
    }

    /// Shorthand for [`StateKind::needs_attention`].
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.kind().needs_attention()
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_follow_the_attention_order() {
        let weights: Vec<u8> = StateKind::ALL.iter().map(|k| k.sort_weight()).collect();
        let mut sorted = weights.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(weights, sorted);
    }

    #[test]
    fn labels_and_descriptions() {
        assert_eq!(
            SessionState::RunningTool {
                tool: "Bash".into()
            }
            .describe(),
            "running Bash"
        );
        assert_eq!(
            SessionState::WaitingUserInput {
                reason: InputReason::Question,
                detail: None
            }
            .describe(),
            "waiting for input: question"
        );
        assert_eq!(
            SessionState::BackgroundAgentsActive { count: 2 }.describe(),
            "2 background agents active"
        );
        assert_eq!(SessionState::Idle.describe(), "idle");
        assert!(SessionState::WaitingPermission { what: None }.needs_attention());
        assert!(!SessionState::Working.needs_attention());
        assert!(!StateKind::Completed.is_live());
    }

    #[test]
    fn serializes_tagged() {
        let s = SessionState::RunningTool {
            tool: "Read".into(),
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["state"], "running_tool");
        assert_eq!(json["tool"], "Read");
        let back: SessionState = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }
}
