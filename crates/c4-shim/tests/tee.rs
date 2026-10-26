// <wtl> project: c4 | file: crates/c4-shim/tests/tee.rs | author: watchthelight </wtl>
//! The tee against a listening server, the way C4 will listen: one line arrives, intact and parsable,
//! and nothing is spooled.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PAYLOAD: &[u8] = b"{\"session_id\":\"0a1b2c3d-0001-4000-8000-000000000001\",\"hook_event_name\":\"Stop\",\"cwd\":\"/home/example/proj\",\"transcript_path\":\"/home/example/.claude/projects/-home-example-proj/0a1b2c3d-0001-4000-8000-000000000001.jsonl\"}\n";

fn run_hook(home: &Path, payload: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_c4-shim"))
        .arg("hook")
        .env("C4_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the shim binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload)
        .expect("the shim reads its stdin");
    child.wait_with_output().unwrap()
}

fn check(home: &c4_shim::Home, out: &Output, received: &[u8]) {
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let line = String::from_utf8(received.to_vec()).unwrap();
    assert!(line.ends_with('\n'), "{line:?}");
    assert_eq!(line.matches('\n').count(), 1, "one line: {line:?}");
    let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(v["kind"], "hook");
    assert!(v["recv_at"].as_u64().unwrap() > 1_700_000_000_000);
    assert!(v["pid"].as_u64().unwrap() > 0);
    assert_eq!(v["payload"]["hook_event_name"], "Stop");
    assert_eq!(
        v["payload"]["session_id"],
        "0a1b2c3d-0001-4000-8000-000000000001"
    );
    assert!(
        !home.spool_dir().exists(),
        "a delivered line leaves no spool behind"
    );
}

#[cfg(unix)]
#[test]
fn a_hook_payload_reaches_a_unix_socket_server() {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let dir = tempfile::Builder::new()
        .prefix("c4-shim-")
        .tempdir()
        .unwrap();
    let home = c4_shim::Home::at(dir.path().join("c4"));
    std::fs::create_dir_all(home.root()).unwrap();
    let listener = UnixListener::bind(home.socket_path()).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut received = Vec::new();
        stream.read_to_end(&mut received).unwrap();
        received
    });
    let out = run_hook(home.root(), PAYLOAD);
    let received = server.join().unwrap();
    check(&home, &out, &received);
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hook_payload_reaches_a_named_pipe_server() {
    use tokio::io::AsyncReadExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    let dir = tempfile::Builder::new()
        .prefix("c4-shim-")
        .tempdir()
        .unwrap();
    let home = c4_shim::Home::at(dir.path().join("c4"));
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(home.pipe_name())
        .unwrap();
    let reader = tokio::spawn(async move {
        server.connect().await.unwrap();
        let mut received = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match server.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => received.extend_from_slice(&chunk[..n]),
                // mio issues a read as soon as the server pipe is registered, before any client
                // exists. On Windows 11 that read completes through the completion port with
                // ERROR_PIPE_LISTENING (536) and mio hands the stale error to the first read after
                // `connect()`. The client's bytes are behind it; read again. The ingest server
                // (c4-ingest) treats 536 the same way.
                Err(e) if e.raw_os_error() == Some(536) => {}
                // The client closed its end once the line was written.
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => break,
                Err(e) => panic!("pipe read: {e}"),
            }
        }
        received
    });
    let root = home.root().to_path_buf();
    let out = tokio::task::spawn_blocking(move || run_hook(&root, PAYLOAD))
        .await
        .unwrap();
    let received = reader.await.unwrap();
    check(&home, &out, &received);
}
