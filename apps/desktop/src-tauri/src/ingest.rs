// <wtl> project: c4 | file: apps/desktop/src-tauri/src/ingest.rs | author: watchthelight </wtl>
//! The app's end of the shim: the ingest server on the C4 home's endpoint, started when the window
//! opens and stopped when it closes, and the loop that folds every line it receives into the
//! session table and emits the `sessions` event.
//!
//! The server runs on a tokio runtime of its own (two worker threads named `c4-ingest`), apart from
//! the Tauri event loop and the refresh thread. A line waits in the server's inbox until the drain
//! loop wakes; then everything queued is folded as one batch under the table's mutex, on a blocking
//! thread, so a refresh pass that holds the mutex (the session list command can take seconds) never
//! stalls the runtime that accepts connections. One event per batch, at most one every
//! [`COALESCE`], so a burst of tool events costs the window one render.
//!
//! A server that cannot start never fails the window. Another C4 on the same home holds the
//! endpoint ([`StartError::EndpointTaken`]): the report says so and the loop tries again every
//! [`RETRY`], so when that C4 exits this one takes over and replays what the shim spooled in
//! between. Any other error is reported and retried the same way.
//!
//! A hook that fires inside a subagent (its payload carries `agent_id`) folds its identity fields
//! and nothing else: the subagent's tool calls would otherwise move the session between running a
//! tool and working while the main agent is waiting on the subagent, or has already ended its turn.
//! The count of such lines is in the report.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use c4_core::{C4Home, Timestamp};
use c4_ingest::{Inbox, Server, ServerConfig, StartError};
use serde::Serialize;
use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::app::AppState;

/// How long the loop waits after a failed start before trying again.
pub const RETRY: Duration = Duration::from_secs(15);
/// The shortest gap between two `sessions` events from the drain loop.
pub const COALESCE: Duration = Duration::from_millis(40);
/// How often the drain loop looks at the stop flag while nothing arrives.
const POLL: Duration = Duration::from_millis(500);
/// How long [`Worker::stop`] waits for the server to shut down.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// Where the server is in its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestState {
    /// Not tried yet.
    Starting,
    /// Listening on the endpoint.
    Listening,
    /// Another process holds the endpoint; the loop tries again every [`RETRY`].
    Taken,
    /// The server could not start for another reason, given in the detail; retried the same way.
    Error,
    /// The window closed and the server shut down.
    Stopped,
}

/// The server and the drain loop as the window sees them, in the document after every pass or
/// batch.
#[derive(Clone, Debug, Serialize)]
pub struct IngestReport {
    /// The C4 home the server serves.
    pub home: String,
    /// What chose it (`C4_HOME` or `~/.c4`).
    pub home_source: &'static str,
    /// The pipe name or socket path.
    pub endpoint: String,
    /// Where the server is.
    pub state: IngestState,
    /// The error text when the state is `taken` or `error`.
    pub detail: Option<String>,
    /// When the state last changed.
    pub since: Timestamp,
    /// Connections accepted since the server started.
    pub connections: u64,
    /// Connections refused because the peer was another user.
    pub refused: u64,
    /// Lines parsed and queued from the socket.
    pub lines: u64,
    /// Lines from the socket the parser refused.
    pub bad_lines: u64,
    /// Connections dropped for a line over the cap.
    pub oversize: u64,
    /// Connections closed for silence.
    pub idle_closed: u64,
    /// Spool files replayed and removed.
    pub replayed_files: u64,
    /// Lines queued from the spool.
    pub replayed_lines: u64,
    /// Spool lines the parser refused.
    pub replay_bad_lines: u64,
    /// Lines the inbox received, from the socket and the spool together.
    pub received: u64,
    /// Lines the inbox dropped for being full.
    pub dropped: u64,
    /// Lines waiting in the inbox.
    pub queued: usize,
    /// Observations folded into the table.
    pub folded: u64,
    /// Lines from inside a subagent, whose event was not folded.
    pub subagent_lines: u64,
    /// The `recv_at` of the newest line folded.
    pub last_line_at: Option<Timestamp>,
}

struct Shared {
    state: IngestState,
    detail: Option<String>,
    since: Timestamp,
    server: Option<Server>,
    folded: u64,
    subagent_lines: u64,
    last_line_at: Option<Timestamp>,
}

/// The app's handle on the server: its home, its inbox and its report. The server itself runs on
/// the [`Worker`]'s runtime and is lent to this struct for its counters while it listens.
pub struct Ingest {
    home: C4Home,
    home_source: &'static str,
    inbox: Arc<Inbox>,
    shared: Mutex<Shared>,
}

impl Ingest {
    /// A handle for `home`, with nothing started.
    #[must_use]
    pub fn new(home: C4Home, home_source: &'static str) -> Self {
        Self {
            home,
            home_source,
            inbox: Arc::new(Inbox::default()),
            shared: Mutex::new(Shared {
                state: IngestState::Starting,
                detail: None,
                since: Timestamp::now(),
                server: None,
                folded: 0,
                subagent_lines: 0,
                last_line_at: None,
            }),
        }
    }

    /// The C4 home.
    #[must_use]
    pub fn home(&self) -> &C4Home {
        &self.home
    }

    /// The inbox the server fills.
    #[must_use]
    pub fn inbox(&self) -> &Arc<Inbox> {
        &self.inbox
    }

    /// The report, from the counters as they are now.
    #[must_use]
    pub fn report(&self) -> IngestReport {
        let shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        let server = shared
            .server
            .as_ref()
            .map(Server::stats)
            .unwrap_or_default();
        let inbox = self.inbox.stats();
        IngestReport {
            home: self.home.root().display().to_string(),
            home_source: self.home_source,
            endpoint: self.home.endpoint(),
            state: shared.state,
            detail: shared.detail.clone(),
            since: shared.since,
            connections: server.connections,
            refused: server.refused,
            lines: server.lines,
            bad_lines: server.bad_lines,
            oversize: server.oversize,
            idle_closed: server.idle_closed,
            replayed_files: server.replayed_files,
            replayed_lines: server.replayed_lines,
            replay_bad_lines: server.replay_bad_lines,
            received: inbox.received,
            dropped: inbox.dropped,
            queued: inbox.queued,
            folded: shared.folded,
            subagent_lines: shared.subagent_lines,
            last_line_at: shared.last_line_at,
        }
    }

    fn set(&self, state: IngestState, detail: Option<String>) {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.state = state;
        shared.detail = detail;
        shared.since = Timestamp::now();
    }

    fn adopt(&self, server: Server) {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.server = Some(server);
        shared.state = IngestState::Listening;
        shared.detail = None;
        shared.since = Timestamp::now();
    }

    fn take_server(&self) -> Option<Server> {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .server
            .take()
    }

    /// Record one batch folded. Called by [`AppState::fold`] under the table's mutex.
    pub(crate) fn note(&self, applied: u64, subagent_lines: u64, last_line_at: Option<Timestamp>) {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.folded += applied;
        shared.subagent_lines += subagent_lines;
        if let Some(at) = last_line_at {
            shared.last_line_at = Some(shared.last_line_at.map_or(at, |known| known.max(at)));
        }
    }
}

type Emit = Arc<dyn Fn(Value) + Send + Sync>;

/// The runtime the server and the drain loop run on.
pub struct Worker {
    runtime: Option<Runtime>,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl Worker {
    /// Start the runtime, the server (trying again every [`RETRY`] while it cannot start) and the
    /// drain loop. `emit` receives the document after every batch folded.
    pub fn spawn(
        state: Arc<AppState>,
        emit: impl Fn(Value) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        Self::spawn_with(state, emit, RETRY)
    }

    /// [`Self::spawn`] with another retry period.
    pub fn spawn_with(
        state: Arc<AppState>,
        emit: impl Fn(Value) + Send + Sync + 'static,
        retry: Duration,
    ) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("c4-ingest")
            .enable_all()
            .build()?;
        let (stop, rx) = watch::channel(false);
        let task = runtime.spawn(serve(state, Arc::new(emit), rx, retry));
        Ok(Self {
            runtime: Some(runtime),
            stop,
            task: Some(task),
        })
    }

    /// Tell the loop to stop and wait up to [`STOP_WAIT`] for the server to shut down: open
    /// connections get a moment to finish and the socket file is removed. Then the runtime ends.
    pub fn stop(&mut self) {
        let _ = self.stop.send(true);
        if let (Some(runtime), Some(task)) = (self.runtime.take(), self.task.take()) {
            // The timeout is built inside the runtime's context, which its timer needs.
            let stopped = runtime.block_on(async { tokio::time::timeout(STOP_WAIT, task).await });
            if stopped.is_err() {
                tracing::warn!("the ingest server did not stop in time");
            }
            runtime.shutdown_timeout(Duration::from_millis(500));
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn serve(state: Arc<AppState>, emit: Emit, mut stop: watch::Receiver<bool>, retry: Duration) {
    let ingest = state.ingest();
    let home = ingest.home().clone();
    let inbox = ingest.inbox().clone();
    loop {
        if *stop.borrow() {
            break;
        }
        match Server::start(home.clone(), inbox.clone(), ServerConfig::default()).await {
            Ok(server) => {
                ingest.adopt(server);
                drain(&state, &emit, &inbox, &mut stop).await;
                if let Some(server) = ingest.take_server() {
                    server.shutdown().await;
                }
                break;
            }
            Err(e) => {
                let state = if matches!(e, StartError::EndpointTaken { .. }) {
                    IngestState::Taken
                } else {
                    IngestState::Error
                };
                tracing::warn!(error = %e, "the ingest server could not start; trying again");
                ingest.set(state, Some(e.to_string()));
                tokio::select! {
                    changed = stop.changed() => {
                        if changed.is_err() {
                            break;
                        }
                    }
                    () = tokio::time::sleep(retry) => {}
                }
            }
        }
    }
    ingest.set(IngestState::Stopped, None);
}

/// Fold what arrives until the stop flag is set.
async fn drain(
    state: &Arc<AppState>,
    emit: &Emit,
    inbox: &Arc<Inbox>,
    stop: &mut watch::Receiver<bool>,
) {
    loop {
        if *stop.borrow() {
            return;
        }
        // A wait cut short by the timeout loses no line: the next call looks at the queue first.
        let _ = tokio::time::timeout(POLL, inbox.wait()).await;
        let items = inbox.drain();
        if items.is_empty() {
            continue;
        }
        let folder = state.clone();
        match tokio::task::spawn_blocking(move || folder.fold(items)).await {
            Ok(document) => emit(document),
            Err(e) => tracing::error!(error = %e, "the fold did not finish"),
        }
        tokio::time::sleep(COALESCE).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::refresh_period;
    use c4_discovery::ClaudeHome;
    use c4_ingest::{Probe, probe};
    use c4_testkit::ScratchHome;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    /// A session no file in the seeded home names, so the line alone creates its view.
    const SESSION: &str = "c4c4c4c4-0001-4000-8000-000000000099";

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures")
            .canonicalize()
            .expect("the fixtures directory")
    }

    /// One shim line around a fixture payload, stamped now and renamed to [`SESSION`].
    fn hook_line(name: &str) -> String {
        let text =
            std::fs::read_to_string(fixtures().join(format!("hook-payloads/{name}.json"))).unwrap();
        let mut payload: Value = serde_json::from_str(&text).unwrap();
        payload["session_id"] = Value::String(SESSION.into());
        let line = serde_json::json!({
            "kind": "hook",
            "recv_at": Timestamp::now().millis(),
            "pid": 41011,
            "payload": payload,
        });
        format!("{line}\n")
    }

    fn state_for(scratch: &ScratchHome, c4_home: &C4Home) -> Arc<AppState> {
        let home: ClaudeHome = scratch.home().clone();
        Arc::new(AppState::new(
            home,
            "test",
            scratch.files_only(),
            refresh_period(),
            Ingest::new(c4_home.clone(), "test"),
            crate::prefs::Prefs::open(c4_home),
        ))
    }

    fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn session<'a>(document: &'a Value, id: &str) -> &'a Value {
        document["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("session {id} missing from {document}"))
    }

    #[test]
    fn a_shim_line_reaches_the_table_and_the_window() {
        let scratch = ScratchHome::seeded().unwrap();
        let c4_home = C4Home::at(scratch.scratch_dir().join("c4"));
        let state = state_for(&scratch, &c4_home);
        let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut worker = Worker::spawn(state.clone(), move |document| {
            sink.lock().unwrap().push(document);
        })
        .unwrap();
        wait_until("the server listens", || {
            state.ingest().report().state == IngestState::Listening
        });
        assert_eq!(probe(&c4_home), Probe::Listening);
        let before = state.refresh(false);
        assert_eq!(before["ingest"]["state"], "listening");
        assert_eq!(before["ingest"]["endpoint"], c4_home.endpoint());
        assert!(
            before["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["id"] != SESSION),
            "the fixture session is not in the seeded home"
        );

        // The shim's own delivery code, as a hook would run it.
        let shim_home = c4_shim::Home::at(c4_home.root());
        let delivery = c4_shim::deliver(&shim_home, &hook_line("user-prompt-submit"));
        assert!(
            matches!(delivery, c4_shim::Delivery::Sent),
            "delivery: {delivery}"
        );
        wait_until("a document is emitted", || !seen.lock().unwrap().is_empty());
        let document = seen.lock().unwrap().last().cloned().unwrap();
        let view = session(&document, SESSION);
        assert_eq!(view["state"]["state"], "working");
        assert_eq!(view["hooks_seen"], true);
        assert_eq!(view["process"], Value::Null, "no file names a process");
        assert_eq!(view["name"], "fixture session one");
        assert!(
            document["transitions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["session"] == SESSION && t["to"]["state"] == "working"),
            "{document}"
        );
        assert_eq!(document["ingest"]["state"], "listening");
        assert_eq!(document["ingest"]["lines"], 1);
        assert_eq!(
            document["ingest"]["folded"], 2,
            "an identity and the prompt"
        );
        assert_eq!(document["ingest"]["subagent_lines"], 0);
        assert_eq!(document["ingest"]["dropped"], 0);
        assert!(document["ingest"]["last_line_at"].is_number());
        assert_eq!(document["tick"], 1, "the pass counter is the last pass's");

        // A tool call from inside a subagent moves nothing; its identity still counts.
        let emitted = seen.lock().unwrap().len();
        let delivery = c4_shim::deliver(&shim_home, &hook_line("subagent-tool-use"));
        assert!(matches!(delivery, c4_shim::Delivery::Sent));
        wait_until("the second batch is emitted", || {
            seen.lock().unwrap().len() > emitted
        });
        let document = seen.lock().unwrap().last().cloned().unwrap();
        let view = session(&document, SESSION);
        assert_eq!(view["state"]["state"], "working");
        assert_eq!(view["current_tool"], Value::Null);
        assert_eq!(document["ingest"]["lines"], 2);
        assert_eq!(document["ingest"]["folded"], 3, "the identity alone");
        assert_eq!(document["ingest"]["subagent_lines"], 1);
        assert!(
            document["transitions"].as_array().unwrap().is_empty(),
            "{document}"
        );

        // The refresh pass carries the same report and keeps the session.
        let after = state.refresh(false);
        assert_eq!(after["ingest"]["lines"], 2);
        assert_eq!(session(&after, SESSION)["state"]["state"], "working");
        assert_eq!(after["tick"], 2);

        worker.stop();
        assert_eq!(state.ingest().report().state, IngestState::Stopped);
        assert_eq!(probe(&c4_home), Probe::Free);
    }

    #[test]
    fn another_c4_on_the_home_is_reported_and_the_endpoint_taken_over_later() {
        let scratch = ScratchHome::seeded().unwrap();
        let c4_home = C4Home::at(scratch.scratch_dir().join("c4"));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let first = runtime
            .block_on(Server::start(
                c4_home.clone(),
                Arc::new(Inbox::default()),
                ServerConfig::default(),
            ))
            .unwrap();

        let state = state_for(&scratch, &c4_home);
        let mut worker =
            Worker::spawn_with(state.clone(), |_| {}, Duration::from_millis(100)).unwrap();
        wait_until("the endpoint is reported taken", || {
            state.ingest().report().state == IngestState::Taken
        });
        let report = state.ingest().report();
        assert!(
            report
                .detail
                .as_deref()
                .is_some_and(|d| d.contains(&c4_home.endpoint())),
            "{report:?}"
        );
        let document = state.refresh(false);
        assert_eq!(document["ingest"]["state"], "taken");
        assert_eq!(document["ingest"]["lines"], 0);
        assert!(
            document["sessions"]
                .as_array()
                .is_some_and(|s| !s.is_empty())
        );

        runtime.block_on(first.shutdown());
        wait_until("the worker takes the endpoint over", || {
            state.ingest().report().state == IngestState::Listening
        });
        assert_eq!(probe(&c4_home), Probe::Listening);
        worker.stop();
        assert_eq!(probe(&c4_home), Probe::Free);
        assert_eq!(state.ingest().report().state, IngestState::Stopped);
    }
}
