// <wtl> project: c4 | file: crates/c4-core/src/reduce.rs | author: watchthelight </wtl>
//! The state machine. `reduce` folds one observation into a session view and reports the transition
//! it caused, if any. The rules that keep the sidebar honest live here:
//!
//! 1. Failed never comes from silence. Only Claude Code's own failure report, a background job in
//!    state `failed`, a non-zero exit of a managed child or an API error record produce it.
//! 2. Completed requires a session end or a process exit, and no known in-flight work.
//! 3. A heuristic never overrides an observed fact younger than [`HEURISTIC_GUARD_MS`].
//! 4. Without hook telemetry a session runs on the registry, the transcript and the terminal
//!    protocol alone and stays marked coarse.
//!
//! Observations carry their source's own timestamp. One that is older than the evidence behind the
//! current state is ignored, which is how a lagging transcript never undoes a fresh hook event.

use crate::clock::Timestamp;
use crate::evidence::{Evidence, Source};
use crate::ids::{AgentId, SessionKind};
use crate::observation::{
    Attachment, BackgroundTask, BgState, HookEvent, Observation, ObservationBody, OscKind,
    OscState, RegistryStatus, TranscriptEvent,
};
use crate::state::{DisconnectReason, InputReason, SessionState, StateKind};
use crate::view::{AgentInfo, SessionView, TaskInfo, ToolInfo};
use serde::{Deserialize, Serialize};

/// How long an observed fact protects the state from heuristics.
pub const HEURISTIC_GUARD_MS: i64 = 30_000;
/// How long a busy session may be silent before the stalled badge appears.
pub const STALL_AFTER_MS: i64 = 10 * 60_000;
/// Tolerance for clocks that disagree slightly between sources.
const CLOCK_SKEW_MS: i64 = 1_000;

/// A state change.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Transition {
    /// The state before.
    pub from: SessionState,
    /// The state after.
    pub to: SessionState,
    /// What caused it.
    pub evidence: Evidence,
}

/// Fold one observation into the view. Returns the transition when the sidebar state changed.
pub fn reduce(view: &mut SessionView, obs: &Observation) -> Option<Transition> {
    debug_assert_eq!(view.id, obs.session, "an observation for another session");
    view.last_activity = view.last_activity.max(obs.at);
    view.seq = view.seq.wrapping_add(1);
    let at = obs.at;
    match &obs.body {
        ObservationBody::Registry {
            process,
            session_kind,
            cwd,
            name,
            name_source,
            status,
            waiting_for,
            version,
            socket,
            started_at,
        } => {
            view.process = Some(*process);
            view.alive = true;
            view.session_kind = *session_kind;
            if cwd.is_some() {
                view.cwd.clone_from(cwd);
            }
            if name.is_some() {
                view.name.clone_from(name);
                view.name_source.clone_from(name_source);
            }
            if version.is_some() {
                view.version.clone_from(version);
            }
            if socket.is_some() {
                view.peer_socket.clone_from(socket);
            }
            if started_at.is_some() {
                view.started_at = *started_at;
            }
            if view.attachment == Attachment::Disconnected {
                view.attachment = Attachment::Observed;
            }
            let ev = Evidence::observed(Source::Registry, at);
            match status {
                RegistryStatus::Busy => set_state(view, busy_state(view), ev),
                RegistryStatus::Idle => set_state(view, rest_state(view), ev),
                RegistryStatus::Waiting => {
                    set_state(view, waiting_state(waiting_for.as_deref()), ev)
                }
                RegistryStatus::Unknown => None,
            }
        }
        ObservationBody::RegistryGone => {
            if view.alive {
                None
            } else {
                set_state(
                    view,
                    SessionState::Disconnected {
                        reason: DisconnectReason::RegistryGone,
                    },
                    Evidence::heuristic(Source::Registry, at),
                )
            }
        }
        ObservationBody::Agents {
            session_kind,
            process,
            status,
            waiting_for,
            bg_state,
            cwd,
            name,
        } => {
            view.session_kind = *session_kind;
            if let Some(p) = process
                && view.process.is_none()
            {
                view.process = Some(*p);
            }
            if view.cwd.is_none() && cwd.is_some() {
                view.cwd.clone_from(cwd);
            }
            if view.name.is_none() && name.is_some() {
                view.name.clone_from(name);
            }
            let ev = Evidence::observed(Source::AgentsJson, at);
            if let Some(status) = status {
                return match status {
                    RegistryStatus::Busy => set_state(view, busy_state(view), ev),
                    RegistryStatus::Idle => set_state(view, rest_state(view), ev),
                    RegistryStatus::Waiting => {
                        set_state(view, waiting_state(waiting_for.as_deref()), ev)
                    }
                    RegistryStatus::Unknown => None,
                };
            }
            let waiting = Some(waiting_state(waiting_for.as_deref()));
            apply_bg_state(view, *bg_state, waiting, ev)
        }
        ObservationBody::Daemon {
            short,
            process,
            state,
            tempo: _,
            detail,
            needs,
            in_flight,
            cwd,
            name,
            name_source,
            version,
        } => {
            view.session_kind = SessionKind::Background;
            view.daemon_short = Some(short.clone());
            view.daemon_in_flight = *in_flight;
            view.daemon_detail.clone_from(detail);
            if let Some(p) = process
                && view.process.is_none()
            {
                view.process = Some(*p);
            }
            if view.cwd.is_none() && cwd.is_some() {
                view.cwd.clone_from(cwd);
            }
            if name.is_some() && view.name_source.as_deref() != Some("user") {
                view.name.clone_from(name);
                view.name_source.clone_from(name_source);
            }
            if view.version.is_none() && version.is_some() {
                view.version.clone_from(version);
            }
            let ev = Evidence::observed(Source::Daemon, at);
            let waiting = needs
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(daemon_waiting_state);
            apply_bg_state(view, Some(*state), waiting, ev)
        }
        ObservationBody::ProcessAlive { process } => {
            view.process = Some(*process);
            view.alive = true;
            if matches!(view.state.kind(), StateKind::Disconnected) {
                view.attachment = Attachment::Observed;
                set_state(
                    view,
                    rest_state(view),
                    Evidence::heuristic(Source::Process, at),
                )
            } else {
                None
            }
        }
        ObservationBody::ProcessDead { process, reason } => {
            let mismatched = match (view.process, process) {
                (Some(known), Some(reported)) => !known.same_process(reported),
                // A report naming no process cannot override a process this view knows.
                (Some(_), None) => true,
                (None, _) => false,
            };
            if mismatched && view.alive {
                // A report about a process this view no longer refers to.
                return None;
            }
            view.alive = false;
            view.current_tool = None;
            close_everything(view, at);
            let ev = Evidence::observed(Source::Process, at);
            if view.ended {
                let reason = completion_reason(view);
                set_state(view, SessionState::Completed { reason }, ev)
            } else if view.session_kind == SessionKind::Background
                && matches!(view.state.kind(), StateKind::Idle | StateKind::Completed)
            {
                // The supervisor stops idle workers; a parked worker is not a broken one.
                None
            } else {
                view.attachment = Attachment::Disconnected;
                set_state(view, SessionState::Disconnected { reason: *reason }, ev)
            }
        }
        ObservationBody::Superseded { process, by } => {
            if view
                .process
                .is_some_and(|known| !known.same_process(process))
            {
                // About a process this view no longer refers to.
                return None;
            }
            view.alive = false;
            view.ended = true;
            view.current_tool = None;
            close_everything(view, at);
            set_state(
                view,
                SessionState::Completed {
                    reason: format!("continued in {by}"),
                },
                Evidence::observed(Source::Registry, at),
            )
        }
        ObservationBody::Hook(event) => {
            view.hooks_seen = true;
            view.coarse = false;
            view.alive = true;
            reduce_hook(view, event, at)
        }
        ObservationBody::Osc {
            state,
            kind,
            message,
        } => {
            view.osc_seen = true;
            view.alive = true;
            let ev = Evidence::observed(Source::Osc7501, at);
            match state {
                OscState::Working => set_state(view, busy_state(view), ev),
                OscState::Blocked => {
                    let next = match kind {
                        Some(OscKind::Permission) => SessionState::WaitingPermission {
                            what: message.clone(),
                        },
                        Some(OscKind::Question) => SessionState::WaitingUserInput {
                            reason: InputReason::Question,
                            detail: message.clone(),
                        },
                        Some(OscKind::Auth) => SessionState::WaitingUserInput {
                            reason: InputReason::Auth,
                            detail: message.clone(),
                        },
                        None => SessionState::WaitingUserInput {
                            reason: InputReason::Other,
                            detail: message.clone(),
                        },
                    };
                    set_state(view, next, ev)
                }
                OscState::Done | OscState::Idle => set_state(view, rest_state(view), ev),
                OscState::Error => set_state(
                    view,
                    SessionState::Failed {
                        error: message
                            .clone()
                            .unwrap_or_else(|| "error reported by Claude Code".into()),
                    },
                    ev,
                ),
            }
        }
        ObservationBody::Transcript(event) => reduce_transcript(view, event, at),
        ObservationBody::ManagedExit { code } => {
            view.alive = false;
            view.current_tool = None;
            close_everything(view, at);
            let ev = Evidence::observed(Source::Managed, at);
            match code {
                Some(c) if *c != 0 && !view.ended => set_state(
                    view,
                    SessionState::Failed {
                        error: format!("exited with code {c}"),
                    },
                    ev,
                ),
                _ => {
                    view.ended = true;
                    let reason = completion_reason(view);
                    set_state(view, SessionState::Completed { reason }, ev)
                }
            }
        }
        ObservationBody::Linked {
            host,
            attachment,
            pane,
        } => {
            view.host = *host;
            view.attachment = *attachment;
            view.pane.clone_from(pane);
            None
        }
        ObservationBody::Identity {
            cwd,
            name,
            transcript_path,
            version,
        } => {
            if cwd.is_some() {
                view.cwd.clone_from(cwd);
            }
            if name.is_some() && view.name_source.as_deref() != Some("user") {
                view.name.clone_from(name);
            }
            if transcript_path.is_some() {
                view.transcript_path.clone_from(transcript_path);
            }
            if version.is_some() {
                view.version.clone_from(version);
            }
            None
        }
        ObservationBody::Statusline {
            model,
            context_used_pct,
            context_size,
            effort,
        } => {
            if view.statusline_at.is_some_and(|last| at < last) {
                // A replayed payload older than the last one: its numbers are history.
                return None;
            }
            view.statusline_at = Some(at);
            if model.is_some() {
                view.model.clone_from(model);
            }
            if context_used_pct.is_some() {
                view.context_pct = *context_used_pct;
            }
            if context_size.is_some() {
                view.context_size = *context_size;
            }
            if effort.is_some() {
                view.effort.clone_from(effort);
            }
            if view.ended {
                return None;
            }
            // The status line runs inside the process, so the process is alive. What it is doing
            // is not in the payload: a disconnected view gets a guessed Idle, like a sighting.
            view.alive = true;
            if matches!(view.state.kind(), StateKind::Disconnected) {
                view.attachment = Attachment::Observed;
                set_state(
                    view,
                    rest_state(view),
                    Evidence::heuristic(Source::Statusline, at),
                )
            } else {
                None
            }
        }
    }
}

/// Apply the clock: set or clear the stalled badge. Returns true when the view changed.
pub fn tick(view: &mut SessionView, now: Timestamp) -> bool {
    let busy = view.state.kind().is_busy() && view.alive;
    let stalled = busy && now.millis_since(view.last_activity) > STALL_AFTER_MS;
    if stalled != view.stalled {
        view.stalled = stalled;
        view.seq = view.seq.wrapping_add(1);
        return true;
    }
    false
}

fn reduce_hook(view: &mut SessionView, event: &HookEvent, at: Timestamp) -> Option<Transition> {
    let name = hook_name(event);
    let ev = Evidence::observed(Source::Hook(name.to_string()), at);
    match event {
        HookEvent::SessionStart { .. } => {
            view.ended = false;
            if view.attachment == Attachment::Disconnected {
                view.attachment = Attachment::Observed;
            }
            match view.state.kind() {
                StateKind::Disconnected | StateKind::Completed | StateKind::Failed => {
                    set_state(view, SessionState::Idle, ev)
                }
                _ => None,
            }
        }
        HookEvent::UserPromptSubmit => {
            view.current_tool = None;
            set_state(view, SessionState::Working, ev)
        }
        HookEvent::PreToolUse {
            tool_name,
            tool_use_id,
        } => {
            view.current_tool = Some(ToolInfo {
                name: tool_name.clone(),
                tool_use_id: tool_use_id.clone(),
                since: at,
            });
            if tool_name == "AskUserQuestion" {
                return set_state(
                    view,
                    SessionState::WaitingUserInput {
                        reason: InputReason::Question,
                        detail: None,
                    },
                    ev,
                );
            }
            set_state(
                view,
                SessionState::RunningTool {
                    tool: tool_name.clone(),
                },
                ev,
            )
        }
        HookEvent::PostToolUse { .. } | HookEvent::PostToolUseFailure { .. } => {
            view.current_tool = None;
            set_state(view, SessionState::Working, ev)
        }
        HookEvent::Notification { notification_type } => match notification_type.as_str() {
            "permission_prompt" | "worker_permission_prompt" => set_state(
                view,
                SessionState::WaitingPermission {
                    what: view.current_tool.as_ref().map(|t| t.name.clone()),
                },
                ev,
            ),
            "elicitation_dialog" => set_state(
                view,
                SessionState::WaitingUserInput {
                    reason: InputReason::Elicitation,
                    detail: None,
                },
                ev,
            ),
            "agent_needs_input" => set_state(
                view,
                SessionState::WaitingUserInput {
                    reason: InputReason::AgentNeedsInput,
                    detail: None,
                },
                ev,
            ),
            // The idle prompt fires after a minute of nothing; it confirms idleness, it is not a request.
            "idle_prompt" => set_state(view, rest_state(view), ev),
            "elicitation_complete" | "elicitation_response" | "auth_success" => {
                if view.state.kind().needs_attention() {
                    set_state(view, busy_state(view), ev)
                } else {
                    None
                }
            }
            _ => None,
        },
        HookEvent::SubagentStart {
            agent_id,
            agent_type,
            transcript_path,
        } => {
            let id = AgentId::new(agent_id.clone());
            if let Some(existing) = view.agents.iter_mut().find(|a| a.id == id) {
                existing.ended_at = None;
                existing.observed_running = true;
                if agent_type.is_some() {
                    existing.agent_type.clone_from(agent_type);
                }
            } else {
                view.agents.push(AgentInfo {
                    id,
                    agent_type: agent_type.clone(),
                    description: None,
                    transcript_path: transcript_path.clone(),
                    started_at: at,
                    ended_at: None,
                    observed_running: true,
                });
            }
            None
        }
        HookEvent::SubagentStop { agent_id } => {
            let id = AgentId::new(agent_id.clone());
            if let Some(agent) = view.agents.iter_mut().find(|a| a.id == id) {
                agent.ended_at = Some(at);
            }
            if matches!(view.state.kind(), StateKind::BackgroundAgentsActive) {
                set_state(view, rest_state(view), ev)
            } else {
                None
            }
        }
        HookEvent::TaskCreated {
            task_id,
            task_subject,
        } => {
            if !view.background_tasks.iter().any(|t| &t.id == task_id) {
                view.background_tasks.push(TaskInfo {
                    id: task_id.clone(),
                    kind: "task".into(),
                    status: "running".into(),
                    description: task_subject.clone(),
                    created_at: at,
                    completed_at: None,
                });
            }
            None
        }
        HookEvent::TaskCompleted { task_id } => {
            if let Some(task) = view.background_tasks.iter_mut().find(|t| &t.id == task_id) {
                task.completed_at = Some(at);
                task.status = "completed".into();
            }
            if matches!(view.state.kind(), StateKind::BackgroundAgentsActive) {
                set_state(view, rest_state(view), ev)
            } else {
                None
            }
        }
        HookEvent::Stop {
            background_tasks, ..
        } => {
            view.current_tool = None;
            merge_tasks(view, background_tasks, at);
            set_state(view, rest_state(view), ev)
        }
        HookEvent::StopFailure { error } => {
            view.current_tool = None;
            set_state(
                view,
                SessionState::Failed {
                    error: error.clone(),
                },
                ev,
            )
        }
        HookEvent::PreCompact { .. } => set_state(view, SessionState::Working, ev),
        HookEvent::PostCompact => {
            view.compactions = view.compactions.saturating_add(1);
            None
        }
        HookEvent::CwdChanged { new_cwd } => {
            view.cwd = Some(new_cwd.clone());
            None
        }
        HookEvent::ConfigChange { .. } | HookEvent::Other { .. } => None,
        HookEvent::SessionEnd { reason } => {
            view.ended = true;
            view.current_tool = None;
            if view.in_flight() > 0 && !matches!(reason.as_str(), "clear" | "resume") {
                // Rule 2: a turn can end while tasks run; completion waits for them or for the exit.
                let count = view.in_flight();
                return set_state(view, SessionState::BackgroundAgentsActive { count }, ev);
            }
            set_state(
                view,
                SessionState::Completed {
                    reason: reason.clone(),
                },
                ev,
            )
        }
    }
}

fn reduce_transcript(
    view: &mut SessionView,
    event: &TranscriptEvent,
    at: Timestamp,
) -> Option<Transition> {
    let ev = Evidence::observed(Source::Transcript, at);
    match event {
        TranscriptEvent::UserPrompt => set_state(view, SessionState::Working, ev),
        TranscriptEvent::AssistantStopped { stop_reason } => match stop_reason.as_deref() {
            Some("tool_use") => None, // the ToolUse record that follows names the tool
            Some("end_turn" | "stop_sequence") => set_state(view, rest_state(view), ev),
            Some("refusal") => set_state(view, rest_state(view), ev),
            _ => None,
        },
        TranscriptEvent::ToolUse { tool_use_id, name } => {
            view.current_tool = Some(ToolInfo {
                name: name.clone(),
                tool_use_id: Some(tool_use_id.clone()),
                since: at,
            });
            if name == "AskUserQuestion" {
                return set_state(
                    view,
                    SessionState::WaitingUserInput {
                        reason: InputReason::Question,
                        detail: None,
                    },
                    ev,
                );
            }
            set_state(view, SessionState::RunningTool { tool: name.clone() }, ev)
        }
        TranscriptEvent::ToolResult { .. } => {
            view.current_tool = None;
            set_state(view, SessionState::Working, ev)
        }
        TranscriptEvent::TurnEnded {
            pending_background_agents,
            pending_workflows,
        } => {
            view.pending_background_agents = *pending_background_agents;
            view.pending_workflows = *pending_workflows;
            view.current_tool = None;
            if *pending_background_agents == 0 {
                // Claude Code's own count says nothing it launched is still running.
                for agent in &mut view.agents {
                    if agent.is_running() {
                        agent.ended_at = Some(at);
                    }
                }
            }
            set_state(view, rest_state(view), ev)
        }
        TranscriptEvent::AgentLaunched { agent_id, is_async } => {
            // A background launch is recorded when the agent starts. A foreground one is recorded
            // when the agent has already returned, so that record also says it ended.
            let id = AgentId::new(agent_id.clone());
            let ended_at = if *is_async { None } else { Some(at) };
            if let Some(existing) = view.agents.iter_mut().find(|a| a.id == id) {
                existing.observed_running = true;
                if existing.ended_at.is_none() {
                    existing.ended_at = ended_at;
                }
            } else {
                view.agents.push(AgentInfo {
                    id,
                    agent_type: None,
                    description: None,
                    transcript_path: None,
                    started_at: at,
                    ended_at,
                    observed_running: true,
                });
            }
            None
        }
        TranscriptEvent::CompactBoundary { .. } => {
            view.compactions = view.compactions.saturating_add(1);
            None
        }
        TranscriptEvent::ApiError { message } => set_state(
            view,
            SessionState::Failed {
                error: message.clone(),
            },
            ev,
        ),
        TranscriptEvent::AgentSeen {
            agent_id,
            agent_type,
            description,
            transcript_path,
        } => {
            let id = AgentId::new(agent_id.clone());
            if let Some(existing) = view.agents.iter_mut().find(|a| a.id == id) {
                if existing.agent_type.is_none() {
                    existing.agent_type.clone_from(agent_type);
                }
                if existing.description.is_none() {
                    existing.description.clone_from(description);
                }
                if existing.transcript_path.is_none() {
                    existing.transcript_path.clone_from(transcript_path);
                }
            } else {
                view.agents.push(AgentInfo {
                    id,
                    agent_type: agent_type.clone(),
                    description: description.clone(),
                    transcript_path: transcript_path.clone(),
                    started_at: at,
                    ended_at: None,
                    observed_running: false,
                });
            }
            None
        }
    }
}

/// The state for "a turn is running": keeps the tool detail when one is in progress.
fn busy_state(view: &SessionView) -> SessionState {
    match &view.current_tool {
        Some(tool) if matches!(view.state.kind(), StateKind::RunningTool) => {
            SessionState::RunningTool {
                tool: tool.name.clone(),
            }
        }
        _ => SessionState::Working,
    }
}

/// The state for "nothing is running on the main thread": background work keeps the session out of
/// Idle (rule 2).
fn rest_state(view: &SessionView) -> SessionState {
    let count = view.in_flight();
    if count > 0 {
        SessionState::BackgroundAgentsActive { count }
    } else {
        SessionState::Idle
    }
}

/// Map a background job state, from the supervisor files or `claude agents --json`, to a transition.
/// `waiting` is the state to show while the job is blocked.
fn apply_bg_state(
    view: &mut SessionView,
    bg_state: Option<BgState>,
    waiting: Option<SessionState>,
    ev: Evidence,
) -> Option<Transition> {
    match bg_state {
        Some(BgState::Working) => set_state(view, SessionState::Working, ev),
        Some(BgState::Blocked) => {
            let next = waiting.unwrap_or_else(|| waiting_state(None));
            set_state(view, next, ev)
        }
        Some(BgState::Done) => set_state(view, rest_state(view), ev),
        Some(BgState::Failed) => set_state(
            view,
            SessionState::Failed {
                error: "background job failed".into(),
            },
            ev,
        ),
        Some(BgState::Stopped) => {
            view.ended = true;
            set_state(
                view,
                SessionState::Completed {
                    reason: "stopped".into(),
                },
                ev,
            )
        }
        Some(BgState::Unknown) | None => None,
    }
}

/// Map the supervisor's free-text `needs` line to a waiting state. The whole line is kept as the
/// detail; a permission request is recognized by its wording.
fn daemon_waiting_state(needs: &str) -> SessionState {
    let lower = needs.to_ascii_lowercase();
    if lower.contains("approve") || lower.contains("permission") {
        SessionState::WaitingPermission {
            what: Some(needs.to_string()),
        }
    } else {
        SessionState::WaitingUserInput {
            reason: InputReason::Other,
            detail: Some(needs.to_string()),
        }
    }
}

/// Map a `waitingFor` string to a waiting state.
fn waiting_state(waiting_for: Option<&str>) -> SessionState {
    let text = waiting_for.unwrap_or("").trim();
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("approve ") || lower.contains("permission") {
        let what = text
            .strip_prefix("approve ")
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        return SessionState::WaitingPermission { what };
    }
    if lower.contains("sandbox") || lower.contains("worker") {
        return SessionState::WaitingPermission {
            what: Some(text.to_string()),
        };
    }
    let reason = if lower.contains("input") {
        InputReason::Question
    } else if lower.contains("dialog") {
        InputReason::Dialog
    } else {
        InputReason::Other
    };
    let detail = if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    };
    SessionState::WaitingUserInput { reason, detail }
}

fn merge_tasks(view: &mut SessionView, tasks: &[BackgroundTask], at: Timestamp) {
    for running in &mut view.background_tasks {
        if running.completed_at.is_none() && !tasks.iter().any(|t| t.id == running.id) {
            running.completed_at = Some(at);
            running.status = "completed".into();
        }
    }
    for task in tasks {
        if let Some(known) = view.background_tasks.iter_mut().find(|t| t.id == task.id) {
            known.status.clone_from(&task.status);
            known.completed_at = None;
            if task.description.is_some() {
                known.description.clone_from(&task.description);
            }
        } else {
            view.background_tasks.push(TaskInfo {
                id: task.id.clone(),
                kind: task.kind.clone(),
                status: task.status.clone(),
                description: task.description.clone(),
                created_at: at,
                completed_at: None,
            });
        }
    }
}

/// When the process is gone, nothing inside it is still running.
fn close_everything(view: &mut SessionView, at: Timestamp) {
    for agent in &mut view.agents {
        if agent.ended_at.is_none() {
            agent.ended_at = Some(at);
        }
    }
    for task in &mut view.background_tasks {
        if task.completed_at.is_none() {
            task.completed_at = Some(at);
            task.status = "ended with the session".into();
        }
    }
    view.pending_background_agents = 0;
    view.pending_workflows = 0;
}

fn completion_reason(view: &SessionView) -> String {
    match &view.state {
        SessionState::Completed { reason } => reason.clone(),
        _ => "exited".into(),
    }
}

fn hook_name(event: &HookEvent) -> &'static str {
    match event {
        HookEvent::SessionStart { .. } => "SessionStart",
        HookEvent::UserPromptSubmit => "UserPromptSubmit",
        HookEvent::PreToolUse { .. } => "PreToolUse",
        HookEvent::PostToolUse { .. } => "PostToolUse",
        HookEvent::PostToolUseFailure { .. } => "PostToolUseFailure",
        HookEvent::Notification { .. } => "Notification",
        HookEvent::SubagentStart { .. } => "SubagentStart",
        HookEvent::SubagentStop { .. } => "SubagentStop",
        HookEvent::TaskCreated { .. } => "TaskCreated",
        HookEvent::TaskCompleted { .. } => "TaskCompleted",
        HookEvent::Stop { .. } => "Stop",
        HookEvent::StopFailure { .. } => "StopFailure",
        HookEvent::PreCompact { .. } => "PreCompact",
        HookEvent::PostCompact => "PostCompact",
        HookEvent::CwdChanged { .. } => "CwdChanged",
        HookEvent::ConfigChange { .. } => "ConfigChange",
        HookEvent::SessionEnd { .. } => "SessionEnd",
        HookEvent::Other { .. } => "Other",
    }
}

/// The one place a state changes. Enforces ordering, the heuristic guard and the attention clock.
fn set_state(view: &mut SessionView, next: SessionState, evidence: Evidence) -> Option<Transition> {
    if evidence.at.plus_millis(CLOCK_SKEW_MS) < view.evidence.at && view.evidence.is_observed() {
        return None; // older than what we already know
    }
    if !evidence.is_observed()
        && view.evidence.is_observed()
        && evidence.at.millis_since(view.evidence.at) < HEURISTIC_GUARD_MS
    {
        return None; // rule 3
    }
    if view.state == next {
        // The same state again: keep the newer evidence, except that an observed fact always
        // replaces a heuristic, however the clocks compare. A fresh view starts as a heuristic
        // Idle from the clock, and a registry record stamped before the view existed is still the
        // better source for it.
        if evidence.at >= view.evidence.at
            || (evidence.is_observed() && !view.evidence.is_observed())
        {
            view.evidence = evidence;
        }
        return None;
    }
    let from = std::mem::replace(&mut view.state, next.clone());
    if from.kind() != next.kind() {
        view.state_since = evidence.at;
        view.waiting_since = if next.needs_attention() {
            Some(evidence.at)
        } else {
            None
        };
    }
    if next.kind().is_busy() {
        view.stalled = false;
    }
    view.evidence = evidence.clone();
    view.seq = view.seq.wrapping_add(1);
    Some(Transition {
        from,
        to: next,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::Certainty;
    use crate::ids::{ProcessRef, SessionId};

    fn sid() -> SessionId {
        SessionId::new("11111111-2222-3333-4444-555555555555")
    }

    fn fresh() -> SessionView {
        let mut v = SessionView::new(sid(), Timestamp::from_millis(0));
        v.alive = true;
        v.process = Some(ProcessRef::new(10, 1));
        v
    }

    fn hook(view: &mut SessionView, at: i64, event: HookEvent) -> Option<Transition> {
        reduce(
            view,
            &Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::Hook(event),
            ),
        )
    }

    fn registry(status: RegistryStatus, waiting_for: Option<&str>, at: i64) -> Observation {
        Observation::new(
            sid(),
            Timestamp::from_millis(at),
            ObservationBody::Registry {
                process: ProcessRef::new(10, 1),
                session_kind: SessionKind::Interactive,
                cwd: Some("/home/example/proj".into()),
                name: Some("proj".into()),
                name_source: Some("derived".into()),
                status,
                waiting_for: waiting_for.map(str::to_string),
                version: Some("2.1.299".into()),
                socket: None,
                started_at: Some(Timestamp::from_millis(0)),
            },
        )
    }

    #[test]
    fn an_observed_fact_upgrades_the_placeholder_evidence_of_a_fresh_view() {
        // A view starts as a heuristic Idle from the clock. A registry record that says idle,
        // stamped before the view was created, is still a better source for the same state.
        let mut v = SessionView::new(sid(), Timestamp::from_millis(1000));
        assert!(!v.evidence.is_observed());
        let t = reduce(&mut v, &registry(RegistryStatus::Idle, None, 500));
        assert!(t.is_none(), "the state did not change: {t:?}");
        assert_eq!(v.state, SessionState::Idle);
        assert!(v.evidence.is_observed());
        assert_eq!(v.evidence.source, Source::Registry);
        assert_eq!(v.evidence.at, Timestamp::from_millis(500));
        // A later heuristic for the same state does not take the observed fact away.
        reduce(
            &mut v,
            &Observation::new(
                sid(),
                Timestamp::from_millis(600),
                ObservationBody::ProcessAlive {
                    process: ProcessRef::new(10, 1),
                },
            ),
        );
        assert!(v.evidence.is_observed());
    }

    #[test]
    fn hooks_drive_the_main_loop() {
        let mut v = fresh();
        let t = hook(&mut v, 1000, HookEvent::UserPromptSubmit).unwrap();
        assert_eq!(t.to, SessionState::Working);
        assert!(!v.coarse && v.hooks_seen);
        hook(
            &mut v,
            1100,
            HookEvent::PreToolUse {
                tool_name: "Bash".into(),
                tool_use_id: Some("t1".into()),
            },
        );
        assert_eq!(
            v.state,
            SessionState::RunningTool {
                tool: "Bash".into()
            }
        );
        assert_eq!(v.current_tool.as_ref().unwrap().name, "Bash");
        hook(
            &mut v,
            1200,
            HookEvent::PostToolUse {
                tool_name: "Bash".into(),
                tool_use_id: Some("t1".into()),
            },
        );
        assert_eq!(v.state, SessionState::Working);
        assert!(v.current_tool.is_none());
        hook(
            &mut v,
            1300,
            HookEvent::Stop {
                background_tasks: vec![],
                session_crons: 0,
            },
        );
        assert_eq!(v.state, SessionState::Idle);
    }

    #[test]
    fn permission_prompt_needs_attention_and_clears() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        hook(
            &mut v,
            1100,
            HookEvent::PreToolUse {
                tool_name: "Edit".into(),
                tool_use_id: None,
            },
        );
        let t = hook(
            &mut v,
            1200,
            HookEvent::Notification {
                notification_type: "permission_prompt".into(),
            },
        )
        .unwrap();
        assert_eq!(
            t.to,
            SessionState::WaitingPermission {
                what: Some("Edit".into())
            }
        );
        assert_eq!(v.waiting_since, Some(Timestamp::from_millis(1200)));
        hook(
            &mut v,
            1500,
            HookEvent::PostToolUse {
                tool_name: "Edit".into(),
                tool_use_id: None,
            },
        );
        assert_eq!(v.state, SessionState::Working);
        assert_eq!(v.waiting_since, None);
    }

    #[test]
    fn turn_end_with_background_tasks_is_not_idle() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        let tasks = vec![BackgroundTask {
            id: "bg1".into(),
            kind: "subagent".into(),
            status: "running".into(),
            description: Some("explore".into()),
        }];
        let t = hook(
            &mut v,
            2000,
            HookEvent::Stop {
                background_tasks: tasks,
                session_crons: 1,
            },
        )
        .unwrap();
        assert_eq!(t.to, SessionState::BackgroundAgentsActive { count: 1 });
        // A session end while the task runs is still not Completed (rule 2).
        hook(
            &mut v,
            2500,
            HookEvent::SessionEnd {
                reason: "other".into(),
            },
        );
        assert_eq!(v.state, SessionState::BackgroundAgentsActive { count: 1 });
        // Once the task is gone from the next Stop payload, the session rests.
        hook(
            &mut v,
            3000,
            HookEvent::Stop {
                background_tasks: vec![],
                session_crons: 0,
            },
        );
        assert_eq!(v.state, SessionState::Idle);
        assert_eq!(v.background_tasks[0].status, "completed");
    }

    #[test]
    fn silence_never_fails_a_session() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        assert!(!tick(
            &mut v,
            Timestamp::from_millis(1000 + STALL_AFTER_MS - 1)
        ));
        assert!(tick(
            &mut v,
            Timestamp::from_millis(1000 + STALL_AFTER_MS + 1)
        ));
        assert!(v.stalled);
        assert_eq!(
            v.state,
            SessionState::Working,
            "rule 1: still Working, only a badge"
        );
        hook(
            &mut v,
            1000 + STALL_AFTER_MS + 5000,
            HookEvent::PreToolUse {
                tool_name: "Read".into(),
                tool_use_id: None,
            },
        );
        assert!(!v.stalled);
    }

    #[test]
    fn only_first_party_failures_fail() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        let t = hook(
            &mut v,
            1500,
            HookEvent::StopFailure {
                error: "rate_limit".into(),
            },
        )
        .unwrap();
        assert_eq!(
            t.to,
            SessionState::Failed {
                error: "rate_limit".into()
            }
        );
        // A new prompt clears it.
        hook(&mut v, 2000, HookEvent::UserPromptSubmit);
        assert_eq!(v.state, SessionState::Working);
        // A managed child exiting non-zero fails too.
        reduce(
            &mut v,
            &Observation::new(
                sid(),
                Timestamp::from_millis(3000),
                ObservationBody::ManagedExit { code: Some(3) },
            ),
        );
        assert_eq!(
            v.state,
            SessionState::Failed {
                error: "exited with code 3".into()
            }
        );
    }

    #[test]
    fn heuristics_never_override_fresh_facts() {
        let mut v = fresh();
        hook(&mut v, 10_000, HookEvent::UserPromptSubmit);
        // The process is alive, so a vanished registry file proves nothing and changes nothing.
        let gone = Observation::new(
            sid(),
            Timestamp::from_millis(12_000),
            ObservationBody::RegistryGone,
        );
        assert!(reduce(&mut v, &gone).is_none());
        assert_eq!(v.state, SessionState::Working);

        let dead = |at| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::ProcessDead {
                    process: Some(ProcessRef::new(10, 1)),
                    reason: DisconnectReason::ProcessGone,
                },
            )
        };
        let alive = |at| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::ProcessAlive {
                    process: ProcessRef::new(10, 1),
                },
            )
        };
        reduce(&mut v, &dead(20_000));
        assert_eq!(v.state.kind(), StateKind::Disconnected);
        // A sighting 5 s after an observed disconnect only yields a guessed Idle, which loses (rule 3).
        assert!(reduce(&mut v, &alive(25_000)).is_none());
        assert_eq!(v.state.kind(), StateKind::Disconnected);
        // Past the guard window the guess is accepted and labeled as a guess.
        let t = reduce(&mut v, &alive(20_000 + HEURISTIC_GUARD_MS + 1)).unwrap();
        assert_eq!(t.to, SessionState::Idle);
        assert_eq!(t.evidence.certainty, Certainty::Heuristic);
        // A fresh observed fact wins at once.
        hook(&mut v, 60_000, HookEvent::UserPromptSubmit);
        assert_eq!(v.state, SessionState::Working);
        assert!(v.evidence.is_observed());
    }

    #[test]
    fn stale_observations_are_ignored() {
        let mut v = fresh();
        hook(
            &mut v,
            5000,
            HookEvent::Stop {
                background_tasks: vec![],
                session_crons: 0,
            },
        );
        assert_eq!(v.state, SessionState::Idle);
        // A transcript record from before the Stop arrives late and must not reopen the turn.
        let late = Observation::new(
            sid(),
            Timestamp::from_millis(2000),
            ObservationBody::Transcript(TranscriptEvent::UserPrompt),
        );
        assert!(reduce(&mut v, &late).is_none());
        assert_eq!(v.state, SessionState::Idle);
        // The registry's own status change time orders it too.
        assert!(reduce(&mut v, &registry(RegistryStatus::Busy, None, 3000)).is_none());
        assert!(reduce(&mut v, &registry(RegistryStatus::Busy, None, 6000)).is_some());
        assert_eq!(v.state, SessionState::Working);
    }

    #[test]
    fn a_process_that_moved_to_another_session_ends_this_one() {
        let mut v = fresh();
        reduce(&mut v, &registry(RegistryStatus::Busy, None, 1000));
        hook(
            &mut v,
            1100,
            HookEvent::SubagentStart {
                agent_id: "a1".into(),
                agent_type: None,
                transcript_path: None,
            },
        );
        assert_eq!(v.in_flight(), 1);
        let other = SessionId::new("66666666-7777-8888-9999-aaaaaaaaaaaa");
        let superseded = |at: i64, process: ProcessRef| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::Superseded {
                    process,
                    by: other.clone(),
                },
            )
        };

        // A report about another process with the same pid says nothing about this session.
        assert!(reduce(&mut v, &superseded(2000, ProcessRef::new(10, 2))).is_none());
        assert!(v.alive);

        let t = reduce(&mut v, &superseded(3000, ProcessRef::new(10, 1))).unwrap();
        assert_eq!(
            t.to,
            SessionState::Completed {
                reason: format!("continued in {other}")
            }
        );
        assert!(t.evidence.is_observed());
        assert!(!v.alive);
        assert!(v.ended);
        assert_eq!(
            v.in_flight(),
            0,
            "the old conversation's agents end with it"
        );
        assert_eq!(v.agents[0].ended_at, Some(Timestamp::from_millis(3000)));
    }

    #[test]
    fn registry_waiting_for_maps_to_states() {
        let mut v = fresh();
        reduce(
            &mut v,
            &registry(RegistryStatus::Waiting, Some("approve Bash"), 1000),
        );
        assert_eq!(
            v.state,
            SessionState::WaitingPermission {
                what: Some("Bash".into())
            }
        );
        reduce(
            &mut v,
            &registry(RegistryStatus::Waiting, Some("input needed"), 2000),
        );
        assert_eq!(
            v.state,
            SessionState::WaitingUserInput {
                reason: InputReason::Question,
                detail: Some("input needed".into())
            }
        );
        reduce(
            &mut v,
            &registry(RegistryStatus::Waiting, Some("sandbox request"), 3000),
        );
        assert_eq!(
            v.state,
            SessionState::WaitingPermission {
                what: Some("sandbox request".into())
            }
        );
        reduce(&mut v, &registry(RegistryStatus::Idle, None, 4000));
        assert_eq!(v.state, SessionState::Idle);
        assert!(v.coarse, "rule 4: no hooks seen, still coarse");
        assert_eq!(v.display_name(), "proj");
    }

    #[test]
    fn process_death_disconnects_or_completes() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        let dead = Observation::new(
            sid(),
            Timestamp::from_millis(2000),
            ObservationBody::ProcessDead {
                process: Some(ProcessRef::new(10, 1)),
                reason: DisconnectReason::ProcessGone,
            },
        );
        let t = reduce(&mut v, &dead).unwrap();
        assert_eq!(
            t.to,
            SessionState::Disconnected {
                reason: DisconnectReason::ProcessGone
            }
        );
        assert_eq!(v.attachment, Attachment::Disconnected);
        assert!(!v.alive);

        let mut w = fresh();
        hook(
            &mut w,
            1000,
            HookEvent::Stop {
                background_tasks: vec![],
                session_crons: 0,
            },
        );
        hook(
            &mut w,
            1500,
            HookEvent::SessionEnd {
                reason: "prompt_input_exit".into(),
            },
        );
        assert_eq!(
            w.state,
            SessionState::Completed {
                reason: "prompt_input_exit".into()
            }
        );
        reduce(
            &mut w,
            &Observation::new(
                sid(),
                Timestamp::from_millis(2000),
                ObservationBody::ProcessDead {
                    process: Some(ProcessRef::new(10, 1)),
                    reason: DisconnectReason::ProcessGone,
                },
            ),
        );
        assert_eq!(
            w.state,
            SessionState::Completed {
                reason: "prompt_input_exit".into()
            }
        );
    }

    #[test]
    fn a_report_about_another_process_is_ignored() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        let other = Observation::new(
            sid(),
            Timestamp::from_millis(2000),
            ObservationBody::ProcessDead {
                process: Some(ProcessRef::new(99, 7)),
                reason: DisconnectReason::ProcessGone,
            },
        );
        assert!(reduce(&mut v, &other).is_none());
        assert!(v.alive);
        // A report that names no process cannot kill a view whose process is known and alive.
        let anonymous = Observation::new(
            sid(),
            Timestamp::from_millis(2100),
            ObservationBody::ProcessDead {
                process: None,
                reason: DisconnectReason::ProcessGone,
            },
        );
        assert!(reduce(&mut v, &anonymous).is_none());
        assert!(v.alive);
    }

    #[test]
    fn an_anonymous_death_report_disconnects_a_view_with_no_process() {
        let mut v = fresh();
        v.process = None;
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        let anonymous = Observation::new(
            sid(),
            Timestamp::from_millis(2000),
            ObservationBody::ProcessDead {
                process: None,
                reason: DisconnectReason::ProcessGone,
            },
        );
        let t = reduce(&mut v, &anonymous).expect("a transition");
        assert_eq!(
            t.to,
            SessionState::Disconnected {
                reason: DisconnectReason::ProcessGone
            }
        );
        assert!(!v.alive);
    }

    #[test]
    fn osc_reports_map_to_states() {
        let mut v = fresh();
        let osc = |state, kind, at| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::Osc {
                    state,
                    kind,
                    message: None,
                },
            )
        };
        reduce(&mut v, &osc(OscState::Working, None, 1000));
        assert_eq!(v.state, SessionState::Working);
        reduce(
            &mut v,
            &osc(OscState::Blocked, Some(OscKind::Permission), 1100),
        );
        assert_eq!(v.state, SessionState::WaitingPermission { what: None });
        reduce(
            &mut v,
            &osc(OscState::Blocked, Some(OscKind::Question), 1200),
        );
        assert_eq!(v.state.kind(), StateKind::WaitingUserInput);
        reduce(&mut v, &osc(OscState::Done, None, 1300));
        assert_eq!(v.state, SessionState::Idle);
        reduce(&mut v, &osc(OscState::Error, None, 1400));
        assert_eq!(v.state.kind(), StateKind::Failed);
        assert!(v.osc_seen);
    }

    #[test]
    fn subagents_and_turn_records() {
        let mut v = fresh();
        let tr = |event, at| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::Transcript(event),
            )
        };
        reduce(&mut v, &tr(TranscriptEvent::UserPrompt, 1000));
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::ToolUse {
                    tool_use_id: "a".into(),
                    name: "Agent".into(),
                },
                1100,
            ),
        );
        assert_eq!(
            v.state,
            SessionState::RunningTool {
                tool: "Agent".into()
            }
        );
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::AgentLaunched {
                    agent_id: "ag1".into(),
                    is_async: true,
                },
                1200,
            ),
        );
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::ToolResult {
                    tool_use_id: "a".into(),
                    is_error: false,
                },
                1300,
            ),
        );
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::TurnEnded {
                    pending_background_agents: 1,
                    pending_workflows: 0,
                },
                1400,
            ),
        );
        assert_eq!(v.state, SessionState::BackgroundAgentsActive { count: 1 });
        assert_eq!(v.running_agents(), 1);
        hook(
            &mut v,
            1500,
            HookEvent::SubagentStop {
                agent_id: "ag1".into(),
            },
        );
        // The transcript count is still 1 until the next turn record; the agent list says 0.
        assert_eq!(v.running_agents(), 0);
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::TurnEnded {
                    pending_background_agents: 0,
                    pending_workflows: 0,
                },
                1600,
            ),
        );
        assert_eq!(v.state, SessionState::Idle);
    }

    #[test]
    fn an_agent_file_fills_in_what_the_launch_record_lacked() {
        let mut v = fresh();
        let tr = |event, at| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::Transcript(event),
            )
        };
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::AgentLaunched {
                    agent_id: "ag1".into(),
                    is_async: true,
                },
                1000,
            ),
        );
        assert!(
            reduce(
                &mut v,
                &tr(
                    TranscriptEvent::AgentSeen {
                        agent_id: "ag1".into(),
                        agent_type: Some("Explore".into()),
                        description: Some("find entry points".into()),
                        transcript_path: Some("/tmp/agent-ag1.jsonl".into()),
                    },
                    1100,
                ),
            )
            .is_none(),
            "metadata is not a state change"
        );
        assert_eq!(v.agents.len(), 1);
        assert_eq!(v.agents[0].agent_type.as_deref(), Some("Explore"));
        assert_eq!(
            v.agents[0].description.as_deref(),
            Some("find entry points")
        );
        assert_eq!(v.agents[0].started_at, Timestamp::from_millis(1000));
        // A file seen before any launch record is metadata only: nothing said the agent runs.
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::AgentSeen {
                    agent_id: "ag2".into(),
                    agent_type: None,
                    description: None,
                    transcript_path: None,
                },
                1200,
            ),
        );
        assert_eq!(v.agents.len(), 2);
        assert!(!v.agents[1].is_running());
        assert_eq!(v.running_agents(), 1);
        // A foreground launch is recorded once the agent has returned.
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::AgentLaunched {
                    agent_id: "ag3".into(),
                    is_async: false,
                },
                1250,
            ),
        );
        assert_eq!(v.agents[2].ended_at, Some(Timestamp::from_millis(1250)));
        assert_eq!(v.running_agents(), 1);
        // The turn record's zero count ends what the transcript launched.
        reduce(
            &mut v,
            &tr(
                TranscriptEvent::TurnEnded {
                    pending_background_agents: 0,
                    pending_workflows: 0,
                },
                1300,
            ),
        );
        assert_eq!(v.agents[0].ended_at, Some(Timestamp::from_millis(1300)));
        assert_eq!(v.running_agents(), 0);
        assert_eq!(v.state, SessionState::Idle);
    }

    #[test]
    fn ask_user_question_waits_for_input() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        hook(
            &mut v,
            1100,
            HookEvent::PreToolUse {
                tool_name: "AskUserQuestion".into(),
                tool_use_id: None,
            },
        );
        assert_eq!(
            v.state,
            SessionState::WaitingUserInput {
                reason: InputReason::Question,
                detail: None
            }
        );
    }

    #[test]
    fn a_status_line_payload_fills_context_and_proves_life_without_a_state() {
        let mut v = fresh();
        hook(&mut v, 1000, HookEvent::UserPromptSubmit);
        let payload = |at, pct| {
            Observation::new(
                sid(),
                Timestamp::from_millis(at),
                ObservationBody::Statusline {
                    model: Some("claude-opus-5-5".into()),
                    context_used_pct: Some(pct),
                    context_size: Some(200_000),
                    effort: Some("high".into()),
                },
            )
        };
        assert!(reduce(&mut v, &payload(1500, 42)).is_none());
        assert_eq!(v.state, SessionState::Working, "presence only");
        assert_eq!(v.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(v.context_pct, Some(42));
        assert_eq!(v.context_size, Some(200_000));
        assert_eq!(v.effort.as_deref(), Some("high"));
        assert_eq!(v.statusline_at, Some(Timestamp::from_millis(1500)));
        // A replayed older payload does not roll the numbers back.
        assert!(reduce(&mut v, &payload(1200, 10)).is_none());
        assert_eq!(v.context_pct, Some(42));
        assert_eq!(v.statusline_at, Some(Timestamp::from_millis(1500)));
        // After an observed disconnect a payload proves the process lives, but what it does is a
        // guess, so the guard window applies (rule 3) before the view leaves Disconnected.
        reduce(
            &mut v,
            &Observation::new(
                sid(),
                Timestamp::from_millis(2000),
                ObservationBody::ProcessDead {
                    process: Some(ProcessRef::new(10, 1)),
                    reason: DisconnectReason::ProcessGone,
                },
            ),
        );
        assert!(!v.alive);
        assert!(reduce(&mut v, &payload(2500, 43)).is_none());
        assert!(v.alive);
        assert_eq!(v.state.kind(), StateKind::Disconnected);
        assert_eq!(v.context_pct, Some(43));
        let t = reduce(&mut v, &payload(2000 + HEURISTIC_GUARD_MS + 1, 44)).unwrap();
        assert_eq!(t.to, SessionState::Idle);
        assert_eq!(t.evidence.source, Source::Statusline);
        assert!(!t.evidence.is_observed());
        assert_eq!(v.attachment, Attachment::Observed);
    }

    #[test]
    fn user_names_win_over_payload_names() {
        let mut v = fresh();
        reduce(
            &mut v,
            &Observation::new(
                sid(),
                Timestamp::from_millis(1),
                ObservationBody::Registry {
                    process: ProcessRef::new(10, 1),
                    session_kind: SessionKind::Interactive,
                    cwd: None,
                    name: Some("my name".into()),
                    name_source: Some("user".into()),
                    status: RegistryStatus::Idle,
                    waiting_for: None,
                    version: None,
                    socket: None,
                    started_at: None,
                },
            ),
        );
        reduce(
            &mut v,
            &Observation::new(
                sid(),
                Timestamp::from_millis(2),
                ObservationBody::Identity {
                    cwd: Some("/home/example/x".into()),
                    name: Some("ai title".into()),
                    transcript_path: Some("/home/example/.claude/projects/x/s.jsonl".into()),
                    version: None,
                },
            ),
        );
        assert_eq!(v.name.as_deref(), Some("my name"));
        assert_eq!(v.cwd.as_deref(), Some("/home/example/x"));
        assert!(v.transcript_path.is_some());
    }
}
