// <wtl> project: c4 | file: apps/desktop/src/sessions/list.ts | author: watchthelight </wtl>
// The session list, as a virtual list over the sessions in the order the core gives (attention
// first). Only the rows near the viewport exist in the DOM, eight beyond each edge, on pooled nodes
// placed with a transform, so a thousand sessions cost the frame that twenty do. Each row carries a
// state square and the state text, a `~` on guessed states, and nothing to type into. A row changes
// its glyph and text at once; a reorder waits while the pointer is inside the list or the list has
// keyboard focus, and applies on leave or once both have rested for two seconds.

import { el } from "../dom";
import {
  attachmentLabel,
  compactTokens,
  contextBand,
  describeState,
  displayName,
  elapsed,
  hostLabel,
  needsAttention,
  sourceLabel,
} from "../format";
import type { SessionView, StateKind } from "../bridge/types";

export type Density = "compact" | "standard" | "expanded";

export const DENSITIES: readonly Density[] = ["compact", "standard", "expanded"];

/** Row heights in CSS pixels; tokens.css carries the same numbers as --row-compact and friends. */
export const ROW_HEIGHT: Readonly<Record<Density, number>> = { compact: 22, standard: 34, expanded: 56 };

/** Rows kept rendered beyond each edge of the viewport. */
export const OVERSCAN = 8;

/** How long the pointer or the keyboard rests inside the list before a pending reorder applies. */
export const SETTLE_MS = 2000;

/** Row nodes kept for reuse once they scroll out. */
const POOL_MAX = 64;

/** The density after `d` in the cycle the keyboard chord walks. */
export function nextDensity(d: Density): Density {
  const i = DENSITIES.indexOf(d);
  return DENSITIES[(i + 1) % DENSITIES.length] ?? "standard";
}

export interface SessionListOptions {
  onSelect(id: string): void;
  density?: Density;
}

export interface SessionList {
  el: HTMLElement;
  readonly density: Density;
  /** Render `views` in the given order. `selected` is the highlighted id; `now` drives the clocks. */
  update(views: readonly SessionView[], selected: string | null, now: number): void;
  /** Place the rows for the current scroll position and height; runs after a scroll or a resize. */
  paint(): void;
  /** Change the row height, keeping the row at the top of the viewport where it is. */
  setDensity(density: Density): void;
  dispose(): void;
}

interface Row {
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
  id: string | null;
  seq: number;
  kind: StateKind | null;
  density: Density | null;
}

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
    id: null,
    seq: -1,
    kind: null,
    density: null,
  };
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

/** The third line's model text: what the last status line payload said, or that none came. */
function modelText(v: SessionView): string {
  const parts: string[] = [];
  if (v.model) {
    parts.push(v.model);
  }
  if (v.effort) {
    parts.push(`effort ${v.effort}`);
  }
  return parts.length === 0 ? "no status line yet" : parts.join(", ");
}

function fill(row: Row, v: SessionView, selected: boolean, now: number, density: Density): void {
  const kind = v.state.state;
  if (row.seq !== v.seq || row.kind !== kind || row.density !== density) {
    row.seq = v.seq;
    row.kind = kind;
    row.density = density;
    row.el.dataset.kind = kind;
    row.el.dataset.attention = String(needsAttention(kind));
    row.el.dataset.certainty = v.evidence.certainty;
    row.el.dataset.alive = String(v.alive);
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
    row.el.setAttribute("aria-label", `${name}, ${text}`);
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
  return a.length === b.length && a.every((id, i) => id === b[i]);
}

export function createSessionList(options: SessionListOptions): SessionList {
  const root = el("div", "sessions");
  root.setAttribute("role", "listbox");
  root.setAttribute("aria-label", "sessions");
  root.tabIndex = 0;
  const spacer = el("div", "spacer");
  const empty = el("p", "empty", "No Claude Code sessions found.");
  root.append(spacer, empty);

  let density: Density = options.density ?? "standard";
  root.dataset.density = density;

  const byId = new Map<string, SessionView>();
  const live = new Map<string, Row>();
  const pool: Row[] = [];
  /** The ids in the order the rows are laid out. */
  let order: string[] = [];
  /** The order the core wants, held back while the pointer or the keyboard is in the list. */
  let pending: string[] | null = null;
  /** offsets[i] is the top of row i; offsets[order.length] is the height of the list. */
  let offsets: number[] = [0];
  let selected: string | null = null;
  let now = 0;
  let hovering = false;
  let focused = false;
  let lastInput = 0;
  let scheduled = false;

  function layout(): void {
    const h = ROW_HEIGHT[density];
    offsets = new Array<number>(order.length + 1);
    for (let i = 0; i <= order.length; i++) {
      offsets[i] = i * h;
    }
  }

  /** The index of the row at `y`, clamped to the list. */
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

  function release(id: string, row: Row): void {
    live.delete(id);
    row.el.remove();
    row.id = null;
    row.seq = -1;
    row.kind = null;
    if (pool.length < POOL_MAX) {
      pool.push(row);
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
    // rows that left the range go back to the pool first, so the rows entering it reuse their nodes
    const keep = new Set(order.slice(first, last));
    for (const [id, row] of live) {
      if (!keep.has(id)) {
        release(id, row);
      }
    }
    for (let i = first; i < last; i++) {
      const id = order[i];
      const view = id === undefined ? undefined : byId.get(id);
      if (id === undefined || view === undefined) {
        continue;
      }
      let row = live.get(id);
      if (!row) {
        row = pool.pop() ?? makeRow();
        row.id = id;
        row.el.id = `session-${id}`;
        row.el.dataset.id = id;
        live.set(id, row);
        spacer.append(row.el);
      }
      row.el.style.transform = `translateY(${offsets[i] ?? 0}px)`;
      row.el.setAttribute("aria-posinset", String(i + 1));
      row.el.setAttribute("aria-setsize", String(n));
      fill(row, view, selected === id, now, density);
    }
    if (selected !== null && live.has(selected)) {
      root.setAttribute("aria-activedescendant", `session-${selected}`);
    } else {
      root.removeAttribute("aria-activedescendant");
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

  function applyPending(): void {
    if (pending !== null) {
      order = pending;
      pending = null;
      layout();
      paint();
    }
  }

  /** Scroll so that row `index` is inside the viewport. */
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

  function touch(): void {
    lastInput = Date.now();
  }

  function onPointerEnter(): void {
    hovering = true;
    touch();
  }

  function onPointerLeave(): void {
    hovering = false;
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
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>(".row") : null;
    const id = target?.dataset.id;
    if (id) {
      options.onSelect(id);
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    touch();
    const n = order.length;
    if (n === 0) {
      return;
    }
    const current = selected === null ? -1 : order.indexOf(selected);
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
    const id = order[next];
    if (id !== undefined) {
      options.onSelect(id);
      reveal(next);
    }
  }

  root.addEventListener("pointerenter", onPointerEnter);
  root.addEventListener("pointermove", touch);
  root.addEventListener("pointerleave", onPointerLeave);
  root.addEventListener("focusin", onFocusIn);
  root.addEventListener("focusout", onFocusOut);
  root.addEventListener("click", onClick);
  root.addEventListener("keydown", onKeyDown);
  root.addEventListener("scroll", schedule);
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
  observer?.observe(root);

  return {
    el: root,
    get density() {
      return density;
    },
    update(views, sel, at) {
      selected = sel;
      now = at;
      byId.clear();
      for (const v of views) {
        byId.set(v.id, v);
      }
      const next = views.map((v) => v.id);
      if (sameOrder(order, next)) {
        pending = null;
      } else if (settled()) {
        order = next;
        pending = null;
      } else {
        // rows that are gone leave now; new ones join at the end until the reorder applies
        order = order.filter((id) => byId.has(id));
        const known = new Set(order);
        for (const id of next) {
          if (!known.has(id)) {
            order.push(id);
          }
        }
        pending = sameOrder(order, next) ? null : next;
      }
      layout();
      empty.hidden = views.length > 0;
      paint();
    },
    paint,
    setDensity(d) {
      if (d === density) {
        return;
      }
      const anchor = indexAt(root.scrollTop);
      density = d;
      root.dataset.density = d;
      layout();
      root.scrollTop = offsets[anchor] ?? 0;
      paint();
    },
    dispose() {
      root.removeEventListener("pointerenter", onPointerEnter);
      root.removeEventListener("pointermove", touch);
      root.removeEventListener("pointerleave", onPointerLeave);
      root.removeEventListener("focusin", onFocusIn);
      root.removeEventListener("focusout", onFocusOut);
      root.removeEventListener("click", onClick);
      root.removeEventListener("keydown", onKeyDown);
      root.removeEventListener("scroll", schedule);
      observer?.disconnect();
      live.clear();
      pool.length = 0;
      byId.clear();
      root.replaceChildren();
    },
  };
}
