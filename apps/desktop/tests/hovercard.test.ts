// <wtl> project: c4 | file: apps/desktop/tests/hovercard.test.ts | author: watchthelight </wtl>
// The hover card: what it says about a session, where it goes beside the row, and the list's part
// of it: the card opens after the pointer has rested on a row for 350 ms, follows the pointer from
// row to row without a second wait, and closes when the pointer leaves, the list scrolls, a key is
// pressed, the list is changed by hand, or the row is gone, and then waits for the pointer to move
// again. jsdom lays nothing out, so the geometry is stubbed.

import { afterEach, describe, expect, it, vi } from "vitest";
import { syntheticSessions } from "../src/bridge/mock";
import type { SessionView } from "../src/bridge/types";
import { CARD_AGENTS, CARD_GAP, cardModel, createHoverCard, modeReason } from "../src/sessions/hovercard";
import { HOVER_DELAY_MS, createSessionList } from "../src/sessions/list";
import type { SessionList } from "../src/sessions/list";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

function byName(views: readonly SessionView[], name: string): SessionView {
  const view = views.find((v) => v.name === name || v.cwd?.endsWith(`/${name}`));
  if (!view) {
    throw new Error(`no synthetic session ${name}`);
  }
  return view;
}

function fact(m: ReturnType<typeof cardModel>, key: string): string | undefined {
  return m.facts.find(([k]) => k === key)?.[1];
}

/** jsdom gives every element a zero size; the card reads its own, so the tests define it. */
function size(node: HTMLElement, width: number, height: number): void {
  Object.defineProperty(node, "offsetWidth", { value: width, configurable: true });
  Object.defineProperty(node, "offsetHeight", { value: height, configurable: true });
}

function windowSize(width: number, height: number): void {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: height, configurable: true });
}

function anchorAt(left: number, top: number, width: number, height: number): HTMLElement {
  const node = document.createElement("div");
  node.getBoundingClientRect = () =>
    ({
      left,
      top,
      width,
      height,
      right: left + width,
      bottom: top + height,
      x: left,
      y: top,
      toJSON: () => ({}),
    }) as DOMRect;
  return node;
}

function viewport(list: SessionList, height: number): void {
  Object.defineProperty(list.el, "clientHeight", { value: height, configurable: true });
}

function y(row: HTMLElement): number {
  return Number(/translateY\((-?\d+)px\)/.exec(row.style.transform)?.[1] ?? Number.NaN);
}

function rowsOf(list: SessionList): HTMLElement[] {
  return [...list.el.querySelectorAll<HTMLElement>(".row")].sort((a, b) => y(a) - y(b));
}

/** A pointer event at (x, y); the same coordinates twice is what the browser sends for a resting pointer. */
function over(node: HTMLElement | null | undefined, x: number, y: number): void {
  node?.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, clientX: x, clientY: y }));
}

function move(node: HTMLElement | null | undefined, x: number, y: number): void {
  node?.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, clientX: x, clientY: y }));
}

afterEach(() => {
  vi.useRealTimers();
});

describe("hover card model", () => {
  it("describes a session the way the row and the inspector do", () => {
    const views = syntheticSessions(NOW);

    const beta = cardModel(byName(views, "beta"), NOW);
    expect(beta.name).toBe("beta");
    expect(beta.mode).toBe("observed");
    expect(beta.reason).toBe("WezTerm, no input from C4");
    expect(beta.kind).toBe("waiting_permission");
    expect(beta.certainty).toBe("observed");
    expect(beta.state).toBe("waiting for permission: Bash for 3m 00s");
    expect(beta.source).toBe("observed from hook Notification");
    expect(beta.context).toEqual({ pct: 78, band: "warn" });
    expect(beta.facts.map(([k]) => k)).toEqual([
      "model",
      "context",
      "agents",
      "cwd",
      "started",
      "last activity",
      "status line",
      "telemetry",
    ]);
    expect(fact(beta, "model")).toBe("claude-sonnet-5-5, effort medium");
    expect(fact(beta, "context")).toBe("78% of 200K");
    expect(fact(beta, "agents")).toBe("none seen");
    expect(fact(beta, "cwd")).toBe("/home/example/proj/beta");
    expect(fact(beta, "started")).toBe("45m 00s ago");
    expect(fact(beta, "last activity")).toBe("3m 00s ago");
    expect(fact(beta, "status line")).toBe("3m 00s ago");
    expect(fact(beta, "telemetry")).toBe("hook telemetry, hooks seen");
    expect(beta.agents).toEqual([]);

    const docs = cardModel(byName(views, "docs"), NOW);
    expect(docs.name).toBe("docs");
    expect(docs.reason).toBe("tmux, no input from C4");
    expect(fact(docs, "tool")).toBe("Read for 4s");
    expect(docs.context).toEqual({ pct: 91, band: "hot" });
    expect(fact(docs, "context")).toBe("91% of 1.0M");

    const nightly = cardModel(byName(views, "nightly"), NOW);
    expect(nightly.reason).toBe("unknown host, no input from C4");
    expect(fact(nightly, "agents")).toBe("2 seen, 2 running");
    expect(nightly.agents).toEqual([
      "Explore: find the config readers, running",
      "Plan: plan the migration, running",
    ]);
    expect(nightly.context).toBeNull();
    expect(fact(nightly, "context")).toBe("unknown");
    expect(fact(nightly, "model")).toBe("no status line yet");
    expect(fact(nightly, "status line")).toBe("none received");

    const review = cardModel(byName(views, "review"), NOW);
    expect(review.mode).toBe("disconnected");
    expect(review.reason).toBe("no live process");
    expect(review.certainty).toBe("heuristic");
    expect(review.state).toBe("~completed (logout) for 30m 00s");
    expect(review.source).toBe("guessed from timer");
    expect(fact(review, "telemetry")).toBe("coarse: no hook telemetry yet, ended");

    expect(cardModel(byName(views, "old worker"), NOW).reason).toBe("process exited");
  });

  it("names the pane of an attached session and the owner of a managed one", () => {
    const beta = byName(syntheticSessions(NOW), "beta");
    expect(modeReason({ ...beta, attachment: "interactive_attached" })).toBe("WezTerm pane 7");
    expect(modeReason({ ...beta, attachment: "interactive_attached", pane: null })).toBe("WezTerm");
    expect(modeReason({ ...beta, attachment: "managed" })).toBe("a pane C4 owns");
  });

  it("names the last three agents and counts the open tasks", () => {
    const nightly = byName(syntheticSessions(NOW), "nightly");
    const agents = [0, 1, 2, 3].map((i) => ({
      id: `agent-${i}`,
      agent_type: null,
      description: null,
      transcript_path: null,
      started_at: NOW - (4 - i) * 60_000,
      ended_at: i === 0 ? NOW : null,
      observed_running: i > 0,
    }));
    const tasks = [
      { id: "t1", kind: "shell", status: "running", description: null, created_at: NOW - 1_000, completed_at: null },
      { id: "t2", kind: "subagent", status: "completed", description: null, created_at: NOW - 5_000, completed_at: NOW - 2_000 },
    ];
    const m = cardModel({ ...nightly, agents, background_tasks: tasks }, NOW);
    expect(m.agents).toHaveLength(CARD_AGENTS);
    expect(m.agents).toEqual(["agent: agent-1, running", "agent: agent-2, running", "agent: agent-3, running"]);
    expect(fact(m, "agents")).toBe("4 seen, 3 running");
    expect(fact(m, "tasks")).toBe("1 open of 2");
    expect(cardModel({ ...nightly, agents: agents.slice(0, 1) }, NOW).agents).toEqual(["agent: agent-0, done"]);
  });
});

describe("hover card", () => {
  it("opens beside the row, keeps itself current and closes", () => {
    windowSize(1280, 800);
    const views = syntheticSessions(NOW);
    const beta = byName(views, "beta");
    const card = createHoverCard();
    document.body.append(card.el);
    size(card.el, 300, 200);
    expect(card.el.hidden).toBe(true);
    expect(card.el.getAttribute("role")).toBe("tooltip");
    expect(card.id).toBeNull();

    card.show(beta, anchorAt(0, 100, 320, 34), NOW);
    expect(card.el.hidden).toBe(false);
    expect(card.id).toBe(beta.id);
    expect(card.el.dataset.kind).toBe("waiting_permission");
    expect(card.el.dataset.certainty).toBe("observed");
    expect(card.el.querySelector(".name")?.textContent).toBe("beta");
    expect(card.el.querySelector(".chip")?.textContent).toBe("observed");
    expect(card.el.querySelector(".reason")?.textContent).toBe("WezTerm, no input from C4");
    expect(card.el.querySelector(".state")?.textContent).toBe("waiting for permission: Bash for 3m 00s");
    expect(card.el.querySelector(".source")?.textContent).toBe("observed from hook Notification");
    expect([...card.el.querySelectorAll("dt")].map((n) => n.textContent)).toEqual([
      "model",
      "context",
      "agents",
      "cwd",
      "started",
      "last activity",
      "status line",
      "telemetry",
    ]);
    const context = card.el.querySelectorAll<HTMLElement>("dd")[1];
    expect(context?.textContent).toBe("78% of 200K");
    expect(context?.dataset.band).toBe("warn");
    expect(context?.querySelector<HTMLElement>(".fill")?.style.width).toBe("78%");
    expect(card.el.querySelector(".agents")?.hasAttribute("hidden")).toBe(true);
    expect(card.el.style.left).toBe(`${320 + CARD_GAP}px`);
    expect(card.el.style.top).toBe("100px");
    // nothing to type into, and no pointer target: the row under it keeps the pointer
    expect(card.el.querySelector("input, textarea, button, [contenteditable]")).toBeNull();

    // the clock moves and the state changes: the card follows without reopening
    const later = NOW + 60_000;
    const working: SessionView = { ...beta, state: { state: "working" }, state_since: later, seq: beta.seq + 1 };
    card.refresh([working], later);
    expect(card.el.querySelector(".state")?.textContent).toBe("working for 0s");
    expect(card.el.dataset.kind).toBe("working");
    expect(card.el.querySelectorAll("dd")[4]?.textContent).toBe("46m 00s ago");
    // the session is gone from the views: the card stays as it is until the list closes it
    card.refresh([], later);
    expect(card.el.hidden).toBe(false);
    expect(card.id).toBe(beta.id);

    const nightly = byName(views, "nightly");
    card.show(nightly, anchorAt(0, 700, 320, 34), NOW);
    expect(card.el.querySelector(".agents")?.hasAttribute("hidden")).toBe(false);
    expect([...card.el.querySelectorAll(".agents li")].map((n) => n.textContent)).toEqual([
      "Explore: find the config readers, running",
      "Plan: plan the migration, running",
    ]);
    // no room below the row: the card moves up to the window's edge
    expect(card.el.style.top).toBe(`${800 - CARD_GAP - 200}px`);

    card.hide();
    expect(card.el.hidden).toBe(true);
    expect(card.id).toBeNull();
    card.refresh(views, NOW);
    expect(card.el.hidden).toBe(true);

    // no room on the right: the card goes to the left of the row, never past the window
    windowSize(600, 800);
    card.show(beta, anchorAt(0, 100, 320, 34), NOW);
    expect(card.el.style.left).toBe(`${CARD_GAP}px`);
    card.show(beta, anchorAt(400, 100, 120, 34), NOW);
    expect(card.el.style.left).toBe(`${400 - CARD_GAP - 300}px`);

    card.dispose();
    expect(card.el.isConnected).toBe(false);
  });
});

describe("the list and the card", () => {
  it("asks for the card after the pointer rests, moves it at once, and closes it", () => {
    vi.useFakeTimers();
    const calls: (string | null)[][] = [];
    const list = createSessionList({
      onSelect: () => undefined,
      onHover: (id, anchor) => calls.push([id, anchor?.dataset.id ?? null]),
    });
    viewport(list, 400);
    const views = syntheticSessions(NOW);
    list.update(views, null, NOW);
    const rows = rowsOf(list);
    const id = (i: number): string | null => rows[i]?.dataset.id ?? null;

    over(rows[0], 10, 10);
    vi.advanceTimersByTime(HOVER_DELAY_MS - 1);
    expect(calls).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(calls).toEqual([[id(0), id(0)]]);

    // movement inside the same row changes nothing; the next row gets the card at once
    move(rows[0], 12, 11);
    expect(calls).toHaveLength(1);
    over(rows[1], 10, 40);
    expect(calls).toEqual([
      [id(0), id(0)],
      [id(1), id(1)],
    ]);

    // a scroll closes it; the browser's own event for the resting pointer reopens nothing, a real
    // movement over the same row starts the wait again
    list.el.dispatchEvent(new Event("scroll"));
    expect(calls.at(-1)).toEqual([null, null]);
    calls.length = 0;
    over(rows[1], 10, 40);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([]);
    move(rows[1], 11, 40);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([[id(1), id(1)]]);

    // a key closes it, and the card stays closed while the pointer rests
    list.el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(calls.at(-1)).toEqual([null, null]);
    calls.length = 0;
    move(rows[1], 11, 40);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([]);

    // the space under the rows is not a row
    over(rows[2], 10, 70);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([[id(2), id(2)]]);
    over(list.el.querySelector<HTMLElement>(".spacer"), 10, 300);
    expect(calls.at(-1)).toEqual([null, null]);

    // leaving before the wait is over opens nothing
    calls.length = 0;
    over(rows[3], 10, 100);
    list.el.dispatchEvent(new Event("pointerleave"));
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([]);

    // the row vanishes while the card is up
    over(rows[0], 10, 10);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([[id(0), id(0)]]);
    list.update(
      views.filter((v) => v.id !== id(0)),
      null,
      NOW,
    );
    expect(calls.at(-1)).toEqual([null, null]);

    // a change by hand closes it too
    calls.length = 0;
    over(rowsOf(list)[0], 10, 12);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toHaveLength(1);
    list.setGroupBy("project");
    expect(calls.at(-1)).toEqual([null, null]);

    // disposing cancels a pending wait
    calls.length = 0;
    over(rowsOf(list)[0], 10, 14);
    list.dispose();
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(calls).toEqual([]);
  });
});
