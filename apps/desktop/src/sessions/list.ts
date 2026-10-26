// <wtl> project: c4 | file: apps/desktop/src/sessions/list.ts | author: watchthelight </wtl>
// The session list, as a virtual list over the items arrange() returns: group headers and session
// rows, filtered, pinned sessions first, then the core's attention order. Only the items near the
// viewport exist in the DOM, eight beyond each edge, on pooled nodes placed with a transform, so a
// thousand sessions cost the frame that twenty do. Each row carries a state square and the state
// text, a `~` on guessed states, and nothing to type into. A row changes its glyph and text at
// once; a reorder from the core waits while the pointer is inside the list or the list has keyboard
// focus, and applies on leave or once both have rested for two seconds. What the user does by hand
// (filter, pin, collapse, regroup) applies at once. The pointer resting on a row for 350 ms asks the
// owner of the list for the hover card, which is drawn outside the list, beside the row; a key, a
// button, a scroll or a change closes it until the pointer moves again.

import { el } from "../dom";
import {
  attachmentLabel,
  compactTokens,
  contextBand,
  describeState,
  displayName,
  elapsed,
  hostLabel,
  modelText,
  needsAttention,
  sourceLabel,
} from "../format";
import { arrange, groupOf, nextGroupBy, parseQuery, rowKey } from "./arrange";
import type { GroupBy, HeadItem, Item, Query } from "./arrange";
import type { SessionView, StateKind } from "../bridge/types";

export type Density = "compact" | "standard" | "expanded";

export const DENSITIES: readonly Density[] = ["compact", "standard", "expanded"];

/** Row heights in CSS pixels; tokens.css carries the same numbers as --row-compact and friends. */
export const ROW_HEIGHT: Readonly<Record<Density, number>> = { compact: 22, standard: 34, expanded: 56 };

/** The group header height; --row-header in tokens.css. */
export const HEADER_HEIGHT = 24;

/** Items kept rendered beyond each edge of the viewport. */
export const OVERSCAN = 8;

/** How long the pointer or the keyboard rests inside the list before a pending reorder applies. */
export const SETTLE_MS = 2000;

/** How long the pointer rests on a row before the hover card is asked for. */
export const HOVER_DELAY_MS = 350;

/** Nodes of each kind kept for reuse once they scroll out. */
const POOL_MAX = 64;

/** The density after `d` in the cycle the keyboard chord walks. */
export function nextDensity(d: Density): Density {
  const i = DENSITIES.indexOf(d);
  return DENSITIES[(i + 1) % DENSITIES.length] ?? "standard";
}

/** Everything the user set by hand; what persistence will keep between runs. */
export interface ListPrefs {
  density: Density;
  groupBy: GroupBy;
  pinned: string[];
  /** Group ids as the headers carry them: `<groupBy>:<key>`. */
  collapsed: string[];
}

export interface SessionListOptions {
  onSelect(id: string): void;
  /** Called after every change made by hand: a pin, a collapse, the grouping, the density. */
  onPrefs?(prefs: ListPrefs): void;
  /**
   * Called with a row once the pointer has rested on it for HOVER_DELAY_MS, again at once when the
   * pointer moves to another row, and with nulls when the card should close: the pointer left the
   * rows, the list scrolled or changed by hand, a key or a button went down, or the row is gone.
   * After such a close the card waits for the pointer to move again.
   */
  onHover?(id: string | null, anchor: HTMLElement | null): void;
  density?: Density;
  groupBy?: GroupBy;
  pinned?: readonly string[];
  collapsed?: readonly string[];
}

export interface SessionList {
  el: HTMLElement;
  readonly density: Density;
  readonly groupBy: GroupBy;
  readonly filter: string;
  /** Render `views`. `selected` is the highlighted id; `now` drives the clocks. */
  update(views: readonly SessionView[], selected: string | null, now: number): void;
  /** Place the items for the current scroll position and height; runs after a scroll or a resize. */
  paint(): void;
  /** Change the row height, keeping the item at the top of the viewport where it is. */
  setDensity(density: Density): void;
  setGroupBy(groupBy: GroupBy): void;
  /**
   * Narrow the list: words match the name, the project and the cwd; `state:`, `project:`, `tool:`,
   * `mode:` and `host:` tokens match one field each. Applies at once.
   */
  setFilter(text: string): void;
  togglePin(id: string): void;
  isPinned(id: string): boolean;
  /** Collapse or expand a group by the id its header carries. Applies at once. */
  toggleGroup(group: string): void;
  prefs(): ListPrefs;
  dispose(): void;
}

interface Row {
  kind: "row";
  el: HTMLElement;
  name: HTMLElement;
  ctx: HTMLElement;
  fill: HTMLElement;
  pct: HTMLElement;
  when: HTMLElement;
  activity: HTMLElement;
  tags: HTMLElement;
  mode: HTMLElement;
  model: HTMLElement;
  cwd: HTMLElement;
  /** What the static content was last filled for; a change in any refills it. */
  seq: number;
  state: StateKind | null;
  density: Density | null;
  pinned: boolean | null;
}

interface Head {
  kind: "head";
  el: HTMLElement;
  idx: HTMLElement;
  title: HTMLElement;
  count: HTMLElement;
  need: HTMLElement;
}

type Node = Row | Head;

function makeRow(): Row {
  const root = el("div", "row");
  root.setAttribute("role", "option");
  const sq = el("span", "sq");
  sq.setAttribute("aria-hidden", "true");
  const name = el("span", "name");
  const ctx = el("span", "ctx");
  const bar = el("span", "bar");
  bar.setAttribute("aria-hidden", "true");
  const fill = el("span", "fill");
  bar.append(fill);
  const pct = el("span", "pct");
  ctx.append(bar, pct);
  const when = el("span", "when");
  const activity = el("span", "activity");
  const tags = el("span", "tags");
  const mode = el("span", "chip");
  const model = el("span", "model");
  const cwd = el("span", "cwd");
  const first = el("span", "line first");
  first.append(name, ctx, when);
  const second = el("span", "line second");
  second.append(activity, tags);
  const third = el("span", "line third");
  third.append(mode, model, cwd);
  root.append(sq, first, second, third);
  return {
    kind: "row",
    el: root,
    name,
    ctx,
    fill,
    pct,
    when,
    activity,
    tags,
    mode,
    model,
    cwd,
    seq: -1,
    state: null,
    density: null,
    pinned: null,
  };
}

/** A group header: the collapse glyph, the index, the title, a rule, the count, the need. */
function makeHead(): Head {
  const root = el("div", "group");
  root.setAttribute("role", "presentation");
  const tog = el("span", "tog");
  tog.setAttribute("aria-hidden", "true");
  const idx = el("span", "idx");
  const title = el("span", "title");
  const rule = el("span", "rule");
  rule.setAttribute("aria-hidden", "true");
  const count = el("span", "count");
  const need = el("span", "need");
  root.append(tog, idx, title, rule, count, need);
  return { kind: "head", el: root, idx, title, count, need };
}

function setText(node: HTMLElement, text: string): void {
  if (node.textContent !== text) {
    node.textContent = text;
  }
}

/** "3" when nobody needs you, "3, 1 needs you" when some do, "2 need you" when all do. */
function fillHead(head: Head, item: HeadItem): void {
  setText(head.idx, String(item.index).padStart(2, "0"));
  setText(head.title, item.title);
  const some = item.attention > 0 && item.attention < item.count;
  setText(head.count, item.attention === 0 ? String(item.count) : some ? `${item.count},` : "");
  setText(head.need, item.attention > 0 ? `${item.attention} need${item.attention === 1 ? "s" : ""} you` : "");
  head.el.dataset.group = item.group;
  head.el.dataset.collapsed = String(item.collapsed);
  head.el.title = `${item.collapsed ? "expand" : "collapse"} ${item.title}`;
}

/** The context figure from the status line, its band, a 24px bar and a tooltip naming the model. */
function fillContext(row: Row, v: SessionView): void {
  if (v.context_pct === null) {
    row.ctx.hidden = true;
    row.pct.textContent = "";
    row.fill.style.width = "0";
    row.ctx.title = "";
    delete row.ctx.dataset.band;
    return;
  }
  row.ctx.hidden = false;
  row.pct.textContent = `${v.context_pct}%`;
  row.fill.style.width = `${Math.max(0, Math.min(100, v.context_pct))}%`;
  row.ctx.dataset.band = contextBand(v.context_pct);
  const size = v.context_size === null ? "" : ` of ${compactTokens(v.context_size)}`;
  row.ctx.title = `context ${v.context_pct}%${size}${v.model ? `, ${v.model}` : ""}`;
}

function tagsFor(v: SessionView): string[] {
  const tags: string[] = [];
  if (v.session_kind === "background") {
    tags.push("background");
  }
  tags.push(hostLabel(v.host), attachmentLabel(v.attachment));
  if (v.process) {
    tags.push(`pid ${v.process.pid}`);
  }
  if (v.coarse) {
    tags.push("coarse");
  }
  return tags;
}

function fill(row: Row, v: SessionView, selected: boolean, now: number, density: Density, pinned: boolean): void {
  const state = v.state.state;
  if (row.seq !== v.seq || row.state !== state || row.density !== density || row.pinned !== pinned) {
    row.seq = v.seq;
    row.state = state;
    row.density = density;
    row.pinned = pinned;
    row.el.dataset.kind = state;
    row.el.dataset.attention = String(needsAttention(state));
    row.el.dataset.certainty = v.evidence.certainty;
    row.el.dataset.alive = String(v.alive);
    row.el.dataset.pinned = String(pinned);
    const name = displayName(v);
    const guessed = v.evidence.certainty === "heuristic";
    const text = describeState(v.state);
    const shown = guessed ? `~${text}` : text;
    row.name.textContent = name;
    row.activity.textContent = shown;
    row.activity.title = `${guessed ? "guessed" : "observed"} from ${sourceLabel(v.evidence.source)}`;
    row.tags.textContent = tagsFor(v).join("  ");
    row.mode.textContent = attachmentLabel(v.attachment);
    row.model.textContent = modelText(v);
    row.cwd.textContent = v.cwd ?? "";
    row.el.setAttribute("aria-label", `${name}, ${text}${pinned ? ", pinned" : ""}`);
    // the compact row has no room for the state text, so the tooltip carries it
    if (density === "compact") {
      row.el.title = shown;
    } else {
      row.el.removeAttribute("title");
    }
    fillContext(row, v);
  }
  row.when.textContent = elapsed(now - v.state_since);
  row.el.setAttribute("aria-selected", String(selected));
  row.el.classList.toggle("selected", selected);
}

function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((key, i) => key === b[i]);
}

/**
 * The order to show while a reorder is held back: items that are gone leave now, and a new row
 * joins after the last row of its group when its header is present, else at the end.
 */
function holdOrder(held: readonly string[], next: readonly string[], byKey: ReadonlyMap<string, Item>): string[] {
  const out = held.filter((key) => byKey.has(key));
  const known = new Set(out);
  for (const key of next) {
    if (known.has(key)) {
      continue;
    }
    known.add(key);
    const item = byKey.get(key);
    const head = item?.kind === "row" ? item.head : null;
    const at = head === null ? -1 : out.indexOf(head);
    if (at === -1) {
      out.push(key);
      continue;
    }
    let end = at + 1;
    while (end < out.length) {
      const member = byKey.get(out[end] ?? "");
      if (member?.kind === "row" && member.head === head) {
        end += 1;
      } else {
        break;
      }
    }
    out.splice(end, 0, key);
  }
  return out;
}

export function createSessionList(options: SessionListOptions): SessionList {
  const root = el("div", "sessions");
  root.setAttribute("role", "listbox");
  root.setAttribute("aria-label", "sessions");
  root.tabIndex = 0;
  const spacer = el("div", "spacer");
  const empty = el("p", "empty");
  root.append(spacer, empty);

  let density: Density = options.density ?? "standard";
  root.dataset.density = density;
  let groupBy: GroupBy = options.groupBy ?? "none";
  root.dataset.group = groupBy;
  let filterText = "";
  let query: Query = parseQuery("");
  const pinned = new Set<string>(options.pinned ?? []);
  const collapsed = new Set<string>(options.collapsed ?? []);

  let views: readonly SessionView[] = [];
  const viewById = new Map<string, SessionView>();
  const byKey = new Map<string, Item>();
  const live = new Map<string, Node>();
  const rowPool: Row[] = [];
  const headPool: Head[] = [];
  /** The item keys in the order they are laid out. */
  let order: string[] = [];
  /** The order arrange() wants, held back while the pointer or the keyboard is in the list. */
  let pending: string[] | null = null;
  /** offsets[i] is the top of item i; offsets[order.length] is the height of the list. */
  let offsets: number[] = [0];
  /** ordinal[i] is how many rows come before item i; ordinal[order.length] is the row count. */
  let ordinal: number[] = [0];
  let selected: string | null = null;
  let now = 0;
  let hovering = false;
  let focused = false;
  let lastInput = 0;
  let scheduled = false;
  /** The row under the pointer, the wait for the card, and the row the card shows. */
  let hoverId: string | null = null;
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let shown: string | null = null;
  /**
   * Where the pointer was at its last event, and whether the card waits for it to move again: the
   * browser sends pointer events for a resting pointer whenever the rows change under it, and a
   * card closed by a key, a button, a scroll or a change must not come back on one of those.
   */
  let lastX = Number.NaN;
  let lastY = Number.NaN;
  let quiet = false;

  function heightOf(item: Item | undefined): number {
    return item?.kind === "head" ? HEADER_HEIGHT : ROW_HEIGHT[density];
  }

  function layout(): void {
    const n = order.length;
    offsets = new Array<number>(n + 1);
    ordinal = new Array<number>(n + 1);
    let y = 0;
    let rows = 0;
    for (let i = 0; i < n; i++) {
      offsets[i] = y;
      ordinal[i] = rows;
      const item = byKey.get(order[i] ?? "");
      y += heightOf(item);
      if (item?.kind === "row") {
        rows += 1;
      }
    }
    offsets[n] = y;
    ordinal[n] = rows;
  }

  /** The index of the item at `y`, clamped to the list. */
  function indexAt(y: number): number {
    let lo = 0;
    let hi = order.length - 1;
    if (hi < 0) {
      return 0;
    }
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if ((offsets[mid] ?? 0) <= y) {
        lo = mid;
      } else {
        hi = mid - 1;
      }
    }
    return lo;
  }

  function release(key: string, node: Node): void {
    live.delete(key);
    node.el.remove();
    if (node.kind === "row") {
      node.seq = -1;
      node.state = null;
      if (rowPool.length < POOL_MAX) {
        rowPool.push(node);
      }
    } else if (headPool.length < POOL_MAX) {
      headPool.push(node);
    }
  }

  function paint(): void {
    const n = order.length;
    spacer.style.height = `${offsets[n] ?? 0}px`;
    const height = root.clientHeight;
    const top = root.scrollTop;
    let first = 0;
    let last = 0;
    if (n > 0 && height > 0) {
      first = Math.max(0, indexAt(top) - OVERSCAN);
      last = Math.min(n, indexAt(top + height - 1) + 1 + OVERSCAN);
    }
    // items that left the range go back to the pools first, so the items entering it reuse nodes
    const keep = new Set(order.slice(first, last));
    for (const [key, node] of live) {
      if (!keep.has(key)) {
        release(key, node);
      }
    }
    const total = ordinal[n] ?? 0;
    for (let i = first; i < last; i++) {
      const key = order[i];
      const item = key === undefined ? undefined : byKey.get(key);
      if (key === undefined || item === undefined) {
        continue;
      }
      const node = live.get(key);
      if (item.kind === "row") {
        let row = node?.kind === "row" ? node : undefined;
        if (!row) {
          row = rowPool.pop() ?? makeRow();
          row.el.id = `session-${item.id}`;
          row.el.dataset.id = item.id;
          live.set(key, row);
          spacer.append(row.el);
        }
        row.el.style.transform = `translateY(${offsets[i] ?? 0}px)`;
        row.el.setAttribute("aria-posinset", String((ordinal[i] ?? 0) + 1));
        row.el.setAttribute("aria-setsize", String(total));
        fill(row, item.view, selected === item.id, now, density, item.pinned);
      } else {
        let head = node?.kind === "head" ? node : undefined;
        if (!head) {
          head = headPool.pop() ?? makeHead();
          live.set(key, head);
          spacer.append(head.el);
        }
        head.el.style.transform = `translateY(${offsets[i] ?? 0}px)`;
        fillHead(head, item);
      }
    }
    if (selected !== null && live.has(rowKey(selected))) {
      root.setAttribute("aria-activedescendant", `session-${selected}`);
    } else {
      root.removeAttribute("aria-activedescendant");
    }
    if (shown !== null && !live.has(rowKey(shown))) {
      hideHover();
    }
  }

  function schedule(): void {
    if (scheduled) {
      return;
    }
    scheduled = true;
    const run = (): void => {
      scheduled = false;
      paint();
    };
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(run);
    } else {
      setTimeout(run, 0);
    }
  }

  /** True when a reorder may apply: nothing is in the list, or it has rested long enough. */
  function settled(): boolean {
    return (!hovering && !focused) || Date.now() - lastInput >= SETTLE_MS;
  }

  /** Arrange the views again. A change made by hand applies at once; one from the core may wait. */
  function rebuild(immediate: boolean): void {
    if (immediate) {
      hideHover();
    }
    const arranged = arrange(views, { groupBy, query, pinned, collapsed });
    byKey.clear();
    for (const item of arranged.items) {
      byKey.set(item.key, item);
    }
    const next = arranged.items.map((item) => item.key);
    if (immediate || sameOrder(order, next) || settled()) {
      order = next;
      pending = null;
    } else {
      order = holdOrder(order, next, byKey);
      pending = sameOrder(order, next) ? null : next;
    }
    layout();
    if (views.length === 0) {
      empty.textContent = "No Claude Code sessions found.";
      empty.hidden = false;
    } else if (arranged.matched === 0) {
      empty.textContent = "No sessions match the filter.";
      empty.hidden = false;
    } else {
      empty.hidden = true;
    }
    paint();
  }

  function applyPending(): void {
    if (pending !== null) {
      order = pending;
      pending = null;
      layout();
      paint();
    }
  }

  /** Scroll so that item `index` is inside the viewport. */
  function reveal(index: number): void {
    const height = root.clientHeight;
    if (height <= 0) {
      return;
    }
    const y0 = offsets[index] ?? 0;
    const y1 = offsets[index + 1] ?? y0;
    if (y0 < root.scrollTop) {
      root.scrollTop = y0;
    } else if (y1 > root.scrollTop + height) {
      root.scrollTop = y1 - height;
    }
  }

  function prefs(): ListPrefs {
    return { density, groupBy, pinned: [...pinned], collapsed: [...collapsed] };
  }

  function notify(): void {
    options.onPrefs?.(prefs());
  }

  function setGroupBy(g: GroupBy): void {
    if (g === groupBy) {
      return;
    }
    groupBy = g;
    root.dataset.group = g;
    rebuild(true);
    notify();
  }

  function togglePin(id: string): void {
    if (pinned.has(id)) {
      pinned.delete(id);
    } else {
      pinned.add(id);
    }
    rebuild(true);
    notify();
  }

  function toggleGroup(group: string): void {
    if (collapsed.has(group)) {
      collapsed.delete(group);
    } else {
      collapsed.add(group);
    }
    rebuild(true);
    notify();
  }

  function touch(): void {
    lastInput = Date.now();
  }

  function cancelHover(): void {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }
  }

  /** Close the card and forget the row under the pointer; the next real movement arms the wait again. */
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
    const node = live.get(rowKey(id));
    if (node?.kind !== "row") {
      hideHover();
      return;
    }
    shown = id;
    options.onHover?.(id, node.el);
  }

  /** The pointer is over `event.target`: arm the wait for a row, or move an open card to it now. */
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
    const id = target?.closest<HTMLElement>(".row")?.dataset.id ?? null;
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

  function onScroll(): void {
    hideHover();
    schedule();
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
    const head = target?.closest<HTMLElement>(".group");
    if (head?.dataset.group !== undefined) {
      toggleGroup(head.dataset.group);
      return;
    }
    const id = target?.closest<HTMLElement>(".row")?.dataset.id;
    if (id) {
      options.onSelect(id);
    }
  }

  /** The item indexes that are rows, in order. */
  function rowIndexes(): number[] {
    const out: number[] = [];
    for (let i = 0; i < order.length; i++) {
      if (byKey.get(order[i] ?? "")?.kind === "row") {
        out.push(i);
      }
    }
    return out;
  }

  /** Left collapses the selected session's group, Right expands it. */
  function fold(collapse: boolean): boolean {
    const view = selected === null ? undefined : viewById.get(selected);
    const group = view === undefined ? null : groupOf(groupBy, view);
    if (group === null || collapsed.has(group) === collapse) {
      return false;
    }
    toggleGroup(group);
    return true;
  }

  function onKeyDown(event: KeyboardEvent): void {
    touch();
    hideHover();
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    switch (event.key) {
      case "p":
        if (selected !== null) {
          event.preventDefault();
          togglePin(selected);
        }
        return;
      case "g":
        event.preventDefault();
        setGroupBy(nextGroupBy(groupBy));
        return;
      case "ArrowLeft":
      case "ArrowRight":
        if (fold(event.key === "ArrowLeft")) {
          event.preventDefault();
        }
        return;
      default:
        break;
    }
    const rows = rowIndexes();
    const n = rows.length;
    if (n === 0) {
      return;
    }
    const current = selected === null ? -1 : rows.indexOf(order.indexOf(rowKey(selected)));
    const from = current === -1 ? 0 : current;
    const page = Math.max(1, Math.floor(root.clientHeight / ROW_HEIGHT[density]));
    let next: number;
    switch (event.key) {
      case "ArrowDown":
        next = Math.min(n - 1, current + 1);
        break;
      case "ArrowUp":
        next = Math.max(0, current === -1 ? 0 : current - 1);
        break;
      case "PageDown":
        next = Math.min(n - 1, from + page);
        break;
      case "PageUp":
        next = Math.max(0, from - page);
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
    const index = rows[next];
    const item = index === undefined ? undefined : byKey.get(order[index] ?? "");
    if (index !== undefined && item?.kind === "row") {
      options.onSelect(item.id);
      reveal(index);
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
  root.addEventListener("keydown", onKeyDown);
  root.addEventListener("scroll", onScroll);
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
  observer?.observe(root);

  return {
    el: root,
    get density() {
      return density;
    },
    get groupBy() {
      return groupBy;
    },
    get filter() {
      return filterText;
    },
    update(v, sel, at) {
      views = v;
      selected = sel;
      now = at;
      viewById.clear();
      for (const view of views) {
        viewById.set(view.id, view);
      }
      rebuild(false);
    },
    paint,
    setDensity(d) {
      if (d === density) {
        return;
      }
      hideHover();
      const anchor = indexAt(root.scrollTop);
      density = d;
      root.dataset.density = d;
      layout();
      root.scrollTop = offsets[anchor] ?? 0;
      paint();
      notify();
    },
    setGroupBy,
    setFilter(text) {
      if (text === filterText) {
        return;
      }
      filterText = text;
      query = parseQuery(text);
      rebuild(true);
    },
    togglePin,
    isPinned: (id) => pinned.has(id),
    toggleGroup,
    prefs,
    dispose() {
      cancelHover();
      root.removeEventListener("pointerenter", onPointerEnter);
      root.removeEventListener("pointerover", onPointerHover);
      root.removeEventListener("pointermove", onPointerHover);
      root.removeEventListener("pointerdown", hideHover);
      root.removeEventListener("pointerleave", onPointerLeave);
      root.removeEventListener("focusin", onFocusIn);
      root.removeEventListener("focusout", onFocusOut);
      root.removeEventListener("click", onClick);
      root.removeEventListener("keydown", onKeyDown);
      root.removeEventListener("scroll", onScroll);
      observer?.disconnect();
      live.clear();
      rowPool.length = 0;
      headPool.length = 0;
      byKey.clear();
      viewById.clear();
      root.replaceChildren();
    },
  };
}
