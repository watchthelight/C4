// <wtl> project: c4 | file: crates/c4-ingest/src/parse.rs | author: watchthelight </wtl>
//! One shim line to observations. The envelope is the shim's (`kind`, `recv_at`, `pid`, `payload`);
//! the payload is Claude Code's status line or hook JSON as it arrived. Unknown fields are ignored,
//! missing ones default, and an event this version does not know becomes [`HookEvent::Other`] so
//! the log still shows it.

use c4_core::observation::{BackgroundTask, HookEvent, ObservationBody};
use c4_core::{Observation, SessionId, Timestamp};
use serde::Deserialize;
use serde_json::{Map, Value};

/// What a line carries, as the shim labels it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// The JSON Claude Code writes to the status line command.
    Statusline,
    /// The JSON Claude Code writes to a hook.
    Hook,
}

impl Kind {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Statusline => "statusline",
            Self::Hook => "hook",
        }
    }
}

/// A line the parser could use.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ingested {
    /// Status line or hook.
    pub kind: Kind,
    /// When the shim read the payload. Every observation carries this time.
    pub recv_at: Timestamp,
    /// The shim's process id, for the log.
    pub shim_pid: u32,
    /// The session the payload names.
    pub session: SessionId,
    /// `hook_event_name`, for a hook line.
    pub event: Option<String>,
    /// `agent_id`, when the hook fired inside a subagent rather than in the main agent. The
    /// observations are the same either way; the consumer decides whether a tool event from a
    /// subagent should move the session. `None` for `SubagentStart` and `SubagentStop`, whose
    /// `agent_id` names the subagent itself and lives in the event.
    pub agent_id: Option<String>,
    /// What the state machine gets: an `Identity` when the payload named a cwd, a title or a
    /// transcript, then the `Hook` or `Statusline` fact.
    pub observations: Vec<Observation>,
}

/// Why a line was skipped. None of these carry the line's content.
#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    /// Not the shim's envelope.
    #[error("not a shim line: {0}")]
    Json(#[from] serde_json::Error),
    /// A `kind` this version does not know.
    #[error("unknown kind {0:?}")]
    UnknownKind(String),
    /// The shim wrapped a payload that was not JSON as `{"raw": ...}`.
    #[error("the payload was not JSON")]
    Raw,
    /// The payload is JSON but not an object.
    #[error("the payload is not an object")]
    NotAnObject,
    /// No `session_id`, so nothing to attach the facts to.
    #[error("the payload names no session")]
    NoSession,
}

#[derive(Deserialize)]
struct Envelope {
    kind: String,
    recv_at: i64,
    #[serde(default)]
    pid: u32,
    payload: Value,
}

/// Parse one line, with or without its trailing newline.
pub fn parse_line(line: &str) -> Result<Ingested, ParseError> {
    let envelope: Envelope = serde_json::from_str(line.trim())?;
    let kind = match envelope.kind.as_str() {
        "statusline" => Kind::Statusline,
        "hook" => Kind::Hook,
        other => return Err(ParseError::UnknownKind(other.chars().take(32).collect())),
    };
    let Value::Object(payload) = envelope.payload else {
        return Err(ParseError::NotAnObject);
    };
    if payload.len() == 1 && payload.contains_key("raw") {
        return Err(ParseError::Raw);
    }
    let session = SessionId::new(string(&payload, "session_id").ok_or(ParseError::NoSession)?);
    let at = Timestamp::from_millis(envelope.recv_at);
    let mut observations = Vec::with_capacity(2);
    let (event, agent_id) = match kind {
        Kind::Hook => {
            push_identity(
                &mut observations,
                &session,
                at,
                &payload,
                "session_title",
                false,
            );
            let event = string(&payload, "hook_event_name");
            let body = hook_event(event.as_deref().unwrap_or("unknown"), &payload);
            observations.push(Observation::new(
                session.clone(),
                at,
                ObservationBody::Hook(body),
            ));
            let agent_id = match event.as_deref() {
                Some("SubagentStart" | "SubagentStop") => None,
                _ => string(&payload, "agent_id"),
            };
            (event, agent_id)
        }
        Kind::Statusline => {
            push_identity(
                &mut observations,
                &session,
                at,
                &payload,
                "session_name",
                true,
            );
            observations.push(Observation::new(session.clone(), at, statusline(&payload)));
            (None, None)
        }
    };
    Ok(Ingested {
        kind,
        recv_at: at,
        shim_pid: envelope.pid,
        session,
        event,
        agent_id,
        observations,
    })
}

/// The identity fields both payloads carry, as one observation when any of them is present.
fn push_identity(
    out: &mut Vec<Observation>,
    session: &SessionId,
    at: Timestamp,
    payload: &Map<String, Value>,
    name_key: &str,
    with_version: bool,
) {
    let cwd = string(payload, "cwd");
    let name = string(payload, name_key);
    let transcript_path = string(payload, "transcript_path");
    let version = if with_version {
        string(payload, "version")
    } else {
        None
    };
    if cwd.is_none() && name.is_none() && transcript_path.is_none() && version.is_none() {
        return;
    }
    out.push(Observation::new(
        session.clone(),
        at,
        ObservationBody::Identity {
            cwd,
            name,
            transcript_path,
            version,
        },
    ));
}

/// The figures the sidebar shows from a status line payload.
fn statusline(payload: &Map<String, Value>) -> ObservationBody {
    let context = payload.get("context_window");
    ObservationBody::Statusline {
        model: nested_string(payload, "model", "id"),
        context_used_pct: context
            .and_then(|c| c.get("used_percentage"))
            .and_then(Value::as_f64)
            .map(|pct| pct.round().clamp(0.0, 100.0) as u8),
        context_size: context
            .and_then(|c| c.get("context_window_size"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        effort: nested_string(payload, "effort", "level"),
    }
}

/// The hook event by `hook_event_name`. An event whose required id is missing becomes `Other`, so a
/// malformed payload cannot create a nameless agent or task.
fn hook_event(name: &str, p: &Map<String, Value>) -> HookEvent {
    match name {
        "SessionStart" => HookEvent::SessionStart {
            source: string(p, "source").unwrap_or_else(|| "unknown".to_string()),
        },
        "UserPromptSubmit" => HookEvent::UserPromptSubmit,
        "PreToolUse" => HookEvent::PreToolUse {
            tool_name: tool_name(p),
            tool_use_id: string(p, "tool_use_id"),
        },
        "PostToolUse" => HookEvent::PostToolUse {
            tool_name: tool_name(p),
            tool_use_id: string(p, "tool_use_id"),
        },
        "PostToolUseFailure" => HookEvent::PostToolUseFailure {
            tool_name: tool_name(p),
            tool_use_id: string(p, "tool_use_id"),
        },
        "Notification" => HookEvent::Notification {
            notification_type: string(p, "notification_type")
                .unwrap_or_else(|| "unknown".to_string()),
        },
        "SubagentStart" => string(p, "agent_id").map_or_else(
            || other(name),
            |agent_id| HookEvent::SubagentStart {
                agent_id,
                agent_type: string(p, "agent_type"),
                transcript_path: string(p, "agent_transcript_path"),
            },
        ),
        "SubagentStop" => string(p, "agent_id").map_or_else(
            || other(name),
            |agent_id| HookEvent::SubagentStop { agent_id },
        ),
        "TaskCreated" => string(p, "task_id").map_or_else(
            || other(name),
            |task_id| HookEvent::TaskCreated {
                task_id,
                task_subject: string(p, "task_subject"),
            },
        ),
        "TaskCompleted" => string(p, "task_id").map_or_else(
            || other(name),
            |task_id| HookEvent::TaskCompleted { task_id },
        ),
        "Stop" => HookEvent::Stop {
            background_tasks: background_tasks(p),
            session_crons: p
                .get("session_crons")
                .and_then(Value::as_array)
                .map_or(0, |crons| u32::try_from(crons.len()).unwrap_or(u32::MAX)),
        },
        "StopFailure" => HookEvent::StopFailure {
            error: string(p, "error").unwrap_or_else(|| "unknown".to_string()),
        },
        "PreCompact" => HookEvent::PreCompact {
            trigger: string(p, "trigger"),
        },
        "PostCompact" => HookEvent::PostCompact,
        "CwdChanged" => string(p, "new_cwd")
            .map_or_else(|| other(name), |new_cwd| HookEvent::CwdChanged { new_cwd }),
        "ConfigChange" => HookEvent::ConfigChange {
            source: string(p, "source"),
        },
        "SessionEnd" => HookEvent::SessionEnd {
            reason: string(p, "reason").unwrap_or_else(|| "other".to_string()),
        },
        _ => other(name),
    }
}

fn other(name: &str) -> HookEvent {
    HookEvent::Other {
        name: name.to_string(),
    }
}

fn tool_name(p: &Map<String, Value>) -> String {
    string(p, "tool_name").unwrap_or_else(|| "unknown".to_string())
}

/// The `background_tasks` array; an entry without an id is left out.
fn background_tasks(p: &Map<String, Value>) -> Vec<BackgroundTask> {
    p.get("background_tasks")
        .and_then(Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|task| serde_json::from_value(task.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A non-empty string field.
fn string(p: &Map<String, Value>, key: &str) -> Option<String> {
    p.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn nested_string(p: &Map<String, Value>, outer: &str, inner: &str) -> Option<String> {
    p.get(outer)
        .and_then(|v| v.get(inner))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hook_without_an_event_name_is_other() {
        let line = r#"{"kind":"hook","recv_at":5,"pid":1,"payload":{"session_id":"s1","cwd":"/home/example/proj"}}"#;
        let got = parse_line(line).unwrap();
        assert_eq!(got.kind, Kind::Hook);
        assert_eq!(got.event, None);
        assert_eq!(got.session, SessionId::new("s1"));
        assert_eq!(got.recv_at, Timestamp::from_millis(5));
        assert_eq!(got.observations.len(), 2);
        assert_eq!(
            got.observations[1].body,
            ObservationBody::Hook(HookEvent::Other {
                name: "unknown".to_string()
            })
        );
    }

    #[test]
    fn a_subagent_start_without_an_id_does_not_make_an_agent() {
        let line = r#"{"kind":"hook","recv_at":5,"pid":1,"payload":{"session_id":"s1","hook_event_name":"SubagentStart","agent_type":"Explore"}}"#;
        let got = parse_line(line).unwrap();
        assert_eq!(got.observations.len(), 1, "no identity fields, no identity");
        assert_eq!(
            got.observations[0].body,
            ObservationBody::Hook(HookEvent::Other {
                name: "SubagentStart".to_string()
            })
        );
    }

    #[test]
    fn a_long_unknown_kind_is_cut_before_it_reaches_the_log() {
        let kind = "x".repeat(200);
        let line = format!(r#"{{"kind":"{kind}","recv_at":5,"pid":1,"payload":{{}}}}"#);
        match parse_line(&line) {
            Err(ParseError::UnknownKind(k)) => assert_eq!(k.len(), 32),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_statusline_percentage_is_rounded_and_clamped() {
        let line = r#"{"kind":"statusline","recv_at":5,"pid":1,"payload":{"session_id":"s1","context_window":{"used_percentage":107.4,"context_window_size":200000}}}"#;
        let got = parse_line(line).unwrap();
        assert_eq!(got.observations.len(), 1);
        assert_eq!(
            got.observations[0].body,
            ObservationBody::Statusline {
                model: None,
                context_used_pct: Some(100),
                context_size: Some(200_000),
                effort: None,
            }
        );
    }
}
