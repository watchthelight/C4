// <wtl> project: c4 | file: apps/desktop/src/env.d.ts | author: watchthelight </wtl>
/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** `mock` forces the synthetic bridge even inside the app window; `tauri` forces the real one. */
  readonly VITE_C4_BRIDGE?: string;
}
