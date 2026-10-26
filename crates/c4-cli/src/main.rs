// <wtl> project: c4 | file: crates/c4-cli/src/main.rs | author: watchthelight </wtl>
//! The headless commands as their own executable, `c4-cli`: it builds in seconds and needs no
//! window, and `c4-cli serve` is the core the desktop shell runs as a child process. The `c4`
//! executable in `apps/desktop` runs the same commands before it opens a window.

fn main() -> std::process::ExitCode {
    c4_cli::main()
}
