// <wtl> project: c4 | file: apps/desktop/tests/mock.test.ts | author: watchthelight </wtl>

import { describe, expect, it } from "vitest";
import { PREFS_KEY, createMock, syntheticSessions } from "../src/bridge/mock";
import type { PrefsStorage } from "../src/bridge/mock";
import { stateWeight } from "../src/format";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

/** A storage stub: what the browser's storage gives, over a map the test can read. */
function memoryStorage(seed: Record<string, string> = {}): PrefsStorage & { map: Map<string, string> } {
  const map = new Map(Object.entries(seed));
  return {
    map,
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
  };
}

describe("mock bridge", () => {
  it("serves unique synthetic sessions in attention order and never a real path", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const snap = await bridge.listSessions();
    expect(snap.schema).toBe(1);
    expect(snap.at).toBe(NOW);
    const ids = snap.sessions.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    const weights = snap.sessions.map((s) => stateWeight(s.state.state));
    expect(weights).toEqual([...weights].sort((a, b) => b - a));
    for (const s of syntheticSessions(NOW)) {
      expect(s.cwd ?? "").toMatch(/example/);
    }
    expect(snap.tick).toBe(0);
    expect(snap.agents).toBeNull();
  });

  it("advances a scripted state machine and reports the transitions", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const seen: number[] = [];
    const stop = await bridge.onSessions((s) => seen.push(s.tick ?? -1));
    const one = bridge.advance();
    expect(one.tick).toBe(1);
    expect(one.transitions?.map((t) => t.to.state)).toEqual(["running_tool"]);
    expect(one.agents?.entries).toBe(3);
    const two = bridge.advance();
    expect(two.transitions?.[0]?.from.state).toBe("running_tool");
    expect(two.transitions?.[0]?.to.state).toBe("working");
    const three = bridge.advance();
    expect(three.transitions?.map((t) => t.to.state)).toEqual(["idle", "waiting_user_input"]);
    expect(seen).toEqual([1, 2, 3]);
    stop();
    bridge.advance();
    expect(seen).toEqual([1, 2, 3]);
  });

  it("holds preferences in memory, hands out copies, and names the file in about", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    expect(await bridge.prefsGet()).toEqual({ schema: 1 });
    const written = await bridge.prefsSet("sidebar", { width: 300, pinned: ["a"] });
    expect(written).toEqual({ schema: 1, sidebar: { width: 300, pinned: ["a"] } });
    // what came back is a copy: changing it changes nothing in the bridge
    (written.sidebar as { pinned: string[] }).pinned.push("b");
    expect(await bridge.prefsGet()).toEqual({ schema: 1, sidebar: { width: 300, pinned: ["a"] } });
    await bridge.prefsSet("notify", { enabled: false });
    expect(await bridge.prefsGet()).toEqual({
      schema: 1,
      sidebar: { width: 300, pinned: ["a"] },
      notify: { enabled: false },
    });
    const about = await bridge.about();
    expect(about.prefs_path).toBe("/home/example/.c4/prefs.json");
    expect(about.prefs_problem).toBeNull();
  });

  it("refuses a section name the core would refuse", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    for (const bad of ["", "schema", "Sidebar", "side bar", "side-bar", "sidebar2"]) {
      await expect(bridge.prefsSet(bad, {})).rejects.toThrow("is not a section name");
    }
    await expect(bridge.prefsSet("side_bar", {})).resolves.toHaveProperty("side_bar");
    expect(await bridge.prefsGet()).toEqual({ schema: 1, side_bar: {} });
  });

  it("keeps preferences in the storage it is given, and starts over when that holds junk", async () => {
    const storage = memoryStorage();
    const first = createMock({ period: 0, now: () => NOW, storage });
    await first.prefsSet("sidebar", { width: 280 });
    expect(JSON.parse(storage.map.get(PREFS_KEY) ?? "")).toEqual({ schema: 1, sidebar: { width: 280 } });
    const second = createMock({ period: 0, now: () => NOW, storage });
    expect(await second.prefsGet()).toEqual({ schema: 1, sidebar: { width: 280 } });

    const junk = memoryStorage({ [PREFS_KEY]: "[1, 2" });
    expect(await createMock({ period: 0, now: () => NOW, storage: junk }).prefsGet()).toEqual({ schema: 1 });
    const list = memoryStorage({ [PREFS_KEY]: "[1, 2]" });
    expect(await createMock({ period: 0, now: () => NOW, storage: list }).prefsGet()).toEqual({ schema: 1 });

    const refusing: PrefsStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    const third = createMock({ period: 0, now: () => NOW, storage: refusing });
    expect(await third.prefsGet()).toEqual({ schema: 1 });
    expect(await third.prefsSet("sidebar", { width: 1 })).toEqual({ schema: 1, sidebar: { width: 1 } });
  });
});
