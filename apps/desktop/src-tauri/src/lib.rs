// <wtl> project: c4 | file: apps/desktop/src-tauri/src/lib.rs | author: watchthelight </wtl>
//! The desktop shell of C4: a Tauri window over the core in `c4_cli::serve`, the same table,
//! ingest worker and preferences file that `c4-cli serve` runs as a child process. The window asks
//! for a snapshot once (`list_sessions`) and then receives a `sessions` event after every refresh
//! pass and after every batch the ingest server folds; the refresh runs on its own thread and the
//! server on a runtime of its own, so a slow session list command never blocks the window, and a
//! window that closes takes nothing down but itself and its own server.

pub mod app;
pub mod console;

pub use app::run;
pub use c4_cli::serve::AppState;
