// <wtl> project: c4 | file: apps/desktop/tests/rail.test.ts | author: watchthelight </wtl>
// The sidebar folded to its rail: one square per session in attention order with pins first, the
// name and state in each square's tooltip and accessible name, the count of sessions that need
// you, selection by click and by the arrow keys, the unfold button, a reorder held while the
// pointer is inside, and the hover card asked for after the pointer rests.

import { afterEach, describe, expect, it, vi } from "vitest";
import { syntheticSessions } from "../src/bridge/mock";
import type { SessionView } from "../src/bridge/types";
import { attentionOrder, describeState, displayName, needsAttention } from "../src/format";
import { HOVER_DELAY_MS, SETTLE_MS } from "../src/sessions/list";
import { createRail } from "../src/sessions/rail";
import type { Rail } from "../src/sessions/rail";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

function itemsOf(rail: Rail): HTMLElement[] {
  return [...rail.el.querySelectorAll<HTMLElement>(".item")];
}

function idsOf(rail: Rail): (string | undefined)[] {
  return itemsOf(rail).map((i) => i.dataset.id);
}

function pointer(target: Element, type: string, x: number, y: number): void {
  target.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX: x, clientY: y }));
}

function key(target: Element, k: string): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

function sessions(): SessionView[] {
  return attentionOrder(syntheticSessions(NOW));
}

afterEach(() => {
  vi.useRealTimers();
});

describe("rail", () => {
  it("shows one square per session in attention order, pinned first, named in the tooltip", () => {
    const views = sessions();
    const rail = createRail({ onSelect: () => {}, onExpand: () => {} });
    document.body.append(rail.el);
    rail.update(views, null, () => false);

    expect(rail.el.getAttribute("aria-label")).toBe("sessions, folded");
    const items = rail.el.querySelector<HTMLElement>(".items");
    expect(items?.getAttribute("role")).toBe("listbox");
    expect(items?.tabIndex).toBe(0);
    expect(idsOf(rail)).toEqual(views.map((v) => v.id));
    const first = itemsOf(rail)[0];
    const view = views[0];
    if (!first || !view) {
      throw new Error("no squares");
    }
    expect(first.getAttribute("role")).toBe("option");
    expect(first.tabIndex).toBe(-1);
    expect(first.title).toBe(`${displayName(view)}: ${describeState(view.state)}`);
    expect(first.getAttribute("aria-label")).toBe(first.title);
    expect(first.dataset.kind).toBe(view.state.state);
    expect(first.dataset.pinned).toBe("false");
    expect(first.getAttribute("aria-selected")).toBe("false");
    expect(first.querySelector(".sq")).not.toBeNull();
    expect(rail.el.querySelectorAll("input, textarea")).toHaveLength(0);

    const attention = views.filter((v) => needsAttention(v.state.state)).length;
    expect(attention).toBeGreaterThan(0);
    expect(rail.el.querySelector(".need")?.textContent).toBe(String(attention));

    // the last session pinned comes first and says so
    const last = views[views.length - 1];
    if (!last) {
      throw new Error("no last session");
    }
    rail.update(views, last.id, (id) => id === last.id);
    expect(idsOf(rail)[0]).toBe(last.id);
    const pinned = itemsOf(rail)[0];
    expect(pinned?.dataset.pinned).toBe("true");
    expect(pinned?.title).toBe(`${displayName(last)}: ${describeState(last.state)}, pinned`);
    expect(pinned?.getAttribute("aria-selected")).toBe("true");
    expect(items?.getAttribute("aria-activedescendant")).toBe(`rail-${last.id}`);

    // no session, no count
    rail.update([], null, () => false);
    expect(idsOf(rail)).toEqual([]);
    expect(rail.el.querySelector(".need")?.textContent).toBe("");
    rail.dispose();
    expect(rail.el.isConnected).toBe(false);
  });

  it("selects on click and with the arrow keys, and the unfold button opens the sidebar", () => {
    const views = sessions();
    const picked: string[] = [];
    let expanded = 0;
    const rail = createRail({
      onSelect: (id) => picked.push(id),
      onExpand: () => {
        expanded += 1;
      },
    });
    document.body.append(rail.el);
    rail.update(views, null, () => false);
    const ids = views.map((v) => v.id);
    const items = rail.el.querySelector<HTMLElement>(".items");
    if (!items) {
      throw new Error("no items");
    }

    itemsOf(rail)[2]?.click();
    expect(picked).toEqual([ids[2]]);

    rail.focus();
    expect(document.activeElement).toBe(items);
    // nothing selected yet as far as the rail knows: the first key lands on the first square
    expect(key(items, "ArrowDown").defaultPrevented).toBe(true);
    expect(picked).toEqual([ids[2], ids[0]]);
    rail.update(views, ids[2] ?? null, () => false);
    key(items, "ArrowDown");
    expect(picked.at(-1)).toBe(ids[3]);
    key(items, "ArrowUp");
    expect(picked.at(-1)).toBe(ids[1]);
    key(items, "End");
    expect(picked.at(-1)).toBe(ids[ids.length - 1]);
    key(items, "Home");
    expect(picked.at(-1)).toBe(ids[0]);
    expect(key(items, "p").defaultPrevented).toBe(false);
    expect(picked).toHaveLength(6);

    rail.el.querySelector<HTMLElement>(".unfold")?.click();
    expect(expanded).toBe(1);
    expect(picked).toHaveLength(6);
    rail.dispose();
  });

  it("holds a reorder while the pointer is inside and applies it on leave or after a rest", () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    const views = sessions();
    const rail = createRail({ onSelect: () => {}, onExpand: () => {} });
    document.body.append(rail.el);
    rail.update(views, null, () => false);
    const before = idsOf(rail);

    // the last session now needs attention: it belongs near the front
    const last = views[views.length - 1];
    if (!last) {
      throw new Error("no last session");
    }
    const changed: SessionView[] = views.map((v) =>
      v.id === last.id ? { ...v, state: { state: "waiting_permission", what: null }, waiting_since: NOW } : v,
    );
    const wanted = attentionOrder(changed).map((v) => v.id);
    expect(wanted).not.toEqual(before);

    pointer(rail.el, "pointerenter", 10, 10);
    rail.update(changed, null, () => false);
    expect(idsOf(rail)).toEqual(before);
    // the square changed its state at once, only its place waits
    expect(itemsOf(rail).at(-1)?.dataset.kind).toBe("waiting_permission");

    pointer(rail.el, "pointerleave", 10, 10);
    expect(idsOf(rail)).toEqual(wanted);

    // the same with the keyboard inside: the order applies once the rail has rested
    const items = rail.el.querySelector<HTMLElement>(".items");
    if (!items) {
      throw new Error("no items");
    }
    rail.focus();
    rail.update(views, null, () => false);
    expect(idsOf(rail)).toEqual(wanted);
    vi.setSystemTime(NOW + SETTLE_MS);
    rail.update(views, null, () => false);
    expect(idsOf(rail)).toEqual(before);
    rail.dispose();
  });

  it("asks for the hover card after the pointer rests, closes it on a key and waits for a move", () => {
    vi.useFakeTimers();
    const views = sessions();
    const hovers: (string | null)[] = [];
    let anchor: HTMLElement | null = null;
    const rail = createRail({
      onSelect: () => {},
      onExpand: () => {},
      onHover: (id, at) => {
        hovers.push(id);
        anchor = at;
      },
    });
    document.body.append(rail.el);
    rail.update(views, null, () => false);
    const [first, second] = itemsOf(rail);
    if (!first || !second) {
      throw new Error("no squares");
    }

    pointer(first, "pointerover", 10, 30);
    expect(hovers).toEqual([]);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(hovers).toEqual([first.dataset.id]);
    expect(anchor).toBe(first);
    // with a card open the next square gets it at once
    pointer(second, "pointermove", 10, 54);
    expect(hovers).toEqual([first.dataset.id, second.dataset.id]);

    // a key closes it, and the browser's repeated event for the resting pointer brings nothing back
    const items = rail.el.querySelector<HTMLElement>(".items");
    if (!items) {
      throw new Error("no items");
    }
    key(items, "ArrowDown");
    expect(hovers.at(-1)).toBeNull();
    pointer(second, "pointerover", 10, 54);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(hovers).toHaveLength(3);
    // a real move arms the wait again
    pointer(second, "pointermove", 11, 55);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(hovers.at(-1)).toBe(second.dataset.id);

    // a square that is gone closes its card
    rail.update(
      views.filter((v) => v.id !== second.dataset.id),
      null,
      () => false,
    );
    expect(hovers.at(-1)).toBeNull();
    expect(itemsOf(rail)).toHaveLength(views.length - 1);

    pointer(rail.el, "pointerleave", 0, 0);
    expect(hovers.filter((h) => h === null)).toHaveLength(2);
    rail.dispose();
  });
});
