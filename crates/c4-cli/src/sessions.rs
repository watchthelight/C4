// <wtl> project: c4 | file: crates/c4-cli/src/sessions.rs | author: watchthelight </wtl>
//! `c4 sessions`: one discovery pass printed as a table or as JSON, or a watch loop that prints
//! every state change. Discovery only: the registry, the session list command, the supervisor
//! files and the process table. No transcript is read, so a session that has written a gigabyte
//! of transcript costs the same to list as one that has written nothing.

use crate::args::SessionsArgs;
use crate::{OUTPUT_SCHEMA, redact_value};
use c4_core::redact::redact;
use c4_core::{SessionId, SessionView, Timestamp, Transition};
use c4_discovery::{
    AgentsCommand, ClaudeHome, Discovery, DiscoveryOptions, ProcessProbe, SnapshotReport,
    SysinfoProbe,
};
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;

const STATE_WIDTH: usize = 26;
const NAME_WIDTH: usize = 22;
const KIND_WIDTH: usize = 11;
const HOST_WIDTH: usize = 16;
const ATTACH_WIDTH: usize = 12;
const PID_WIDTH: usize = 7;
const VERSION_WIDTH: usize = 8;

/// Run the command.
pub fn run(args: &SessionsArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let home = args.home.resolve();
    let mut disc = Discovery::new(options(&home, args));
    let mut probe = SysinfoProbe::new();
    let now = Timestamp::now();
    let res = disc.refresh(&mut probe, now);
    if args.json {
        let mut value = snapshot_json(&home, &disc, &res.report, now);
        if args.redact {
            value = redact_value(value);
        }
        if args.watch {
            writeln!(out, "{value}")?;
        } else {
            writeln!(out, "{}", serde_json::to_string_pretty(&value)?)?;
        }
    } else {
        write_table(out, &home, &disc, &res.report)?;
    }
    out.flush()?;
    if args.watch {
        watch(args, &mut disc, &mut probe, out)?;
    }
    Ok(())
}

/// The discovery options the flags describe.
#[must_use]
pub fn options(home: &ClaudeHome, args: &SessionsArgs) -> DiscoveryOptions {
    let mut opts = DiscoveryOptions::files_only(home.clone());
    if !args.files_only {
        opts = opts.with_agents(AgentsCommand::from_env());
    }
    opts.agents_timeout = Duration::from_millis(args.agents_timeout_ms);
    opts
}

/// The JSON document of one pass: the views in attention order and the snapshot report.
#[must_use]
pub fn snapshot_json(
    home: &ClaudeHome,
    disc: &Discovery,
    report: &SnapshotReport,
    at: Timestamp,
) -> serde_json::Value {
    serde_json::json!({
        "schema": OUTPUT_SCHEMA,
        "at": at,
        "home": home.root().display().to_string(),
        "sessions": disc.views(),
        "report": report,
    })
}

fn write_table(
    out: &mut dyn Write,
    home: &ClaudeHome,
    disc: &Discovery,
    report: &SnapshotReport,
) -> std::io::Result<()> {
    let views = disc.views();
    let alive = views.iter().filter(|v| v.alive).count();
    writeln!(
        out,
        "{} session{} ({alive} alive) in {}, {} ms",
        views.len(),
        if views.len() == 1 { "" } else { "s" },
        redact(&home.root().display().to_string()),
        report.elapsed_ms
    )?;
    if let Some(err) = &report.agents_error {
        writeln!(out, "session list command: {}", redact(err))?;
    }
    for problem in &report.problems {
        writeln!(out, "problem: {}", redact(&problem.to_string()))?;
    }
    if views.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(
        out,
        "{} {} {} {} {} {} {} CWD",
        fit("STATE", STATE_WIDTH),
        fit("NAME", NAME_WIDTH),
        fit("KIND", KIND_WIDTH),
        fit("HOST", HOST_WIDTH),
        fit("ATTACH", ATTACH_WIDTH),
        fit("PID", PID_WIDTH),
        fit("VERSION", VERSION_WIDTH),
    )?;
    for view in views {
        writeln!(out, "{}", row(view))?;
    }
    Ok(())
}

/// One table line. Heuristic states carry a `~`; paths are redacted.
#[must_use]
pub fn row(v: &SessionView) -> String {
    let state = if v.evidence.is_observed() {
        v.state.describe()
    } else {
        format!("~{}", v.state.describe())
    };
    let kind = format!("{:?}", v.session_kind).to_ascii_lowercase();
    let pid = v
        .process
        .map_or_else(|| "-".to_string(), |p| p.pid.to_string());
    format!(
        "{} {} {} {} {} {} {} {}",
        fit(&state, STATE_WIDTH),
        fit(&redact(&v.display_name()), NAME_WIDTH),
        fit(&kind, KIND_WIDTH),
        fit(v.host.label(), HOST_WIDTH),
        fit(v.attachment.label(), ATTACH_WIDTH),
        fit(&pid, PID_WIDTH),
        fit(v.version.as_deref().unwrap_or("-"), VERSION_WIDTH),
        redact(v.cwd.as_deref().unwrap_or("-")),
    )
}

/// Pad or cut `text` to `width` columns; a cut ends in `..`.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return format!("{text:<width$}");
    }
    let cut: String = text.chars().take(width.saturating_sub(2)).collect();
    format!("{cut}..")
}

/// Refresh forever, printing a line for every session that appears and every state change. A
/// session that appears idle has no transition (idle is where a view starts), so the arrival is
/// its own line.
fn watch(
    args: &SessionsArgs,
    disc: &mut Discovery,
    probe: &mut dyn ProcessProbe,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let interval = Duration::from_millis(args.interval_ms.max(100));
    let mut known: BTreeSet<SessionId> = disc.views().iter().map(|v| v.id.clone()).collect();
    loop {
        std::thread::sleep(interval);
        let now = Timestamp::now();
        let res = disc.refresh(probe, now);
        for view in disc.views() {
            if known.insert(view.id.clone()) {
                write_line(out, args, now, view, None)?;
            }
        }
        for (id, t) in &res.transitions {
            if let Some(view) = disc.table().get(id) {
                write_line(out, args, now, view, Some(t))?;
            }
        }
        out.flush()?;
    }
}

/// One watch line: the arrival of `view` when `transition` is `None`, else the change.
fn write_line(
    out: &mut dyn Write,
    args: &SessionsArgs,
    now: Timestamp,
    view: &SessionView,
    transition: Option<&Transition>,
) -> std::io::Result<()> {
    let name = view.display_name();
    if args.json {
        let mut line = match transition {
            Some(t) => serde_json::json!({
                "at": now,
                "event": "transition",
                "session": view.id,
                "name": name,
                "from": t.from,
                "to": t.to,
                "describe": t.to.describe(),
                "evidence": t.evidence,
            }),
            None => serde_json::json!({
                "at": now,
                "event": "new",
                "session": view.id,
                "name": name,
                "to": view.state,
                "describe": view.state.describe(),
                "evidence": view.evidence,
            }),
        };
        if args.redact {
            line = redact_value(line);
        }
        return writeln!(out, "{line}");
    }
    let (change, evidence) = match transition {
        Some(t) => (
            format!("{} -> {}", t.from.describe(), t.to.describe()),
            &t.evidence,
        ),
        None => (format!("new, {}", view.state.describe()), &view.evidence),
    };
    writeln!(
        out,
        "{} {} {}: {change} ({}{})",
        now.to_rfc3339(),
        view.id.short(),
        redact(&name),
        if evidence.is_observed() { "" } else { "~" },
        evidence.source.label(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use c4_core::SessionState;

    #[test]
    fn fit_pads_and_cuts() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdefgh", 5), "abc..");
        assert_eq!(fit("abcde", 5), "abcde");
    }

    #[test]
    fn a_row_marks_heuristic_states_and_redacts_the_cwd() {
        let mut v = SessionView::new(
            SessionId::new("0a1b2c3d-0001-4000-8000-000000000001"),
            Timestamp::from_millis(0),
        );
        v.cwd = Some("/home/example/proj".into());
        v.state = SessionState::Working;
        let line = row(&v);
        assert!(line.starts_with("~working"), "{line}");
        assert!(line.contains("proj"), "{line}");
        assert!(!line.contains("/home/example"), "{line}");
    }
}
