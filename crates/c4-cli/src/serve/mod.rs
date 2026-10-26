// <wtl> project: c4 | file: crates/c4-cli/src/serve/mod.rs | author: watchthelight </wtl>
//! `c4 serve`: the window's core as a child process. The desktop shell starts it with pipes on
//! stdin and stdout and talks JSON lines:
//!
//! - A request on stdin is one object per line, `{"id": 1, "method": "about", "params": {}}`.
//!   `id` is any JSON value the sender wants back; `params` may be left out.
//! - The answer on stdout carries the same id: `{"id": 1, "ok": <value>}` or
//!   `{"id": 1, "error": "<text>"}`. A line that is not a request is answered with a null id.
//! - Events on stdout have no id: `{"event": "sessions", "data": <document>}` after every refresh
//!   pass and every batch the ingest server folds. The first line is
//!   `{"event": "hello", "data": {"protocol": 1, "schema": 1, "version": "...", "pid": 1234}}`.
//! - Logs go to stderr, warnings and worse unless `--log` says otherwise.
//!
//! The methods are the ones the window calls: `list_sessions` (a files-only pass, then the
//! document), `about`, `prefs_get` and `prefs_set` (`params` is `{"section": "sidebar", "value":
//! {...}}`; the answer is the document as written). The process ends when stdin closes or stdout
//! cannot be written: the ingest server shuts down first, so the endpoint is free for the next
//! C4, and the exit code is 0.
//!
//! Three threads of its own: one reads stdin and answers, one writes stdout (every other thread
//! hands it whole lines over a channel, so an event never lands inside an answer), and the refresh
//! thread runs the passes. The ingest server runs on a runtime of its own (`ingest.rs`). The
//! table, the worker and the preferences file live in `state.rs`, `ingest.rs` and `prefs.rs`.

pub mod ingest;
pub mod prefs;
pub mod state;

pub use ingest::{Ingest, IngestReport, IngestState, Worker};
pub use prefs::{Prefs, PrefsError};
pub use state::{
    AGENTS_EVERY, About, AgentsRun, AppState, DEFAULT_REFRESH_MS, MIN_REFRESH_MS, REFRESH_MS_ENV,
    SESSIONS_EVENT, refresh_period,
};

use crate::ServeArgs;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io::{self, BufRead, Write};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

/// The version of the line protocol. Bumped when a line's shape changes meaning; a new method or
/// a new field does not bump it.
pub const PROTOCOL: u32 = 1;

/// `prefs_set`'s parameters.
#[derive(Deserialize)]
struct SetParams {
    section: String,
    value: Value,
}

/// Why the core stops.
enum End {
    /// Stdin reached its end: the parent closed the pipe or went away.
    Stdin,
    /// A write to stdout failed: the parent stopped reading.
    Stdout,
}

/// Run until stdin closes or stdout cannot be written.
pub fn run(args: &ServeArgs) -> anyhow::Result<()> {
    // Warnings reach stderr even without --log; the subscriber --log installed stays.
    let _ = crate::init_logging("warn");
    let state = Arc::new(AppState::from_args(args));
    let (lines, inbox) = mpsc::channel::<String>();
    let (ended, ends) = mpsc::channel::<End>();
    spawn_writer(inbox, ended.clone())?;
    lines
        .send(hello())
        .map_err(|_| anyhow::anyhow!("stdout closed before the first line"))?;
    spawn_reader(state.clone(), lines.clone(), ended)?;
    spawn_refresh(state.clone(), lines.clone(), state.period())?;
    let emit = lines;
    let worker = match Worker::spawn(state, move |document| {
        let _ = emit.send(event(SESSIONS_EVENT, document));
    }) {
        Ok(worker) => Some(worker),
        Err(e) => {
            tracing::warn!(error = %e, "the ingest server could not get a runtime");
            None
        }
    };
    match ends.recv() {
        Ok(End::Stdin) => tracing::info!("stdin closed; stopping"),
        Ok(End::Stdout) => tracing::warn!("stdout could not be written; stopping"),
        Err(_) => tracing::warn!("both the reader and the writer ended; stopping"),
    }
    if let Some(mut worker) = worker {
        worker.stop();
    }
    Ok(())
}

/// The first line on stdout.
fn hello() -> String {
    line(&json!({
        "event": "hello",
        "data": {
            "protocol": PROTOCOL,
            "schema": crate::OUTPUT_SCHEMA,
            "version": c4_core::VERSION,
            "pid": std::process::id(),
        },
    }))
}

/// An event line. The document moves in; a pass's document is never copied.
fn event(name: &str, data: Value) -> String {
    let mut object = Map::with_capacity(2);
    object.insert("event".into(), Value::String(name.into()));
    object.insert("data".into(), data);
    line(&Value::Object(object))
}

fn line(value: &Value) -> String {
    serde_json::to_string(value).expect("a JSON value serializes")
}

/// The answer to one line of stdin: the request's id back with the result, or a null id and what
/// was wrong with the line.
fn answer(state: &AppState, text: &str) -> String {
    let (id, result) = match serde_json::from_str::<Value>(text) {
        Ok(mut request) if request.is_object() => {
            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let result = match method {
                Some(method) => {
                    let params = request
                        .get_mut("params")
                        .map(Value::take)
                        .unwrap_or_default();
                    dispatch(state, &method, params)
                }
                None => Err("not a request: no \"method\" string".to_string()),
            };
            (id, result)
        }
        Ok(_) => (
            Value::Null,
            Err("not a request: not a JSON object".to_string()),
        ),
        Err(e) => (Value::Null, Err(format!("not a request: {e}"))),
    };
    let mut reply = Map::with_capacity(2);
    reply.insert("id".into(), id);
    match result {
        Ok(ok) => reply.insert("ok".into(), ok),
        Err(error) => reply.insert("error".into(), Value::String(error)),
    };
    line(&Value::Object(reply))
}

/// One method, as the window calls it.
fn dispatch(state: &AppState, method: &str, params: Value) -> Result<Value, String> {
    match method {
        "list_sessions" => Ok(state.refresh(false)),
        "about" => serde_json::to_value(state.about()).map_err(|e| e.to_string()),
        "prefs_get" => Ok(state.prefs().document()),
        "prefs_set" => {
            let p: SetParams = serde_json::from_value(params)
                .map_err(|e| format!("prefs_set needs {{section, value}}: {e}"))?;
            state
                .prefs()
                .set_section(&p.section, p.value)
                .map_err(|e| e.to_string())
        }
        other => Err(format!("unknown method {other:?}")),
    }
}

/// The one thread that writes stdout, a line at a time, flushed. A failed write ends the core.
fn spawn_writer(inbox: mpsc::Receiver<String>, ended: mpsc::Sender<End>) -> io::Result<()> {
    thread::Builder::new()
        .name("c4-stdout".into())
        .spawn(move || {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            while let Ok(text) = inbox.recv() {
                let written = out
                    .write_all(text.as_bytes())
                    .and_then(|()| out.write_all(b"\n"))
                    .and_then(|()| out.flush());
                if written.is_err() {
                    let _ = ended.send(End::Stdout);
                    return;
                }
            }
        })
        .map(drop)
}

/// The thread that reads stdin and answers each line. Its end is the core's end.
fn spawn_reader(
    state: Arc<AppState>,
    lines: mpsc::Sender<String>,
    ended: mpsc::Sender<End>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("c4-stdin".into())
        .spawn(move || {
            let stdin = io::stdin();
            let mut input = stdin.lock();
            let mut text = String::new();
            loop {
                text.clear();
                match input.read_line(&mut text) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "could not read stdin");
                        break;
                    }
                }
                if text.trim().is_empty() {
                    continue;
                }
                // The writer is gone: it has reported the end itself.
                if lines.send(answer(&state, text.trim_end())).is_err() {
                    return;
                }
            }
            let _ = ended.send(End::Stdin);
        })
        .map(drop)
}

/// The refresh loop on its own thread: a pass, an event, a sleep. It ends with the writer.
fn spawn_refresh(
    state: Arc<AppState>,
    lines: mpsc::Sender<String>,
    period: Duration,
) -> io::Result<()> {
    thread::Builder::new()
        .name("c4-refresh".into())
        .spawn(move || {
            let mut pass: u64 = 0;
            loop {
                let with_agents = pass.is_multiple_of(AGENTS_EVERY);
                let document = state.refresh(with_agents);
                if lines.send(event(SESSIONS_EVENT, document)).is_err() {
                    return;
                }
                pass += 1;
                thread::sleep(period);
            }
        })
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use c4_core::C4Home;
    use c4_testkit::ScratchHome;

    fn fresh() -> (ScratchHome, AppState) {
        let scratch = ScratchHome::seeded().unwrap();
        let c4_home = C4Home::at(scratch.scratch_dir().join("c4"));
        let state = AppState::new(
            scratch.home().clone(),
            "test",
            scratch.files_only(),
            Duration::from_millis(DEFAULT_REFRESH_MS),
            Ingest::new(c4_home.clone(), "test"),
            Prefs::open(&c4_home),
        );
        (scratch, state)
    }

    fn parse(text: &str) -> Value {
        assert!(!text.contains('\n'), "one line: {text:?}");
        serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    #[test]
    fn the_hello_line_names_the_protocol_and_the_process() {
        let hello = parse(&hello());
        assert_eq!(hello["event"], "hello");
        assert_eq!(hello["data"]["protocol"], PROTOCOL);
        assert_eq!(hello["data"]["schema"], crate::OUTPUT_SCHEMA);
        assert_eq!(hello["data"]["version"], c4_core::VERSION);
        assert_eq!(hello["data"]["pid"], std::process::id());
        let event = parse(&event(SESSIONS_EVENT, json!({ "tick": 3 })));
        assert_eq!(event["event"], "sessions");
        assert_eq!(event["data"]["tick"], 3);
        assert!(event.get("id").is_none());
    }

    #[test]
    fn answers_carry_the_id_back_and_name_what_went_wrong() {
        let (scratch, state) = fresh();
        let about = parse(&answer(&state, r#"{"id": 7, "method": "about"}"#));
        assert_eq!(about["id"], 7);
        assert_eq!(about["ok"]["home_source"], "test");
        assert_eq!(about["ok"]["home"], scratch.root().display().to_string());
        assert!(about.get("error").is_none());

        let listed = parse(&answer(
            &state,
            r#"{"id": "first", "method": "list_sessions", "params": null}"#,
        ));
        assert_eq!(listed["id"], "first");
        assert_eq!(listed["ok"]["tick"], 1);
        assert!(
            listed["ok"]["sessions"]
                .as_array()
                .is_some_and(|s| !s.is_empty())
        );

        let written = parse(&answer(
            &state,
            r#"{"id": 8, "method": "prefs_set", "params": {"section": "sidebar", "value": {"width": 300}}}"#,
        ));
        assert_eq!(written["id"], 8);
        assert_eq!(written["ok"]["sidebar"]["width"], 300);
        assert_eq!(written["ok"]["schema"], 1);
        let read = parse(&answer(&state, r#"{"id": 9, "method": "prefs_get"}"#));
        assert_eq!(read["ok"], written["ok"]);

        let bad_section = parse(&answer(
            &state,
            r#"{"id": 10, "method": "prefs_set", "params": {"section": "Sidebar", "value": {}}}"#,
        ));
        assert_eq!(bad_section["id"], 10);
        assert!(bad_section.get("ok").is_none());
        assert!(
            bad_section["error"]
                .as_str()
                .is_some_and(|e| e.contains("is not a section name")),
            "{bad_section}"
        );
        let bad_params = parse(&answer(
            &state,
            r#"{"id": 11, "method": "prefs_set", "params": {"section": "sidebar"}}"#,
        ));
        assert!(
            bad_params["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("prefs_set needs {section, value}")),
            "{bad_params}"
        );
        let unknown = parse(&answer(&state, r#"{"id": 12, "method": "frobnicate"}"#));
        assert_eq!(unknown["error"], "unknown method \"frobnicate\"");

        let no_method = parse(&answer(&state, r#"{"id": 13, "params": {}}"#));
        assert_eq!(no_method["id"], 13, "the id comes back even so");
        assert_eq!(no_method["error"], "not a request: no \"method\" string");
        let no_id = parse(&answer(&state, r#"{"method": "about"}"#));
        assert!(no_id["id"].is_null());
        assert_eq!(no_id["ok"]["home_source"], "test");
        let array = parse(&answer(&state, "[1, 2]"));
        assert!(array["id"].is_null());
        assert_eq!(array["error"], "not a request: not a JSON object");
        let garbage = parse(&answer(&state, "this is not json"));
        assert!(garbage["id"].is_null());
        assert!(
            garbage["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("not a request: ")),
            "{garbage}"
        );
    }
}
