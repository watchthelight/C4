// <wtl> project: c4 | file: crates/c4-cli/src/doctor.rs | author: watchthelight </wtl>
//! `c4 doctor`: what discovery would find on this machine and why it might find nothing. It reads
//! what discovery reads, checks whether `settings.json` exists without opening it, runs
//! `claude --version` and the session list command with a time budget, and asks the process table
//! about its own pid. Paths are redacted in the text and in the JSON, so a report can go into an
//! issue as it is.

use crate::args::DoctorArgs;
use crate::{OUTPUT_SCHEMA, redact_value};
use c4_core::redact::redact;
use c4_discovery::{
    AgentsCommand, ClaudeHome, ProcessProbe, SysinfoProbe, resolve_on_path, scan_daemon,
    scan_registry,
};
use serde::Serialize;
use std::io::Write;
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
    /// True when `settings.json` exists. It is never opened.
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
    Report {
        schema: OUTPUT_SCHEMA,
        c4_version: c4_core::VERSION,
        os: std::env::consts::OS,
        home: home_report,
        claude,
        session_list,
        process_table,
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
            version: c
                .stdout
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string),
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

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
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
    if !r.notes.is_empty() {
        writeln!(out, "notes:")?;
        for note in &r.notes {
            writeln!(out, "  - {}", redact(note))?;
        }
    }
    Ok(())
}
