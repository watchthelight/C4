// <wtl> project: c4 | file: apps/desktop/src/bridge/index.ts | author: watchthelight </wtl>
// Picks the bridge: the real one inside the app window, the synthetic one in a browser (`pnpm dev`
// opened directly) and in tests. VITE_C4_BRIDGE=mock or =tauri forces either.

import { createMock } from "./mock";
import { tauriBridge } from "./tauri";
import type { Bridge } from "./types";

function insideTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function selectBridge(): Bridge {
  const forced = import.meta.env.VITE_C4_BRIDGE;
  if (forced === "mock") {
    return createMock();
  }
  if (forced === "tauri" || insideTauri()) {
    return tauriBridge();
  }
  return createMock();
}
