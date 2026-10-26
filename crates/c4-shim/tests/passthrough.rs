// <wtl> project: c4 | file: crates/c4-shim/tests/passthrough.rs | author: watchthelight </wtl>
//! The shim binary end to end with no C4 listening: the original command gets the same stdin, its
//! output and exit code come back unchanged, and the payload waits in the spool.
//!
//! This file runs without the libtest harness because the test executable doubles as the command the
//! shim wraps: with `C4_SHIM_TEST_CHILD` set to an exit code it echoes its stdin behind an `echo:`
//! prefix, writes one line to stderr and exits with that code. That gives every platform a child to
//! wrap without a shell or an interpreter.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const CHILD: &str = "C4_SHIM_TEST_CHILD";
const PAYLOAD: &[u8] = b"{\"session_id\":\"0a1b2c3d-0001-4000-8000-000000000001\",\"model\":{\"id\":\"claude-opus-5-5\"},\"context_window\":{\"used_percentage\":42,\"context_window_size\":200000}}\n";

fn main() {
    if let Some(code) = std::env::var_os(CHILD).and_then(|v| v.to_str()?.parse::<i32>().ok()) {
        child(code);
    }
    let tests: &[(&str, fn())] = &[
        (
            "statusline passes stdin, output and the exit code through and spools the payload",
            statusline_passes_through,
        ),
        (
            "hook spools the payload, prints nothing and exits 0",
            hook_is_silent,
        ),
        (
            "a payload over the cap reaches the command whole and is not teed",
            oversize_is_passed_through_untouched,
        ),
        (
            "a command that cannot start becomes the status line text",
            missing_command_is_reported,
        ),
        ("usage, version and endpoint", usage_version_endpoint),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        if std::panic::catch_unwind(*test).is_ok() {
            println!("test {name} ... ok");
        } else {
            failed += 1;
            println!("test {name} ... FAILED");
        }
    }
    let verdict = if failed == 0 { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {verdict}. {} passed; {failed} failed",
        tests.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

fn child(code: i32) -> ! {
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"echo:");
    let _ = out.write_all(&input);
    let _ = out.flush();
    eprintln!("child stderr");
    std::process::exit(code);
}

struct Scratch {
    _dir: tempfile::TempDir,
    home: PathBuf,
}

fn scratch() -> Scratch {
    let dir = tempfile::Builder::new()
        .prefix("c4-shim-")
        .tempdir()
        .unwrap();
    let home = dir.path().join("c4");
    Scratch { _dir: dir, home }
}

fn me() -> PathBuf {
    std::env::current_exe().unwrap()
}

/// Run the shim with `C4_HOME` at `home`, the run log on, `stdin_bytes` on stdin and, when given,
/// the child exit code in the environment the wrapped command inherits.
fn run_shim(home: &Path, args: &[&OsStr], stdin_bytes: &[u8], child_exit: Option<i32>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_c4-shim"));
    cmd.args(args)
        .env("C4_HOME", home)
        .env("C4_SHIM_LOG", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match child_exit {
        Some(code) => {
            cmd.env(CHILD, code.to_string());
        }
        None => {
            cmd.env_remove(CHILD);
        }
    }
    let mut child = cmd.spawn().expect("the shim binary runs");
    let mut stdin = child.stdin.take().unwrap();
    let data = stdin_bytes.to_vec();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&data).unwrap();
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    out
}

/// Every spooled line under `home`, parsed, in file order.
fn spool_lines(home: &Path) -> Vec<serde_json::Value> {
    let Ok(entries) = std::fs::read_dir(home.join("spool")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    paths.sort();
    let mut lines = Vec::new();
    for path in paths {
        assert_eq!(
            path.extension().and_then(OsStr::to_str),
            Some("jsonl"),
            "{}",
            path.display()
        );
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            lines.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")));
        }
    }
    lines
}

fn shim_log(home: &Path) -> String {
    std::fs::read_to_string(home.join("logs").join("shim.log")).unwrap()
}

fn statusline_passes_through() {
    let s = scratch();
    let me = me();
    let out = run_shim(
        &s.home,
        &[OsStr::new("statusline"), OsStr::new("--"), me.as_os_str()],
        PAYLOAD,
        Some(3),
    );
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.stdout, [b"echo:".as_slice(), PAYLOAD].concat());
    assert!(String::from_utf8_lossy(&out.stderr).contains("child stderr"));

    let lines = spool_lines(&s.home);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(line["kind"], "statusline");
    assert!(line["recv_at"].as_u64().unwrap() > 1_700_000_000_000);
    assert!(line["pid"].as_u64().unwrap() > 0);
    assert_eq!(
        line["payload"]["session_id"],
        "0a1b2c3d-0001-4000-8000-000000000001"
    );
    assert_eq!(line["payload"]["context_window"]["used_percentage"], 42);

    let log = shim_log(&s.home);
    assert!(
        log.contains("statusline") && log.contains("spooled") && log.contains("child=3"),
        "{log}"
    );
    assert!(
        !log.contains("claude-opus"),
        "payload content stays out of the log: {log}"
    );
}

fn hook_is_silent() {
    let s = scratch();
    let out = run_shim(
        &s.home,
        &[OsStr::new("hook")],
        b"{\"session_id\":\"0a1b2c3d-0001-4000-8000-000000000002\",\"hook_event_name\":\"UserPromptSubmit\",\"cwd\":\"/home/example/proj\"}",
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    // A word after `hook` is ignored, so an installer may label the group with the event.
    let out = run_shim(
        &s.home,
        &[OsStr::new("hook"), OsStr::new("Stop")],
        b"{\"hook_event_name\":\"Stop\"}\n",
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    // Something that is not JSON still travels, wrapped as a string.
    let out = run_shim(&s.home, &[OsStr::new("hook")], b"plain \"text\"\n", None);
    assert_eq!(out.status.code(), Some(0));
    // A blank payload is not worth a line.
    let out = run_shim(&s.home, &[OsStr::new("hook")], b"\n", None);
    assert_eq!(out.status.code(), Some(0));

    let lines = spool_lines(&s.home);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["kind"], "hook");
    assert_eq!(lines[0]["payload"]["hook_event_name"], "UserPromptSubmit");
    assert_eq!(lines[1]["payload"]["hook_event_name"], "Stop");
    assert_eq!(lines[2]["payload"]["raw"], "plain \"text\"");
    assert!(shim_log(&s.home).contains("skipped: empty payload"));
}

fn oversize_is_passed_through_untouched() {
    let s = scratch();
    let me = me();
    let big = vec![b'x'; c4_shim::MAX_TEE_BYTES + 10];
    let out = run_shim(
        &s.home,
        &[OsStr::new("statusline"), OsStr::new("--"), me.as_os_str()],
        &big,
        Some(0),
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout.len(), b"echo:".len() + big.len());
    assert_eq!(&out.stdout[b"echo:".len()..], &big[..]);
    assert_eq!(spool_lines(&s.home), Vec::<serde_json::Value>::new());
    assert!(shim_log(&s.home).contains("skipped: payload over the tee cap"));
}

fn missing_command_is_reported() {
    let s = scratch();
    let out = run_shim(
        &s.home,
        &[
            OsStr::new("statusline"),
            OsStr::new("--"),
            OsStr::new("c4-shim-no-such-command-7f3a"),
        ],
        PAYLOAD,
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("c4-shim: could not start c4-shim-no-such-command-7f3a"),
        "{text}"
    );
    assert_eq!(
        spool_lines(&s.home).len(),
        1,
        "the payload was teed before the command ran"
    );
}

fn usage_version_endpoint() {
    let s = scratch();
    let out = run_shim(&s.home, &[], b"", None);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage"));
    let out = run_shim(&s.home, &[OsStr::new("nonsense")], b"", None);
    assert_eq!(out.status.code(), Some(2));
    let out = run_shim(&s.home, &[OsStr::new("statusline")], b"", None);
    assert_eq!(out.status.code(), Some(2), "statusline needs a command");

    let out = run_shim(&s.home, &[OsStr::new("--version")], b"", None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("c4-shim {}", env!("CARGO_PKG_VERSION"))
    );

    let out = run_shim(&s.home, &[OsStr::new("endpoint")], b"", None);
    assert_eq!(out.status.code(), Some(0));
    let endpoint = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(endpoint, c4_shim::Home::at(&s.home).endpoint());
    assert_eq!(endpoint, c4_core::C4Home::at(&s.home).endpoint());
    assert_eq!(spool_lines(&s.home), Vec::<serde_json::Value>::new());
}
