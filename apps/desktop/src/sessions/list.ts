// <wtl> project: c4 | file: apps/desktop/src/sessions/list.ts | author: watchthelight </wtl>
// The session list: one row per session in the order the core gives (attention first), a state
// square and the state text, a `~` on guessed states, and nothing to type into. Rows are reused
// across updates; reordering waits while the pointer is inside the list so a row cannot move out
// from under a click.

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

export interface SessionListOptions {
  onSelect(id: string): void;
}

export interface SessionList {
  el: HTMLElement;
  /** Render `views` in the given order. `selected` is the highlighted id; `now` drives the clocks. */
  update(views: readonly SessionView[], selected: string | null, now: number): void;
  dispose(): void;
}

interface Row {
  el: HTMLElement;
  name: HTMLElement;
  ctx: HTMLElement;
  when: HTMLElement;
  activity: HTMLElement;
  tags: HTMLElement;
  seq: number;
  kind: StateKind | null;
}

function makeRow(id: string): Row {
  const root = el("div", "row");
  root.setAttribute("role", "option");
  root.dataset.id = id;
  const sq = el("span", "sq");
  sq.setAttribute("aria-hidden", "true");
  const name = el("span", "name");
  const ctx = el("span", "ctx");
  const when = el("span", "when");
  const activity = el("span", "activity");
  const tags = el("span", "tags");
  const first = el("span", "line");
  first.append(name, ctx, when);
  const second = el("span", "line");
  second.append(activity, tags);
  root.append(sq, first, second);
  return { el: root, name, ctx, when, activity, tags, seq: -1, kind: null };
}

/** The context figure from the status line, with its band and a tooltip naming the model. */
function fillContext(row: Row, v: SessionView): void {
  if (v.context_pct === null) {
    row.ctx.textContent = "";
    row.ctx.title = "";
    delete row.ctx.dataset.band;
    return;
  }
  row.ctx.textContent = `${v.context_pct}%`;
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

function fill(row: Row, v: SessionView, selected: boolean, now: number): void {
  const kind = v.state.state;
  if (row.seq !== v.seq || row.kind !== kind) {
    row.seq = v.seq;
    row.kind = kind;
    row.el.dataset.kind = kind;
    row.el.dataset.attention = String(needsAttention(kind));
    row.el.dataset.certainty = v.evidence.certainty;
    row.el.dataset.alive = String(v.alive);
    const name = displayName(v);
    const guessed = v.evidence.certainty === "heuristic";
    const text = describeState(v.state);
    row.name.textContent = name;
    row.activity.textContent = guessed ? `~${text}` : text;
    row.activity.title = `${guessed ? "guessed" : "observed"} from ${sourceLabel(v.evidence.source)}`;
    row.tags.textContent = tagsFor(v).join("  ");
    row.el.setAttribute("aria-label", `${name}, ${text}`);
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
  const empty = el("p", "empty", "No Claude Code sessions found.");
  root.append(empty);

  const rows = new Map<string, Row>();
  let order: string[] = [];
  let latest: readonly SessionView[] = [];
  let hovering = false;
  let orderPending = false;

  function applyOrder(): void {
    order = latest.map((v) => v.id);
    for (const id of order) {
      const row = rows.get(id);
      if (row) {
        root.append(row.el);
      }
    }
    orderPending = false;
  }

  function onPointerEnter(): void {
    hovering = true;
  }

  function onPointerLeave(): void {
    hovering = false;
    if (orderPending) {
      applyOrder();
    }
  }

  function onClick(event: Event): void {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>(".row") : null;
    const id = target?.dataset.id;
    if (id) {
      options.onSelect(id);
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (order.length === 0) {
      return;
    }
    const current = order.findIndex((id) => rows.get(id)?.el.getAttribute("aria-selected") === "true");
    let next: number;
    switch (event.key) {
      case "ArrowDown":
        next = Math.min(order.length - 1, current + 1);
        break;
      case "ArrowUp":
        next = Math.max(0, current === -1 ? 0 : current - 1);
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = order.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    const id = order[next];
    if (id !== undefined) {
      options.onSelect(id);
    }
  }

  root.addEventListener("pointerenter", onPointerEnter);
  root.addEventListener("pointerleave", onPointerLeave);
  root.addEventListener("click", onClick);
  root.addEventListener("keydown", onKeyDown);

  return {
    el: root,
    update(views, selected, now) {
      latest = views;
      const seen = new Set<string>();
      for (const v of views) {
        seen.add(v.id);
        let row = rows.get(v.id);
        if (!row) {
          row = makeRow(v.id);
          rows.set(v.id, row);
          root.append(row.el);
        }
        fill(row, v, selected === v.id, now);
      }
      for (const [id, row] of rows) {
        if (!seen.has(id)) {
          row.el.remove();
          rows.delete(id);
        }
      }
      const next = views.map((v) => v.id);
      if (!sameOrder(order, next)) {
        if (hovering) {
          orderPending = true;
        } else {
          applyOrder();
        }
      }
      empty.hidden = views.length > 0;
    },
    dispose() {
      root.removeEventListener("pointerenter", onPointerEnter);
      root.removeEventListener("pointerleave", onPointerLeave);
      root.removeEventListener("click", onClick);
      root.removeEventListener("keydown", onKeyDown);
      rows.clear();
      root.replaceChildren();
    },
  };
}
