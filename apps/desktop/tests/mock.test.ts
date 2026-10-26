// <wtl> project: c4 | file: apps/desktop/tests/mock.test.ts | author: watchthelight </wtl>

import { describe, expect, it } from "vitest";
import { createMock, syntheticSessions } from "../src/bridge/mock";
import { stateWeight } from "../src/format";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

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
});
