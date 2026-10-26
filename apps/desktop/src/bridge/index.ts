// <wtl> project: c4 | file: apps/desktop/src/bridge/index.ts | author: watchthelight </wtl>
// Picks the bridge: the real one inside the app window, the synthetic one in a browser (`pnpm
// dev:web` opened directly) and in tests. VITE_C4_BRIDGE=mock, =electron or =tauri forces one. The
// synthetic bridge keeps its preferences in the browser's storage, so a reload finds them again.

import { electronBridge, insideElectron } from "./electron";
import { createMock } from "./mock";
import { tauriBridge } from "./tauri";
import type { Bridge } from "./types";

function insideTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** The browser's storage, or null where there is none or it is refused. */
function browserStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export function selectBridge(): Bridge {
  const forced = import.meta.env.VITE_C4_BRIDGE;
  if (forced === "mock") {
    return createMock({ storage: browserStorage() });
  }
  if (forced === "electron" || insideElectron()) {
    return electronBridge();
  }
  if (forced === "tauri" || insideTauri()) {
    return tauriBridge();
  }
  return createMock({ storage: browserStorage() });
}
