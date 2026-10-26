// <wtl> project: c4 | file: apps/desktop/src-tauri/src/console.rs | author: watchthelight </wtl>
//! A release build of `c4` is a windowed executable on Windows, so it starts without a console.
//! When it was started from a terminal with arguments, it attaches to that terminal's console first
//! so the headless commands can print. On the other platforms the executable is an ordinary console
//! program and nothing is needed.

/// Attach to the parent process's console, when there is one. A failure (no parent console, or a
/// console already attached, as in a debug build) leaves the process as it was.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn attach_parent() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: `AttachConsole` takes a process id constant, touches no memory of ours and reports
    // failure through its return value, which is ignored on purpose.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Nothing to do: the executable has the console it was started from.
#[cfg(not(windows))]
pub fn attach_parent() {}
