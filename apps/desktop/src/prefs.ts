// <wtl> project: c4 | file: apps/desktop/src/prefs.ts | author: watchthelight </wtl>
// What the window keeps between runs. The core stores one JSON object under the C4 home with a
// schema number and a section per part of the window; the sidebar section holds the list's
// preferences and the sidebar's width. Reading is tolerant: a missing or malformed value falls back
// to its default and an unknown key is left alone, so a file from another version never stops the
// window. Writes go to the core one section at a time, after a short quiet, so a splitter drag
// costs one write; the store flushes on demand when the page is about to go away.

import type { Bridge, PrefsDoc } from "./bridge/types";
import { GROUP_BYS } from "./sessions/arrange";
import { DENSITIES } from "./sessions/list";
import type { ListPrefs } from "./sessions/list";

/** The sidebar's width when nothing is saved; --sidebar-w in tokens.css. */
export const SIDEBAR_WIDTH = 320;
/** The narrowest the sidebar goes; --sidebar-min in tokens.css. */
export const SIDEBAR_MIN = 220;
/** The workspace keeps at least this much, so the sidebar's widest is the window minus it. */
export const WORKSPACE_MIN = 400;
/** How long the store waits after a change before writing. */
export const WRITE_DELAY_MS = 250;
/** Pins and folded groups kept, the newest last; older ones drop off the front. */
export const PINS_MAX = 500;

export interface SidebarPrefs extends ListPrefs {
  /** The sidebar's width in CSS pixels as the user set it; the layout holds it inside the window. */
  width: number;
}

export const SIDEBAR_DEFAULTS: Readonly<SidebarPrefs> = {
  density: "standard",
  groupBy: "none",
  pinned: [],
  collapsed: [],
  width: SIDEBAR_WIDTH,
};

/** `width` held between the minimum and what leaves the workspace its room in a window `windowWidth` wide. */
export function clampWidth(width: number, windowWidth: number): number {
  const max = Math.max(SIDEBAR_MIN, Math.floor(windowWidth) - WORKSPACE_MIN);
  return Math.round(Math.min(max, Math.max(SIDEBAR_MIN, width)));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function oneOf<T extends string>(value: unknown, choices: readonly T[], fallback: T): T {
  return typeof value === "string" && (choices as readonly string[]).includes(value) ? (value as T) : fallback;
}

/** The strings of an array, each once, the last `max` of them; anything else is nothing. */
function strings(value: unknown, max: number): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const seen = new Set<string>();
  for (const item of value) {
    if (typeof item === "string" && item !== "") {
      seen.add(item);
    }
  }
  return [...seen].slice(-max);
}

/** The sidebar section of `doc`, with the default for whatever is missing or malformed. */
export function readSidebar(doc: unknown): SidebarPrefs {
  const s = isRecord(doc) && isRecord(doc.sidebar) ? doc.sidebar : {};
  const width = typeof s.width === "number" && Number.isFinite(s.width) ? s.width : SIDEBAR_WIDTH;
  return {
    density: oneOf(s.density, DENSITIES, SIDEBAR_DEFAULTS.density),
    groupBy: oneOf(s.group_by, GROUP_BYS, SIDEBAR_DEFAULTS.groupBy),
    pinned: strings(s.pinned, PINS_MAX),
    collapsed: strings(s.collapsed, PINS_MAX),
    width: Math.max(SIDEBAR_MIN, Math.round(width)),
  };
}

/** The section as the file holds it: snake_case keys, the sets as arrays. */
export function sidebarSection(prefs: SidebarPrefs): Record<string, unknown> {
  return {
    density: prefs.density,
    group_by: prefs.groupBy,
    pinned: [...prefs.pinned],
    collapsed: [...prefs.collapsed],
    width: prefs.width,
  };
}

export interface PrefsStoreOptions {
  /** The quiet before a write; WRITE_DELAY_MS by default. */
  delay?: number;
  /** Where a failed write is reported. The value is dropped and the window keeps running. */
  onError?(section: string, error: unknown): void;
}

export interface PrefsStore {
  /** The document as last read or written. */
  readonly doc: PrefsDoc;
  /**
   * Replace a section. The write happens after the quiet, or on flush; a value set again before
   * then replaces the one waiting.
   */
  set(section: string, value: unknown): void;
  /** Write everything waiting now. Resolves once the writes are done. */
  flush(): Promise<void>;
  /** Drop what is waiting and take nothing more. */
  dispose(): void;
}

export function createPrefsStore(
  bridge: Pick<Bridge, "prefsSet">,
  initial: PrefsDoc,
  options: PrefsStoreOptions = {},
): PrefsStore {
  const delay = options.delay ?? WRITE_DELAY_MS;
  const pending = new Map<string, unknown>();
  let doc = initial;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let chain: Promise<void> = Promise.resolve();
  let disposed = false;

  function cancel(): void {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  /** The sections waiting, in the order they were set, one call each. */
  async function writeAll(): Promise<void> {
    const batch = [...pending];
    pending.clear();
    for (const [section, value] of batch) {
      try {
        doc = await bridge.prefsSet(section, value);
      } catch (error) {
        options.onError?.(section, error);
      }
    }
  }

  function flush(): Promise<void> {
    cancel();
    if (pending.size > 0) {
      chain = chain.then(writeAll);
    }
    return chain;
  }

  return {
    get doc() {
      return doc;
    },
    set(section, value) {
      if (disposed) {
        return;
      }
      pending.set(section, value);
      cancel();
      timer = setTimeout(() => {
        timer = null;
        void flush();
      }, delay);
    },
    flush,
    dispose() {
      disposed = true;
      cancel();
      pending.clear();
    },
  };
}
