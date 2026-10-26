// <wtl> project: c4 | file: apps/desktop/electron/tests/bounds.test.ts | author: watchthelight </wtl>
// @vitest-environment node
// The `windows` section of the preferences and where the sidebar window opens: a saved place used
// again when enough of it is still on a display, else a place over the main window inside its
// display.

import { describe, expect, it } from "vitest";
import {
  CASCADE,
  SIDEBAR_WIDTH,
  SIDEBAR_WINDOW_MIN,
  VISIBLE_MIN,
  overlap,
  placeSidebar,
  readRect,
  readWindows,
  savedSidebarWidth,
  visible,
  windowsSection,
} from "../bounds.js";

const display = { x: 0, y: 0, width: 1920, height: 1040 };
const second = { x: 1920, y: 0, width: 1920, height: 1040 };
const main = { x: 100, y: 60, width: 1280, height: 800 };
/** Where the window goes over `main` when nothing saved can be used. */
const fallback = { x: main.x + CASCADE, y: main.y + CASCADE, width: 320, height: main.height - 2 * CASCADE };

describe("the windows section", () => {
  it("reads whether the sidebar was detached and where its window sat, with the defaults", () => {
    const none = { sidebar: { detached: false, bounds: null } };
    expect(readWindows({})).toEqual(none);
    expect(readWindows(undefined)).toEqual(none);
    expect(readWindows({ windows: "x" })).toEqual(none);
    expect(readWindows({ windows: { sidebar: { detached: "yes", bounds: 4 } } })).toEqual(none);
    expect(
      readWindows({ windows: { sidebar: { detached: true, bounds: { x: 1, y: 2, width: 300, height: 400 } } } }),
    ).toEqual({ sidebar: { detached: true, bounds: { x: 1, y: 2, width: 300, height: 400 } } });
  });

  it("reads a rectangle only when all four numbers are there and it has a size", () => {
    expect(readRect({ x: 1.4, y: 2.6, width: 300.2, height: 400 })).toEqual({ x: 1, y: 3, width: 300, height: 400 });
    expect(readRect({ x: -10, y: -20, width: 300, height: 400 })).toEqual({ x: -10, y: -20, width: 300, height: 400 });
    expect(readRect({ x: 1, y: 2, width: 0, height: 400 })).toBeNull();
    expect(readRect({ x: 1, y: 2, width: 300 })).toBeNull();
    expect(readRect({ x: "1", y: 2, width: 300, height: 400 })).toBeNull();
    expect(readRect({ x: Number.NaN, y: 2, width: 300, height: 400 })).toBeNull();
    expect(readRect(null)).toBeNull();
    expect(readRect([1, 2, 3, 4])).toBeNull();
  });

  it("writes the section as it reads back, with a copy of the bounds", () => {
    const bounds = { x: 1, y: 2, width: 300, height: 400 };
    const section = windowsSection({ sidebar: { detached: true, bounds } });
    expect(section).toEqual({ sidebar: { detached: true, bounds: { x: 1, y: 2, width: 300, height: 400 } } });
    expect((section.sidebar as { bounds: unknown }).bounds).not.toBe(bounds);
    expect(readWindows({ windows: section })).toEqual({ sidebar: { detached: true, bounds } });
    expect(windowsSection({ sidebar: { detached: false, bounds: null } })).toEqual({
      sidebar: { detached: false, bounds: null },
    });
  });

  it("takes the sidebar's saved width for the window's first opening, never under the floor", () => {
    expect(savedSidebarWidth({})).toBe(SIDEBAR_WIDTH);
    expect(savedSidebarWidth({ sidebar: { width: 400 } })).toBe(400);
    expect(savedSidebarWidth({ sidebar: { width: 100 } })).toBe(SIDEBAR_WINDOW_MIN.width);
    expect(savedSidebarWidth({ sidebar: { width: "wide" } })).toBe(SIDEBAR_WIDTH);
  });
});

describe("placing the sidebar window", () => {
  it("measures the overlap of two rectangles, zero where they do not meet", () => {
    expect(overlap(display, { x: 1800, y: 900, width: 300, height: 300 })).toEqual({ width: 120, height: 140 });
    // side by side: they meet along the whole height and nowhere across
    expect(overlap(display, second)).toEqual({ width: 0, height: 1040 });
    expect(overlap(display, { x: 2000, y: 2000, width: 10, height: 10 })).toEqual({ width: 0, height: 0 });
    expect(overlap(display, { x: -50, y: -50, width: 100, height: 100 })).toEqual({ width: 50, height: 50 });
  });

  it("uses a saved place again when enough of it is on a display that is still there", () => {
    const saved = { x: 2000, y: 100, width: 320, height: 700 };
    expect(visible(saved, [display, second])).toBe(true);
    expect(visible(saved, [display])).toBe(false);
    expect(placeSidebar(saved, [display, second], main, 320)).toEqual(saved);
    // a saved size under the floor grows to it
    expect(placeSidebar({ x: 10, y: 10, width: 100, height: 100 }, [display], main, 320)).toEqual({
      x: 10,
      y: 10,
      width: SIDEBAR_WINDOW_MIN.width,
      height: SIDEBAR_WINDOW_MIN.height,
    });
  });

  it("falls back to a place over the main window when the saved one is gone or nearly off", () => {
    const saved = { x: 2000, y: 100, width: 320, height: 700 };
    expect(placeSidebar(saved, [display], main, 320)).toEqual(fallback);
    const edge = { x: display.width - VISIBLE_MIN + 1, y: 100, width: 320, height: 700 };
    expect(visible(edge, [display])).toBe(false);
    expect(placeSidebar(edge, [display], main, 320)).toEqual(fallback);
    expect(placeSidebar(null, [display], main, 400)).toEqual({ ...fallback, width: 400 });
    expect(placeSidebar(null, [], main, 320)).toEqual(fallback);
  });

  it("keeps the fallback inside the display the main window is mostly on", () => {
    // the main window straddles two displays, most of it on the second
    const straddling = { x: 1800, y: 900, width: 1280, height: 800 };
    expect(placeSidebar(null, [display, second], straddling, 320)).toEqual({ x: 1920, y: 336, width: 320, height: 704 });
    // a small display caps the size and the place
    const small = { x: 0, y: 0, width: 300, height: 300 };
    expect(placeSidebar(null, [small], small, 320)).toEqual({ x: 0, y: 48, width: 300, height: 240 });
  });
});
