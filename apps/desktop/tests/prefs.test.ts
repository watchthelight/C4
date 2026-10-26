// <wtl> project: c4 | file: apps/desktop/tests/prefs.test.ts | author: watchthelight </wtl>
// The saved preferences: a tolerant read of the sidebar section, the width's limits, the section
// as written, and the store that coalesces writes and reports what fails.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PrefsDoc } from "../src/bridge/types";
import {
  PINS_MAX,
  SIDEBAR_DEFAULTS,
  SIDEBAR_MIN,
  SIDEBAR_WIDTH,
  WORKSPACE_MIN,
  WRITE_DELAY_MS,
  clampWidth,
  createPrefsStore,
  readSidebar,
  sidebarSection,
} from "../src/prefs";

describe("readSidebar", () => {
  it("gives the defaults for nothing, for a document that is not an object and for a section that is not one", () => {
    expect(readSidebar({})).toEqual(SIDEBAR_DEFAULTS);
    expect(readSidebar(undefined)).toEqual(SIDEBAR_DEFAULTS);
    expect(readSidebar(null)).toEqual(SIDEBAR_DEFAULTS);
    expect(readSidebar([1, 2])).toEqual(SIDEBAR_DEFAULTS);
    expect(readSidebar({ schema: 1, sidebar: "wide" })).toEqual(SIDEBAR_DEFAULTS);
    expect(readSidebar({ schema: 1, sidebar: [320] })).toEqual(SIDEBAR_DEFAULTS);
  });

  it("reads a whole section back", () => {
    const doc = {
      schema: 1,
      sidebar: {
        density: "compact",
        group_by: "project",
        pinned: ["a", "b"],
        collapsed: ["project:c4"],
        width: 400,
      },
    };
    expect(readSidebar(doc)).toEqual({
      density: "compact",
      groupBy: "project",
      pinned: ["a", "b"],
      collapsed: ["project:c4"],
      width: 400,
    });
  });

  it("falls back one value at a time and leaves unknown keys alone", () => {
    const got = readSidebar({
      sidebar: {
        density: "huge",
        group_by: 3,
        pinned: ["a", 7, "", null, "a", "b"],
        collapsed: "project:c4",
        width: "wide",
        future: { kept: true },
      },
    });
    expect(got).toEqual({ density: "standard", groupBy: "none", pinned: ["a", "b"], collapsed: [], width: SIDEBAR_WIDTH });
    expect(readSidebar({ sidebar: { width: Number.NaN } }).width).toBe(SIDEBAR_WIDTH);
    expect(readSidebar({ sidebar: { width: Number.POSITIVE_INFINITY } }).width).toBe(SIDEBAR_WIDTH);
    expect(readSidebar({ sidebar: { width: 12.6 } }).width).toBe(SIDEBAR_MIN);
    expect(readSidebar({ sidebar: { width: 333.4 } }).width).toBe(333);
  });

  it("keeps the newest pins when there are more than the cap", () => {
    const pinned = Array.from({ length: PINS_MAX + 5 }, (_, i) => `id-${i}`);
    const got = readSidebar({ sidebar: { pinned } });
    expect(got.pinned).toHaveLength(PINS_MAX);
    expect(got.pinned[0]).toBe("id-5");
    expect(got.pinned[PINS_MAX - 1]).toBe(`id-${PINS_MAX + 4}`);
  });
});

describe("clampWidth", () => {
  it("holds the width between the minimum and what leaves the workspace its room", () => {
    expect(clampWidth(320, 1280)).toBe(320);
    expect(clampWidth(100, 1280)).toBe(SIDEBAR_MIN);
    expect(clampWidth(2000, 1280)).toBe(1280 - WORKSPACE_MIN);
    expect(clampWidth(320.4, 1280)).toBe(320);
    expect(clampWidth(320, 1280.9)).toBe(320);
  });

  it("never goes under the minimum, even in a window too narrow for both", () => {
    expect(clampWidth(320, 500)).toBe(SIDEBAR_MIN);
    expect(clampWidth(320, 0)).toBe(SIDEBAR_MIN);
    expect(clampWidth(230, SIDEBAR_MIN + WORKSPACE_MIN)).toBe(SIDEBAR_MIN);
  });
});

describe("sidebarSection", () => {
  it("writes snake_case keys that read back as the same preferences", () => {
    const prefs = { density: "expanded" as const, groupBy: "host" as const, pinned: ["x"], collapsed: ["host:a"], width: 260 };
    const section = sidebarSection(prefs);
    expect(section).toEqual({ density: "expanded", group_by: "host", pinned: ["x"], collapsed: ["host:a"], width: 260 });
    expect(readSidebar({ sidebar: section })).toEqual(prefs);
    // the arrays are copies, not the caller's
    (section.pinned as string[]).push("y");
    expect(prefs.pinned).toEqual(["x"]);
  });
});

describe("prefs store", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function bridgeStub(initial: PrefsDoc = { schema: 1 }) {
    let doc: PrefsDoc = initial;
    const prefsSet = vi.fn((section: string, value: unknown): Promise<PrefsDoc> => {
      doc = { ...doc, [section]: value };
      return Promise.resolve(doc);
    });
    return { prefsSet };
  }

  it("writes once after the quiet, with the last value set", async () => {
    const bridge = bridgeStub();
    const store = createPrefsStore(bridge, { schema: 1 });
    store.set("sidebar", { width: 300 });
    store.set("sidebar", { width: 310 });
    await vi.advanceTimersByTimeAsync(WRITE_DELAY_MS - 1);
    expect(bridge.prefsSet).not.toHaveBeenCalled();
    store.set("sidebar", { width: 320 });
    await vi.advanceTimersByTimeAsync(WRITE_DELAY_MS - 1);
    expect(bridge.prefsSet).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(bridge.prefsSet).toHaveBeenCalledTimes(1);
    expect(bridge.prefsSet).toHaveBeenCalledWith("sidebar", { width: 320 });
    expect(store.doc).toEqual({ schema: 1, sidebar: { width: 320 } });
  });

  it("flushes on demand, one call per section in the order they were set", async () => {
    const bridge = bridgeStub();
    const store = createPrefsStore(bridge, { schema: 1 }, { delay: 1000 });
    store.set("sidebar", { width: 300 });
    store.set("notify", { enabled: false });
    await store.flush();
    expect(bridge.prefsSet.mock.calls).toEqual([
      ["sidebar", { width: 300 }],
      ["notify", { enabled: false }],
    ]);
    expect(store.doc).toEqual({ schema: 1, sidebar: { width: 300 }, notify: { enabled: false } });
    // nothing waits any more: the timer is gone and a flush writes nothing
    await vi.advanceTimersByTimeAsync(2000);
    await store.flush();
    expect(bridge.prefsSet).toHaveBeenCalledTimes(2);
  });

  it("reports a failed write and keeps the document it had", async () => {
    const errors: [string, unknown][] = [];
    const prefsSet = vi.fn((): Promise<PrefsDoc> => Promise.reject(new Error("disk full")));
    const store = createPrefsStore({ prefsSet }, { schema: 1 }, { onError: (s, e) => errors.push([s, e]) });
    store.set("sidebar", { width: 300 });
    await vi.advanceTimersByTimeAsync(WRITE_DELAY_MS);
    expect(prefsSet).toHaveBeenCalledTimes(1);
    expect(errors).toHaveLength(1);
    expect(errors[0]?.[0]).toBe("sidebar");
    expect(String(errors[0]?.[1])).toContain("disk full");
    expect(store.doc).toEqual({ schema: 1 });
  });

  it("writes a second batch after the first, never two at once", async () => {
    let resolveFirst: ((doc: PrefsDoc) => void) | null = null;
    const prefsSet = vi.fn(
      (section: string, value: unknown): Promise<PrefsDoc> =>
        new Promise((resolve) => {
          if (resolveFirst === null) {
            resolveFirst = resolve;
          } else {
            resolve({ schema: 1, [section]: value });
          }
        }),
    );
    const store = createPrefsStore({ prefsSet }, { schema: 1 }, { delay: 10 });
    store.set("sidebar", { width: 300 });
    await vi.advanceTimersByTimeAsync(10);
    expect(prefsSet).toHaveBeenCalledTimes(1);
    store.set("sidebar", { width: 310 });
    await vi.advanceTimersByTimeAsync(10);
    // the first write is still in flight, so the second waits
    expect(prefsSet).toHaveBeenCalledTimes(1);
    (resolveFirst as unknown as (doc: PrefsDoc) => void)({ schema: 1, sidebar: { width: 300 } });
    await vi.advanceTimersByTimeAsync(0);
    expect(prefsSet).toHaveBeenCalledTimes(2);
    expect(prefsSet).toHaveBeenLastCalledWith("sidebar", { width: 310 });
  });

  it("drops what waits on dispose and takes nothing more", async () => {
    const bridge = bridgeStub();
    const store = createPrefsStore(bridge, { schema: 1 });
    store.set("sidebar", { width: 300 });
    store.dispose();
    store.set("sidebar", { width: 310 });
    await vi.advanceTimersByTimeAsync(WRITE_DELAY_MS * 2);
    await store.flush();
    expect(bridge.prefsSet).not.toHaveBeenCalled();
  });
});
