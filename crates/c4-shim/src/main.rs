// <wtl> project: c4 | file: crates/c4-shim/src/main.rs | author: watchthelight </wtl>
//! `c4-shim`: the sidecar. See the library documentation for what each subcommand does.

use std::ffi::OsString;
use std::io::Write;

fn main() {
    // Nothing the shim could hit is worth a panic message inside a status line or a hook's stderr.
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let hook_mode = args.first().is_some_and(|a| a.as_os_str() == "hook");
    let code = std::panic::catch_unwind(|| c4_shim::run(&args)).unwrap_or_else(|_| {
        if hook_mode {
            0
        } else {
            println!("c4-shim: internal error");
            1
        }
    });
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
