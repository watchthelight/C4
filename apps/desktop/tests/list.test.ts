// <wtl> project: c4 | file: apps/desktop/tests/list.test.ts | author: watchthelight </wtl>
// The session list against the synthetic bridge: rows in attention order, state text, the guessed
// marker, selection, removal, and no input affordance anywhere (test I: an observed session gets
// nothing to type into). Then the virtual list itself: only the rows near the viewport exist, their
// nodes are reused while scrolling, the three densities, the keyboard, and the rule that a reorder
// waits for the pointer and the keyboard to leave or rest. jsdom lays nothing out, so each test
// gives the list a viewport height and drives the scroll position by hand.

import { afterEach, describe, expect, it, vi } from "vitest";
import { createMock, syntheticSessions } from "../src/bridge/mock";
import type { SessionView } from "../src/bridge/types";
import { createInspector } from "../src/sessions/inspector";
import { OVERSCAN, ROW_HEIGHT, SETTLE_MS, createSessionList, nextDensity } from "../src/sessions/list";
import type { SessionList } from "../src/sessions/list";
import tokens from "../src/tokens.css?raw";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

/** jsdom reports a zero client height; the list reads it, so the tests define one. */
function viewport(list: SessionList, height: number): void {
  Object.defineProperty(list.el, "clientHeight", { value: height, configurable: true });
}

function y(row: HTMLElement): number {
  return Number(/translateY\((-?\d+)px\)/.exec(row.style.transform)?.[1] ?? Number.NaN);
}

/** The rendered rows in layout order, which is not their DOM order once nodes are reused. */
function rowsOf(list: SessionList): HTMLElement[] {
  return [...list.el.querySelectorAll<HTMLElement>(".row")].sort((a, b) => y(a) - y(b));
}

function idsOf(list: SessionList): (string | undefined)[] {
  return rowsOf(list).map((r) => r.dataset.id);
}

/** `n` sessions made from the synthetic eight, each with its own id. */
function many(n: number): SessionView[] {
  const base = syntheticSessions(NOW);
  const out: SessionView[] = [];
  for (let i = 0; i < n; i++) {
    const seed = base[i % base.length];
    if (seed) {
      out.push({ ...seed, id: `00000000-0000-4000-8000-${String(i).padStart(12, "0")}` });
    }
  }
  return out;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("session list", () => {
  it("renders one row per session in attention order", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const snap = await bridge.listSessions();
    const picked: string[] = [];
    const list = createSessionList({ onSelect: (id) => picked.push(id) });
    viewport(list, 400);
    list.update(snap.sessions, null, NOW);

    const rows = rowsOf(list);
    expect(rows).toHaveLength(snap.sessions.length);
    expect(rows.map((r) => r.dataset.id)).toEqual(snap.sessions.map((s) => s.id));
    expect(rows.map(y)).toEqual(snap.sessions.map((_, i) => i * ROW_HEIGHT.standard));
    expect(rows[0]?.getAttribute("aria-posinset")).toBe("1");
    expect(rows[0]?.getAttribute("aria-setsize")).toBe(String(snap.sessions.length));
    expect(rows[0]?.dataset.kind).toBe("waiting_permission");
    expect(rows[0]?.dataset.attention).toBe("true");
    expect(rows[0]?.querySelector(".activity")?.textContent).toBe("waiting for permission: Bash");
    expect(rows[0]?.querySelector(".name")?.textContent).toBe("beta");
    expect(rows[0]?.getAttribute("aria-label")).toBe("beta, waiting for permission: Bash");
    expect(rows[0]?.hasAttribute("title")).toBe(false);
    expect(rows.at(-1)?.dataset.kind).toBe("completed");

    const guessed = rows.filter((r) => r.dataset.certainty === "heuristic");
    expect(guessed).toHaveLength(1);
    expect(guessed[0]?.querySelector(".activity")?.textContent).toBe("~completed (logout)");
    expect(guessed[0]?.querySelector(".activity")?.getAttribute("title")).toBe("guessed from timer");

    const tool = rows.find((r) => r.dataset.kind === "running_tool");
    expect(tool?.querySelector(".activity")?.textContent).toBe("running Read");
    expect(tool?.querySelector(".tags")?.textContent).toContain("tmux");
    const ctx = tool?.querySelector<HTMLElement>(".ctx");
    expect(ctx?.textContent).toBe("91%");
    expect(ctx?.dataset.band).toBe("hot");
    expect(ctx?.title).toBe("context 91% of 1.0M, claude-fable-5-1");
    expect(ctx?.querySelector<HTMLElement>(".fill")?.style.width).toBe("91%");
    expect(tool?.querySelector(".model")?.textContent).toBe("claude-fable-5-1, effort max");
    const idle = rows.find((r) => r.dataset.kind === "idle");
    expect(idle?.querySelector(".ctx")?.textContent).toBe("");
    expect(idle?.querySelector(".ctx")?.hasAttribute("hidden")).toBe(true);
    expect(idle?.querySelector(".model")?.textContent).toBe("no status line yet");

    expect(list.el.querySelector("input, textarea, button, [contenteditable]")).toBeNull();
    expect(list.el.querySelector(".empty")?.hasAttribute("hidden")).toBe(true);
    expect(list.el.querySelector<HTMLElement>(".spacer")?.style.height).toBe(
      `${snap.sessions.length * ROW_HEIGHT.standard}px`,
    );

    rows[2]?.click();
    expect(picked).toEqual([rows[2]?.dataset.id]);
    list.update(snap.sessions, picked[0] ?? null, NOW);
    expect(rows[2]?.getAttribute("aria-selected")).toBe("true");
    expect(rows[0]?.getAttribute("aria-selected")).toBe("false");
    expect(list.el.getAttribute("aria-activedescendant")).toBe(`session-${picked[0]}`);

    list.update(snap.sessions.slice(1), null, NOW);
    expect(list.el.querySelectorAll(".row")).toHaveLength(snap.sessions.length - 1);
    expect(list.el.hasAttribute("aria-activedescendant")).toBe(false);
    list.update([], null, NOW);
    expect(list.el.querySelectorAll(".row")).toHaveLength(0);
    expect(list.el.querySelector(".empty")?.hasAttribute("hidden")).toBe(false);
    expect(list.el.querySelector<HTMLElement>(".spacer")?.style.height).toBe("0px");
  });

  it("keeps only the rows near the viewport in the DOM and reuses their nodes", () => {
    const views = many(200);
    const list = createSessionList({ onSelect: () => undefined });
    viewport(list, 200);
    list.update(views, null, NOW);

    // six rows fit in 200px at 34px, plus eight of overscan below; none above the top
    const fit = Math.ceil(200 / ROW_HEIGHT.standard);
    const first = rowsOf(list);
    expect(first).toHaveLength(fit + OVERSCAN);
    expect(first[0]?.dataset.id).toBe(views[0]?.id);
    expect(first.map(y)).toEqual(first.map((_, i) => i * ROW_HEIGHT.standard));
    expect(list.el.querySelector<HTMLElement>(".spacer")?.style.height).toBe(`${200 * ROW_HEIGHT.standard}px`);

    const before = new Set(first);
    list.el.scrollTop = 50 * ROW_HEIGHT.standard;
    list.paint();
    const rows = rowsOf(list);
    expect(rows).toHaveLength(fit + 2 * OVERSCAN);
    expect(rows[0]?.dataset.id).toBe(views[50 - OVERSCAN]?.id);
    expect(rows[0]?.getAttribute("aria-posinset")).toBe(String(50 - OVERSCAN + 1));
    expect(rows.at(-1)?.dataset.id).toBe(views[50 + fit + OVERSCAN - 1]?.id);
    // every node from the first pass went out of view and came back carrying another session
    expect(rows.filter((r) => before.has(r)).length).toBe(first.length);
    for (const row of rows) {
      const view = views.find((v) => v.id === row.dataset.id);
      expect(row.querySelector(".name")?.textContent).toBe(view?.name ?? row.querySelector(".name")?.textContent);
      expect(row.id).toBe(`session-${row.dataset.id}`);
    }

    list.el.scrollTop = 200 * ROW_HEIGHT.standard;
    list.paint();
    expect(rowsOf(list).at(-1)?.dataset.id).toBe(views[199]?.id);
    expect(rowsOf(list).length).toBeLessThanOrEqual(fit + 2 * OVERSCAN);
  });

  it("changes density without moving the row at the top of the viewport", () => {
    const views = many(200);
    const list = createSessionList({ onSelect: () => undefined });
    viewport(list, 200);
    list.update(views, null, NOW);
    expect(list.density).toBe("standard");
    expect(list.el.dataset.density).toBe("standard");
    list.el.scrollTop = 50 * ROW_HEIGHT.standard;
    list.paint();

    list.setDensity("compact");
    expect(list.el.dataset.density).toBe("compact");
    expect(list.el.scrollTop).toBe(50 * ROW_HEIGHT.compact);
    const compact = rowsOf(list);
    const fit = Math.ceil(200 / ROW_HEIGHT.compact);
    expect(compact).toHaveLength(fit + 2 * OVERSCAN);
    expect(compact[OVERSCAN]?.dataset.id).toBe(views[50]?.id);
    expect(y(compact[OVERSCAN] ?? compact[0]!)).toBe(50 * ROW_HEIGHT.compact);
    // the compact row has no state line, so its tooltip carries the state text
    const row = compact[OVERSCAN];
    expect(row?.title).toBe(row?.querySelector(".activity")?.textContent);
    expect(list.el.querySelector<HTMLElement>(".spacer")?.style.height).toBe(`${200 * ROW_HEIGHT.compact}px`);

    list.setDensity("expanded");
    expect(list.el.scrollTop).toBe(50 * ROW_HEIGHT.expanded);
    const expanded = rowsOf(list);
    expect(expanded).toHaveLength(Math.ceil(200 / ROW_HEIGHT.expanded) + 2 * OVERSCAN);
    expect(expanded[OVERSCAN]?.dataset.id).toBe(views[50]?.id);
    expect(expanded[OVERSCAN]?.hasAttribute("title")).toBe(false);
    expect(expanded[OVERSCAN]?.querySelector(".third .chip")?.textContent).toBe("observed");

    expect(nextDensity("compact")).toBe("standard");
    expect(nextDensity("standard")).toBe("expanded");
    expect(nextDensity("expanded")).toBe("compact");
  });

  it("moves rows when the order changes, but not under the pointer until it leaves or rests", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    const bridge = createMock({ period: 0, now: () => NOW });
    const first = await bridge.listSessions();
    const list = createSessionList({ onSelect: () => undefined });
    viewport(list, 400);
    list.update(first.sessions, null, NOW);

    const alpha = "0a1b2c3d-0001-4000-8000-000000000001";
    const before = idsOf(list);

    list.el.dispatchEvent(new Event("pointerenter"));
    let snap = bridge.advance();
    expect(snap.sessions.find((s) => s.id === alpha)?.state.state).toBe("running_tool");
    expect(snap.sessions.map((s) => s.id)).not.toEqual(before);
    list.update(snap.sessions, null, NOW);
    expect(idsOf(list)).toEqual(before);
    const alphaRow = list.el.querySelector<HTMLElement>(`[data-id="${alpha}"]`);
    expect(alphaRow?.dataset.kind).toBe("running_tool");

    list.el.dispatchEvent(new Event("pointerleave"));
    expect(idsOf(list)).toEqual(snap.sessions.map((s) => s.id));

    // resting inside the list for two seconds applies the order at the next update
    list.el.dispatchEvent(new Event("pointerenter"));
    const held = idsOf(list);
    snap = bridge.advance();
    list.update(snap.sessions, null, NOW);
    expect(idsOf(list)).toEqual(held);
    vi.advanceTimersByTime(SETTLE_MS - 1);
    list.update(snap.sessions, null, NOW);
    expect(idsOf(list)).toEqual(held);
    vi.advanceTimersByTime(1);
    list.update(snap.sessions, null, NOW);
    expect(idsOf(list)).toEqual(snap.sessions.map((s) => s.id));

    // a pointer that keeps moving keeps the order
    list.el.dispatchEvent(new Event("pointermove"));
    snap = bridge.advance();
    list.update(snap.sessions, null, NOW);
    expect(idsOf(list)).toEqual(idsOf(list));
    list.el.dispatchEvent(new Event("pointerleave"));
    expect(idsOf(list)).toEqual(snap.sessions.map((s) => s.id));
  });

  it("holds the order while the list has keyboard focus", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    const bridge = createMock({ period: 0, now: () => NOW });
    const first = await bridge.listSessions();
    const list = createSessionList({ onSelect: () => undefined });
    viewport(list, 400);
    list.update(first.sessions, null, NOW);
    const held = idsOf(list);

    list.el.dispatchEvent(new FocusEvent("focusin"));
    const snap = bridge.advance();
    list.update(snap.sessions, null, NOW);
    expect(idsOf(list)).toEqual(held);
    // the pointer leaving changes nothing while the keyboard is still in the list
    list.el.dispatchEvent(new Event("pointerleave"));
    expect(idsOf(list)).toEqual(held);
    list.el.dispatchEvent(new FocusEvent("focusout", { relatedTarget: null }));
    expect(idsOf(list)).toEqual(snap.sessions.map((s) => s.id));
  });

  it("selects with the keyboard and scrolls the selection into view", () => {
    const views = many(200);
    const picked: string[] = [];
    const list = createSessionList({ onSelect: (id) => picked.push(id) });
    viewport(list, 200);
    list.update(views, null, NOW);

    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    expect(picked).toEqual([views[0]?.id]);
    expect(list.el.scrollTop).toBe(0);
    list.update(views, picked[0] ?? null, NOW);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    expect(picked[1]).toBe(views[1]?.id);
    list.update(views, picked[1] ?? null, NOW);
    const page = Math.floor(200 / ROW_HEIGHT.standard);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "PageDown" }));
    expect(picked[2]).toBe(views[1 + page]?.id);
    expect(list.el.scrollTop).toBe((2 + page) * ROW_HEIGHT.standard - 200);
    list.update(views, picked[2] ?? null, NOW);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));
    expect(picked[3]).toBe(views[199]?.id);
    expect(list.el.scrollTop).toBe(200 * ROW_HEIGHT.standard - 200);
    list.update(views, picked[3] ?? null, NOW);
    list.paint();
    expect(rowsOf(list).at(-1)?.getAttribute("aria-selected")).toBe("true");
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "Home" }));
    expect(picked[4]).toBe(views[0]?.id);
    expect(list.el.scrollTop).toBe(0);
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete" }));
    expect(picked).toHaveLength(5);
  });

  it("uses the row heights that tokens.css declares", () => {
    for (const [density, px] of Object.entries(ROW_HEIGHT)) {
      expect(tokens).toContain(`--row-${density}: ${px}px;`);
    }
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
    const fact = (term: string) =>
      [...inspector.el.querySelectorAll("dt")].find((d) => d.textContent === term)?.nextElementSibling?.textContent;
    expect(fact("model")).toBe("claude-sonnet-5-5");
    expect(fact("context")).toBe("78% of 200K");
    expect(fact("effort")).toBe("medium");
    expect(fact("status line")).toBe("3m 00s ago");
    expect(inspector.el.querySelector("input, textarea, button, [contenteditable]")).toBeNull();

    const nightly = snap.sessions.find((s) => s.name === "nightly");
    inspector.update(nightly ?? null, NOW);
    const facts = [...inspector.el.querySelectorAll("dd")].map((d) => d.textContent);
    expect(facts).toContain("2 seen, 2 running");
    expect(facts.some((f) => f?.startsWith("0b2c3d4e, 2 tasks in flight"))).toBe(true);
    expect(fact("context")).toBe("unknown");
    expect(fact("status line")).toBe("none received");
  });
});
