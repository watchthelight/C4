// <wtl> project: c4 | file: apps/desktop/src-tauri/build.rs | author: watchthelight </wtl>
//! Tauri's build step: reads `tauri.conf.json`, checks the capabilities and, on Windows, embeds the
//! icon and the application manifest into the executable.

fn main() {
    tauri_build::build();
}
