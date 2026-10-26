// <wtl> project: c4 | file: crates/c4-ingest/tests/parse.rs | author: watchthelight </wtl>
//! Every fixture payload through the parser: the session, the identity fields and the event each
//! one becomes.

use c4_core::observation::{BackgroundTask, HookEvent, ObservationBody};
use c4_core::{SessionId, Timestamp};
use c4_ingest::{Kind, ParseError, parse_line};
use std::path::PathBuf;

const SESSION: &str = "0a1b2c3d-0001-4000-8000-000000000001";
const CWD: &str = "/home/example/proj";
const TRANSCRIPT: &str =
    "/home/example/.claude/projects/-home-example-proj/0a1b2c3d-0001-4000-8000-000000000001.jsonl";
const RECV_AT: i64 = 1_792_972_800_123;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn fixture(rel: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(fixtures().join(rel)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn line(kind: &str, payload: &serde_json::Value) -> String {
    format!("{{\"kind\":\"{kind}\",\"recv_at\":{RECV_AT},\"pid\":41011,\"payload\":{payload}}}\n")
}

fn hook_line(name: &str) -> String {
    line("hook", &fixture(&format!("hook-payloads/{name}.json")))
}

fn identity(cwd: &str, name: Option<&str>) -> ObservationBody {
    ObservationBody::Identity {
        cwd: Some(cwd.to_string()),
        name: name.map(str::to_string),
        transcript_path: Some(TRANSCRIPT.to_string()),
        version: None,
    }
}

#[test]
fn every_hook_fixture_becomes_its_event() {
    let cases: Vec<(&str, &str, ObservationBody, HookEvent)> = vec![
        (
            "session-start",
            "SessionStart",
            identity(CWD, Some("fixture session one")),
            HookEvent::SessionStart {
                source: "startup".into(),
            },
        ),
        (
            "user-prompt-submit",
            "UserPromptSubmit",
            identity(CWD, Some("fixture session one")),
            HookEvent::UserPromptSubmit,
        ),
        (
            "pre-tool-use",
            "PreToolUse",
            identity(CWD, None),
            HookEvent::PreToolUse {
                tool_name: "Bash".into(),
                tool_use_id: Some("toolu_fixture_0000000001".into()),
            },
        ),
        (
            "post-tool-use",
            "PostToolUse",
            identity(CWD, None),
            HookEvent::PostToolUse {
                tool_name: "Write".into(),
                tool_use_id: Some("toolu_fixture_0000000002".into()),
            },
        ),
        (
            "post-tool-use-failure",
            "PostToolUseFailure",
            identity(CWD, None),
            HookEvent::PostToolUseFailure {
                tool_name: "Bash".into(),
                tool_use_id: Some("toolu_fixture_0000000001".into()),
            },
        ),
        (
            "notification-permission",
            "Notification",
            identity(CWD, None),
            HookEvent::Notification {
                notification_type: "permission_prompt".into(),
            },
        ),
        (
            "subagent-start",
            "SubagentStart",
            identity(CWD, None),
            HookEvent::SubagentStart {
                agent_id: "e1c0a0d1".into(),
                agent_type: Some("Explore".into()),
                transcript_path: None,
            },
        ),
        (
            "subagent-stop",
            "SubagentStop",
            identity(CWD, None),
            HookEvent::SubagentStop {
                agent_id: "e1c0a0d1".into(),
            },
        ),
        (
            "task-created",
            "TaskCreated",
            identity(CWD, None),
            HookEvent::TaskCreated {
                task_id: "task-001".into(),
                task_subject: Some("Add the ingest server".into()),
            },
        ),
        (
            "task-completed",
            "TaskCompleted",
            identity(CWD, None),
            HookEvent::TaskCompleted {
                task_id: "task-001".into(),
            },
        ),
        (
            "stop-with-tasks",
            "Stop",
            identity(CWD, None),
            HookEvent::Stop {
                background_tasks: vec![
                    BackgroundTask {
                        id: "bash_1".into(),
                        kind: "shell".into(),
                        status: "running".into(),
                        description: Some("Build the workspace".into()),
                    },
                    BackgroundTask {
                        id: "e1c0a0d1".into(),
                        kind: "subagent".into(),
                        status: "running".into(),
                        description: Some("Scan the call sites".into()),
                    },
                ],
                session_crons: 1,
            },
        ),
        (
            "stop-idle",
            "Stop",
            identity(CWD, None),
            HookEvent::Stop {
                background_tasks: vec![],
                session_crons: 0,
            },
        ),
        (
            "stop-failure",
            "StopFailure",
            identity(CWD, None),
            HookEvent::StopFailure {
                error: "rate_limit".into(),
            },
        ),
        (
            "pre-compact",
            "PreCompact",
            identity(CWD, None),
            HookEvent::PreCompact {
                trigger: Some("auto".into()),
            },
        ),
        (
            "post-compact",
            "PostCompact",
            identity(CWD, None),
            HookEvent::PostCompact,
        ),
        (
            "session-end",
            "SessionEnd",
            identity(CWD, None),
            HookEvent::SessionEnd {
                reason: "prompt_input_exit".into(),
            },
        ),
        (
            "cwd-changed",
            "CwdChanged",
            identity("/home/example/proj/crates/c4-ingest", None),
            HookEvent::CwdChanged {
                new_cwd: "/home/example/proj/crates/c4-ingest".into(),
            },
        ),
        (
            "config-change",
            "ConfigChange",
            identity(CWD, None),
            HookEvent::ConfigChange {
                source: Some("user_settings".into()),
            },
        ),
        (
            "unknown-event",
            "PreModelSwitch",
            identity(CWD, None),
            HookEvent::Other {
                name: "PreModelSwitch".into(),
            },
        ),
    ];
    for (file, event_name, identity, event) in cases {
        let got = parse_line(&hook_line(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(got.kind, Kind::Hook, "{file}");
        assert_eq!(got.session, SessionId::new(SESSION), "{file}");
        assert_eq!(got.recv_at, Timestamp::from_millis(RECV_AT), "{file}");
        assert_eq!(got.shim_pid, 41011, "{file}");
        assert_eq!(got.event.as_deref(), Some(event_name), "{file}");
        assert_eq!(got.agent_id, None, "{file}");
        assert_eq!(got.observations.len(), 2, "{file}");
        for o in &got.observations {
            assert_eq!(o.session, SessionId::new(SESSION), "{file}");
            assert_eq!(o.at, Timestamp::from_millis(RECV_AT), "{file}");
        }
        assert_eq!(got.observations[0].body, identity, "{file}");
        assert_eq!(
            got.observations[1].body,
            ObservationBody::Hook(event),
            "{file}"
        );
    }
}

#[test]
fn a_hook_from_inside_a_subagent_carries_the_agent_id() {
    let got = parse_line(&hook_line("subagent-tool-use")).unwrap();
    assert_eq!(got.event.as_deref(), Some("PreToolUse"));
    assert_eq!(got.agent_id.as_deref(), Some("e1c0a0d1"));
    assert_eq!(
        got.observations[1].body,
        ObservationBody::Hook(HookEvent::PreToolUse {
            tool_name: "Grep".into(),
            tool_use_id: Some("toolu_fixture_0000000003".into()),
        })
    );
}

#[test]
fn the_status_line_payloads_become_identity_and_figures() {
    let full = parse_line(&line(
        "statusline",
        &fixture("statusline-payload/full.json"),
    ))
    .unwrap();
    assert_eq!(full.kind, Kind::Statusline);
    assert_eq!(full.session, SessionId::new(SESSION));
    assert_eq!(full.event, None);
    assert_eq!(full.agent_id, None);
    assert_eq!(
        full.observations[0].body,
        ObservationBody::Identity {
            cwd: Some(CWD.into()),
            name: Some("fixture session one".into()),
            transcript_path: Some(TRANSCRIPT.into()),
            version: Some("2.1.299".into()),
        }
    );
    assert_eq!(
        full.observations[1].body,
        ObservationBody::Statusline {
            model: Some("claude-opus-5-5".into()),
            context_used_pct: Some(42),
            context_size: Some(200_000),
            effort: Some("high".into()),
        }
    );

    let minimal = parse_line(&line(
        "statusline",
        &fixture("statusline-payload/minimal.json"),
    ))
    .unwrap();
    assert_eq!(
        minimal.session,
        SessionId::new("0a1b2c3d-0002-4000-8000-000000000002")
    );
    assert_eq!(minimal.observations.len(), 1, "no identity fields");
    assert_eq!(
        minimal.observations[0].body,
        ObservationBody::Statusline {
            model: Some("claude-sonnet-5-5".into()),
            context_used_pct: Some(8),
            context_size: Some(200_000),
            effort: None,
        }
    );
}

#[test]
fn lines_the_parser_refuses() {
    assert!(matches!(parse_line("not json"), Err(ParseError::Json(_))));
    assert!(matches!(parse_line(""), Err(ParseError::Json(_))));
    assert!(matches!(
        parse_line(r#"{"kind":"other","recv_at":1,"pid":1,"payload":{"session_id":"s"}}"#),
        Err(ParseError::UnknownKind(k)) if k == "other"
    ));
    assert!(matches!(
        parse_line(r#"{"kind":"hook","recv_at":1,"pid":1,"payload":{"raw":"plain text"}}"#),
        Err(ParseError::Raw)
    ));
    assert!(matches!(
        parse_line(r#"{"kind":"hook","recv_at":1,"pid":1,"payload":[1,2]}"#),
        Err(ParseError::NotAnObject)
    ));
    assert!(matches!(
        parse_line(r#"{"kind":"hook","recv_at":1,"pid":1,"payload":{"hook_event_name":"Stop"}}"#),
        Err(ParseError::NoSession)
    ));
    assert!(matches!(
        parse_line(r#"{"kind":"hook","recv_at":1,"pid":1,"payload":{"session_id":""}}"#),
        Err(ParseError::NoSession)
    ));
    assert!(matches!(
        parse_line(r#"{"kind":"hook","pid":1,"payload":{"session_id":"s"}}"#),
        Err(ParseError::Json(_))
    ));
}

#[test]
fn the_spool_fixture_parses_line_by_line() {
    let text = std::fs::read_to_string(fixtures().join("spool/2026-10-26.jsonl")).unwrap();
    let results: Vec<Result<Kind, String>> = text
        .lines()
        .map(|l| parse_line(l).map(|i| i.kind).map_err(|e| e.to_string()))
        .collect();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0], Ok(Kind::Statusline));
    assert_eq!(results[1], Ok(Kind::Hook));
    assert!(
        results[2]
            .as_ref()
            .unwrap_err()
            .starts_with("not a shim line")
    );
    assert_eq!(results[3], Err("the payload was not JSON".to_string()));
}
