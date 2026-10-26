// <wtl> project: c4 | file: crates/c4-cli/tests/serve.rs | author: watchthelight </wtl>
//! `c4-cli serve` end to end, as the desktop shell drives it: the hello line, the four methods
//! answered by id, a bad line answered with a null id, the `sessions` events from the refresh
//! thread and from a hook line delivered through the shim's own code, and the exit when stdin
//! closes, with the endpoint free afterwards. Scratch homes keep this off the real Claude home and
//! the real C4 home.

use c4_core::{C4Home, Timestamp};
use c4_ingest::{Probe, probe};
use c4_testkit::{ScratchHome, fixtures_root};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A session no file in the seeded home names, so the hook line alone creates its view.
const SESSION: &str = "c4c4c4c4-0001-4000-8000-000000000099";
/// How long any one line or the exit may take on a busy runner.
const WAIT: Duration = Duration::from_secs(30);

/// One shim line around a fixture payload, stamped now and renamed to [`SESSION`].
fn hook_line(name: &str) -> String {
    let text = std::fs::read_to_string(fixtures_root().join(format!("hook-payloads/{name}.json")))
        .unwrap();
    let mut payload: Value = serde_json::from_str(&text).unwrap();
    payload["session_id"] = Value::String(SESSION.into());
    let line = json!({
        "kind": "hook",
        "recv_at": Timestamp::now().millis(),
        "pid": 41011,
        "payload": payload,
    });
    format!("{line}\n")
}

/// The child process with its stdin and a reader thread feeding its stdout lines to a channel.
struct Core {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
}

impl Core {
    fn start(scratch: &ScratchHome, c4_home: &C4Home) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_c4-cli"));
        let (key, value) = scratch.env_var();
        cmd.args(["serve", "--files-only"])
            .env(key, value)
            .env("C4_HOME", c4_home.root())
            .env("C4_REFRESH_MS", "300")
            .env_remove("C4_AGENTS_CMD")
            .env_remove("CLAUDE_CONFIG_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd.spawn().expect("c4-cli serve starts");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stdin = child.stdin.take();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(text) => {
                        if tx.send(text).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, text: &str) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        writeln!(stdin, "{text}").unwrap();
        stdin.flush().unwrap();
    }

    /// The next line, as JSON.
    fn next(&self) -> Value {
        let text = self
            .lines
            .recv_timeout(WAIT)
            .expect("a line from serve within the wait");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    /// Lines until one satisfies `want`; the others (events, mostly) are passed over.
    fn until(&self, what: &str, mut want: impl FnMut(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let text = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
            let value: Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
            if want(&value) {
                return value;
            }
        }
    }

    /// Send a request and wait for its answer.
    fn answer(&mut self, id: u64, method: &str, params: Value) -> Value {
        let mut request = serde_json::Map::with_capacity(3);
        request.insert("id".into(), json!(id));
        request.insert("method".into(), Value::String(method.into()));
        request.insert("params".into(), params);
        self.send(&Value::Object(request).to_string());
        self.until(&format!("the answer to {method}"), |v| v["id"] == id)
    }

    fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "serve did not exit after stdin closed"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find<'a>(document: &'a Value, id: &str) -> Option<&'a Value> {
    document["sessions"]
        .as_array()
        .and_then(|s| s.iter().find(|x| x["id"] == id))
}

#[test]
fn serve_answers_requests_and_streams_events_until_stdin_closes() {
    let scratch = ScratchHome::seeded().unwrap();
    let c4_home = C4Home::at(scratch.scratch_dir().join("c4"));
    let mut core = Core::start(&scratch, &c4_home);

    let hello = core.next();
    assert_eq!(hello["event"], "hello", "{hello}");
    assert_eq!(hello["data"]["protocol"], 1);
    assert_eq!(hello["data"]["schema"], 1);
    assert_eq!(hello["data"]["version"], c4_core::VERSION);
    assert_eq!(hello["data"]["pid"], core.child.id());

    // The refresh thread's first pass arrives on its own, before any request.
    let first = core.until("the first sessions event", |v| v["event"] == "sessions");
    assert_eq!(first["data"]["tick"], 1);
    assert_eq!(first["data"]["schema"], 1);
    assert_eq!(first["data"]["home"], scratch.root().display().to_string());
    assert!(
        first["data"]["sessions"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(
        first["data"]["agents"].is_object(),
        "pass 0 would run the command"
    );
    assert!(
        first["data"]["agents"]["entries"].is_null() && first["data"]["agents"]["error"].is_null(),
        "--files-only: no command ran: {}",
        first["data"]["agents"]
    );

    let about = core.answer(1, "about", Value::Null);
    assert_eq!(about["ok"]["home"], scratch.root().display().to_string());
    assert_eq!(about["ok"]["home_source"], "C4_CLAUDE_HOME");
    assert_eq!(about["ok"]["c4_home"], c4_home.root().display().to_string());
    assert_eq!(about["ok"]["c4_home_source"], "C4_HOME");
    assert_eq!(about["ok"]["endpoint"], c4_home.endpoint());
    assert_eq!(
        about["ok"]["prefs_path"],
        c4_home.prefs_path().display().to_string()
    );
    assert!(about["ok"]["prefs_problem"].is_null());
    assert_eq!(about["ok"]["refresh_ms"], 300);
    assert_eq!(about["ok"]["version"], c4_core::VERSION);

    let listed = core.answer(2, "list_sessions", json!({}));
    assert!(listed["ok"]["tick"].as_u64().is_some_and(|t| t >= 2));
    assert!(
        listed["ok"]["sessions"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(listed["ok"]["ingest"]["state"].is_string());

    let written = core.answer(
        3,
        "prefs_set",
        json!({ "section": "sidebar", "value": { "width": 300, "pinned": ["a"] } }),
    );
    assert_eq!(written["ok"]["sidebar"]["width"], 300);
    assert_eq!(written["ok"]["schema"], 1);
    assert!(c4_home.prefs_path().is_file(), "the file is written");
    let read = core.answer(4, "prefs_get", Value::Null);
    assert_eq!(read["ok"], written["ok"]);
    let refused = core.answer(5, "prefs_set", json!({ "section": "schema", "value": 2 }));
    assert!(refused.get("ok").is_none());
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|e| e.contains("not a section name")),
        "{refused}"
    );
    let unknown = core.answer(6, "frobnicate", Value::Null);
    assert_eq!(unknown["error"], "unknown method \"frobnicate\"");

    core.send("this is not json");
    let garbage = core.until("the answer to a bad line", |v| {
        v.get("error").is_some() && v["id"].is_null()
    });
    assert!(
        garbage["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("not a request")),
        "{garbage}"
    );
    core.send("");
    let still = core.answer(7, "about", Value::Null);
    assert_eq!(
        still["ok"]["os"],
        std::env::consts::OS,
        "an empty line is skipped"
    );

    // The ingest server listens; a hook line through the shim's own delivery code becomes an
    // event with the session the line names.
    core.until("the ingest server listening", |v| {
        v["event"] == "sessions" && v["data"]["ingest"]["state"] == "listening"
    });
    assert_eq!(probe(&c4_home), Probe::Listening);
    let shim_home = c4_shim::Home::at(c4_home.root());
    let delivery = c4_shim::deliver(&shim_home, &hook_line("user-prompt-submit"));
    assert!(
        matches!(delivery, c4_shim::Delivery::Sent),
        "delivery: {delivery}"
    );
    let folded = core.until("the session from the hook line", |v| {
        v["event"] == "sessions" && find(&v["data"], SESSION).is_some()
    });
    let view = find(&folded["data"], SESSION).unwrap();
    assert_eq!(view["state"]["state"], "working");
    assert_eq!(view["hooks_seen"], true);
    assert_eq!(view["name"], "fixture session one");
    assert!(
        folded["data"]["ingest"]["lines"]
            .as_u64()
            .is_some_and(|n| n >= 1)
    );
    let after = core.answer(8, "list_sessions", Value::Null);
    assert_eq!(
        find(&after["ok"], SESSION).unwrap()["state"]["state"],
        "working",
        "the pass keeps the session"
    );

    core.close_stdin();
    let status = core.wait();
    assert!(status.success(), "{status:?}");
    assert_eq!(
        probe(&c4_home),
        Probe::Free,
        "the endpoint is freed on exit"
    );
}

#[test]
fn serve_rejects_an_unknown_flag_without_starting() {
    let scratch = ScratchHome::empty().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_c4-cli"))
        .args(["serve", "--files-only", "--no-such-flag"])
        .env(scratch.env_var().0, scratch.env_var().1)
        .env("C4_HOME", scratch.scratch_dir().join("c4"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "a usage error");
    assert_eq!(out.stdout, Vec::<u8>::new(), "nothing on stdout");
}
