// <wtl> project: c4 | file: crates/c4-core/src/view.rs | author: watchthelight </wtl>
//! What the sidebar and the inspector render for one session.

use crate::clock::Timestamp;
use crate::evidence::{Evidence, Source};
use crate::ids::{AgentId, HostKind, ProcessRef, SessionId, SessionKind};
use crate::observation::Attachment;
use crate::state::SessionState;
use serde::{Deserialize, Serialize};

/// A subagent the session launched.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct AgentInfo {
    /// The agent id.
    pub id: AgentId,
    /// The agent type (`Explore`, `Plan`, `workflow-subagent`, ...), when known.
    pub agent_type: Option<String>,
    /// The description Claude Code gave, when known.
    pub description: Option<String>,
    /// The agent transcript path, when known.
    pub transcript_path: Option<String>,
    /// When it started.
    pub started_at: Timestamp,
    /// When it stopped, if it has.
    pub ended_at: Option<Timestamp>,
    /// True once a launch record or a `SubagentStart` hook said the agent ran. False when the only
    /// evidence is its transcript file, which proves it existed and nothing more.
    pub observed_running: bool,
}

impl AgentInfo {
    /// True while the agent was seen running and has not stopped.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.observed_running && self.ended_at.is_none()
    }
}

/// A background task (shell, monitor, workflow, teammate) from the `Stop` payload or the task hooks.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct TaskInfo {
    /// The task id.
    pub id: String,
    /// The task kind as reported.
    pub kind: String,
    /// The status as reported.
    pub status: String,
    /// The description or subject.
    pub description: Option<String>,
    /// When C4 first saw it.
    pub created_at: Timestamp,
    /// When it completed, if it has.
    pub completed_at: Option<Timestamp>,
}

/// The tool call in progress.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ToolInfo {
    /// The tool name.
    pub name: String,
    /// The tool use id, when known.
    pub tool_use_id: Option<String>,
    /// When it started.
    pub since: Timestamp,
}

/// Everything C4 knows about one session.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct SessionView {
    /// The durable key.
    pub id: SessionId,
    /// Interactive or background.
    pub session_kind: SessionKind,
    /// The process, when one is known.
    pub process: Option<ProcessRef>,
    /// Whether the process was alive at the last check.
    pub alive: bool,
    /// The working directory.
    pub cwd: Option<String>,
    /// The display name, when Claude Code has one.
    pub name: Option<String>,
    /// Where the name came from (`user`, `auto`, `derived`).
    pub name_source: Option<String>,
    /// The Claude Code version.
    pub version: Option<String>,
    /// The terminal host.
    pub host: HostKind,
    /// How C4 is attached.
    pub attachment: Attachment,
    /// The pane id in the host's terms.
    pub pane: Option<String>,
    /// The state shown in the sidebar.
    pub state: SessionState,
    /// What produced the state.
    pub evidence: Evidence,
    /// True while no hook telemetry has arrived for this session.
    pub coarse: bool,
    /// True once any hook event with this session id arrived.
    pub hooks_seen: bool,
    /// True once an OSC 7501 report arrived from a pane C4 renders.
    pub osc_seen: bool,
    /// Set by the timer when a busy session has produced nothing for a while. A guess, never a state.
    pub stalled: bool,
    /// The tool call in progress.
    pub current_tool: Option<ToolInfo>,
    /// Subagents, running and finished, in start order.
    pub agents: Vec<AgentInfo>,
    /// Background tasks known from the `Stop` payload and the task hooks.
    pub background_tasks: Vec<TaskInfo>,
    /// `pendingBackgroundAgentCount` from the last `turn_duration` record.
    pub pending_background_agents: u32,
    /// `pendingWorkflowCount` from the last `turn_duration` record.
    pub pending_workflows: u32,
    /// The short job id of a background session, for `claude attach` and friends.
    pub daemon_short: Option<String>,
    /// Tasks the background supervisor counts as in flight.
    pub daemon_in_flight: u32,
    /// The supervisor's detail line for a background session.
    pub daemon_detail: Option<String>,
    /// Compactions seen.
    pub compactions: u32,
    /// When C4 first saw the session.
    pub first_seen: Timestamp,
    /// When the process started, from the registry.
    pub started_at: Option<Timestamp>,
    /// The last time any fact arrived.
    pub last_activity: Timestamp,
    /// When the current state began.
    pub state_since: Timestamp,
    /// When the session started needing attention, for the attention sort.
    pub waiting_since: Option<Timestamp>,
    /// `SessionEnd` was seen.
    pub ended: bool,
    /// The peer-message socket path, when the registry had one.
    pub peer_socket: Option<String>,
    /// The transcript path, when known.
    pub transcript_path: Option<String>,
    /// Bumped on every change, so consumers can skip unchanged views.
    pub seq: u64,
}

impl SessionView {
    /// A view for a session seen for the first time: idle, coarse, observed, with heuristic evidence.
    #[must_use]
    pub fn new(id: SessionId, now: Timestamp) -> Self {
        Self {
            id,
            session_kind: SessionKind::Interactive,
            process: None,
            alive: false,
            cwd: None,
            name: None,
            name_source: None,
            version: None,
            host: HostKind::Unknown,
            attachment: Attachment::Observed,
            pane: None,
            state: SessionState::Idle,
            evidence: Evidence::heuristic(Source::Timer, now),
            coarse: true,
            hooks_seen: false,
            osc_seen: false,
            stalled: false,
            current_tool: None,
            agents: Vec::new(),
            background_tasks: Vec::new(),
            pending_background_agents: 0,
            pending_workflows: 0,
            daemon_short: None,
            daemon_in_flight: 0,
            daemon_detail: None,
            compactions: 0,
            first_seen: now,
            started_at: None,
            last_activity: now,
            state_since: now,
            waiting_since: None,
            ended: false,
            peer_socket: None,
            transcript_path: None,
            seq: 0,
        }
    }

    /// The name to show: Claude Code's name, else the last path segment of the cwd, else the short id.
    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|n| !n.trim().is_empty()) {
            return name.trim().to_string();
        }
        if let Some(tail) = self.cwd_tail() {
            return tail;
        }
        self.id.short().to_string()
    }

    /// The last path segment of the cwd.
    #[must_use]
    pub fn cwd_tail(&self) -> Option<String> {
        let cwd = self.cwd.as_deref()?.trim_end_matches(['/', '\\']);
        let tail = cwd.rsplit(['/', '\\']).next().filter(|t| !t.is_empty())?;
        Some(tail.to_string())
    }

    /// Subagents that have not stopped.
    #[must_use]
    pub fn running_agents(&self) -> usize {
        self.agents.iter().filter(|a| a.is_running()).count()
    }

    /// Background tasks that have not completed.
    #[must_use]
    pub fn running_tasks(&self) -> usize {
        self.background_tasks
            .iter()
            .filter(|t| t.completed_at.is_none())
            .count()
    }

    /// How much work is known to be in flight after a turn ends: running tasks, running agents,
    /// whatever the last turn record counted as pending, and the supervisor's own count.
    #[must_use]
    pub fn in_flight(&self) -> u32 {
        let known = self.running_tasks().max(self.running_agents());
        (known as u32)
            .max(self.pending_background_agents + self.pending_workflows)
            .max(self.daemon_in_flight)
    }

    /// The attention sort key: higher weight first, then the longest wait, then the most recent
    /// activity. Pinning is a UI concern and sorts above this.
    #[must_use]
    pub fn sort_key(&self) -> (u8, i64, i64) {
        (
            u8::MAX - self.state.sort_weight(),
            self.waiting_since.map_or(i64::MAX, Timestamp::millis),
            -self.last_activity.millis(),
        )
    }

    /// True when the view represents a live process.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.alive && self.state.kind().is_live()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_falls_back() {
        let mut v = SessionView::new(
            SessionId::new("a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d"),
            Timestamp::from_millis(0),
        );
        assert_eq!(v.display_name(), "a1b2c3d4");
        v.cwd = Some("C:\\work\\proj\\".into());
        assert_eq!(v.display_name(), "proj");
        v.cwd = Some("/home/example/repo".into());
        assert_eq!(v.display_name(), "repo");
        v.name = Some("  nightly build ".into());
        assert_eq!(v.display_name(), "nightly build");
    }

    #[test]
    fn sort_key_puts_waiting_first() {
        let now = Timestamp::from_millis(1000);
        let mut a = SessionView::new(SessionId::new("a"), now);
        let mut b = SessionView::new(SessionId::new("b"), now);
        a.state = SessionState::WaitingPermission { what: None };
        a.waiting_since = Some(Timestamp::from_millis(500));
        b.state = SessionState::Working;
        assert!(a.sort_key() < b.sort_key());
        let mut c = a.clone();
        c.waiting_since = Some(Timestamp::from_millis(100));
        assert!(c.sort_key() < a.sort_key(), "the longer wait sorts first");
    }
}
