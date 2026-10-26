// <wtl> project: c4 | file: crates/c4-cli/tests/install.rs | author: watchthelight </wtl>
//! `c4 install` and `c4 uninstall` end to end on a scratch Claude home seeded from
//! `fixtures/settings/full.json`, and test M of the directive: the agents repo's own installer run
//! over the same file twice, with C4's entries surviving and C4's uninstall leaving the agents'
//! entries in place. Every run gets a scratch Claude home and a scratch C4 home through the
//! environment, so nothing here touches the real ones.

use c4_cli::install::{HOOK_EVENTS, shim_file_name};
use c4_testkit::fixtures_root;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

const STUB: &[u8] = b"stub shim v1\n";
const FIXTURE_STATUSLINE: &str = "PYTHONIOENCODING=utf-8 python ~/.claude/statusline.py";
const TURN_END: &str = "PYTHONIOENCODING=utf-8 python ~/.claude/hooks/turn_end.py";

/// A scratch Claude home, a scratch C4 home and a stand-in shim, all under one temp directory.
struct Bench {
    tmp: tempfile::TempDir,
}

impl Bench {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        let stub = tmp.path().join("stub").join(shim_file_name());
        std::fs::create_dir_all(stub.parent().unwrap()).unwrap();
        std::fs::write(&stub, STUB).unwrap();
        Self { tmp }
    }

    fn claude_home(&self) -> PathBuf {
        self.tmp.path().join(".claude")
    }

    fn settings(&self) -> PathBuf {
        self.claude_home().join("settings.json")
    }

    fn c4_home(&self) -> PathBuf {
        self.tmp.path().join("c4home")
    }

    fn stub(&self) -> PathBuf {
        self.tmp.path().join("stub").join(shim_file_name())
    }

    fn manifest(&self) -> PathBuf {
        self.c4_home().join("install-manifest.json")
    }

    fn shim_copy(&self) -> PathBuf {
        self.c4_home().join("bin").join(shim_file_name())
    }

    fn seed_fixture(&self) -> Vec<u8> {
        let bytes = std::fs::read(fixture()).unwrap();
        std::fs::write(self.settings(), &bytes).unwrap();
        bytes
    }

    fn c4(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_c4-cli"));
        cmd.env("C4_CLAUDE_HOME", self.claude_home())
            .env("C4_HOME", self.c4_home())
            .env_remove("C4_AGENTS_CMD")
            .env_remove("CLAUDE_CONFIG_DIR");
        cmd
    }

    fn install(&self, extra: &[&str]) -> Output {
        let mut cmd = self.c4();
        cmd.arg("install")
            .arg("--shim")
            .arg(self.stub())
            .args(extra);
        cmd.output().expect("the c4 binary runs")
    }

    fn uninstall(&self, extra: &[&str]) -> Output {
        let mut cmd = self.c4();
        cmd.arg("uninstall").args(extra);
        cmd.output().expect("the c4 binary runs")
    }

    /// `c4 doctor --no-commands`: the stub shim is text, not a program, so nothing here runs it.
    fn doctor(&self, extra: &[&str]) -> Output {
        let mut cmd = self.c4();
        cmd.args(["doctor", "--no-commands"]).args(extra);
        cmd.output().expect("the c4 binary runs")
    }

    fn settings_bytes(&self) -> Vec<u8> {
        std::fs::read(self.settings()).unwrap()
    }

    fn settings_json(&self) -> Value {
        serde_json::from_slice(&self.settings_bytes()).unwrap()
    }

    fn manifest_json(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.manifest()).unwrap()).unwrap()
    }

    /// C4's backups beside the settings file, oldest first.
    fn backups(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.claude_home())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("settings.json.bak-c4-")
            })
            .collect();
        found.sort();
        found
    }
}

fn fixture() -> PathBuf {
    fixtures_root().join("settings").join("full.json")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "exit {:?}, stdout: {}, stderr: {}",
        out.status.code(),
        stdout(out),
        stderr(out)
    );
    stdout(out)
}

fn json(out: &Output) -> Value {
    serde_json::from_str(&ok(out)).unwrap()
}

fn notes(doc: &Value) -> Vec<String> {
    doc["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

/// The command strings of every handler under one event, in file order.
fn handlers_of<'a>(settings: &'a Value, event: &str) -> Vec<&'a str> {
    settings["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| g["hooks"].as_array().into_iter().flatten())
        .filter_map(|h| h["command"].as_str())
        .collect()
}

#[test]
fn install_wraps_the_status_line_adds_the_hooks_and_leaves_a_manifest() {
    let bench = Bench::new();
    let original = bench.seed_fixture();
    let before: Value = serde_json::from_slice(&original).unwrap();
    let claude_home = bench.claude_home();
    let c4_home = bench.c4_home();

    let text = ok(&bench.install(&[
        "--home",
        claude_home.to_str().unwrap(),
        "--c4-home",
        c4_home.to_str().unwrap(),
    ]));
    assert!(text.starts_with("c4 install\n"), "{text}");
    assert!(text.contains("(from --home)"), "{text}");
    assert!(text.contains("(from --c4-home)"), "{text}");
    assert!(text.contains("shim: copied "), "{text}");
    assert!(text.contains("statusLine: wrapped: "), "{text}");
    assert!(text.contains("hooks: added 17 groups"), "{text}");
    assert!(text.contains("settings.json: written, backup "), "{text}");
    assert!(text.contains("(restorable: yes)"), "{text}");
    assert!(!text.contains("note:"), "{text}");

    // Everything that is not C4's is as it was, in the same order.
    let after = bench.settings_json();
    assert_eq!(keys(&after), keys(&before));
    for key in [
        "permissions",
        "env",
        "model",
        "cleanupPeriodDays",
        "enabledPlugins",
    ] {
        assert_eq!(after[key], before[key], "{key}");
    }

    // The status line runs through the shim, with its assignment kept ahead of it.
    let command = after["statusLine"]["command"].as_str().unwrap();
    assert!(
        command.starts_with("PYTHONIOENCODING=utf-8 C4_HOME="),
        "{command}"
    );
    assert!(
        command.ends_with(" statusline -- python ~/.claude/statusline.py"),
        "{command}"
    );
    assert_eq!(after["statusLine"]["type"], "command");
    assert_eq!(after["statusLine"]["padding"], 1);

    // One C4 handler per event, after whatever was there, with the agreed timeouts.
    let manifest = bench.manifest_json();
    let hook = manifest["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(hook.starts_with("C4_HOME="), "{hook}");
    assert!(hook.contains("c4-shim"), "{hook}");
    assert!(hook.ends_with(" hook"), "{hook}");
    for (event, timeout, run_async) in HOOK_EVENTS {
        let handlers = handlers_of(&after, event);
        assert_eq!(
            handlers.iter().filter(|c| **c == hook).count(),
            1,
            "{event}: {handlers:?}"
        );
        assert_eq!(
            handlers.last(),
            Some(&hook.as_str()),
            "{event}: C4 goes last"
        );
        let group = after["hooks"][event].as_array().unwrap().last().unwrap();
        assert!(group.get("matcher").is_none(), "{event}: no matcher");
        let handler = &group["hooks"][0];
        assert_eq!(handler["type"], "command", "{event}");
        assert_eq!(handler["timeout"], timeout, "{event}");
        assert_eq!(
            handler.get("async").and_then(Value::as_bool),
            run_async.then_some(true),
            "{event}"
        );
    }
    assert_eq!(after["hooks"].as_object().unwrap().len(), 17);
    assert_eq!(keys(&after["hooks"])[0], "SessionStart");
    assert_eq!(handlers_of(&after, "Stop")[0], TURN_END);
    assert_eq!(handlers_of(&after, "SessionStart").len(), 8);

    // The layout is the fixture's: two spaces, LF, one trailing newline.
    let bytes = bench.settings_bytes();
    assert!(!bytes.contains(&b'\r'));
    assert!(bytes.ends_with(b"}\n"));
    assert!(bytes.starts_with(b"{\n  \"permissions\": {\n    \"allow\": [\n"));

    // The backup is the original byte for byte; the shim is copied; the manifest says it all.
    let backups = bench.backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read(&backups[0]).unwrap(), original);
    assert_eq!(std::fs::read(bench.shim_copy()).unwrap(), STUB);
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["c4_version"], c4_core::VERSION);
    assert_eq!(manifest["restorable"], true);
    assert_eq!(manifest["hooks"].as_array().unwrap().len(), 17);
    assert_eq!(manifest["hooks_created"], false);
    assert_eq!(manifest["original"]["kind"], "backup");
    assert_eq!(
        manifest["original"]["path"],
        backups[0].display().to_string()
    );
    assert_eq!(manifest["original"]["sha256"], sha256(&original));
    assert_eq!(manifest["after_sha256"], sha256(&bytes));
    assert_eq!(manifest["statusline"]["before"], FIXTURE_STATUSLINE);
    assert_eq!(manifest["statusline"]["after"], command);
    assert_eq!(
        manifest["settings_path"],
        bench.settings().display().to_string()
    );
    assert_eq!(
        manifest["shim"]["path"],
        bench.shim_copy().display().to_string()
    );
    assert_eq!(
        manifest["shim"]["source"],
        bench.stub().display().to_string()
    );
    assert_eq!(manifest["shim"]["sha256"], sha256(STUB));
    assert!(manifest["installed_at"].is_i64());
    assert_eq!(manifest["installed_at"], manifest["updated_at"]);
}

#[test]
fn doctor_audits_the_install() {
    let bench = Bench::new();
    bench.seed_fixture();

    // Before any install: the file is there and nothing of C4's is in it.
    let doc = json(&bench.doctor(&["--json"]));
    assert_eq!(doc["c4"]["install"]["recorded"], false);
    assert_eq!(doc["c4"]["install"]["settings"], "present");
    assert_eq!(doc["c4"]["install"]["hooks_present"], 0);
    assert_eq!(doc["c4"]["install"]["statusline"], "unwrapped");
    assert_eq!(doc["c4"]["shim"]["copy_present"], false);
    let found = notes(&doc);
    assert!(
        found
            .iter()
            .any(|n| n.starts_with("c4 install has not run")),
        "{found:?}"
    );

    ok(&bench.install(&[]));

    // Installed and untouched: the manifest, the file and the copy agree.
    let doc = json(&bench.doctor(&["--json"]));
    let install = &doc["c4"]["install"];
    assert_eq!(install["recorded"], true);
    // Paths in the report are redacted, so the home directory reads as `~` when the bench is under it.
    assert_eq!(
        install["recorded_settings"],
        c4_core::redact::redact(&bench.settings().display().to_string())
    );
    assert_eq!(install["restorable"], true);
    assert_eq!(install["settings"], "as_installed");
    assert_eq!(install["hooks_present"], 17);
    assert_eq!(install["hooks_total"], 17);
    assert_eq!(install["statusline"], "wrapped");
    assert_eq!(install["foreign"], Value::Array(Vec::new()));
    assert!(install["installed_at"].is_string());
    assert_eq!(install["installed_at"], install["updated_at"]);
    let shim = &doc["c4"]["shim"];
    assert_eq!(
        shim["copy"],
        c4_core::redact::redact(&bench.shim_copy().display().to_string())
    );
    assert_eq!(shim["copy_present"], true);
    assert_eq!(shim["copy_matches_manifest"], true);
    assert!(shim["version"].is_null(), "no command ran");
    assert!(shim["error"].is_null());
    assert_eq!(doc["c4"]["endpoint"]["state"], "free");
    assert_eq!(doc["c4"]["spool"]["files"], 0);
    let found = notes(&doc);
    assert!(!found.iter().any(|n| n.contains("not whole")), "{found:?}");
    assert!(
        !found
            .iter()
            .any(|n| n.starts_with("c4 install has not run")),
        "{found:?}"
    );
    let text = ok(&bench.doctor(&[]));
    assert!(
        text.contains("settings.json: as c4 install left it"),
        "{text}"
    );
    assert!(
        text.contains("hooks: 17 of 17 groups present; statusLine: wrapped"),
        "{text}"
    );
    assert!(text.contains(": as the manifest recorded"), "{text}");
    assert!(text.contains("shim run: not checked"), "{text}");
    assert!(text.contains(", restorable yes"), "{text}");

    // Another program takes the wrapper off, as the agents installer does on every run.
    let mut settings = bench.settings_json();
    settings["statusLine"]["command"] = Value::String(FIXTURE_STATUSLINE.into());
    std::fs::write(
        bench.settings(),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
    let doc = json(&bench.doctor(&["--json"]));
    assert_eq!(doc["c4"]["install"]["settings"], "changed");
    assert_eq!(doc["c4"]["install"]["hooks_present"], 17);
    assert_eq!(doc["c4"]["install"]["statusline"], "unwrapped");
    let found = notes(&doc);
    assert!(
        found.iter().any(|n| n
            == "the install is not whole: the status line is not wrapped; run c4 install again"),
        "{found:?}"
    );
    let text = ok(&bench.doctor(&[]));
    assert!(
        text.contains("settings.json: changed since c4 install"),
        "{text}"
    );
    assert!(text.contains("statusLine: not wrapped"), "{text}");

    // The copy goes missing while the hooks still name it.
    std::fs::remove_file(bench.shim_copy()).unwrap();
    let doc = json(&bench.doctor(&["--json"]));
    assert_eq!(doc["c4"]["shim"]["copy_present"], false);
    assert!(doc["c4"]["shim"]["copy_matches_manifest"].is_null());
    let found = notes(&doc);
    assert!(
        found
            .iter()
            .any(|n| n.starts_with("the shim copy is missing while settings.json names it")),
        "{found:?}"
    );

    // After uninstall the audit reads as it did before the install.
    ok(&bench.uninstall(&[]));
    let doc = json(&bench.doctor(&["--json"]));
    assert_eq!(doc["c4"]["install"]["recorded"], false);
    assert_eq!(doc["c4"]["install"]["settings"], "present");
    assert_eq!(doc["c4"]["install"]["hooks_present"], 0);
    assert_eq!(doc["c4"]["install"]["statusline"], "unwrapped");
    assert_eq!(doc["c4"]["shim"]["copy_present"], false);
}

#[test]
fn a_second_install_changes_nothing() {
    let bench = Bench::new();
    bench.seed_fixture();
    ok(&bench.install(&[]));
    let bytes = bench.settings_bytes();
    let manifest = bench.manifest_json();

    let text = ok(&bench.install(&[]));
    assert!(text.contains("(from C4_CLAUDE_HOME)"), "{text}");
    assert!(text.contains("(from C4_HOME)"), "{text}");
    assert!(text.contains(" is current"), "{text}");
    assert!(text.contains("statusLine: already wrapped"), "{text}");
    assert!(text.contains("hooks: all 17 groups present"), "{text}");
    assert!(text.contains("settings.json: nothing to change"), "{text}");
    assert_eq!(bench.settings_bytes(), bytes);
    assert_eq!(bench.backups().len(), 1);
    let again = bench.manifest_json();
    assert_eq!(again["installed_at"], manifest["installed_at"]);
    assert!(again["updated_at"].as_i64() >= manifest["updated_at"].as_i64());
    assert_eq!(again["after_sha256"], manifest["after_sha256"]);
    assert_eq!(again["restorable"], true);
    assert_eq!(again["original"], manifest["original"]);
    assert_eq!(again["statusline"], manifest["statusline"]);
    assert_eq!(again["hooks"], manifest["hooks"]);
}

#[test]
fn uninstall_restores_the_original_bytes() {
    let bench = Bench::new();
    let original = bench.seed_fixture();
    ok(&bench.install(&[]));

    let text = ok(&bench.uninstall(&[]));
    assert!(text.starts_with("c4 uninstall\n"), "{text}");
    assert!(text.contains("settings.json: restored from "), "{text}");
    assert!(text.contains("backup: "), "{text}");
    assert!(text.contains("shim: removed "), "{text}");
    assert!(text.contains("manifest: removed"), "{text}");
    assert_eq!(bench.settings_bytes(), original);
    assert!(!bench.manifest().exists());
    assert!(!bench.shim_copy().exists());
    assert_eq!(
        bench.backups().len(),
        2,
        "one from the install, one from the uninstall"
    );

    // Once more: nothing of C4's is left to find.
    let text = ok(&bench.uninstall(&[]));
    assert!(text.contains("settings.json: nothing to remove"), "{text}");
    assert!(text.contains("shim: no copy at "), "{text}");
    assert!(text.contains("manifest: none found"), "{text}");
    assert!(
        text.contains("note: settings.json holds nothing of C4's"),
        "{text}"
    );
    assert_eq!(bench.settings_bytes(), original);
    assert_eq!(bench.backups().len(), 2);
}

#[test]
fn uninstall_after_a_foreign_edit_removes_only_what_is_c4s() {
    let bench = Bench::new();
    bench.seed_fixture();
    ok(&bench.install(&[]));
    let hook = bench.manifest_json()["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();

    // Someone else edits the file: the wrapper comes off by hand, a permission and a key appear.
    let mut doc = bench.settings_json();
    doc["statusLine"]["command"] = Value::String("python ~/.claude/statusline.py".into());
    doc["permissions"]["allow"]
        .as_array_mut()
        .unwrap()
        .push(Value::String("Bash(ls:*)".into()));
    doc.as_object_mut()
        .unwrap()
        .insert("alwaysThinkingEnabled".into(), Value::Bool(true));
    std::fs::write(
        bench.settings(),
        serde_json::to_string_pretty(&doc).unwrap() + "\n",
    )
    .unwrap();

    let text = ok(&bench.uninstall(&[]));
    assert!(
        text.contains("settings.json: removed 17 hook handlers from 17 events; other changes kept"),
        "{text}"
    );
    assert!(!text.contains("statusLine wrapper"), "{text}");
    assert!(text.contains("hooks: 9 emptied and dropped: "), "{text}");
    let after = bench.settings_json();
    assert_eq!(
        after["statusLine"]["command"],
        "python ~/.claude/statusline.py"
    );
    assert!(
        after["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Bash(ls:*)")
    );
    assert_eq!(after["alwaysThinkingEnabled"], true);
    assert_eq!(
        keys(&after).last().copied(),
        Some("alwaysThinkingEnabled"),
        "key order kept"
    );
    let hooks = after["hooks"].as_object().unwrap();
    assert_eq!(
        hooks.len(),
        8,
        "the agents' events stay: {:?}",
        keys(&after["hooks"])
    );
    for event in hooks.keys() {
        assert!(
            !handlers_of(&after, event).contains(&hook.as_str()),
            "{event}"
        );
    }
    assert_eq!(handlers_of(&after, "Stop"), vec![TURN_END]);
    assert_eq!(handlers_of(&after, "SessionStart").len(), 7);
    assert!(!bench.manifest().exists());
    assert!(!bench.shim_copy().exists());
    assert_eq!(bench.backups().len(), 2);
}

#[test]
fn a_dry_run_writes_nothing() {
    let bench = Bench::new();
    let original = bench.seed_fixture();

    let text = ok(&bench.install(&["--dry-run"]));
    assert!(
        text.starts_with("c4 install (dry run, nothing written)\n"),
        "{text}"
    );
    assert!(text.contains("shim: would copy "), "{text}");
    assert!(text.contains("statusLine: would wrap: "), "{text}");
    assert!(text.contains("hooks: would add 17 groups"), "{text}");
    assert!(
        text.contains("settings.json: would be written after a backup"),
        "{text}"
    );
    assert!(
        text.contains("would be written (restorable: yes)"),
        "{text}"
    );
    assert_eq!(bench.settings_bytes(), original);
    assert_eq!(bench.backups(), Vec::<PathBuf>::new());
    assert!(!bench.manifest().exists());
    assert!(!bench.shim_copy().exists());
    assert!(
        !bench.c4_home().exists(),
        "a dry run creates nothing under the C4 home"
    );

    let text = ok(&bench.uninstall(&["--dry-run"]));
    assert!(text.contains("settings.json: nothing to remove"), "{text}");
    assert_eq!(bench.settings_bytes(), original);

    ok(&bench.install(&[]));
    let installed = bench.settings_bytes();
    let text = ok(&bench.uninstall(&["--dry-run"]));
    assert!(
        text.starts_with("c4 uninstall (dry run, nothing written)\n"),
        "{text}"
    );
    assert!(
        text.contains("settings.json: would be restored from "),
        "{text}"
    );
    assert!(text.contains("backup: would be taken first"), "{text}");
    assert!(text.contains("shim: would remove "), "{text}");
    assert!(text.contains("manifest: would be removed"), "{text}");
    assert_eq!(bench.settings_bytes(), installed);
    assert!(bench.manifest().exists());
    assert!(bench.shim_copy().exists());
    assert_eq!(bench.backups().len(), 1);
}

#[test]
fn install_without_a_settings_file_creates_one_and_uninstall_removes_it() {
    let bench = Bench::new();

    let text = ok(&bench.install(&[]));
    assert!(
        text.contains("statusLine: none in settings.json, nothing to wrap"),
        "{text}"
    );
    assert!(text.contains("hooks: added 17 groups"), "{text}");
    assert!(
        text.contains("hooks: the file had no \"hooks\" key; it was created"),
        "{text}"
    );
    assert!(text.contains("settings.json: created"), "{text}");
    let after = bench.settings_json();
    assert_eq!(keys(&after), ["hooks"]);
    assert_eq!(after["hooks"].as_object().unwrap().len(), 17);
    assert_eq!(bench.backups(), Vec::<PathBuf>::new(), "nothing to back up");
    let manifest = bench.manifest_json();
    assert_eq!(manifest["original"]["kind"], "missing");
    assert_eq!(manifest["hooks_created"], true);
    assert_eq!(manifest["restorable"], true);
    assert!(manifest["statusline"].is_null());
    let bytes = bench.settings_bytes();
    assert!(bytes.ends_with(b"}\n"));
    assert!(bytes.starts_with(b"{\n  \"hooks\": {\n    \"SessionStart\": [\n"));

    let text = ok(&bench.uninstall(&[]));
    assert!(
        text.contains("settings.json: removed (it did not exist before c4 install)"),
        "{text}"
    );
    assert!(!bench.settings().exists());
    assert_eq!(
        bench.backups().len(),
        1,
        "the file C4 made is kept as a backup"
    );
    assert!(!bench.manifest().exists());
    assert!(!bench.shim_copy().exists());
}

#[test]
fn errors_leave_the_file_alone() {
    let bench = Bench::new();
    let original = bench.seed_fixture();

    // No shim to copy.
    let out = bench
        .c4()
        .arg("install")
        .arg("--shim")
        .arg(bench.tmp.path().join("missing").join("c4-shim"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.starts_with("c4: no shim at "), "{err}");
    assert!(err.contains("--shim"), "{err}");
    assert_eq!(stdout(&out), "");
    assert_eq!(bench.settings_bytes(), original);
    assert_eq!(bench.backups(), Vec::<PathBuf>::new());
    assert!(!bench.c4_home().exists());

    // A file that is not JSON: refused before the shim is copied.
    std::fs::write(bench.settings(), b"{ not json").unwrap();
    let out = bench.install(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("not valid JSON"), "{err}");
    assert!(err.contains("nothing was changed"), "{err}");
    assert_eq!(bench.settings_bytes(), b"{ not json");
    assert_eq!(bench.backups(), Vec::<PathBuf>::new());
    assert!(!bench.c4_home().exists());

    // A file that holds something other than an object.
    std::fs::write(bench.settings(), b"[1, 2]\n").unwrap();
    let out = bench.install(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("JSON object"), "{}", stderr(&out));
    assert_eq!(bench.settings_bytes(), b"[1, 2]\n");

    // A Claude home that does not exist.
    let out = bench
        .c4()
        .arg("install")
        .arg("--shim")
        .arg(bench.stub())
        .arg("--home")
        .arg(bench.tmp.path().join("nowhere"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("does not exist"), "{}", stderr(&out));
    assert!(!bench.c4_home().exists());

    // A manifest that records an install into another Claude home.
    std::fs::write(bench.settings(), &original).unwrap();
    ok(&bench.install(&[]));
    let other = bench.tmp.path().join("other-claude");
    std::fs::create_dir_all(&other).unwrap();
    let out = bench
        .c4()
        .arg("install")
        .arg("--shim")
        .arg(bench.stub())
        .arg("--home")
        .arg(&other)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("records an install into"),
        "{}",
        stderr(&out)
    );
    assert!(!other.join("settings.json").exists());

    // Uninstall refuses a file it cannot parse and keeps the manifest for a later try.
    std::fs::write(bench.settings(), b"{ not json").unwrap();
    let out = bench.uninstall(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("not valid JSON"), "{}", stderr(&out));
    assert_eq!(bench.settings_bytes(), b"{ not json");
    assert!(bench.manifest().exists());
    assert!(bench.shim_copy().exists());
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

/// The agents repo with its installer: `C4_AGENTS_REPO`, else `~/src/agents`, when either holds
/// `install/install.py`.
fn agents_repo() -> Option<PathBuf> {
    let candidate = std::env::var_os("C4_AGENTS_REPO").map_or_else(
        || c4_core::home::user_home().join("src").join("agents"),
        PathBuf::from,
    );
    candidate
        .join("install")
        .join("install.py")
        .is_file()
        .then_some(candidate)
}

/// Test M: the agents repo's installer runs over a file C4 installed into, twice, and C4's entries
/// survive it; `c4 install` again puts back the one thing it takes away (the statusLine wrapper);
/// `c4 uninstall` then removes C4's entries one by one and leaves the agents' own.
#[test]
fn coexists_with_the_agents_installer() {
    let Some(python) = python3() else {
        eprintln!("skipping test M: no python 3 on PATH");
        return;
    };
    let Some(agents) = agents_repo() else {
        eprintln!(
            "skipping test M: no agents repo; set C4_AGENTS_REPO or clone watchthelight-labs/agents to ~/src/agents"
        );
        return;
    };
    let bench = Bench::new();
    let original = bench.seed_fixture();
    let before: Value = serde_json::from_slice(&original).unwrap();
    ok(&bench.install(&[]));
    let hook = bench.manifest_json()["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();

    // The agents installer, twice; the second run has nothing to change.
    for pass in 1..=2 {
        let out = Command::new(python)
            .arg("-I")
            .arg(agents.join("install").join("install.py"))
            .arg("--home")
            .arg(bench.tmp.path())
            .output()
            .expect("python runs");
        assert!(
            out.status.success(),
            "agents installer pass {pass}: {}\n{}",
            stdout(&out),
            stderr(&out)
        );
        if pass == 2 {
            assert!(
                stdout(&out).contains("0 settings change(s)"),
                "{}",
                stdout(&out)
            );
        }
    }
    let after = bench.settings_json();
    for (event, _, _) in HOOK_EVENTS {
        let handlers = handlers_of(&after, event);
        assert_eq!(
            handlers.iter().filter(|c| **c == hook).count(),
            1,
            "{event}: {handlers:?}"
        );
    }
    assert!(
        handlers_of(&after, "Stop")
            .iter()
            .any(|c| c.ends_with("hooks/turn_end.py"))
    );
    assert_eq!(after["permissions"], before["permissions"]);
    assert_eq!(
        after["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
        "claude-sonnet-5-5"
    );
    assert_eq!(after["env"]["MCP_TIMEOUT"], "30000");
    assert_eq!(after["model"], "claude-opus-5-5");
    // The agents installer writes its own statusLine, which takes the wrapper off: the documented
    // risk, and the reason doctor and the app watch for it.
    let command = after["statusLine"]["command"].as_str().unwrap();
    assert!(!command.contains("c4-shim"), "{command}");
    assert!(command.ends_with("~/.claude/statusline.py"), "{command}");
    assert!(bench.settings_bytes().ends_with(b"\n"));

    // c4 install again puts the wrapper back and keeps everything the agents installer added.
    let text = ok(&bench.install(&[]));
    assert!(text.contains("statusLine: wrapped: "), "{text}");
    assert!(text.contains("hooks: all 17 groups present"), "{text}");
    assert!(text.contains("(restorable: no)"), "{text}");
    let after = bench.settings_json();
    let rewrapped = after["statusLine"]["command"].as_str().unwrap();
    assert!(rewrapped.contains(" statusline -- "), "{rewrapped}");
    assert!(
        rewrapped.ends_with("~/.claude/statusline.py"),
        "{rewrapped}"
    );
    let manifest = bench.manifest_json();
    assert_eq!(manifest["restorable"], false);
    assert_eq!(manifest["statusline"]["after"], rewrapped);
    assert_eq!(
        manifest["original"]["kind"], "backup",
        "the first install's backup is still named"
    );

    // Uninstall takes C4's entries out one by one and leaves the rest.
    let text = ok(&bench.uninstall(&[]));
    assert!(
        text.contains(
            "settings.json: removed 17 hook handlers from 17 events and the statusLine wrapper; other changes kept"
        ),
        "{text}"
    );
    let after = bench.settings_json();
    for (event, _, _) in HOOK_EVENTS {
        assert!(
            !handlers_of(&after, event).contains(&hook.as_str()),
            "{event}"
        );
    }
    assert!(
        handlers_of(&after, "Stop")
            .iter()
            .any(|c| c.ends_with("hooks/turn_end.py"))
    );
    assert!(handlers_of(&after, "SessionStart").len() >= 7);
    assert_eq!(after["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "312500");
    assert_eq!(after["env"]["MCP_TIMEOUT"], "30000");
    assert_eq!(after["permissions"], before["permissions"]);
    let command = after["statusLine"]["command"].as_str().unwrap();
    assert!(!command.contains("c4-shim"), "{command}");
    assert!(command.ends_with("~/.claude/statusline.py"), "{command}");
    assert!(!bench.manifest().exists());
    assert!(!bench.shim_copy().exists());
    // The agents installer's own backups are untouched beside C4's.
    let agents_backups = std::fs::read_dir(bench.claude_home())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("settings.json.bak-") && !n.contains("bak-c4-"))
        .count();
    assert!(agents_backups >= 1);
    assert!(bench.backups().len() >= 3);
}
