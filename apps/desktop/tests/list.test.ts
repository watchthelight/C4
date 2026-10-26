// <wtl> project: c4 | file: apps/desktop/tests/list.test.ts | author: watchthelight </wtl>
// The session list against the synthetic bridge: rows in attention order, state text, the guessed
// marker, selection, removal, and no input affordance anywhere (test I: an observed session gets
// nothing to type into).

import { describe, expect, it } from "vitest";
import { createMock } from "../src/bridge/mock";
import { createSessionList } from "../src/sessions/list";
import { createInspector } from "../src/sessions/inspector";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

describe("session list", () => {
  it("renders one row per session in attention order", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const snap = await bridge.listSessions();
    const picked: string[] = [];
    const list = createSessionList({ onSelect: (id) => picked.push(id) });
    list.update(snap.sessions, null, NOW);

    const rows = [...list.el.querySelectorAll<HTMLElement>(".row")];
    expect(rows).toHaveLength(snap.sessions.length);
    expect(rows.map((r) => r.dataset.id)).toEqual(snap.sessions.map((s) => s.id));
    expect(rows[0]?.dataset.kind).toBe("waiting_permission");
    expect(rows[0]?.dataset.attention).toBe("true");
    expect(rows[0]?.querySelector(".activity")?.textContent).toBe("waiting for permission: Bash");
    expect(rows[0]?.querySelector(".name")?.textContent).toBe("beta");
    expect(rows[0]?.getAttribute("aria-label")).toBe("beta, waiting for permission: Bash");
    expect(rows.at(-1)?.dataset.kind).toBe("completed");

    const guessed = rows.filter((r) => r.dataset.certainty === "heuristic");
    expect(guessed).toHaveLength(1);
    expect(guessed[0]?.querySelector(".activity")?.textContent).toBe("~completed (logout)");
    expect(guessed[0]?.querySelector(".activity")?.getAttribute("title")).toBe("guessed from timer");

    const tool = rows.find((r) => r.dataset.kind === "running_tool");
    expect(tool?.querySelector(".activity")?.textContent).toBe("running Read");
    expect(tool?.querySelector(".tags")?.textContent).toContain("tmux");

    expect(list.el.querySelector("input, textarea, button, [contenteditable]")).toBeNull();
    expect(list.el.querySelector(".empty")?.hasAttribute("hidden")).toBe(true);

    rows[2]?.click();
    expect(picked).toEqual([rows[2]?.dataset.id]);
    list.update(snap.sessions, picked[0] ?? null, NOW);
    expect(rows[2]?.getAttribute("aria-selected")).toBe("true");
    expect(rows[0]?.getAttribute("aria-selected")).toBe("false");

    list.update(snap.sessions.slice(1), null, NOW);
    expect(list.el.querySelectorAll(".row")).toHaveLength(snap.sessions.length - 1);
    list.update([], null, NOW);
    expect(list.el.querySelectorAll(".row")).toHaveLength(0);
    expect(list.el.querySelector(".empty")?.hasAttribute("hidden")).toBe(false);
  });

  it("moves rows when the order changes, but not under the pointer", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const first = await bridge.listSessions();
    const list = createSessionList({ onSelect: () => undefined });
    list.update(first.sessions, null, NOW);

    const ids = () => [...list.el.querySelectorAll<HTMLElement>(".row")].map((r) => r.dataset.id);
    const alpha = "0a1b2c3d-0001-4000-8000-000000000001";
    const before = ids();

    list.el.dispatchEvent(new Event("pointerenter"));
    let snap = bridge.advance();
    expect(snap.sessions.find((s) => s.id === alpha)?.state.state).toBe("running_tool");
    list.update(snap.sessions, null, NOW);
    expect(ids()).toEqual(before);
    const alphaRow = list.el.querySelector<HTMLElement>(`[data-id="${alpha}"]`);
    expect(alphaRow?.dataset.kind).toBe("running_tool");

    list.el.dispatchEvent(new Event("pointerleave"));
    expect(ids()).toEqual(snap.sessions.map((s) => s.id));

    snap = bridge.advance();
    list.update(snap.sessions, null, NOW);
    expect(ids()).toEqual(snap.sessions.map((s) => s.id));
  });

  it("selects with the keyboard", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const snap = await bridge.listSessions();
    const picked: string[] = [];
    const list = createSessionList({ onSelect: (id) => picked.push(id) });
    list.update(snap.sessions, null, NOW);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    expect(picked).toEqual([snap.sessions[0]?.id]);
    list.update(snap.sessions, picked[0] ?? null, NOW);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    expect(picked[1]).toBe(snap.sessions[1]?.id);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));
    expect(picked[2]).toBe(snap.sessions.at(-1)?.id);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete" }));
    expect(picked).toHaveLength(3);
  });
});

describe("inspector", () => {
  it("lists the facts of the selected session and nothing to type into", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const snap = await bridge.listSessions();
    const inspector = createInspector();
    inspector.update(null, NOW);
    expect(inspector.el.querySelector(".hint")?.hasAttribute("hidden")).toBe(false);

    const beta = snap.sessions.find((s) => s.name === "beta");
    expect(beta).toBeDefined();
    inspector.update(beta ?? null, NOW);
    expect(inspector.el.querySelector(".title")?.textContent).toBe("beta");
    expect(inspector.el.querySelector(".state")?.textContent).toContain("waiting for permission: Bash for 3m 00s");
    expect(inspector.el.querySelector(".state")?.textContent).toContain("observed from hook Notification");
    expect(inspector.el.querySelector(".note")?.textContent).toContain("WezTerm");
    const terms = [...inspector.el.querySelectorAll("dt")].map((d) => d.textContent);
    expect(terms).toContain("cwd");
    expect(terms).toContain("telemetry");
    expect(inspector.el.querySelector("input, textarea, button, [contenteditable]")).toBeNull();

    const nightly = snap.sessions.find((s) => s.name === "nightly");
    inspector.update(nightly ?? null, NOW);
    const facts = [...inspector.el.querySelectorAll("dd")].map((d) => d.textContent);
    expect(facts).toContain("2 seen, 2 running");
    expect(facts.some((f) => f?.startsWith("0b2c3d4e, 2 tasks in flight"))).toBe(true);
  });
});
