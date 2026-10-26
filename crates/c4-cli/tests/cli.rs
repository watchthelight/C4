// <wtl> project: c4 | file: crates/c4-cli/tests/cli.rs | author: watchthelight </wtl>
//! The `c4` binary end to end: tests A and B of the directive through `c4 sessions --json`, the
//! doctor on a scratch home, the telemetry of the alpha fixture, and the exit codes. Every run
//! gets `--home` and `C4_CLAUDE_HOME` pointed at a scratch directory, so nothing here reads the
//! real Claude home.

use c4_discovery::scan_registry;
use c4_testkit::{FakeClaude, FakeClaudeOptions, ScratchHome, fixtures_root};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn c4(scratch: &ScratchHome) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_c4-cli"));
    let (key, value) = scratch.env_var();
    cmd.env(key, value)
        .env("C4_HOME", scratch.scratch_dir().join("c4"))
        .env_remove("C4_AGENTS_CMD")
        .env_remove("CLAUDE_CONFIG_DIR");
    cmd
}

fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("the c4 binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn json(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "exit {:?}, stderr: {}",
        out.status.code(),
        stderr(out)
    );
    serde_json::from_str(&stdout(out)).unwrap_or_else(|e| panic!("{e}: {}", stdout(out)))
}

fn alpha_transcript() -> PathBuf {
    fixtures_root()
        .join("home")
        .join("projects")
        .join("-home-example-proj-alpha")
        .join("0a1b2c3d-0001-4000-8000-000000000001.jsonl")
}

/// Three stand-ins: one working with a user name, one waiting for permission, one idle.
fn three(scratch: &ScratchHome) -> (FakeClaude, FakeClaude, FakeClaude) {
    let home = scratch.home();
    let alpha = FakeClaude::start(
        home,
        FakeClaudeOptions::new(scratch.project_dir("alpha").unwrap())
            .status("busy", None)
            .name("alpha build", "user"),
    )
    .unwrap();
    let beta = FakeClaude::start(
        home,
        FakeClaudeOptions::new(scratch.project_dir("beta").unwrap())
            .status("waiting", Some("permission prompt")),
    )
    .unwrap();
    let gamma = FakeClaude::start(
        home,
        FakeClaudeOptions::new(scratch.project_dir("gamma").unwrap()).status("idle", None),
    )
    .unwrap();
    (alpha, beta, gamma)
}

#[test]
fn sessions_json_lists_the_stand_ins_in_attention_order() {
    let scratch = ScratchHome::empty().unwrap();
    let (alpha, beta, gamma) = three(&scratch);

    let out = run(c4(&scratch).args(["sessions", "--json", "--files-only"]));
    let doc = json(&out);
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["home"], scratch.root().display().to_string());
    let report = &doc["report"];
    assert_eq!(report["registry_entries"], 3);
    assert_eq!(report["alive"], 3);
    assert!(report["agents_entries"].is_null(), "no command ran");
    assert!(report["agents_error"].is_null());
    let elapsed = report["elapsed_ms"].as_u64().unwrap();
    assert!(elapsed < 2000, "discovery took {elapsed} ms");

    let sessions = doc["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0]["id"], beta.session().as_str());
    assert_eq!(sessions[0]["state"]["state"], "waiting_permission");
    assert!(
        sessions[0]["state"]["what"].is_null(),
        "a plain permission prompt names no tool to approve: {}",
        sessions[0]["state"]
    );
    assert_eq!(sessions[1]["id"], alpha.session().as_str());
    assert_eq!(sessions[1]["state"]["state"], "working");
    assert_eq!(sessions[1]["name"], "alpha build");
    assert_eq!(sessions[1]["name_source"], "user");
    assert_eq!(sessions[1]["cwd"], alpha.cwd());
    assert_eq!(sessions[1]["process"]["pid"], alpha.pid());
    assert_eq!(sessions[2]["id"], gamma.session().as_str());
    assert_eq!(sessions[2]["state"]["state"], "idle");
    assert!(sessions.iter().all(|s| s["alive"] == true));
    assert!(sessions.iter().all(|s| s["attachment"] == "observed"));
    assert!(
        sessions
            .iter()
            .all(|s| s["evidence"]["certainty"] == "observed")
    );

    // With --home pointing elsewhere the environment variable loses.
    let other = ScratchHome::empty().unwrap();
    let out = run(c4(&scratch)
        .args(["sessions", "--json", "--files-only", "--home"])
        .arg(other.root()));
    let doc = json(&out);
    assert_eq!(doc["sessions"].as_array().unwrap().len(), 0);
    assert_eq!(doc["home"], other.root().display().to_string());
}

#[test]
fn sessions_runs_the_session_list_command_from_the_environment() {
    let scratch = ScratchHome::empty().unwrap();
    let _alpha = FakeClaude::start(
        scratch.home(),
        FakeClaudeOptions::new(scratch.project_dir("alpha").unwrap()).status("busy", None),
    )
    .unwrap();
    let empty_list = if cfg!(windows) {
        "cmd.exe /d /c echo []"
    } else {
        "echo []"
    };
    let out = run(c4(&scratch)
        .env("C4_AGENTS_CMD", empty_list)
        .args(["sessions", "--json"]));
    let doc = json(&out);
    assert_eq!(doc["report"]["agents_entries"], 0, "{}", doc["report"]);
    assert!(doc["report"]["agents_error"].is_null());
    assert_eq!(doc["sessions"].as_array().unwrap().len(), 1);

    // A command that is not there is reported, not fatal.
    let out = run(c4(&scratch)
        .env("C4_AGENTS_CMD", "c4-no-such-program-0000 agents --json")
        .args(["sessions", "--json"]));
    let doc = json(&out);
    assert!(doc["report"]["agents_entries"].is_null());
    assert!(
        doc["report"]["agents_error"]
            .as_str()
            .unwrap()
            .contains("c4-no-such-program-0000")
    );
    assert_eq!(doc["sessions"].as_array().unwrap().len(), 1);
}

#[test]
fn sessions_table_names_the_states() {
    let scratch = ScratchHome::empty().unwrap();
    let (alpha, _beta, _gamma) = three(&scratch);
    let out = run(c4(&scratch).args(["sessions", "--files-only"]));
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("3 sessions (3 alive) in "), "{text}");
    assert!(text.contains("STATE"), "{text}");
    let rows: Vec<&str> = text.lines().skip(3).collect();
    assert_eq!(rows.len(), 3, "{text}");
    assert!(rows[0].starts_with("waiting for permission "), "{text}");
    assert!(rows[1].starts_with("working "), "{text}");
    assert!(rows[1].contains("alpha build"), "{text}");
    assert!(
        rows[2].starts_with("idle "),
        "registry idle is an observed fact, not a heuristic: {text}"
    );
    assert!(rows[0].contains("beta"), "{text}");
    assert!(rows[2].contains("gamma"), "{text}");
    assert!(text.contains(&alpha.pid().to_string()), "{text}");
    assert!(text.contains("interactive"), "{text}");
    assert!(text.contains("observed"), "{text}");

    let out = run(c4(&scratch)
        .args(["sessions", "--files-only", "--home"])
        .arg(scratch.scratch_dir()));
    let text = stdout(&out);
    assert!(text.starts_with("0 sessions (0 alive) in "), "{text}");
}

#[test]
fn doctor_examines_a_scratch_home_without_running_commands() {
    let scratch = ScratchHome::seeded().unwrap();
    let c4_home = scratch.scratch_dir().join("c4-explicit");
    let out = run(c4(&scratch)
        .args(["doctor", "--json", "--no-commands", "--c4-home"])
        .arg(&c4_home));
    let doc = json(&out);
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["os"], std::env::consts::OS);
    assert_eq!(doc["home"]["source"], "C4_CLAUDE_HOME");
    assert_eq!(doc["home"]["exists"], true);
    assert_eq!(doc["home"]["sessions_dir"], true);
    assert_eq!(doc["home"]["registry_entries"], 4);
    assert_eq!(doc["home"]["registry_skipped"], 2);
    assert_eq!(doc["home"]["registry_problems"], 1, "broken.json");
    assert_eq!(doc["home"]["roster"], true);
    assert_eq!(doc["home"]["jobs"], 2);
    assert_eq!(doc["home"]["projects_dir"], true);
    assert_eq!(doc["home"]["settings_json"], false);
    assert!(doc["claude"].is_null());
    assert!(doc["session_list"].is_null());
    assert_eq!(doc["process_table"]["found"], true);
    assert_eq!(doc["process_table"]["start_time"], true);
    // C4's own parts, before any install: nothing there, nothing listening, nothing spooled.
    assert_eq!(doc["c4"]["home"]["source"], "--c4-home");
    assert_eq!(doc["c4"]["home"]["exists"], false);
    assert_eq!(doc["c4"]["shim"]["copy_present"], false);
    assert!(doc["c4"]["shim"]["copy_matches_manifest"].is_null());
    assert!(doc["c4"]["shim"]["version"].is_null(), "no command ran");
    assert!(doc["c4"]["shim"]["error"].is_null());
    assert_eq!(doc["c4"]["install"]["recorded"], false);
    assert_eq!(doc["c4"]["install"]["settings"], "missing");
    assert_eq!(doc["c4"]["install"]["hooks_present"], 0);
    assert_eq!(doc["c4"]["install"]["hooks_total"], 17);
    assert_eq!(doc["c4"]["install"]["statusline"], "absent");
    assert_eq!(doc["c4"]["endpoint"]["state"], "free");
    assert_eq!(doc["c4"]["spool"]["files"], 0);
    let notes: Vec<&str> = doc["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert!(
        notes.iter().any(|n| n.contains("commands skipped")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.starts_with("registry:")),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("c4 install has not run")),
        "{notes:?}"
    );

    // Without --c4-home the environment names the C4 home, as it does for every test here.
    let out = run(c4(&scratch).args(["doctor", "--no-commands"]));
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("c4 0."), "{text}");
    assert!(text.contains("(from C4_CLAUDE_HOME)"), "{text}");
    assert!(
        text.contains("4 records, 2 other files, 1 unreadable"),
        "{text}"
    );
    assert!(text.contains("claude: not checked"), "{text}");
    assert!(text.contains("process table: ok"), "{text}");
    assert!(text.contains("c4 home: "), "{text}");
    assert!(text.contains("(from C4_HOME)"), "{text}");
    assert!(text.contains("shim copy: "), "{text}");
    assert!(text.contains(": missing"), "{text}");
    assert!(text.contains("shim run: no copy to run"), "{text}");
    assert!(text.contains("manifest: none at "), "{text}");
    assert!(text.contains("settings.json: missing"), "{text}");
    assert!(text.contains("endpoint: "), "{text}");
    assert!(text.contains("free (nothing listens"), "{text}");
    assert!(text.contains("spool: 0 files, 0 bytes"), "{text}");

    // A home that does not exist is a finding, not an error.
    let missing = scratch.scratch_dir().join("nowhere");
    let out = run(c4(&scratch)
        .args(["doctor", "--json", "--no-commands", "--home"])
        .arg(&missing));
    let doc = json(&out);
    assert_eq!(doc["home"]["source"], "--home");
    assert_eq!(doc["home"]["exists"], false);
    assert_eq!(doc["home"]["registry_entries"], 0);
    assert!(
        doc["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("does not exist"))
    );
}

#[test]
fn telemetry_reads_the_alpha_fixture() {
    let scratch = ScratchHome::empty().unwrap();
    let out = run(c4(&scratch)
        .args(["telemetry", "--json", "--transcript"])
        .arg(alpha_transcript()));
    let doc = json(&out);
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["session"], "0a1b2c3d-0001-4000-8000-000000000001");
    assert_eq!(doc["records"]["parsed"], 15);
    assert_eq!(doc["records"]["skipped"], 0);
    assert!(doc["records"]["by_type"]["assistant"].as_u64().unwrap() > 0);
    assert_eq!(doc["identity"]["version"], "2.1.299");
    assert_eq!(doc["identity"]["cwd"], "/home/example/proj-alpha");
    assert_eq!(doc["usage"]["input"], 24);
    assert_eq!(doc["usage"]["cache_creation"], 1950);
    assert_eq!(doc["usage"]["cache_read"], 45_660);
    assert_eq!(doc["usage"]["output"], 136);
    assert_eq!(doc["usage"]["total_input"], 24 + 1950 + 45_660);
    assert!((doc["cost"]["total_usd"].as_f64().unwrap() - 0.42).abs() < 1e-9);
    assert_eq!(
        doc["cost"]["by_model"]["claude-opus-5-5"]["cache_read"],
        45_660
    );
    let agents = doc["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "a0000001");
    assert_eq!(agents[0]["agent_type"], "Explore");
    assert_eq!(agents[0]["description"], "Find build entry points");
    assert_eq!(agents[0]["model"], "claude-sonnet-5-5");
    assert_eq!(agents[0]["usage"]["input"], 24);
    assert_eq!(agents[0]["usage"]["output"], 54);
    assert_eq!(doc["agents_usage"]["output"], 54);
    assert_eq!(doc["recorded"]["kind"], "idle");
    assert_eq!(doc["recorded"]["compactions"], 1);
    assert_eq!(doc["recorded"]["agents_seen"], 1);
    assert_eq!(doc["recorded"]["agents_running_at_end"], 0);
    assert!(
        doc["recorded"]["note"]
            .as_str()
            .unwrap()
            .contains("not whether a process is alive")
    );
    assert_eq!(doc["notices"].as_array().unwrap().len(), 0);

    let out = run(c4(&scratch)
        .args(["telemetry", "--transcript"])
        .arg(alpha_transcript()));
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("usage: input 24, cache write 1950, cache read 45660, output 136"),
        "{text}"
    );
    assert!(text.contains("cost: $0.42"), "{text}");
    assert!(
        text.contains("a0000001 Explore \"Find build entry points\""),
        "{text}"
    );
    assert!(
        text.contains("recorded state: idle (observed), 1 compaction, 1 agent seen"),
        "{text}"
    );
}

#[test]
fn exit_codes_tell_usage_errors_from_failures() {
    let scratch = ScratchHome::empty().unwrap();
    let out = run(c4(&scratch).arg("frobnicate"));
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let out = run(&mut c4(&scratch));
    assert_eq!(out.status.code(), Some(2), "no subcommand");
    let out = run(c4(&scratch).args(["telemetry"]));
    assert_eq!(out.status.code(), Some(2), "--transcript is required");

    let out = run(c4(&scratch).args(["telemetry", "--transcript", "no-such-dir/none.jsonl"]));
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).starts_with("c4: no transcript file"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");

    let out = run(c4(&scratch).args(["--version"]));
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("c4 0."), "{}", stdout(&out));

    let out = run(c4(&scratch).args(["--log", "not a filter[", "sessions", "--files-only"]));
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("bad --log filter"),
        "{}",
        stderr(&out)
    );
}

/// A Python 3 on PATH, or none. The Microsoft Store alias named `python3` prints a hint and fails,
/// so the version banner is what counts.
fn python3() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|p| {
        Command::new(p).arg("--version").output().is_ok_and(|o| {
            o.status.success()
                && (stdout(&o).starts_with("Python 3") || stderr(&o).starts_with("Python 3"))
        })
    })
}

#[test]
fn the_python_stand_in_registers_and_cleans_up() {
    let Some(python) = python3() else {
        eprintln!("skipping: no python 3 on PATH");
        return;
    };
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .join("scripts")
        .join("fake_claude.py");
    let scratch = ScratchHome::empty().unwrap();
    let cwd = scratch.project_dir("py").unwrap();
    let session = "0a1b2c3d-00aa-4000-8000-0000000000aa";
    let mut child = Command::new(python)
        .arg("-I")
        .arg(&script)
        .arg("--home")
        .arg(scratch.root())
        .args(["--cwd", &cwd, "--session", session, "--status", "busy"])
        .args(["--name", "pydemo", "--name-source", "auto", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("python starts");

    let sessions_dir = scratch.home().sessions_dir();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && scan_registry(&sessions_dir).entries.is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let scan = scan_registry(&sessions_dir);
    assert_eq!(
        scan.entries.len(),
        1,
        "the script registered: {:?}",
        scan.problems
    );
    assert_eq!(scan.entries[0].pid, child.id());

    let out = run(c4(&scratch).args(["sessions", "--json", "--files-only"]));
    let doc = json(&out);
    let sessions = doc["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], session);
    assert_eq!(sessions[0]["alive"], true);
    assert_eq!(sessions[0]["state"]["state"], "working");
    assert_eq!(sessions[0]["name"], "pydemo");
    assert_eq!(sessions[0]["name_source"], "auto");
    assert_eq!(sessions[0]["cwd"], cwd);
    assert_eq!(sessions[0]["process"]["pid"], child.id());
    assert!(
        sessions[0]["process"]["proc_start"].is_u64(),
        "the record carries a start time: {}",
        sessions[0]["process"]
    );

    // Closing its stdin ends it cleanly, which removes the record.
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && child.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = child.try_wait().unwrap().unwrap_or_else(|| {
        let _ = child.kill();
        let _ = child.wait();
        panic!("the stand-in did not exit after its stdin closed")
    });
    assert!(status.success(), "{status}");
    assert!(
        scan_registry(&sessions_dir).entries.is_empty(),
        "the record is gone"
    );
    let out = run(c4(&scratch).args(["sessions", "--json", "--files-only"]));
    assert_eq!(json(&out)["sessions"].as_array().unwrap().len(), 0);
}
