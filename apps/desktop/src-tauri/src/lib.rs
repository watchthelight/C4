// <wtl> project: c4 | file: apps/desktop/src-tauri/src/lib.rs | author: watchthelight </wtl>
//! The desktop shell of C4: a Tauri window over the same discovery the command line uses. The
//! window asks for a snapshot once (`list_sessions`) and then receives a `sessions` event after
//! every refresh; the refresh runs on its own thread, so a slow session list command never blocks
//! the window, and a window that closes takes nothing down but itself.

pub mod app;
pub mod console;

pub use app::{AppState, run};
