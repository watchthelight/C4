// <wtl> project: c4 | file: apps/desktop/tests/format.test.ts | author: watchthelight </wtl>

import { describe, expect, it } from "vitest";
import { attentionOrder, cwdTail, displayName, elapsed, plural, shortId } from "../src/format";

describe("format", () => {
  it("takes the cwd tail with either separator", () => {
    expect(cwdTail("C:\\work\\proj\\")).toBe("proj");
    expect(cwdTail("/home/example/repo")).toBe("repo");
    expect(cwdTail("/")).toBeNull();
    expect(cwdTail(null)).toBeNull();
  });

  it("falls back from the name to the cwd to the short id", () => {
    const id = "a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d";
    expect(displayName({ id, name: null, cwd: null })).toBe("a1b2c3d4");
    expect(displayName({ id, name: null, cwd: "/home/example/repo" })).toBe("repo");
    expect(displayName({ id, name: "  nightly build ", cwd: "/x" })).toBe("nightly build");
    expect(displayName({ id, name: "   ", cwd: "/x/y" })).toBe("y");
    expect(shortId("abc")).toBe("abc");
  });

  it("formats durations", () => {
    expect(elapsed(0)).toBe("0s");
    expect(elapsed(-5)).toBe("0s");
    expect(elapsed(Number.NaN)).toBe("0s");
    expect(elapsed(12_000)).toBe("12s");
    expect(elapsed(184_000)).toBe("3m 04s");
    expect(elapsed(72 * 60_000)).toBe("1h 12m");
    expect(elapsed(51 * 3_600_000)).toBe("2d 3h");
    expect(plural(1, "session")).toBe("1 session");
    expect(plural(3, "session")).toBe("3 sessions");
  });

  it("orders by weight, then the longest wait, then the latest activity", () => {
    const base = { waiting_since: null as number | null, last_activity: 0 };
    const idle = { ...base, state: { state: "idle" as const }, last_activity: 50 };
    const working = { ...base, state: { state: "working" as const }, last_activity: 10 };
    const waitLong = { ...base, state: { state: "waiting_permission" as const, what: null }, waiting_since: 100 };
    const waitShort = { ...base, state: { state: "waiting_permission" as const, what: null }, waiting_since: 500 };
    const idleNewer = { ...base, state: { state: "idle" as const }, last_activity: 90 };
    const sorted = attentionOrder([idle, working, waitShort, waitLong, idleNewer]);
    expect(sorted).toEqual([waitLong, waitShort, working, idleNewer, idle]);
  });
});
