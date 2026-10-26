// <wtl> project: c4 | file: crates/c4-cli/src/args.rs | author: watchthelight </wtl>
//! The command line. The desktop binary parses the same types, so `c4 sessions` means the same
//! thing whichever executable runs it.

use c4_core::C4Home;
use c4_discovery::{CONFIG_DIR_ENV, ClaudeHome, HOME_ENV};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// `c4`, the headless commands.
#[derive(Parser, Debug)]
#[command(
    name = "c4",
    version = c4_core::VERSION,
    about = "C4, a command center for Claude Code sessions"
)]
pub struct Cli {
    /// Diagnostics on stderr, as a tracing filter such as `debug` or `c4_discovery=trace`.
    #[arg(long, global = true, value_name = "FILTER")]
    pub log: Option<String>,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// List every session the discovery readers can see, in attention order.
    Sessions(SessionsArgs),
    /// Check the Claude home, the claude command and the process table.
    Doctor(DoctorArgs),
    /// Token totals, cost and agents recorded in one transcript file.
    Telemetry(TelemetryArgs),
    /// Copy the shim into the C4 home and wire it into settings.json, with a backup and a manifest.
    Install(InstallArgs),
    /// Take the shim out of settings.json again, from the backup when nothing else changed the file.
    Uninstall(UninstallArgs),
}

/// Where the Claude home is.
#[derive(Args, Debug, Clone, Default)]
pub struct HomeArgs {
    /// The Claude home to read instead of the one the environment names.
    #[arg(
        long,
        value_name = "DIR",
        help = "The Claude home to read, instead of the one the environment names (C4_CLAUDE_HOME, then CLAUDE_CONFIG_DIR, then ~/.claude)"
    )]
    pub home: Option<PathBuf>,
}

impl HomeArgs {
    /// The home: `--home`, else the environment rule of [`ClaudeHome::resolve`].
    #[must_use]
    pub fn resolve(&self) -> ClaudeHome {
        self.home
            .as_ref()
            .map_or_else(ClaudeHome::resolve, ClaudeHome::at)
    }

    /// What decided the home, for the doctor's report.
    #[must_use]
    pub fn source(&self) -> &'static str {
        if self.home.is_some() {
            "--home"
        } else if env_set(HOME_ENV) {
            HOME_ENV
        } else if env_set(CONFIG_DIR_ENV) {
            CONFIG_DIR_ENV
        } else {
            "~/.claude"
        }
    }
}

/// Where the C4 home is: the shim copy, the spool, the logs and the install manifest.
#[derive(Args, Debug, Clone, Default)]
pub struct C4HomeArgs {
    /// The C4 home to use instead of the one the environment names.
    #[arg(
        long,
        value_name = "DIR",
        help = "The C4 home to use, instead of the one the environment names (C4_HOME, then ~/.c4)"
    )]
    pub c4_home: Option<PathBuf>,
}

impl C4HomeArgs {
    /// The home: `--c4-home`, else the environment rule of [`C4Home::resolve`].
    #[must_use]
    pub fn resolve(&self) -> C4Home {
        self.c4_home
            .as_ref()
            .map_or_else(C4Home::resolve, C4Home::at)
    }

    /// What decided the home, for the report.
    #[must_use]
    pub fn source(&self) -> &'static str {
        if self.c4_home.is_some() {
            "--c4-home"
        } else if env_set(c4_core::home::HOME_ENV) {
            c4_core::home::HOME_ENV
        } else {
            "~/.c4"
        }
    }
}

fn env_set(key: &str) -> bool {
    std::env::var_os(key).is_some_and(|v| !v.is_empty())
}

/// `c4 sessions`.
#[derive(Args, Debug, Clone)]
pub struct SessionsArgs {
    /// Where the Claude home is.
    #[command(flatten)]
    pub home: HomeArgs,
    /// Print JSON instead of a table.
    #[arg(long)]
    pub json: bool,
    /// Read the files under the home only; do not run the session list command.
    #[arg(long)]
    pub files_only: bool,
    /// How long the session list command may take.
    #[arg(long, value_name = "MS", default_value_t = 5000)]
    pub agents_timeout_ms: u64,
    /// Keep refreshing and print every state change until interrupted.
    #[arg(long)]
    pub watch: bool,
    /// The refresh interval while watching.
    #[arg(long, value_name = "MS", default_value_t = 1000)]
    pub interval_ms: u64,
    /// Rewrite home paths and anything that looks like a secret in the JSON output. The table is
    /// always redacted.
    #[arg(long)]
    pub redact: bool,
}

/// `c4 doctor`.
#[derive(Args, Debug, Clone)]
pub struct DoctorArgs {
    /// Where the Claude home is.
    #[command(flatten)]
    pub home: HomeArgs,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Do not run claude or the session list command, which can start Claude Code's background
    /// supervisor on macOS and Linux; check the files and the process table only.
    #[arg(long)]
    pub no_commands: bool,
    /// How long each command may take.
    #[arg(long, value_name = "MS", default_value_t = 5000)]
    pub timeout_ms: u64,
}

/// `c4 telemetry`.
#[derive(Args, Debug, Clone)]
pub struct TelemetryArgs {
    /// The main transcript file: <home>/projects/<encoded cwd>/<session id>.jsonl.
    #[arg(long, value_name = "FILE")]
    pub transcript: PathBuf,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

/// `c4 install`.
#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    /// Where the Claude home is.
    #[command(flatten)]
    pub home: HomeArgs,
    /// Where the C4 home is.
    #[command(flatten)]
    pub c4_home: C4HomeArgs,
    /// The shim executable to copy into the C4 home, instead of the c4-shim beside this executable.
    #[arg(long, value_name = "FILE")]
    pub shim: Option<PathBuf>,
    /// Print what would change and write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// `c4 uninstall`.
#[derive(Args, Debug, Clone)]
pub struct UninstallArgs {
    /// Where the Claude home is.
    #[command(flatten)]
    pub home: HomeArgs,
    /// Where the C4 home is.
    #[command(flatten)]
    pub c4_home: C4HomeArgs,
    /// Print what would change and write nothing.
    #[arg(long)]
    pub dry_run: bool,
}
