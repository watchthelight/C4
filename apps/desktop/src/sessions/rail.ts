// <wtl> project: c4 | file: apps/desktop/src/sessions/rail.ts | author: watchthelight </wtl>
// The sidebar folded, or away in its own window: a 36px rail with one state square per session,
// pinned sessions first and the rest in the core's attention order, a button at the top that opens
// the sidebar again (or brings it back from its window) and the count of sessions that need you at
// the bottom. A square carries the session's name and state in
// its tooltip and its accessible name, a click selects the session, and the arrow keys move the
// selection. The rail follows the list's two rules: a reorder from the core waits while the pointer
// is inside or the rail has keyboard focus, and the pointer resting on a square for 350 ms asks for
// the hover card, which closes on a key, a button or a scroll and waits for the pointer to move.

import { el } from "../dom";
import { attentionOrder, describeState, displayName, needsAttention } from "../format";
import { HOVER_DELAY_MS, SETTLE_MS } from "./list";
import type { SessionView } from "../bridge/types";

export interface RailOptions {
  onSelect(id: string): void;
  /** The button at the top: open the sidebar in place of the rail, or bring it back from its window. */
  onExpand(): void;
  /** As the list's: a square once the pointer has rested on it, nulls when the card should close. */
  onHover?(id: string | null, anchor: HTMLElement | null): void;
}

export interface Rail {
  el: HTMLElement;
  /** Render `views`; `selected` is the highlighted id; `isPinned` says which come first. */
  update(views: readonly SessionView[], selected: string | null, isPinned: (id: string) => boolean): void;
  /** Focus the squares, so the arrow keys move the selection. */
  focus(): void;
  /** Say whether the sidebar is away in its own window: the button at the top then brings it back. */
  setDetached(detached: boolean): void;
  dispose(): void;
}

function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id, i) => id === b[i]);
}

/** `order` with the ids no longer in `next` dropped and the new ones of `next` appended. */
function holdOrder(order: readonly string[], next: readonly string[]): string[] {
  const wanted = new Set(next);
  const kept = order.filter((id) => wanted.has(id));
  const have = new Set(kept);
  for (const id of next) {
    if (!have.has(id)) {
      kept.push(id);
    }
  }
  return kept;
}

export function createRail(options: RailOptions): Rail {
  const root = el("nav", "rail");
  root.setAttribute("aria-label", "sessions, folded");
  root.dataset.detached = "false";
  const unfold = el("button", "unfold");
  unfold.type = "button";
  const glyph = el("span", "glyph-sidebar");
  glyph.setAttribute("aria-hidden", "true");
  unfold.append(glyph);

  function setDetached(detached: boolean): void {
    root.dataset.detached = String(detached);
    const what = detached ? "bring the sidebar back from its window" : "open the sidebar";
    unfold.setAttribute("aria-label", what);
    unfold.title = `${what} (Ctrl+Shift+B)`;
    glyph.className = detached ? "glyph-window" : "glyph-sidebar";
  }
  setDetached(false);
  const items = el("div", "items");
  items.setAttribute("role", "listbox");
  items.setAttribute("aria-label", "sessions");
  items.tabIndex = 0;
  const need = el("span", "need");
  need.title = "sessions that need you";
  root.append(unfold, items, need);

  const nodes = new Map<string, HTMLButtonElement>();
  const viewById = new Map<string, SessionView>();
  /** The ids in the order they are laid out. */
  let order: string[] = [];
  /** The order the core wants, held back while the pointer or the keyboard is in the rail. */
  let pending: string[] | null = null;
  let selected: string | null = null;
  let pinnedOf: (id: string) => boolean = () => false;
  let hovering = false;
  let focused = false;
  let lastInput = 0;
  let hoverId: string | null = null;
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let shown: string | null = null;
  let lastX = Number.NaN;
  let lastY = Number.NaN;
  let quiet = false;

  function touch(): void {
    lastInput = Date.now();
  }

  function settled(): boolean {
    return (!hovering && !focused) || Date.now() - lastInput >= SETTLE_MS;
  }

  function makeItem(id: string): HTMLButtonElement {
    const item = el("button", "item");
    item.type = "button";
    item.tabIndex = -1;
    item.setAttribute("role", "option");
    item.id = `rail-${id}`;
    item.dataset.id = id;
    const sq = el("span", "sq");
    sq.setAttribute("aria-hidden", "true");
    item.append(sq);
    return item;
  }

  function fillItem(item: HTMLButtonElement, view: SessionView): void {
    const pinned = pinnedOf(view.id);
    const text = `${displayName(view)}: ${describeState(view.state)}${pinned ? ", pinned" : ""}`;
    if (item.title !== text) {
      item.title = text;
      item.setAttribute("aria-label", text);
    }
    item.dataset.kind = view.state.state;
    item.dataset.attention = needsAttention(view.state.state) ? "true" : "false";
    item.dataset.pinned = pinned ? "true" : "false";
    item.dataset.alive = view.alive ? "true" : "false";
    item.setAttribute("aria-selected", view.id === selected ? "true" : "false");
  }

  function paint(): void {
    const current = [...items.children].map((node) => (node as HTMLElement).dataset.id ?? "");
    const moved = !sameOrder(current, order);
    let attention = 0;
    for (const id of order) {
      const view = viewById.get(id);
      if (view === undefined) {
        continue;
      }
      let item = nodes.get(id);
      if (item === undefined) {
        item = makeItem(id);
        nodes.set(id, item);
      }
      fillItem(item, view);
      if (needsAttention(view.state.state)) {
        attention += 1;
      }
      if (moved) {
        items.append(item);
      }
    }
    need.textContent = attention === 0 ? "" : String(attention);
    if (selected !== null && nodes.has(selected)) {
      items.setAttribute("aria-activedescendant", `rail-${selected}`);
    } else {
      items.removeAttribute("aria-activedescendant");
    }
    if (shown !== null && !nodes.has(shown)) {
      hideHover();
    }
  }

  function applyPending(): void {
    if (pending !== null) {
      order = pending;
      pending = null;
      paint();
    }
  }

  function cancelHover(): void {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }
  }

  function hideHover(): void {
    cancelHover();
    hoverId = null;
    quiet = true;
    if (shown !== null) {
      shown = null;
      options.onHover?.(null, null);
    }
  }

  function openHover(id: string): void {
    const item = nodes.get(id);
    if (item === undefined || !item.isConnected) {
      hideHover();
      return;
    }
    shown = id;
    options.onHover?.(id, item);
  }

  function onPointerHover(event: MouseEvent): void {
    touch();
    const moved = event.clientX !== lastX || event.clientY !== lastY;
    lastX = event.clientX;
    lastY = event.clientY;
    if (moved) {
      quiet = false;
    } else if (quiet) {
      return;
    }
    const target = event.target instanceof Element ? event.target : null;
    const id = target?.closest<HTMLElement>(".item")?.dataset.id ?? null;
    if (id === hoverId) {
      return;
    }
    cancelHover();
    hoverId = id;
    if (id === null) {
      hideHover();
    } else if (shown !== null) {
      openHover(id);
    } else {
      hoverTimer = setTimeout(() => {
        hoverTimer = null;
        if (hoverId === id) {
          openHover(id);
        }
      }, HOVER_DELAY_MS);
    }
  }

  function onPointerEnter(): void {
    hovering = true;
    touch();
  }

  function onPointerLeave(): void {
    hovering = false;
    hideHover();
    lastX = Number.NaN;
    lastY = Number.NaN;
    if (settled()) {
      applyPending();
    }
  }

  function onFocusIn(): void {
    focused = true;
    touch();
  }

  function onFocusOut(event: FocusEvent): void {
    if (event.relatedTarget instanceof Node && root.contains(event.relatedTarget)) {
      return;
    }
    focused = false;
    if (settled()) {
      applyPending();
    }
  }

  function onClick(event: Event): void {
    touch();
    hideHover();
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest(".unfold")) {
      options.onExpand();
      return;
    }
    const id = target?.closest<HTMLElement>(".item")?.dataset.id;
    if (id) {
      options.onSelect(id);
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    touch();
    hideHover();
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    const n = order.length;
    if (n === 0) {
      return;
    }
    const current = selected === null ? -1 : order.indexOf(selected);
    let next: number;
    switch (event.key) {
      case "ArrowDown":
        next = Math.min(n - 1, current + 1);
        break;
      case "ArrowUp":
        next = Math.max(0, current === -1 ? 0 : current - 1);
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = n - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    const id = order[next];
    if (id !== undefined) {
      options.onSelect(id);
      const item = nodes.get(id);
      if (item !== undefined && typeof item.scrollIntoView === "function") {
        item.scrollIntoView({ block: "nearest" });
      }
    }
  }

  root.addEventListener("pointerenter", onPointerEnter);
  root.addEventListener("pointerover", onPointerHover);
  root.addEventListener("pointermove", onPointerHover);
  root.addEventListener("pointerdown", hideHover);
  root.addEventListener("pointerleave", onPointerLeave);
  root.addEventListener("focusin", onFocusIn);
  root.addEventListener("focusout", onFocusOut);
  root.addEventListener("click", onClick);
  items.addEventListener("keydown", onKeyDown);
  items.addEventListener("scroll", hideHover);

  return {
    el: root,
    update(views, sel, isPinned) {
      selected = sel;
      pinnedOf = isPinned;
      viewById.clear();
      for (const view of views) {
        viewById.set(view.id, view);
      }
      for (const [id, item] of nodes) {
        if (!viewById.has(id)) {
          item.remove();
          nodes.delete(id);
        }
      }
      const sorted = attentionOrder(views);
      const next = [...sorted.filter((v) => isPinned(v.id)), ...sorted.filter((v) => !isPinned(v.id))].map(
        (v) => v.id,
      );
      if (sameOrder(order, next) || settled()) {
        order = next;
        pending = null;
      } else {
        order = holdOrder(order, next);
        pending = sameOrder(order, next) ? null : next;
      }
      paint();
    },
    focus() {
      items.focus();
    },
    setDetached,
    dispose() {
      hideHover();
      root.removeEventListener("pointerenter", onPointerEnter);
      root.removeEventListener("pointerover", onPointerHover);
      root.removeEventListener("pointermove", onPointerHover);
      root.removeEventListener("pointerdown", hideHover);
      root.removeEventListener("pointerleave", onPointerLeave);
      root.removeEventListener("focusin", onFocusIn);
      root.removeEventListener("focusout", onFocusOut);
      root.removeEventListener("click", onClick);
      items.removeEventListener("keydown", onKeyDown);
      items.removeEventListener("scroll", hideHover);
      root.remove();
    },
  };
}
