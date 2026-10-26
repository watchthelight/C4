// <wtl> project: c4 | file: apps/desktop/src/bridge/tauri.ts | author: watchthelight </wtl>
// The real bridge: Tauri commands and events, named as in apps/desktop/src-tauri/src/app.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { About, Bridge, Snapshot } from "./types";

export const SESSIONS_EVENT = "sessions";

export function tauriBridge(): Bridge {
  return {
    kind: "tauri",
    listSessions: () => invoke<Snapshot>("list_sessions"),
    about: () => invoke<About>("about"),
    onSessions: async (listener) => {
      const unlisten = await listen<Snapshot>(SESSIONS_EVENT, (event) => listener(event.payload));
      return unlisten;
    },
  };
}
