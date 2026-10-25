// <wtl> project: c4 | file: crates/c4-core/src/observation.rs | author: watchthelight </wtl>
//! What the readers hand to the state machine. Each observation names the session it is about, when
//! it was true, and one typed fact.

use crate::clock::Timestamp;
use crate::ids::{HostKind, ProcessRef, SessionId, SessionKind};
use crate::state::DisconnectReason;
use serde::{Deserialize, Serialize};

/// The `status` field of a registry entry or of an interactive `claude agents --json` entry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum RegistryStatus {
    /// A turn is running.
    Busy,
    /// Nothing pending.
    Idle,
    /// Blocked on the user; `waitingFor` says why.
    Waiting,
    /// A value this version of C4 does not know.
    #[default]
    #[serde(other)]
    Unknown,
}

/// The `state` of a background session in `claude agents --json`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum BgState {
    /// Running.
    Working,
    /// Waiting on a permission decision or input.
    Blocked,
    /// Finished its work and parked.
    Done,
    /// Failed.
    Failed,
    /// Stopped by the supervisor or the user.
    Stopped,
    /// A value this version of C4 does not know.
    #[default]
    #[serde(other)]
    Unknown,
}

/// The `state` of an OSC 7501 report.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum OscState {
    /// Busy.
    Working,
    /// Waiting on the user; see [`OscKind`].
    Blocked,
    /// Finished the last turn.
    Done,
    /// Failed.
    Error,
    /// Nothing happening.
    Idle,
}

/// The `kind` of an OSC 7501 `blocked` report.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum OscKind {
    /// A permission prompt.
    Permission,
    /// A question for the user.
    Question,
    /// A sign-in step.
    Auth,
}

/// How C4 is attached to a session.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Attachment {
    /// C4 owns the terminal.
    Managed,
    /// A verified pane in a terminal host with a control surface.
    InteractiveAttached,
    /// Read-only: data from Claude Code's files, window focus at most.
    #[default]
    Observed,
    /// No live process.
    Disconnected,
}

impl Attachment {
    /// The badge text.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Attachment::Managed => "managed",
            Attachment::InteractiveAttached => "attached",
            Attachment::Observed => "observed",
            Attachment::Disconnected => "disconnected",
        }
    }
}

/// One entry of the `background_tasks` array in a `Stop` hook payload.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct BackgroundTask {
    /// The task id.
    pub id: String,
    /// `shell`, `subagent`, `monitor`, `workflow`, `teammate`, and others.
    #[serde(default, rename = "type")]
    pub kind: String,
    /// The status as reported.
    #[serde(default)]
    pub status: String,
    /// The description as reported.
    #[serde(default)]
    pub description: Option<String>,
}

/// A hook event, reduced to the fields the state machine uses.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum HookEvent {
    /// `SessionStart` with its `source` (`startup`, `resume`, `clear`, `compact`, `fork`).
    SessionStart {
        /// The source field.
        source: String,
    },
    /// `UserPromptSubmit`.
    UserPromptSubmit,
    /// `PreToolUse`.
    PreToolUse {
        /// The tool name.
        tool_name: String,
        /// The tool use id, when present.
        tool_use_id: Option<String>,
    },
    /// `PostToolUse`.
    PostToolUse {
        /// The tool name.
        tool_name: String,
        /// The tool use id, when present.
        tool_use_id: Option<String>,
    },
    /// `PostToolUseFailure`.
    PostToolUseFailure {
        /// The tool name.
        tool_name: String,
        /// The tool use id, when present.
        tool_use_id: Option<String>,
    },
    /// `Notification` with its `notification_type`.
    Notification {
        /// `permission_prompt`, `idle_prompt`, `elicitation_dialog`, `agent_needs_input`, and others.
        notification_type: String,
    },
    /// `SubagentStart`.
    SubagentStart {
        /// The agent id.
        agent_id: String,
        /// The agent type, when present.
        agent_type: Option<String>,
        /// The agent transcript path, when present.
        transcript_path: Option<String>,
    },
    /// `SubagentStop`.
    SubagentStop {
        /// The agent id.
        agent_id: String,
    },
    /// `TaskCreated`.
    TaskCreated {
        /// The task id.
        task_id: String,
        /// The subject, when present.
        task_subject: Option<String>,
    },
    /// `TaskCompleted`.
    TaskCompleted {
        /// The task id.
        task_id: String,
    },
    /// `Stop`: the turn ended.
    Stop {
        /// Tasks still running when the turn ended.
        background_tasks: Vec<BackgroundTask>,
        /// Scheduled crons (not running work).
        session_crons: u32,
    },
    /// `StopFailure`: the turn ended with an API error.
    StopFailure {
        /// The error class (`rate_limit`, `overloaded`, `authentication_failed`, ...).
        error: String,
    },
    /// `PreCompact`.
    PreCompact {
        /// `manual` or `auto`.
        trigger: Option<String>,
    },
    /// `PostCompact`.
    PostCompact,
    /// `CwdChanged`.
    CwdChanged {
        /// The new working directory.
        new_cwd: String,
    },
    /// `ConfigChange`.
    ConfigChange {
        /// Which settings file changed.
        source: Option<String>,
    },
    /// `SessionEnd`.
    SessionEnd {
        /// `clear`, `resume`, `logout`, `prompt_input_exit`, `other`.
        reason: String,
    },
    /// Any other event, kept for the log.
    Other {
        /// The event name.
        name: String,
    },
}

/// A transcript record reduced to what the state machine uses. Transcript facts are observed but
/// can lag behind hooks, so they carry the record's own timestamp.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "record")]
pub enum TranscriptEvent {
    /// A user prompt record.
    UserPrompt,
    /// An assistant record that stopped; `tool_use` means a tool call follows.
    AssistantStopped {
        /// The stop reason.
        stop_reason: Option<String>,
    },
    /// A `tool_use` block.
    ToolUse {
        /// The tool use id.
        tool_use_id: String,
        /// The tool name.
        name: String,
    },
    /// A `tool_result` block.
    ToolResult {
        /// The tool use id it answers.
        tool_use_id: String,
        /// Whether the tool reported an error.
        is_error: bool,
    },
    /// A `turn_duration` system record: the cheapest turn-end marker.
    TurnEnded {
        /// `pendingBackgroundAgentCount`.
        pending_background_agents: u32,
        /// `pendingWorkflowCount`.
        pending_workflows: u32,
    },
    /// An `Agent` tool result with `status: async_launched`.
    AgentLaunched {
        /// The agent id.
        agent_id: String,
        /// Whether it runs in the background.
        is_async: bool,
    },
    /// A `compact_boundary` system record.
    CompactBoundary {
        /// `manual` or `auto`.
        trigger: Option<String>,
    },
    /// An assistant record flagged `isApiErrorMessage`.
    ApiError {
        /// The message text.
        message: String,
    },
}

/// One fact about one session.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Observation {
    /// When the fact was true.
    pub at: Timestamp,
    /// Which session it is about.
    pub session: SessionId,
    /// The fact.
    pub body: ObservationBody,
}

impl Observation {
    /// Build an observation.
    #[must_use]
    pub fn new(session: SessionId, at: Timestamp, body: ObservationBody) -> Self {
        Self { at, session, body }
    }
}

/// The typed facts the readers produce.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "fact")]
pub enum ObservationBody {
    /// A registry entry was read or changed.
    Registry {
        /// The process.
        process: ProcessRef,
        /// Interactive or background.
        session_kind: SessionKind,
        /// The working directory.
        cwd: Option<String>,
        /// The display name.
        name: Option<String>,
        /// `user`, `auto` or `derived`.
        name_source: Option<String>,
        /// The status field.
        status: RegistryStatus,
        /// The `waitingFor` field.
        waiting_for: Option<String>,
        /// The Claude Code version.
        version: Option<String>,
        /// The peer-message socket path.
        socket: Option<String>,
        /// When the process started, from the registry.
        started_at: Option<Timestamp>,
    },
    /// The registry entry for this session vanished.
    RegistryGone,
    /// An entry from `claude agents --json`.
    Agents {
        /// Interactive or background.
        session_kind: SessionKind,
        /// The process, when the entry had one.
        process: Option<ProcessRef>,
        /// The interactive status, when present.
        status: Option<RegistryStatus>,
        /// The `waitingFor` field.
        waiting_for: Option<String>,
        /// The background state, when present.
        bg_state: Option<BgState>,
        /// The working directory.
        cwd: Option<String>,
        /// The display name.
        name: Option<String>,
    },
    /// The process was seen alive.
    ProcessAlive {
        /// The process.
        process: ProcessRef,
    },
    /// The process is gone or its pid now belongs to something else.
    ProcessDead {
        /// The process that was expected.
        process: ProcessRef,
        /// Why it counts as gone.
        reason: DisconnectReason,
    },
    /// A hook event.
    Hook(HookEvent),
    /// An OSC 7501 or OSC 21337 report from a pane C4 renders.
    Osc {
        /// The state.
        state: OscState,
        /// The kind, for `blocked`.
        kind: Option<OscKind>,
        /// The message, decoded.
        message: Option<String>,
    },
    /// A transcript record.
    Transcript(TranscriptEvent),
    /// A managed child process exited.
    ManagedExit {
        /// The exit code, when the platform gave one.
        code: Option<i32>,
    },
    /// The terminal host and attachment were decided.
    Linked {
        /// The host.
        host: HostKind,
        /// The attachment mode.
        attachment: Attachment,
        /// The pane id in the host's own terms.
        pane: Option<String>,
    },
    /// Identity fields from a status line or hook payload.
    Identity {
        /// The working directory.
        cwd: Option<String>,
        /// The session name.
        name: Option<String>,
        /// The transcript path.
        transcript_path: Option<String>,
        /// The Claude Code version.
        version: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_enum_values_do_not_fail() {
        let s: RegistryStatus = serde_json::from_str("\"dreaming\"").unwrap();
        assert_eq!(s, RegistryStatus::Unknown);
        let b: BgState = serde_json::from_str("\"paused\"").unwrap();
        assert_eq!(b, BgState::Unknown);
        let busy: RegistryStatus = serde_json::from_str("\"busy\"").unwrap();
        assert_eq!(busy, RegistryStatus::Busy);
    }

    #[test]
    fn background_task_reads_type_field() {
        let t: BackgroundTask =
            serde_json::from_str(r#"{"id":"t1","type":"subagent","status":"running"}"#).unwrap();
        assert_eq!(t.kind, "subagent");
        assert_eq!(t.description, None);
    }

    #[test]
    fn observation_round_trip() {
        let o = Observation::new(
            SessionId::new("s1"),
            Timestamp::from_millis(10),
            ObservationBody::Hook(HookEvent::PreToolUse {
                tool_name: "Bash".into(),
                tool_use_id: Some("tu1".into()),
            }),
        );
        let json = serde_json::to_string(&o).unwrap();
        let back: Observation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, o);
        assert!(json.contains("\"fact\":\"hook\""));
        assert!(json.contains("\"event\":\"pre_tool_use\""));
    }
}
