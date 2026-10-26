// <wtl> project: c4 | file: crates/c4-ingest/tests/server.rs | author: watchthelight </wtl>
//! The server end to end on the real local socket: lines from a client, the shim's own delivery
//! code, the spool replayed from disk, a second server refused, the probe, and the two ways a
//! connection gets dropped.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use c4_core::C4Home;
use c4_ingest::{Inbox, Ingested, Kind, Probe, Server, ServerConfig, StartError, probe};
use tokio::io::AsyncWriteExt;

const RECV_AT: i64 = 1_792_972_800_123;

struct Scratch {
    _dir: tempfile::TempDir,
    home: C4Home,
}

fn scratch() -> Scratch {
    let dir = tempfile::Builder::new()
        .prefix("c4-ingest-")
        .tempdir()
        .unwrap();
    let home = C4Home::at(dir.path().join("c4"));
    Scratch { _dir: dir, home }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn hook_line(name: &str) -> String {
    let text =
        std::fs::read_to_string(fixtures().join(format!("hook-payloads/{name}.json"))).unwrap();
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
    format!("{{\"kind\":\"hook\",\"recv_at\":{RECV_AT},\"pid\":41011,\"payload\":{payload}}}\n")
}

#[cfg(windows)]
async fn client(home: &C4Home) -> tokio::net::windows::named_pipe::NamedPipeClient {
    use tokio::net::windows::named_pipe::ClientOptions;
    let name = home.pipe_name();
    for _ in 0..100 {
        match ClientOptions::new().open(&name) {
            Ok(client) => return client,
            // ERROR_PIPE_BUSY: the server is between serving one client and creating the next
            // instance. The shim sees the same and spools; a test can afford to wait.
            Err(e) if e.raw_os_error() == Some(231) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => panic!("open {name}: {e}"),
        }
    }
    panic!("the pipe stayed busy");
}

#[cfg(unix)]
async fn client(home: &C4Home) -> tokio::net::UnixStream {
    tokio::net::UnixStream::connect(home.socket_path())
        .await
        .unwrap()
}

/// Drain the inbox until `want` items have arrived.
async fn collect(inbox: &Inbox, want: usize) -> Vec<Ingested> {
    let mut items = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            items.extend(inbox.drain());
            if items.len() >= want {
                break;
            }
            inbox.wait().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("only {} of {want} items arrived", items.len()));
    items
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lines_over_the_socket_reach_the_inbox() {
    let s = scratch();
    assert_eq!(probe(&s.home), Probe::Free);
    let inbox = Arc::new(Inbox::new(64));
    let server = Server::start(s.home.clone(), inbox.clone(), ServerConfig::default())
        .await
        .unwrap();
    assert_eq!(server.endpoint(), s.home.endpoint());
    assert_eq!(probe(&s.home), Probe::Listening);

    let mut c = client(&s.home).await;
    c.write_all(hook_line("user-prompt-submit").as_bytes())
        .await
        .unwrap();
    c.write_all(b"not a shim line\n").await.unwrap();
    // The last line without its newline still counts once the client closes.
    let trailing = hook_line("stop-idle");
    c.write_all(trailing.trim_end().as_bytes()).await.unwrap();
    c.flush().await.unwrap();
    drop(c);

    let items = collect(&inbox, 2).await;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, Kind::Hook);
    assert_eq!(items[0].event.as_deref(), Some("UserPromptSubmit"));
    assert_eq!(items[1].event.as_deref(), Some("Stop"));
    eventually("the bad line is counted", || server.stats().bad_lines == 1).await;
    let stats = server.stats();
    assert_eq!(stats.lines, 2);
    assert!(
        stats.connections >= 2,
        "the probe and the client: {stats:?}"
    );
    assert_eq!(stats.oversize, 0);
    assert_eq!(stats.refused, 0);
    assert_eq!(inbox.stats().dropped, 0);

    server.shutdown().await;
    assert_eq!(probe(&s.home), Probe::Free, "{}", s.home.endpoint());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_shims_own_delivery_code_reaches_the_server() {
    let s = scratch();
    let inbox = Arc::new(Inbox::new(64));
    let server = Server::start(s.home.clone(), inbox.clone(), ServerConfig::default())
        .await
        .unwrap();
    let shim_home = c4_shim::Home::at(s.home.root());
    let line = hook_line("pre-tool-use");
    let delivery = tokio::task::spawn_blocking(move || c4_shim::deliver(&shim_home, &line))
        .await
        .unwrap();
    assert!(
        matches!(delivery, c4_shim::Delivery::Sent),
        "delivery: {delivery}"
    );
    let items = collect(&inbox, 1).await;
    assert_eq!(items[0].event.as_deref(), Some("PreToolUse"));
    assert!(
        !s.home.spool_dir().exists(),
        "a delivered line leaves no spool behind"
    );
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_spool_is_replayed_at_start_then_removed() {
    let s = scratch();
    let spool = s.home.spool_dir();
    std::fs::create_dir_all(&spool).unwrap();
    std::fs::copy(
        fixtures().join("spool/2026-10-26.jsonl"),
        spool.join("2026-10-26.jsonl"),
    )
    .unwrap();
    std::fs::write(spool.join("2026-10-25.jsonl"), hook_line("pre-tool-use")).unwrap();
    // Left behind by a C4 that died mid-replay.
    std::fs::write(
        spool.join("2026-10-24.jsonl.replaying"),
        hook_line("user-prompt-submit"),
    )
    .unwrap();

    let inbox = Arc::new(Inbox::new(64));
    let server = Server::start(s.home.clone(), inbox.clone(), ServerConfig::default())
        .await
        .unwrap();
    let items = inbox.drain();
    let events: Vec<Option<&str>> = items.iter().map(|i| i.event.as_deref()).collect();
    assert_eq!(
        events,
        vec![
            Some("UserPromptSubmit"),
            Some("PreToolUse"),
            None,
            Some("Stop")
        ],
        "oldest file first, then the fixture's status line and Stop"
    );
    assert_eq!(items[2].kind, Kind::Statusline);
    let stats = server.stats();
    assert_eq!(stats.replayed_files, 3);
    assert_eq!(stats.replayed_lines, 4);
    assert_eq!(
        stats.replay_bad_lines, 2,
        "the non-JSON line and the raw payload"
    );
    let left: Vec<PathBuf> = std::fs::read_dir(&spool)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(left, Vec::<PathBuf>::new());
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spool_file_written_later_is_replayed_on_schedule() {
    let s = scratch();
    let inbox = Arc::new(Inbox::new(64));
    let config = ServerConfig {
        replay_every: Some(Duration::from_millis(100)),
        ..ServerConfig::default()
    };
    let server = Server::start(s.home.clone(), inbox.clone(), config)
        .await
        .unwrap();
    let spool = s.home.spool_dir();
    std::fs::create_dir_all(&spool).unwrap();
    std::fs::write(spool.join("2026-10-26.jsonl"), hook_line("session-end")).unwrap();
    let items = collect(&inbox, 1).await;
    assert_eq!(items[0].event.as_deref(), Some("SessionEnd"));
    eventually("the file is gone", || {
        !spool.join("2026-10-26.jsonl").exists()
    })
    .await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_server_on_the_same_home_is_refused() {
    let s = scratch();
    let inbox = Arc::new(Inbox::new(64));
    let first = Server::start(s.home.clone(), inbox.clone(), ServerConfig::default())
        .await
        .unwrap();
    let second = Server::start(s.home.clone(), inbox.clone(), ServerConfig::default()).await;
    match second {
        Err(StartError::EndpointTaken { endpoint }) => assert_eq!(endpoint, s.home.endpoint()),
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("two servers on one home"),
    }
    first.shutdown().await;
    // Once the first is gone the home is free again.
    let third = Server::start(s.home.clone(), inbox, ServerConfig::default())
        .await
        .unwrap();
    third.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_home_is_free_the_moment_shutdown_returns() {
    let s = scratch();
    let inbox = Arc::new(Inbox::new(64));
    for round in 0..12u32 {
        let server = Server::start(s.home.clone(), inbox.clone(), ServerConfig::default())
            .await
            .unwrap_or_else(|e| panic!("round {round}: {e}"));
        // Every other round leaves a connection open through the shutdown, so the instance that
        // serves it is dropped with a read in flight, the way the one waiting for a connect is.
        let open = if round % 2 == 1 {
            let mut c = client(&s.home).await;
            c.write_all(hook_line("stop-idle").as_bytes())
                .await
                .unwrap();
            let _ = collect(&inbox, 1).await;
            Some(c)
        } else {
            None
        };
        server.shutdown().await;
        // A client end left open holds no claim on the name; only server ends do.
        assert_eq!(probe(&s.home), Probe::Free, "round {round}");
        drop(open);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silence_and_oversize_lines_drop_the_connection() {
    let s = scratch();
    let inbox = Arc::new(Inbox::new(64));
    let config = ServerConfig {
        max_line_bytes: 1024,
        idle_timeout: Duration::from_millis(200),
        replay_every: None,
        ..ServerConfig::default()
    };
    let server = Server::start(s.home.clone(), inbox.clone(), config)
        .await
        .unwrap();

    let quiet = client(&s.home).await;
    eventually("the quiet connection is closed", || {
        server.stats().idle_closed == 1
    })
    .await;
    drop(quiet);

    let mut loud = client(&s.home).await;
    let big = vec![b'x'; 4096];
    // The server may close before every byte is written; either way the count says why.
    let _ = loud.write_all(&big).await;
    let _ = loud.flush().await;
    eventually("the oversize line is counted", || {
        server.stats().oversize == 1
    })
    .await;
    drop(loud);

    assert!(inbox.is_empty());
    assert_eq!(server.stats().lines, 0);
    server.shutdown().await;
}
