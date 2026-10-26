// <wtl> project: c4 | file: apps/desktop/src-tauri/src/app.rs | author: watchthelight </wtl>
//! The Tauri side of the window: the four commands the page calls, the refresh thread that emits a
//! `sessions` event after every pass, and the ingest worker that emits the same event after every
//! batch it folds. The table, the worker and the preferences file live in `c4_cli::serve`, shared
//! with `c4-cli serve`, which carries the same documents over stdio to a shell in another process.

use c4_cli::serve::{AGENTS_EVERY, About, AppState, SESSIONS_EVENT, Worker};
use serde_json::Value;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// The window's first request: a files-only pass, then the document.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value
fn list_sessions(state: State<'_, Arc<AppState>>) -> Value {
    state.refresh(false)
}

/// The static facts for the status strip.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn about(state: State<'_, Arc<AppState>>) -> About {
    state.about()
}

/// The preferences document as it is now.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn prefs_get(state: State<'_, Arc<AppState>>) -> Value {
    state.prefs().document()
}

/// Replace one section of the preferences and write the file. Returns the document as written.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn prefs_set(
    state: State<'_, Arc<AppState>>,
    section: String,
    value: Value,
) -> Result<Value, String> {
    state
        .prefs()
        .set_section(&section, value)
        .map_err(|e| e.to_string())
}

/// Open the window and run until it closes. Returns the process exit code.
///
/// The ingest worker starts once the app is built and stops on the exit event, before the process
/// ends, so the socket file is removed and open connections get their moment. A worker that cannot
/// start (no runtime) is reported on stderr and the window runs without it.
#[must_use]
pub fn run() -> ExitCode {
    let state = Arc::new(AppState::from_env());
    let period = state.period();
    let built = tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            list_sessions,
            about,
            prefs_get,
            prefs_set
        ])
        .setup(move |app| {
            spawn_refresh(app.handle().clone(), period);
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            eprintln!("c4: {e}");
            return ExitCode::from(1);
        }
    };
    let handle = app.handle().clone();
    let mut worker = match Worker::spawn(state, move |document| emit(&handle, document)) {
        Ok(worker) => Some(worker),
        Err(e) => {
            eprintln!("c4: the ingest server could not get a runtime: {e}");
            None
        }
    };
    app.run(move |_, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(worker) = worker.as_mut()
        {
            worker.stop();
        }
    });
    ExitCode::SUCCESS
}

fn emit(handle: &AppHandle, document: Value) {
    if let Err(e) = handle.emit(SESSIONS_EVENT, document) {
        tracing::warn!(error = %e, "could not emit the sessions event");
    }
}

/// The refresh loop on its own thread: a pass, an event, a sleep. The thread ends with the process.
fn spawn_refresh(handle: AppHandle, period: Duration) {
    let spawned = std::thread::Builder::new()
        .name("c4-refresh".into())
        .spawn(move || {
            let mut pass: u64 = 0;
            loop {
                let with_agents = pass.is_multiple_of(AGENTS_EVERY);
                let document = handle.state::<Arc<AppState>>().refresh(with_agents);
                emit(&handle, document);
                pass += 1;
                std::thread::sleep(period);
            }
        });
    if let Err(e) = spawned {
        eprintln!("c4: could not start the refresh thread: {e}");
    }
}
