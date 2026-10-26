// <wtl> project: c4 | file: apps/desktop/electron/ipc.ts | author: watchthelight </wtl>
// The two channels between the window and the main process: `c4:invoke`, a request the preload
// forwards and the core answers, and `c4:event`, a line from the core sent to every window. Only
// the four methods the window needs pass, and only from a frame of the app's own page.

import { BrowserWindow, ipcMain } from "electron";
import type { IpcMainInvokeEvent } from "electron";
import type { CoreEvent, Sidecar } from "./sidecar.js";

export const INVOKE = "c4:invoke";
export const EVENT = "c4:event";
export const METHODS: ReadonlySet<string> = new Set(["list_sessions", "about", "prefs_get", "prefs_set"]);
/** A request waits this long for a core that is still starting (or starting again). */
export const START_WAIT_MS = 15_000;

/** True when the request comes from a page served under one of `origins`. */
export function fromPage(url: string | undefined, origins: readonly string[]): boolean {
  if (url === undefined) {
    return false;
  }
  return origins.some((origin) => {
    const prefix = origin.endsWith("/") ? origin : `${origin}/`;
    return url === origin || url.startsWith(prefix);
  });
}

/** Send a line to every window. */
export function broadcast(payload: CoreEvent): void {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) {
      win.webContents.send(EVENT, payload);
    }
  }
}

/** Wire the channels to the core. `origins` are the pages allowed to ask. */
export function installIpc(core: Sidecar, origins: readonly string[]): void {
  ipcMain.handle(INVOKE, async (event: IpcMainInvokeEvent, method: unknown, params: unknown) => {
    if (!fromPage(event.senderFrame?.url, origins)) {
      throw new Error("the request did not come from the app's page");
    }
    if (typeof method !== "string" || !METHODS.has(method)) {
      throw new Error(`unknown method ${JSON.stringify(method)}`);
    }
    // the window loads while the core is still starting; its first request waits for the hello
    await core.whenRunning(START_WAIT_MS);
    return core.invoke(method, params ?? null);
  });
  core.on("event", (payload) => {
    broadcast(payload);
  });
  core.on("state", (state) => {
    broadcast({ event: "core", data: state });
  });
}
