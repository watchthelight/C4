// <wtl> project: c4 | file: apps/desktop/electron/bounds.ts | author: watchthelight </wtl>
// Where the sidebar window goes, and the `windows` section of the preferences that keeps whether
// the sidebar is detached and where its window sat. Pure functions, nothing of Electron in them,
// so the rules run in node under vitest. A saved rectangle is used again only when at least 100
// by 100 pixels of it fall on a display that is still there; otherwise the window opens over the
// main window, 48px in from its top left corner, as wide as the sidebar was and nearly as tall as
// the main window, held inside the display the main window is on.

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface WindowsPrefs {
  sidebar: {
    detached: boolean;
    /** Where the sidebar window last sat; null until it has been opened once. */
    bounds: Rect | null;
  };
}

/** The sidebar window never goes under this. */
export const SIDEBAR_WINDOW_MIN: Readonly<Pick<Rect, "width" | "height">> = { width: 220, height: 240 };
/** The sidebar's width when nothing is saved; the same figure as --sidebar-w in the page. */
export const SIDEBAR_WIDTH = 320;
/** How much of a saved rectangle must still be on a display, in each direction, to be used again. */
export const VISIBLE_MIN = 100;
/** The offset from the main window's corner when nothing is saved. */
export const CASCADE = 48;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function integer(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? Math.round(value) : null;
}

/** A rectangle from a saved value, or null unless all four numbers are there and it has a size. */
export function readRect(value: unknown): Rect | null {
  if (!isRecord(value)) {
    return null;
  }
  const x = integer(value.x);
  const y = integer(value.y);
  const width = integer(value.width);
  const height = integer(value.height);
  if (x === null || y === null || width === null || height === null || width <= 0 || height <= 0) {
    return null;
  }
  return { x, y, width, height };
}

/** The `windows` section of `doc`, with the default for whatever is missing or malformed. */
export function readWindows(doc: unknown): WindowsPrefs {
  const section = isRecord(doc) && isRecord(doc.windows) ? doc.windows : {};
  const sidebar = isRecord(section.sidebar) ? section.sidebar : {};
  return {
    sidebar: {
      detached: sidebar.detached === true,
      bounds: readRect(sidebar.bounds),
    },
  };
}

/** The section as the file holds it. */
export function windowsSection(prefs: WindowsPrefs): Record<string, unknown> {
  return {
    sidebar: {
      detached: prefs.sidebar.detached,
      bounds: prefs.sidebar.bounds === null ? null : { ...prefs.sidebar.bounds },
    },
  };
}

/** The sidebar's saved width from the page's own section, for a sidebar window opened the first time. */
export function savedSidebarWidth(doc: unknown): number {
  const sidebar = isRecord(doc) && isRecord(doc.sidebar) ? doc.sidebar : {};
  const width = integer(sidebar.width);
  return width === null ? SIDEBAR_WIDTH : Math.max(SIDEBAR_WINDOW_MIN.width, width);
}

/** The size of the overlap of two rectangles; zero in a direction where they do not meet. */
export function overlap(a: Rect, b: Rect): { width: number; height: number } {
  const width = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
  const height = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
  return { width: Math.max(0, width), height: Math.max(0, height) };
}

/** True when enough of `rect` is on one of `displays` to grab. */
export function visible(rect: Rect, displays: readonly Rect[]): boolean {
  return displays.some((d) => {
    const o = overlap(rect, d);
    return o.width >= VISIBLE_MIN && o.height >= VISIBLE_MIN;
  });
}

/** The display `rect` has the most of itself on, or null when there is none. */
function displayOf(rect: Rect, displays: readonly Rect[]): Rect | null {
  let best: Rect | null = null;
  let area = 0;
  for (const d of displays) {
    const o = overlap(rect, d);
    if (o.width * o.height > area) {
      area = o.width * o.height;
      best = d;
    }
  }
  return best ?? displays[0] ?? null;
}

/**
 * Where to open the sidebar window: the saved place when enough of it is still on a display, else
 * over the main window (`main`, in screen coordinates), 48px in from its corner, `width` wide.
 */
export function placeSidebar(saved: Rect | null, displays: readonly Rect[], main: Rect, width: number): Rect {
  if (saved !== null && visible(saved, displays)) {
    return {
      x: saved.x,
      y: saved.y,
      width: Math.max(SIDEBAR_WINDOW_MIN.width, saved.width),
      height: Math.max(SIDEBAR_WINDOW_MIN.height, saved.height),
    };
  }
  const rect: Rect = {
    x: main.x + CASCADE,
    y: main.y + CASCADE,
    width: Math.max(SIDEBAR_WINDOW_MIN.width, Math.round(width)),
    height: Math.max(SIDEBAR_WINDOW_MIN.height, main.height - 2 * CASCADE),
  };
  const display = displayOf(main, displays);
  if (display !== null) {
    rect.width = Math.min(rect.width, display.width);
    rect.height = Math.min(rect.height, display.height);
    rect.x = Math.max(display.x, Math.min(rect.x, display.x + display.width - rect.width));
    rect.y = Math.max(display.y, Math.min(rect.y, display.y + display.height - rect.height));
  }
  return rect;
}
