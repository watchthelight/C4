// <wtl> project: c4 | file: apps/desktop/src-tauri/tests/cli.rs | author: watchthelight </wtl>
//! The `c4` executable serves the headless commands before any webview starts. The commands
//! themselves are tested in `crates/c4-cli/tests/cli.rs`; this checks the branch in `main`.

use c4_testkit::ScratchHome;
use std::process::{Command, Output};

fn c4(scratch: &ScratchHome) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_c4"));
    let (key, value) = scratch.env_var();
    cmd.env(key, value)
        .env_remove("C4_AGENTS_CMD")
        .env_remove("CLAUDE_CONFIG_DIR");
    cmd
}

fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("the c4 binary runs")
}

#[test]
fn sessions_json_runs_without_a_window() {
    let scratch = ScratchHome::seeded().unwrap();
    let out = run(c4(&scratch).args(["sessions", "--json", "--files-only"]));
    assert!(
        out.status.success(),
        "exit {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["schema"], c4_cli::OUTPUT_SCHEMA);
    assert_eq!(doc["home"], scratch.home().root().display().to_string());
    assert!(doc["sessions"].as_array().is_some_and(|s| !s.is_empty()));
    assert!(
        doc.get("tick").is_none(),
        "the command line prints the plain shape"
    );
}

#[test]
fn version_and_usage_errors_come_from_the_command_line() {
    let scratch = ScratchHome::empty().unwrap();
    let out = run(c4(&scratch).arg("--version"));
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("c4 0."));
    let out = run(c4(&scratch).arg("frobnicate"));
    assert_eq!(out.status.code(), Some(2));
}
