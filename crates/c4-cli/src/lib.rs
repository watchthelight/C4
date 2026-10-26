// <wtl> project: c4 | file: crates/c4-cli/src/lib.rs | author: watchthelight </wtl>
//! The headless side of C4: the `c4` subcommands that run without a window. The desktop binary
//! parses the same [`Cli`] and calls [`run`] before it starts a webview, so `c4 sessions --json`
//! behaves the same from either executable.
//!
//! - `c4 sessions`: one discovery pass, as a table or as JSON, or a watch loop that prints state
//!   changes. Discovery only; no transcript is read.
//! - `c4 doctor`: where the Claude home is, what it holds, whether `claude` runs and whether the
//!   process table answers; then C4's own parts: the shim copy and the sidecar, the install as the
//!   manifest records it against `settings.json` now, the ingest endpoint and the spool. The
//!   settings file is opened for C4's own entries alone.
//! - `c4 telemetry`: the token totals, cost, agents and recorded state of one transcript file.
//! - `c4 install` and `c4 uninstall`: the shim copied into the C4 home and wired into
//!   `settings.json`, with a backup beside the file and a manifest for the way back.
//!
//! Every command reads the Claude home and never writes to it, except `install` and `uninstall`,
//! which write `settings.json` alone, each time after a backup, and record what they touched.

pub mod args;
pub mod doctor;
pub mod install;
pub mod sessions;
pub mod telemetry;

pub use args::{
    C4HomeArgs, Cli, Command, DoctorArgs, HomeArgs, InstallArgs, SessionsArgs, TelemetryArgs,
    UninstallArgs,
};

use clap::Parser;
use std::process::ExitCode;

/// The version of the JSON shapes the commands print. Bumped when a field changes meaning or goes
/// away; a new field does not bump it.
pub const OUTPUT_SCHEMA: u32 = 1;

/// Parse the process arguments and run the command. The exit code is 0 when it succeeded, 1 when
/// it could not do its job (the reason goes to stderr) and 2 for a usage error.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("c4: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Run a parsed command line, writing to stdout.
pub fn run(cli: &Cli) -> anyhow::Result<()> {
    if let Some(filter) = &cli.log {
        init_logging(filter)?;
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match &cli.command {
        Command::Sessions(a) => sessions::run(a, &mut out),
        Command::Doctor(a) => doctor::run(a, &mut out),
        Command::Telemetry(a) => telemetry::run(a, &mut out),
        Command::Install(a) => install::run_install(a, &mut out),
        Command::Uninstall(a) => install::run_uninstall(a, &mut out),
    }
}

fn init_logging(filter: &str) -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_new(filter).map_err(|e| anyhow::anyhow!("bad --log filter: {e}"))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|e| anyhow::anyhow!("could not start logging: {e}"))
}

/// [`c4_core::redact::redact`] applied to every string in a JSON value. Keys stay as they are.
#[must_use]
pub fn redact_value(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => Value::String(c4_core::redact::redact(&s)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_value).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, redact_value(v))).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redaction_walks_the_whole_value() {
        let v = json!({
            "token": "Bearer abcdefghijklmnopqrstuvwxyz0123456789", // scan:allow
            "nested": [{"path": "/home/example/.claude/sessions"}, 3, true, null],
        });
        let r = redact_value(v);
        assert!(!r["token"].as_str().unwrap().contains("abcdefghijklmnop"));
        assert_eq!(r["nested"][1], 3);
        assert_eq!(r["nested"][2], true);
        assert!(r["nested"][3].is_null());
        assert!(r["nested"][0]["path"].is_string());
    }

    #[test]
    fn the_command_line_parses() {
        let cli = Cli::try_parse_from(["c4", "sessions", "--json", "--files-only"]).unwrap();
        match cli.command {
            Command::Sessions(a) => {
                assert!(a.json);
                assert!(a.files_only);
                assert!(!a.watch);
                assert_eq!(a.agents_timeout_ms, 5000);
            }
            other => panic!("{other:?}"),
        }
        let cli = Cli::try_parse_from(["c4", "telemetry", "--transcript", "x.jsonl"]).unwrap();
        assert!(matches!(cli.command, Command::Telemetry(_)));
        let cli = Cli::try_parse_from([
            "c4",
            "install",
            "--dry-run",
            "--c4-home",
            "/tmp/c4",
            "--shim",
            "/tmp/c4-shim",
        ])
        .unwrap();
        match cli.command {
            Command::Install(a) => {
                assert!(a.dry_run);
                assert_eq!(a.c4_home.source(), "--c4-home");
                assert_eq!(
                    a.shim.as_deref(),
                    Some(std::path::Path::new("/tmp/c4-shim"))
                );
                assert!(a.home.home.is_none());
            }
            other => panic!("{other:?}"),
        }
        let cli = Cli::try_parse_from(["c4", "uninstall"]).unwrap();
        assert!(matches!(cli.command, Command::Uninstall(a) if !a.dry_run));
        assert!(Cli::try_parse_from(["c4", "frobnicate"]).is_err());
        assert!(
            Cli::try_parse_from(["c4", "telemetry"]).is_err(),
            "the file is required"
        );
    }
}
