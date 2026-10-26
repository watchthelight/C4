// <wtl> project: c4 | file: crates/c4-discovery/tests/ingest_fold.rs | author: watchthelight </wtl>
//! The shim's lines, parsed by `c4-ingest` and folded into a `Discovery` table: the path the app
//! takes between a drain of the inbox and the next `sessions` event, short of the socket.

use c4_core::{SessionId, SessionState, StateKind, Timestamp};
use c4_discovery::{ClaudeHome, Discovery, DiscoveryOptions};
use c4_ingest::parse_line;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .join("fixtures")
}

/// One shim line around a fixture payload.
fn line(kind: &str, recv_at: i64, file: &Path) -> String {
    let payload: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    serde_json::json!({
        "kind": kind,
        "recv_at": recv_at,
        "pid": 41011,
        "payload": payload,
    })
    .to_string()
}

#[test]
fn a_replayed_batch_lands_in_recv_at_order() {
    let hooks = fixtures().join("hook-payloads");
    let status = fixtures().join("statusline-payload");
    let base = 1_792_972_800_000;
    let lines = [
        line("hook", base + 1, &hooks.join("session-start.json")),
        line("hook", base + 2, &hooks.join("user-prompt-submit.json")),
        line("hook", base + 3, &hooks.join("pre-tool-use.json")),
        line("hook", base + 4, &hooks.join("post-tool-use.json")),
        line("statusline", base + 5, &status.join("full.json")),
        line("hook", base + 6, &hooks.join("stop-idle.json")),
    ];
    // The order a spool replay can deliver them in: newest first.
    let observations: Vec<_> = lines
        .iter()
        .rev()
        .map(|l| parse_line(l).unwrap())
        .flat_map(|item| item.observations)
        .collect();
    assert_eq!(observations.len(), 12, "an identity and a fact per line");

    let tmp = tempfile::tempdir().unwrap();
    let mut d = Discovery::new(DiscoveryOptions::files_only(ClaudeHome::at(tmp.path())));
    let now = Timestamp::from_millis(base + 10_000);
    let r = d.fold(observations, now);
    assert_eq!(r.applied, 12);
    let kinds: Vec<StateKind> = r.transitions.iter().map(|(_, t)| t.to.kind()).collect();
    assert_eq!(
        kinds,
        [
            StateKind::Working,
            StateKind::RunningTool,
            StateKind::Working,
            StateKind::Idle
        ],
        "the prompt, the tool, its end, the stop; the session start and the status line move nothing"
    );
    let sid = SessionId::new("0a1b2c3d-0001-4000-8000-000000000001");
    assert_eq!(r.touched, BTreeSet::from([sid.clone()]));

    let v = d.table().get(&sid).unwrap();
    assert_eq!(v.state, SessionState::Idle);
    assert!(v.alive && v.hooks_seen && !v.coarse);
    assert_eq!(v.display_name(), "fixture session one");
    assert_eq!(v.cwd.as_deref(), Some("/home/example/proj"));
    assert_eq!(v.version.as_deref(), Some("2.1.299"));
    assert_eq!(
        v.transcript_path.as_deref(),
        Some(
            "/home/example/.claude/projects/-home-example-proj/0a1b2c3d-0001-4000-8000-000000000001.jsonl"
        )
    );
    assert_eq!(v.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(v.context_pct, Some(42));
    assert_eq!(v.context_size, Some(200_000));
    assert_eq!(v.effort.as_deref(), Some("high"));
    assert_eq!(v.statusline_at, Some(Timestamp::from_millis(base + 5)));
    assert_eq!(v.current_tool, None);
    assert_eq!(v.process, None, "no file named a process");
    assert_eq!(v.first_seen, now);
}
