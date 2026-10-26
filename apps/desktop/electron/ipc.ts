// <wtl> project: c4 | file: apps/desktop/electron/ipc.ts | author: watchthelight </wtl>
// The channels between the pages and the main process: `c4:invoke`, a request the preload forwards
// and the core answers; `c4:shell`, a request the main process answers itself (the sidebar's
// placement, and the selected session passed to the other window); and `c4:event`, a line from the
// core or a word from the shell sent to every window. Only the four methods the pages need reach
// the core, only three request kinds reach the shell, and only from a frame of the app's own page.

import { BrowserWindow, ipcMain } from "electron";
import type { IpcMainInvokeEvent } from "electron";
import type { CoreEvent, Sidecar } from "./sidecar.js";
import type { Placement } from "./windows.js";

export const INVOKE = "c4:invoke";
export const SHELL = "c4:shell";
export const EVENT = "c4:event";
export const METHODS: ReadonlySet<string> = new Set(["list_sessions", "about", "prefs_get", "prefs_set"]);
/** A request waits this long for a core that is still starting (or starting again). */
export const START_WAIT_MS = 15_000;
/** A selected id longer than this is not one of ours. */
export const ID_MAX = 200;

/** What a page may ask the shell. */
export type ShellRequest =
  | { kind: "windows" }
  | { kind: "sidebar"; action: "detach" | "attach" | "focus" }
  | { kind: "select"; id: string | null };

/** What the shell needs from the window controller; `Windows` in windows.ts is one. */
export interface Shell {
  readonly placement: Placement;
  /** Settles once the placement of the last run has been put back. */
  readonly ready: Promise<void>;
  detach(): Promise<Placement>;
  attach(): Placement;
  focusSidebar(): boolean;
  on(event: "change", listener: (placement: Placement) => void): unknown;
}

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

/** The request a page sent, or null when it is not one the shell takes. */
export function shellRequest(raw: unknown): ShellRequest | null {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return null;
  }
  const r = raw as Record<string, unknown>;
  switch (r.kind) {
    case "windows":
      return { kind: "windows" };
    case "sidebar":
      return r.action === "detach" || r.action === "attach" || r.action === "focus"
        ? { kind: "sidebar", action: r.action }
        : null;
    case "select":
      if (r.id === null) {
        return { kind: "select", id: null };
      }
      return typeof r.id === "string" && r.id !== "" && r.id.length <= ID_MAX ? { kind: "select", id: r.id } : null;
    default:
      return null;
  }
}

/** Send a payload to every window, or to every window but the one whose contents id is `except`. */
export function broadcast(payload: CoreEvent, except?: number): void {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed() && win.webContents.id !== except) {
      win.webContents.send(EVENT, payload);
    }
  }
}

/** Wire the channels to the core and the window controller. `origins` are the pages allowed to ask. */
export function installIpc(core: Sidecar, origins: readonly string[], shell: Shell): void {
  ipcMain.handle(INVOKE, async (event: IpcMainInvokeEvent, method: unknown, params: unknown) => {
    if (!fromPage(event.senderFrame?.url, origins)) {
      throw new Error("the request did not come from the app's page");
    }
    if (typeof method !== "string" || !METHODS.has(method)) {
      throw new Error(`unknown method ${JSON.stringify(method)}`);
    }
    // the window loads while the core is still starting; its first request waits for the hello
    await core.whenRunning(START_WAIT_MS);
    const result = await core.invoke(method, params ?? null);
    // the other window keeps its sidebar from the same file: it is told what was written
    if (method === "prefs_set") {
      broadcast({ event: "shell", data: { kind: "prefs", doc: result } }, event.sender.id);
    }
    return result;
  });
  ipcMain.handle(SHELL, async (event: IpcMainInvokeEvent, raw: unknown) => {
    if (!fromPage(event.senderFrame?.url, origins)) {
      throw new Error("the request did not come from the app's page");
    }
    const request = shellRequest(raw);
    if (request === null) {
      throw new Error("not a request the shell takes");
    }
    switch (request.kind) {
      case "windows":
        await shell.ready;
        return { sidebar: shell.placement };
      case "sidebar":
        if (request.action === "detach") {
          return { sidebar: await shell.detach() };
        }
        if (request.action === "attach") {
          return { sidebar: shell.attach() };
        }
        return { sidebar: shell.placement, focused: shell.focusSidebar() };
      case "select":
        broadcast({ event: "shell", data: { kind: "select", id: request.id } }, event.sender.id);
        return null;
    }
  });
  core.on("event", (payload) => {
    broadcast(payload);
  });
  core.on("state", (state) => {
    broadcast({ event: "core", data: state });
  });
  shell.on("change", (placement) => {
    broadcast({ event: "shell", data: { kind: "windows", sidebar: placement } });
  });
}
