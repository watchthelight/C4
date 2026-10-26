// <wtl> project: c4 | file: apps/desktop/src/shell.ts | author: watchthelight </wtl>
// The page's side of the shell: which window this page is in, where the sidebar is, and the words
// the two windows pass each other through the main process. Inside the app `window.c4.shell`, from
// the preload script, carries the requests and `window.c4.on` brings the words back; in a browser
// (`pnpm dev:web`) and in tests a stand-in keeps the placement in memory and opens the sidebar as a
// popup, with nothing passing between the two pages.

import { insideElectron, unwrapError } from "./bridge/electron";
import type { C4Window } from "./bridge/electron";
import type { PrefsDoc } from "./bridge/types";

/** Which window a page is in: `?window=sidebar` names the sidebar's own window. */
export type WindowRole = "main" | "sidebar";

/** Where the sidebar is: in the main window, or in a window of its own. */
export type Placement = "attached" | "detached";

export type SidebarAction = "detach" | "attach" | "focus";

/**
 * A word from the shell: the placement changed; the other window selected a session; the other
 * window wrote a section of the preferences, here the whole document after the write; this window
 * should put the focus in its filter box.
 */
export type ShellMessage =
  | { kind: "windows"; sidebar: Placement }
  | { kind: "select"; id: string | null }
  | { kind: "prefs"; doc: PrefsDoc }
  | { kind: "focus" };

export interface Shell {
  readonly kind: "electron" | "mock";
  readonly role: WindowRole;
  /** Where the sidebar is now; inside the app, once the last run's placement has been put back. */
  windows(): Promise<Placement>;
  /** Move the sidebar into its own window or back, or bring its window up; the placement after. */
  sidebar(action: SidebarAction): Promise<Placement>;
  /** Tell the other window which session was chosen by hand here. */
  select(id: string | null): void;
  /** Listen for the shell's words; the returned function stops it. */
  onMessage(listener: (message: ShellMessage) => void): () => void;
}

/** The role the page's query string names; the main window has none. */
export function windowRole(search: string): WindowRole {
  return new URLSearchParams(search).get("window") === "sidebar" ? "sidebar" : "main";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function placementOf(value: unknown): Placement | null {
  return value === "attached" || value === "detached" ? value : null;
}

/** The message in an event's data, or null when it is not one the page knows. */
export function shellMessage(data: unknown): ShellMessage | null {
  if (!isRecord(data)) {
    return null;
  }
  switch (data.kind) {
    case "windows": {
      const sidebar = placementOf(data.sidebar);
      return sidebar === null ? null : { kind: "windows", sidebar };
    }
    case "select":
      return data.id === null || typeof data.id === "string" ? { kind: "select", id: data.id } : null;
    case "prefs":
      return isRecord(data.doc) ? { kind: "prefs", doc: data.doc as PrefsDoc } : null;
    case "focus":
      return { kind: "focus" };
    default:
      return null;
  }
}

/** The shell inside the app, over `window.c4`. */
export function electronShell(c4: C4Window | undefined, role: WindowRole): Shell {
  if (c4 === undefined) {
    throw new Error("window.c4 is missing: the page is not inside the app");
  }
  const api: C4Window = c4;
  async function ask(request: unknown): Promise<Placement> {
    let answer: unknown;
    try {
      answer = await api.shell(request);
    } catch (error) {
      throw unwrapError(error);
    }
    const sidebar = isRecord(answer) ? placementOf(answer.sidebar) : null;
    if (sidebar === null) {
      throw new Error("the shell did not say where the sidebar is");
    }
    return sidebar;
  }
  return {
    kind: "electron",
    role,
    windows: () => ask({ kind: "windows" }),
    sidebar: (action) => ask({ kind: "sidebar", action }),
    select: (id) => {
      // a word for the other window; nothing here depends on it arriving
      void api.shell({ kind: "select", id }).catch(() => undefined);
    },
    onMessage: (listener) =>
      api.on((payload) => {
        if (payload.event === "shell") {
          const message = shellMessage(payload.data);
          if (message !== null) {
            listener(message);
          }
        }
      }),
  };
}

export interface MockShellOptions {
  /** Open the sidebar's window; what comes back is closed again when the sidebar is attached. */
  opener?(): { close(): void } | null;
  /** Close this window, when it is the sidebar's own and the sidebar goes back. */
  closer?(): void;
}

/** The stand-in: the placement in memory, the windows through the options. */
export function mockShell(role: WindowRole, options: MockShellOptions = {}): Shell {
  let sidebar: Placement = role === "sidebar" ? "detached" : "attached";
  let opened: { close(): void } | null = null;
  const listeners = new Set<(message: ShellMessage) => void>();

  function set(next: Placement): void {
    if (next === sidebar) {
      return;
    }
    sidebar = next;
    for (const listener of listeners) {
      listener({ kind: "windows", sidebar: next });
    }
  }

  return {
    kind: "mock",
    role,
    windows: () => Promise.resolve(sidebar),
    sidebar: (action) => {
      if (action === "detach") {
        if (sidebar === "attached") {
          opened = options.opener?.() ?? null;
        }
        set("detached");
      } else if (action === "attach") {
        set("attached");
        opened?.close();
        opened = null;
        if (role === "sidebar") {
          options.closer?.();
        }
      }
      return Promise.resolve(sidebar);
    },
    select: () => undefined,
    onMessage: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The shell for this page: the real one inside the app, the stand-in elsewhere, as the bridge is chosen. */
export function selectShell(role: WindowRole): Shell {
  const forced = import.meta.env.VITE_C4_BRIDGE;
  if (forced !== "mock" && (forced === "electron" || insideElectron())) {
    return electronShell(globalThis.window?.c4, role);
  }
  return mockShell(role, {
    opener: () =>
      typeof window === "undefined" ? null : window.open("?window=sidebar", "c4-sidebar", "popup,width=320,height=720"),
    closer: () => {
      window.close();
    },
  });
}
