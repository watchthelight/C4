// <wtl> project: c4 | file: apps/desktop/src/bridge/electron.ts | author: watchthelight </wtl>
// The real bridge: `window.c4`, which the preload script (apps/desktop/electron/preload.cts)
// exposes, carrying requests to `c4-cli serve` through the main process and its event lines back.

import type { About, Bridge, PrefsDoc, Snapshot } from "./types";

/** A line the core sent on its own, as the main process relays it. */
export interface CorePayload {
  event: string;
  data: unknown;
}

/** What the preload script puts on the window. */
export interface C4Window {
  invoke(method: string, params?: unknown): Promise<unknown>;
  on(listener: (payload: CorePayload) => void): () => void;
  platform: string;
  versions: { electron: string; chrome: string; node: string };
}

declare global {
  interface Window {
    c4?: C4Window;
  }
}

export const SESSIONS_EVENT = "sessions";

/** Electron wraps a rejection from the main process in its own words; the core's text is enough. */
const WRAPPER = /^Error invoking remote method '[^']*': (?:Error: )?/;

function unwrap(error: unknown): Error {
  const text = error instanceof Error ? error.message : String(error);
  return new Error(text.replace(WRAPPER, ""));
}

export function insideElectron(): boolean {
  return typeof window !== "undefined" && window.c4 !== undefined;
}

export function electronBridge(c4: C4Window | undefined = globalThis.window?.c4): Bridge {
  if (c4 === undefined) {
    throw new Error("window.c4 is missing: the page is not inside the app");
  }
  const api: C4Window = c4;
  function call<T>(method: string, params?: unknown): Promise<T> {
    return api.invoke(method, params).then(
      (value) => value as T,
      (error: unknown) => {
        throw unwrap(error);
      },
    );
  }
  return {
    kind: "electron",
    listSessions: () => call<Snapshot>("list_sessions"),
    about: () => call<About>("about"),
    onSessions: (listener) =>
      Promise.resolve(
        api.on((payload) => {
          if (payload.event === SESSIONS_EVENT) {
            listener(payload.data as Snapshot);
          }
        }),
      ),
    prefsGet: () => call<PrefsDoc>("prefs_get"),
    prefsSet: (section, value) => call<PrefsDoc>("prefs_set", { section, value }),
  };
}
