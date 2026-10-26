// <wtl> project: c4 | file: apps/desktop/src-tauri/src/lib.rs | author: watchthelight </wtl>
//! The desktop shell of C4: a Tauri window over the same discovery the command line uses. The
//! window asks for a snapshot once (`list_sessions`) and then receives a `sessions` event after
//! every refresh pass and after every batch the ingest server folds; the refresh runs on its own
//! thread and the server on a runtime of its own, so a slow session list command never blocks the
//! window, and a window that closes takes nothing down but itself and its own server. What the
//! window keeps between runs goes to `prefs.json` under the C4 home through two more commands.

pub mod app;
pub mod console;
pub mod ingest;
pub mod prefs;

pub use app::{AppState, run};
pub use ingest::{Ingest, IngestReport, IngestState, Worker};
pub use prefs::{Prefs, PrefsError};
