// <wtl> project: c4 | file: crates/c4-cli/src/main.rs | author: watchthelight </wtl>
//! The headless commands as their own executable, `c4-cli`, for development: it builds in seconds
//! and needs no webview. The `c4` executable in `apps/desktop` runs the same commands before it
//! opens a window.

fn main() -> std::process::ExitCode {
    c4_cli::main()
}
