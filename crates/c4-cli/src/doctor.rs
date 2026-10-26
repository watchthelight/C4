// <wtl> project: c4 | file: crates/c4-cli/src/doctor.rs | author: watchthelight </wtl>
//! `c4 doctor`: what discovery would find on this machine and why it might find nothing, and
//! whether C4's own parts are in place. It reads what discovery reads, runs `claude --version` and
//! the session list command with a time budget, asks the process table about its own pid, and then
//! turns to the C4 home: the shim copy against the manifest and against the sidecar beside this
//! executable, the copy's own `--version` and `endpoint`, the install as the manifest recorded it
//! against what `settings.json` holds now, the ingest endpoint and the spool. `settings.json` is
//! opened for C4's own entries alone; nothing else in it is reported. Paths are redacted in the
//! text and in the JSON, so a report can go into an issue as it is.

use crate::args::DoctorArgs;
use crate::install::{
    HOOK_EVENTS, Manifest, ShimCommand, StatuslineOutcome, apply_install, default_shim_source,
    read_manifest, read_settings, sha256_hex, shim_file_name,
};
use crate::{OUTPUT_SCHEMA, redact_value};
use c4_core::C4Home;
use c4_core::home::{HOME_ENV as C4_HOME_ENV, user_home};
use c4_core::redact::redact;
use c4_discovery::{
    AgentsCommand, ClaudeHome, ProcessProbe, SysinfoProbe, resolve_on_path, scan_daemon,
    scan_registry,
};
use c4_ingest::Probe;
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// The report.
#[derive(Serialize, Debug)]
pub struct Report {
    /// The output schema version.
    pub schema: u32,
    /// The C4 version.
    pub c4_version: &'static str,
    /// The operating system, as Rust names it.
    pub os: &'static str,
    /// The Claude home.
    pub home: HomeReport,
    /// The `claude` command, unless commands were skipped.
    pub claude: Option<ClaudeReport>,
    /// The session list command, unless commands were skipped.
    pub session_list: Option<CommandReport>,
    /// The process table.
    pub process_table: ProcessReport,
    /// C4's own parts: the C4 home, the shim, the install, the endpoint and the spool.
    pub c4: C4Report,
    /// Things worth knowing, in plain words.
    pub notes: Vec<String>,
}

/// The Claude home and what it holds.
#[derive(Serialize, Debug)]
pub struct HomeReport {
    /// The root directory.
    pub path: String,
    /// What decided it: `--home`, an environment variable, or the default.
    pub source: &'static str,
    /// True when the root exists.
    pub exists: bool,
    /// True when `sessions/` exists.
    pub sessions_dir: bool,
    /// Registry records read.
    pub registry_entries: usize,
    /// Files under `sessions/` that are not records (key files and the like).
    pub registry_skipped: usize,
    /// Records that could not be parsed.
    pub registry_problems: usize,
    /// True when the background supervisor's roster exists.
    pub roster: bool,
    /// Background job directories.
    pub jobs: usize,
    /// True when `projects/` exists.
    pub projects_dir: bool,
    /// True when `settings.json` exists. The install audit under `c4` is what opens it.
    pub settings_json: bool,
}

/// The `claude` command.
#[derive(Serialize, Debug)]
pub struct ClaudeReport {
    /// Where PATH resolves it, when it does.
    pub on_path: Option<String>,
    /// The first line `claude --version` printed.
    pub version: Option<String>,
    /// Why no version came back.
    pub error: Option<String>,
    /// How long `claude --version` took.
    pub elapsed_ms: Option<u64>,
}

/// The session list command.
#[derive(Serialize, Debug)]
pub struct CommandReport {
    /// The command line.
    pub command: String,
    /// Entries it listed.
    pub entries: Option<usize>,
    /// Why it listed nothing.
    pub error: Option<String>,
    /// How long it took.
    pub elapsed_ms: u64,
}

/// The process table, checked against this very process.
#[derive(Serialize, Debug)]
pub struct ProcessReport {
    /// This process.
    pub own_pid: u32,
    /// True when the table lists it.
    pub found: bool,
    /// True when the table gives it a start time, which the liveness check needs.
    pub start_time: bool,
    /// The name the table gives it.
    pub name: Option<String>,
}

/// C4's own parts.
#[derive(Serialize, Debug)]
pub struct C4Report {
    /// The C4 home.
    pub home: C4HomeReport,
    /// The shim copy and the sidecar.
    pub shim: ShimReport,
    /// The install, as recorded and as `settings.json` holds it now.
    pub install: InstallAudit,
    /// The ingest endpoint.
    pub endpoint: EndpointReport,
    /// The spool.
    pub spool: SpoolReport,
}

/// The C4 home.
#[derive(Serialize, Debug)]
pub struct C4HomeReport {
    /// The root directory.
    pub path: String,
    /// What decided it: `--c4-home`, the environment variable, or the default.
    pub source: &'static str,
    /// True when the root exists.
    pub exists: bool,
}

/// The shim: the copy in the C4 home and the sidecar beside this executable.
#[derive(Serialize, Debug)]
pub struct ShimReport {
    /// Where `c4 install` puts the copy.
    pub copy: String,
    /// True when the copy is there.
    pub copy_present: bool,
    /// Whether the copy is the one the manifest recorded; nothing without a manifest or a copy.
    pub copy_matches_manifest: Option<bool>,
    /// The `c4-shim` beside this executable, when this executable's own path is known.
    pub sidecar: Option<String>,
    /// True when the sidecar is there.
    pub sidecar_present: bool,
    /// Whether the copy and the sidecar are the same bytes; nothing when either is missing.
    pub copy_matches_sidecar: Option<bool>,
    /// What the copy printed for `--version`, unless commands were skipped or there is no copy.
    pub version: Option<String>,
    /// What the copy printed for `endpoint`.
    pub endpoint: Option<String>,
    /// Whether that is the endpoint this C4 listens on.
    pub endpoint_agrees: Option<bool>,
    /// Why the copy could not be run, or how it failed.
    pub error: Option<String>,
    /// How long the two runs took together.
    pub elapsed_ms: Option<u64>,
}

/// The install, from the manifest and from `settings.json` as it is now.
#[derive(Serialize, Debug)]
pub struct InstallAudit {
    /// Where the manifest lives.
    pub manifest: String,
    /// True when a manifest was read.
    pub recorded: bool,
    /// The settings file the manifest names.
    pub recorded_settings: Option<String>,
    /// When the first install ran, RFC 3339.
    pub installed_at: Option<String>,
    /// When the manifest was last written.
    pub updated_at: Option<String>,
    /// Whether `c4 uninstall` would restore the backup byte for byte.
    pub restorable: Option<bool>,
    /// `settings.json` now.
    pub settings: SettingsState,
    /// Events whose hook groups carry this C4 home's shim.
    pub hooks_present: usize,
    /// Events `c4 install` wires.
    pub hooks_total: usize,
    /// The status line command now.
    pub statusline: StatuslineState,
    /// Hook handlers that run a `c4-shim` of another C4 home, as `event: command`.
    pub foreign: Vec<String>,
}

/// `settings.json` against the manifest.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsState {
    /// The bytes `c4 install` last wrote.
    AsInstalled,
    /// Something wrote the file since `c4 install`.
    Changed,
    /// The file exists and no manifest records an install.
    Present,
    /// No file.
    Missing,
    /// Not UTF-8, not JSON or not an object; the notes say which.
    Unreadable,
}

/// The status line command as it is now.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatuslineState {
    /// Wrapped by this C4 home's shim.
    Wrapped,
    /// A command without the wrapper.
    Unwrapped,
    /// No `statusLine` in the file.
    Absent,
    /// Wrapped by a `c4-shim` of another C4 home.
    WrappedElsewhere,
    /// A `statusLine` that is not a command, or whose command names no program.
    NotACommand,
    /// Could not be read.
    Unknown,
}

/// The ingest endpoint.
#[derive(Serialize, Debug)]
pub struct EndpointReport {
    /// The pipe name or socket path.
    pub name: String,
    /// Whether something listens.
    pub state: EndpointState,
    /// Why the state is unknown.
    pub detail: Option<String>,
}

/// What a connection attempt found.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointState {
    /// Something accepted the connection: the app is running.
    Listening,
    /// Nothing listens; the shim spools.
    Free,
    /// Could not tell.
    Unknown,
}

/// The spool the shim writes to while nothing listens.
#[derive(Serialize, Debug)]
pub struct SpoolReport {
    /// The directory.
    pub dir: String,
    /// Spool files waiting.
    pub files: usize,
    /// Their size together.
    pub bytes: u64,
}

const NOT_INSTALLED: &str =
    "c4 install has not run for this C4 home; settings.json carries none of C4's entries";

/// Run the command.
pub fn run(args: &DoctorArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let report = examine(args);
    if args.json {
        let value = redact_value(serde_json::to_value(&report)?);
        writeln!(out, "{}", serde_json::to_string_pretty(&value)?)?;
    } else {
        write_text(out, &report)?;
    }
    out.flush()?;
    Ok(())
}

/// Gather the report without printing it.
#[must_use]
pub fn examine(args: &DoctorArgs) -> Report {
    let home = args.home.resolve();
    let c4_home = args.c4_home.resolve();
    let timeout = Duration::from_millis(args.timeout_ms);
    let mut notes = Vec::new();
    let home_report = examine_home(&home, args.home.source(), &mut notes);
    let (claude, session_list) = if args.no_commands {
        notes.push("commands skipped (--no-commands)".into());
        (None, None)
    } else {
        (
            Some(examine_claude(timeout, &mut notes)),
            Some(examine_session_list(timeout, &mut notes)),
        )
    };
    let process_table = examine_process_table(&mut notes);
    let c4 = examine_c4(
        &c4_home,
        args.c4_home.source(),
        &home,
        !args.no_commands,
        timeout,
        &mut notes,
    );
    Report {
        schema: OUTPUT_SCHEMA,
        c4_version: c4_core::VERSION,
        os: std::env::consts::OS,
        home: home_report,
        claude,
        session_list,
        process_table,
        c4,
        notes,
    }
}

fn examine_home(home: &ClaudeHome, source: &'static str, notes: &mut Vec<String>) -> HomeReport {
    let exists = home.exists();
    let sessions_dir = home.sessions_dir().is_dir();
    let registry = scan_registry(&home.sessions_dir());
    let daemon = scan_daemon(home);
    let settings_json = home.settings_path().is_file();
    if !exists {
        notes.push("the Claude home does not exist; there is nothing to discover".into());
    } else if !sessions_dir {
        notes.push(
            "no sessions directory: no Claude Code session has registered under this home".into(),
        );
    }
    if exists && !settings_json {
        notes.push(
            "no settings.json under the home: hooks and the status line are not configured there"
                .into(),
        );
    }
    for problem in &registry.problems {
        notes.push(format!("registry: {problem}"));
    }
    for problem in &daemon.problems {
        notes.push(format!("supervisor files: {problem}"));
    }
    HomeReport {
        path: home.root().display().to_string(),
        source,
        exists,
        sessions_dir,
        registry_entries: registry.entries.len(),
        registry_skipped: registry.skipped.len(),
        registry_problems: registry.problems.len(),
        roster: daemon.roster.is_some(),
        jobs: daemon.jobs.len(),
        projects_dir: home.projects_dir().is_dir(),
        settings_json,
    }
}

/// The first line with anything on it.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

fn examine_claude(timeout: Duration, notes: &mut Vec<String>) -> ClaudeReport {
    let on_path = resolve_on_path("claude").map(|p| p.display().to_string());
    if on_path.is_none() {
        notes.push(
            "claude is not on PATH: the session list command cannot run; discovery works from the files alone"
                .into(),
        );
    }
    match AgentsCommand::new("claude", ["--version"]).capture(timeout) {
        Ok(c) if c.success => ClaudeReport {
            on_path,
            version: first_line(&c.stdout),
            error: None,
            elapsed_ms: Some(c.elapsed.as_millis() as u64),
        },
        Ok(c) => ClaudeReport {
            on_path,
            version: None,
            error: Some(format!(
                "claude --version failed ({}): {}",
                c.status,
                c.stderr.trim()
            )),
            elapsed_ms: Some(c.elapsed.as_millis() as u64),
        },
        Err(e) => ClaudeReport {
            on_path,
            version: None,
            error: Some(e.to_string()),
            elapsed_ms: None,
        },
    }
}

fn examine_session_list(timeout: Duration, notes: &mut Vec<String>) -> CommandReport {
    let command = AgentsCommand::from_env();
    let text = std::iter::once(command.program.as_str())
        .chain(command.args.iter().map(String::as_str))
        .collect::<Vec<&str>>()
        .join(" ");
    let started = Instant::now();
    let result = command.run(timeout);
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(entries) => CommandReport {
            command: text,
            entries: Some(entries.len()),
            error: None,
            elapsed_ms,
        },
        Err(e) => {
            notes.push(format!("session list: {e}"));
            CommandReport {
                command: text,
                entries: None,
                error: Some(e.to_string()),
                elapsed_ms,
            }
        }
    }
}

fn examine_process_table(notes: &mut Vec<String>) -> ProcessReport {
    let own_pid = std::process::id();
    let info = SysinfoProbe::new().probe(own_pid);
    let found = info.is_some();
    let start_time = info.as_ref().is_some_and(|i| i.start_time_secs.is_some());
    if !start_time {
        notes.push(
            "the process table gives no start time for this process; liveness checks fall back to the process name"
                .into(),
        );
    }
    ProcessReport {
        own_pid,
        found,
        start_time,
        name: info.and_then(|i| i.name),
    }
}

fn examine_c4(
    c4_home: &C4Home,
    source: &'static str,
    claude_home: &ClaudeHome,
    run_commands: bool,
    timeout: Duration,
    notes: &mut Vec<String>,
) -> C4Report {
    let manifest = match read_manifest(c4_home) {
        Ok(m) => m,
        Err(e) => {
            notes.push(format!("manifest: {e:#}"));
            None
        }
    };
    let shim_command = match ShimCommand::for_home(c4_home, &user_home()) {
        Ok(s) => Some(s),
        Err(e) => {
            notes.push(format!(
                "the C4 home cannot be named in settings.json: {e:#}"
            ));
            None
        }
    };
    let shim = examine_shim(c4_home, manifest.as_ref(), run_commands, timeout, notes);
    let install = audit_install(
        c4_home,
        claude_home,
        manifest.as_ref(),
        shim_command.as_ref(),
        shim.copy_present,
        notes,
    );
    let wired = install.hooks_present > 0 || install.statusline == StatuslineState::Wrapped;
    let endpoint = examine_endpoint(c4_home, wired, notes);
    let spool = examine_spool(c4_home, notes);
    C4Report {
        home: C4HomeReport {
            path: c4_home.root().display().to_string(),
            source,
            exists: c4_home.root().is_dir(),
        },
        shim,
        install,
        endpoint,
        spool,
    }
}

/// The file's bytes, `None` when it does not exist; any other failure becomes a note.
fn read_optional(path: &Path, notes: &mut Vec<String>) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            notes.push(format!("could not read {}: {e}", path.display()));
            None
        }
    }
}

fn examine_shim(
    c4_home: &C4Home,
    manifest: Option<&Manifest>,
    run_commands: bool,
    timeout: Duration,
    notes: &mut Vec<String>,
) -> ShimReport {
    let copy_path = c4_home.bin_dir().join(shim_file_name());
    let copy = read_optional(&copy_path, notes);
    let sidecar_path = default_shim_source().ok();
    let sidecar = sidecar_path
        .as_deref()
        .and_then(|p| read_optional(p, notes));
    let copy_matches_manifest = match (&copy, manifest) {
        (Some(bytes), Some(m)) => Some(sha256_hex(bytes) == m.shim.sha256),
        _ => None,
    };
    let copy_matches_sidecar = match (&copy, &sidecar) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    if copy_matches_manifest == Some(false) {
        notes.push(
            "the shim copy is not the one c4 install wrote; c4 install puts the current one back"
                .into(),
        );
    } else if copy_matches_sidecar == Some(false) {
        notes.push(
            "the shim copy differs from the c4-shim beside this executable; c4 install refreshes it"
                .into(),
        );
    }
    let mut report = ShimReport {
        copy: copy_path.display().to_string(),
        copy_present: copy.is_some(),
        copy_matches_manifest,
        sidecar: sidecar_path.map(|p| p.display().to_string()),
        sidecar_present: sidecar.is_some(),
        copy_matches_sidecar,
        version: None,
        endpoint: None,
        endpoint_agrees: None,
        error: None,
        elapsed_ms: None,
    };
    if run_commands && copy.is_some() {
        run_shim(c4_home, &copy_path, timeout, &mut report, notes);
    }
    report
}

/// Run the copy with `--version` and `endpoint`, with the C4 home in its environment so it resolves
/// the same home this process did.
fn run_shim(
    c4_home: &C4Home,
    copy: &Path,
    timeout: Duration,
    report: &mut ShimReport,
    notes: &mut Vec<String>,
) {
    let program = copy.display().to_string();
    let home = c4_home.root().display().to_string();
    let command = |arg: &str| {
        AgentsCommand::new(program.as_str(), [arg]).with_env(C4_HOME_ENV, home.as_str())
    };
    let started = Instant::now();
    match command("--version").capture(timeout) {
        Ok(c) if c.success => report.version = first_line(&c.stdout),
        Ok(c) => {
            report.error = Some(format!(
                "--version failed ({}): {}",
                c.status,
                c.stderr.trim()
            ));
        }
        Err(e) => report.error = Some(e.to_string()),
    }
    if report.error.is_none() {
        match command("endpoint").capture(timeout) {
            Ok(c) if c.success => {
                let printed = first_line(&c.stdout);
                report.endpoint_agrees = printed.as_deref().map(|p| p == c4_home.endpoint());
                report.endpoint = printed;
            }
            Ok(c) => {
                report.error = Some(format!(
                    "endpoint failed ({}): {}",
                    c.status,
                    c.stderr.trim()
                ));
            }
            Err(e) => report.error = Some(e.to_string()),
        }
    }
    report.elapsed_ms = Some(started.elapsed().as_millis() as u64);
    if let Some(e) = &report.error {
        notes.push(format!("the shim copy did not run: {e}"));
    }
    if let (Some(theirs), Some(false)) = (&report.endpoint, report.endpoint_agrees) {
        notes.push(format!(
            "the shim copy would connect to {theirs} while this C4 listens on {}; the two resolve the C4 home differently",
            c4_home.endpoint()
        ));
    }
    if let Some(v) = &report.version
        && report.copy_matches_sidecar != Some(true)
        && !v.ends_with(c4_core::VERSION)
    {
        notes.push(format!(
            "the shim copy reports {v}; this is C4 {}; c4 install refreshes it",
            c4_core::VERSION
        ));
    }
}

/// Errors from the install code say "nothing was changed", which is beside the point here.
fn plain(e: &anyhow::Error) -> String {
    format!("{e:#}").replace("; nothing was changed", "")
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn audit_install(
    c4_home: &C4Home,
    claude_home: &ClaudeHome,
    manifest: Option<&Manifest>,
    shim: Option<&ShimCommand>,
    copy_present: bool,
    notes: &mut Vec<String>,
) -> InstallAudit {
    let settings_path = claude_home.settings_path();
    let mut audit = InstallAudit {
        manifest: c4_home.manifest_path().display().to_string(),
        recorded: manifest.is_some(),
        recorded_settings: manifest.map(|m| m.settings_path.clone()),
        installed_at: manifest.map(|m| m.installed_at.to_rfc3339()),
        updated_at: manifest.map(|m| m.updated_at.to_rfc3339()),
        restorable: manifest.map(|m| m.restorable),
        settings: SettingsState::Missing,
        hooks_present: 0,
        hooks_total: HOOK_EVENTS.len(),
        statusline: StatuslineState::Absent,
        foreign: Vec::new(),
    };
    if let Some(m) = manifest
        && !same_path(Path::new(&m.settings_path), &settings_path)
    {
        notes.push(format!(
            "the manifest records an install into {}, not this Claude home's settings.json",
            m.settings_path
        ));
    }
    let file = match read_settings(&settings_path) {
        Ok(f) => f,
        Err(e) => {
            audit.settings = SettingsState::Unreadable;
            audit.statusline = StatuslineState::Unknown;
            notes.push(format!("settings.json: {}", plain(&e)));
            return audit;
        }
    };
    let Some(bytes) = &file.bytes else {
        notes.push(if manifest.is_some() {
            "settings.json is gone although the manifest records an install into it; c4 install writes it again, c4 uninstall drops the record".to_string()
        } else {
            NOT_INSTALLED.to_string()
        });
        return audit;
    };
    audit.settings = match manifest {
        Some(m) if sha256_hex(bytes) == m.after_sha256 => SettingsState::AsInstalled,
        Some(_) => SettingsState::Changed,
        None => SettingsState::Present,
    };
    let Some(shim) = shim else {
        audit.statusline = StatuslineState::Unknown;
        return audit;
    };
    let mut probe = file.map.clone();
    let plan = match apply_install(&mut probe, shim) {
        Ok(plan) => plan,
        Err(e) => {
            audit.statusline = StatuslineState::Unknown;
            notes.push(format!("settings.json: {}", plain(&e)));
            return audit;
        }
    };
    audit.hooks_present = plan.present.len();
    audit.statusline = match plan.statusline {
        StatuslineOutcome::Absent => StatuslineState::Absent,
        StatuslineOutcome::NotACommand | StatuslineOutcome::Empty => StatuslineState::NotACommand,
        StatuslineOutcome::AlreadyWrapped => StatuslineState::Wrapped,
        StatuslineOutcome::WrappedElsewhere(_) => StatuslineState::WrappedElsewhere,
        StatuslineOutcome::Wrapped { .. } => StatuslineState::Unwrapped,
    };
    audit.foreign = plan.foreign;
    let mut gaps = Vec::new();
    if !plan.added.is_empty() {
        gaps.push(format!(
            "{} of {} hook groups missing",
            plan.added.len(),
            HOOK_EVENTS.len()
        ));
    }
    if audit.statusline == StatuslineState::Unwrapped {
        gaps.push("the status line is not wrapped".to_string());
    }
    let wired = audit.hooks_present > 0 || audit.statusline == StatuslineState::Wrapped;
    match (manifest.is_some(), gaps.is_empty()) {
        (true, true) => {}
        (true, false) => notes.push(format!(
            "the install is not whole: {}; run c4 install again",
            gaps.join(" and ")
        )),
        (false, _) if !wired => notes.push(NOT_INSTALLED.to_string()),
        (false, _) => notes.push(
            "settings.json carries C4's entries but this C4 home has no manifest; c4 install records one"
                .to_string(),
        ),
    }
    if !audit.foreign.is_empty() {
        notes.push(
            "a c4-shim of another C4 home is wired in (c4.install.foreign lists it); c4 uninstall with that --c4-home takes it out"
                .to_string(),
        );
    }
    if wired && !copy_present {
        notes.push(
            "the shim copy is missing while settings.json names it: every hook and status line refresh fails until c4 install puts it back, or c4 uninstall takes the entries out"
                .to_string(),
        );
    }
    audit
}

fn examine_endpoint(c4_home: &C4Home, wired: bool, notes: &mut Vec<String>) -> EndpointReport {
    let name = c4_home.endpoint();
    let (state, detail) = match c4_ingest::probe(c4_home) {
        Probe::Listening => (EndpointState::Listening, None),
        Probe::Free => {
            if wired {
                notes.push(
                    "nothing listens on the endpoint: the app is not running, so the shim spools what it receives"
                        .into(),
                );
            }
            (EndpointState::Free, None)
        }
        Probe::Unknown(why) => {
            notes.push(format!("endpoint: {why}"));
            (EndpointState::Unknown, Some(why))
        }
    };
    EndpointReport {
        name,
        state,
        detail,
    }
}

fn examine_spool(c4_home: &C4Home, notes: &mut Vec<String>) -> SpoolReport {
    let dir = c4_home.spool_dir();
    let mut files = 0;
    let mut bytes = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl")
                && let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    if files > 0 {
        notes.push(format!(
            "{files} spooled {}, {bytes} bytes, wait for the app to replay them",
            plural(files, "file", "files")
        ));
    }
    SpoolReport {
        dir: dir.display().to_string(),
        files,
        bytes,
    }
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 { one } else { many }
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn copy_text(s: &ShimReport) -> String {
    if !s.copy_present {
        return "missing".to_string();
    }
    let manifest = match s.copy_matches_manifest {
        Some(true) => "as the manifest recorded",
        Some(false) => "not what the manifest recorded",
        None => "present",
    };
    let sidecar = match s.copy_matches_sidecar {
        Some(true) => ", same as the sidecar",
        Some(false) => ", differs from the sidecar",
        None => "",
    };
    format!("{manifest}{sidecar}")
}

fn shim_run_text(s: &ShimReport) -> String {
    if let Some(v) = &s.version {
        let endpoint = match s.endpoint_agrees {
            Some(true) => "endpoint agrees",
            Some(false) => "endpoint differs",
            None => "endpoint not printed",
        };
        let took = s
            .elapsed_ms
            .map_or(String::new(), |ms| format!(", {ms} ms"));
        format!("{v}, {endpoint}{took}")
    } else if let Some(e) = &s.error {
        format!("error: {}", redact(e))
    } else if s.copy_present {
        "not checked".to_string()
    } else {
        "no copy to run".to_string()
    }
}

fn settings_text(s: SettingsState) -> &'static str {
    match s {
        SettingsState::AsInstalled => "as c4 install left it",
        SettingsState::Changed => "changed since c4 install",
        SettingsState::Present => "present, no install recorded",
        SettingsState::Missing => "missing",
        SettingsState::Unreadable => "unreadable",
    }
}

fn statusline_text(s: StatuslineState) -> &'static str {
    match s {
        StatuslineState::Wrapped => "wrapped",
        StatuslineState::Unwrapped => "not wrapped",
        StatuslineState::Absent => "absent",
        StatuslineState::WrappedElsewhere => "wrapped by another C4 home's shim",
        StatuslineState::NotACommand => "not a command",
        StatuslineState::Unknown => "unknown",
    }
}

fn endpoint_text(e: &EndpointReport) -> String {
    match e.state {
        EndpointState::Listening => "listening".to_string(),
        EndpointState::Free => "free (nothing listens; the shim spools)".to_string(),
        EndpointState::Unknown => format!(
            "unknown: {}",
            e.detail.as_deref().map_or("no reason", |d| d)
        ),
    }
}

fn write_text(out: &mut dyn Write, r: &Report) -> std::io::Result<()> {
    writeln!(out, "c4 {} on {}", r.c4_version, r.os)?;
    writeln!(
        out,
        "home: {} (from {})",
        redact(&r.home.path),
        r.home.source
    )?;
    writeln!(out, "  exists: {}", yes(r.home.exists))?;
    writeln!(
        out,
        "  sessions/: {}, {} records, {} other files, {} unreadable",
        yes(r.home.sessions_dir),
        r.home.registry_entries,
        r.home.registry_skipped,
        r.home.registry_problems
    )?;
    writeln!(out, "  daemon/roster.json: {}", yes(r.home.roster))?;
    writeln!(out, "  jobs/: {}", r.home.jobs)?;
    writeln!(out, "  projects/: {}", yes(r.home.projects_dir))?;
    writeln!(out, "  settings.json: {}", yes(r.home.settings_json))?;
    match &r.claude {
        Some(c) => {
            writeln!(
                out,
                "claude: {}",
                c.on_path
                    .as_deref()
                    .map_or_else(|| "not on PATH".to_string(), redact)
            )?;
            if let Some(v) = &c.version {
                let took = c
                    .elapsed_ms
                    .map_or(String::new(), |ms| format!(", {ms} ms"));
                writeln!(out, "  version: {v}{took}")?;
            }
            if let Some(e) = &c.error {
                writeln!(out, "  error: {}", redact(e))?;
            }
        }
        None => writeln!(out, "claude: not checked")?,
    }
    match &r.session_list {
        Some(s) => {
            writeln!(out, "session list: {}", redact(&s.command))?;
            match (s.entries, &s.error) {
                (Some(n), _) => writeln!(out, "  result: {n} entries, {} ms", s.elapsed_ms)?,
                (None, Some(e)) => writeln!(out, "  error: {}", redact(e))?,
                (None, None) => writeln!(out, "  result: nothing")?,
            }
        }
        None => writeln!(out, "session list: not checked")?,
    }
    let table = if r.process_table.start_time {
        "ok"
    } else if r.process_table.found {
        "found, no start time"
    } else {
        "own pid not found"
    };
    writeln!(
        out,
        "process table: {table} (pid {})",
        r.process_table.own_pid
    )?;
    let c = &r.c4;
    writeln!(
        out,
        "c4 home: {} (from {})",
        redact(&c.home.path),
        c.home.source
    )?;
    writeln!(out, "  exists: {}", yes(c.home.exists))?;
    writeln!(
        out,
        "  shim copy: {}: {}",
        redact(&c.shim.copy),
        copy_text(&c.shim)
    )?;
    match &c.shim.sidecar {
        Some(p) => writeln!(
            out,
            "  sidecar: {}: {}",
            redact(p),
            if c.shim.sidecar_present {
                "present"
            } else {
                "missing"
            }
        )?,
        None => writeln!(
            out,
            "  sidecar: unknown, this executable's own path could not be found"
        )?,
    }
    writeln!(out, "  shim run: {}", shim_run_text(&c.shim))?;
    match (&c.install.installed_at, &c.install.updated_at) {
        (Some(installed), Some(updated)) => writeln!(
            out,
            "  manifest: {}: installed {installed}, updated {updated}, restorable {}",
            redact(&c.install.manifest),
            c.install.restorable.map_or("unknown", yes)
        )?,
        _ => writeln!(out, "  manifest: none at {}", redact(&c.install.manifest))?,
    }
    writeln!(
        out,
        "  settings.json: {}",
        settings_text(c.install.settings)
    )?;
    if matches!(
        c.install.settings,
        SettingsState::AsInstalled | SettingsState::Changed | SettingsState::Present
    ) {
        writeln!(
            out,
            "    hooks: {} of {} groups present; statusLine: {}",
            c.install.hooks_present,
            c.install.hooks_total,
            statusline_text(c.install.statusline)
        )?;
    }
    for f in &c.install.foreign {
        writeln!(out, "    another C4 home's shim: {}", redact(f))?;
    }
    writeln!(
        out,
        "  endpoint: {}: {}",
        redact(&c.endpoint.name),
        endpoint_text(&c.endpoint)
    )?;
    writeln!(
        out,
        "  spool: {} {}, {} bytes",
        c.spool.files,
        plural(c.spool.files, "file", "files"),
        c.spool.bytes
    )?;
    if !r.notes.is_empty() {
        writeln!(out, "notes:")?;
        for note in &r.notes {
            writeln!(out, "  - {}", redact(note))?;
        }
    }
    Ok(())
}
