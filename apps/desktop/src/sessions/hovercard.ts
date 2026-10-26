// <wtl> project: c4 | file: apps/desktop/src/sessions/hovercard.ts | author: watchthelight </wtl>
// The card that opens beside a session row once the pointer has rested on it: the attachment and
// why, the state with its evidence, the model and the context figures, the open tool, the agents,
// the cwd and the clocks, all from the same view the row and the inspector read. It takes no
// pointer events and holds nothing to type into; the keyboard has the inspector instead. The list
// decides when it opens and closes (sessions/list.ts); this module only draws and places it.

import { el } from "../dom";
import {
  attachmentLabel,
  contextBand,
  describeState,
  disconnectReasonLabel,
  displayName,
  elapsed,
  hostLabel,
  modelText,
  sourceLabel,
} from "../format";
import { contextText, telemetry } from "./inspector";
import type { AgentInfo, Certainty, SessionView, StateKind } from "../bridge/types";

/** The gap between the row and the card, and between the card and the window's edge. */
export const CARD_GAP = 8;

/** The most agents the card names; the count above the list covers the rest. */
export const CARD_AGENTS = 3;

export interface CardContext {
  pct: number;
  band: "ok" | "warn" | "hot";
}

export interface CardModel {
  name: string;
  /** The attachment, for the chip. */
  mode: string;
  /** Why the session is in that mode: the host and pane, or what ended it. */
  reason: string;
  kind: StateKind;
  certainty: Certainty;
  /** The state text with the time in it, `~` in front of a guess. */
  state: string;
  /** "observed from hook Notification", "guessed from timer". */
  source: string;
  /** The fact rows, in display order. */
  facts: [string, string][];
  /** The last agents started, newest last, one line each. */
  agents: string[];
  /** The context figure for the bar; null until a status line payload named it. */
  context: CardContext | null;
}

/** Why the session is attached the way it is, in a few words. */
export function modeReason(v: SessionView): string {
  switch (v.attachment) {
    case "managed":
      return "a pane C4 owns";
    case "interactive_attached":
      return v.pane ? `${hostLabel(v.host)} pane ${v.pane}` : hostLabel(v.host);
    case "observed":
      return `${hostLabel(v.host)}, no input from C4`;
    case "disconnected":
      return v.state.state === "disconnected" ? disconnectReasonLabel(v.state.reason) : "no live process";
  }
}

function agentLine(a: AgentInfo): string {
  const status = a.ended_at !== null ? "done" : a.observed_running ? "running" : "seen";
  return `${a.agent_type ?? "agent"}: ${a.description ?? a.id}, ${status}`;
}

/** What the card says about `v` at `now`. */
export function cardModel(v: SessionView, now: number): CardModel {
  const guessed = v.evidence.certainty === "heuristic";
  const running = v.agents.filter((a) => a.observed_running && a.ended_at === null).length;
  const open = v.background_tasks.filter((t) => t.completed_at === null).length;
  const facts: [string, string][] = [
    ["model", modelText(v)],
    ["context", contextText(v)],
  ];
  if (v.current_tool) {
    facts.push(["tool", `${v.current_tool.name} for ${elapsed(now - v.current_tool.since)}`]);
  }
  facts.push(["agents", v.agents.length === 0 ? "none seen" : `${v.agents.length} seen, ${running} running`]);
  if (v.background_tasks.length > 0) {
    facts.push(["tasks", `${open} open of ${v.background_tasks.length}`]);
  }
  facts.push(
    ["cwd", v.cwd ?? "unknown"],
    ["started", v.started_at === null ? "unknown" : `${elapsed(now - v.started_at)} ago`],
    ["last activity", `${elapsed(now - v.last_activity)} ago`],
    ["status line", v.statusline_at === null ? "none received" : `${elapsed(now - v.statusline_at)} ago`],
    ["telemetry", telemetry(v)],
  );
  return {
    name: displayName(v),
    mode: attachmentLabel(v.attachment),
    reason: modeReason(v),
    kind: v.state.state,
    certainty: v.evidence.certainty,
    state: `${guessed ? "~" : ""}${describeState(v.state)} for ${elapsed(now - v.state_since)}`,
    source: `${guessed ? "guessed" : "observed"} from ${sourceLabel(v.evidence.source)}`,
    facts,
    agents: v.agents.slice(-CARD_AGENTS).map(agentLine),
    context: v.context_pct === null ? null : { pct: v.context_pct, band: contextBand(v.context_pct) },
  };
}

export interface HoverCard {
  el: HTMLElement;
  /** The id the card shows, or null while it is hidden. */
  readonly id: string | null;
  /** Open the card for `view` beside `anchor`, its row. */
  show(view: SessionView, anchor: Element, now: number): void;
  /**
   * Redraw from the current views and clock. A view that is gone leaves the card as it is: the
   * list, which knows its rows, closes it.
   */
  refresh(views: readonly SessionView[], now: number): void;
  hide(): void;
  dispose(): void;
}

export function createHoverCard(): HoverCard {
  const root = el("div", "hovercard");
  root.setAttribute("role", "tooltip");
  root.hidden = true;
  const head = el("div", "head");
  const sq = el("span", "sq");
  sq.setAttribute("aria-hidden", "true");
  const name = el("span", "name");
  const chip = el("span", "chip");
  head.append(sq, name, chip);
  const reason = el("p", "reason");
  const state = el("p", "state");
  const source = el("p", "source");
  const facts = el("dl", "facts");
  const agents = el("ul", "agents");
  root.append(head, reason, state, source, facts, agents);

  let id: string | null = null;
  let anchor: Element | null = null;

  function render(v: SessionView, now: number): void {
    const m = cardModel(v, now);
    root.dataset.kind = m.kind;
    root.dataset.certainty = m.certainty;
    name.textContent = m.name;
    chip.textContent = m.mode;
    reason.textContent = m.reason;
    state.textContent = m.state;
    source.textContent = m.source;
    facts.replaceChildren();
    for (const [key, value] of m.facts) {
      const dd = el("dd", undefined, value);
      if (key === "context" && m.context !== null) {
        const bar = el("span", "bar");
        bar.setAttribute("aria-hidden", "true");
        const fill = el("span", "fill");
        fill.style.width = `${Math.max(0, Math.min(100, m.context.pct))}%`;
        bar.append(fill);
        dd.prepend(bar);
        dd.dataset.band = m.context.band;
      }
      facts.append(el("dt", undefined, key), dd);
    }
    agents.replaceChildren(...m.agents.map((line) => el("li", undefined, line)));
    agents.hidden = m.agents.length === 0;
  }

  /** Beside the row, to its right when there is room, else to its left; never past the window. */
  function place(): void {
    if (anchor === null) {
      return;
    }
    const r = anchor.getBoundingClientRect();
    const w = root.offsetWidth;
    const h = root.offsetHeight;
    let left = r.right + CARD_GAP;
    if (left + w > window.innerWidth - CARD_GAP) {
      left = Math.max(CARD_GAP, r.left - CARD_GAP - w);
    }
    let top = r.top;
    if (top + h > window.innerHeight - CARD_GAP) {
      top = Math.max(CARD_GAP, window.innerHeight - CARD_GAP - h);
    }
    root.style.left = `${Math.round(left)}px`;
    root.style.top = `${Math.round(top)}px`;
  }

  return {
    el: root,
    get id() {
      return id;
    },
    show(view, at, now) {
      id = view.id;
      anchor = at;
      render(view, now);
      root.hidden = false;
      place();
    },
    refresh(views, now) {
      if (id === null) {
        return;
      }
      const view = views.find((v) => v.id === id);
      if (view) {
        render(view, now);
        place();
      }
    },
    hide() {
      id = null;
      anchor = null;
      root.hidden = true;
    },
    dispose() {
      id = null;
      anchor = null;
      root.remove();
    },
  };
}
