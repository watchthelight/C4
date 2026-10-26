// <wtl> project: c4 | file: apps/desktop/src-tauri/src/main.rs | author: watchthelight </wtl>
//! The `c4` executable. With arguments it is the headless command line (`c4 sessions`, `c4 doctor`,
//! `c4 telemetry`) and no webview starts; without any it opens the window.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_some() {
        c4_desktop::console::attach_parent();
        return c4_cli::main();
    }
    c4_desktop::run()
}
