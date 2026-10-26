// <wtl> project: c4 | file: crates/c4-cli/src/main.rs | author: watchthelight </wtl>
//! The `c4` executable, headless. The desktop app takes this binary name over later and branches
//! into the same commands before it opens a window.

fn main() -> std::process::ExitCode {
    c4_cli::main()
}
