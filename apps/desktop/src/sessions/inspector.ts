// <wtl> project: c4 | file: apps/desktop/src/sessions/inspector.ts | author: watchthelight </wtl>
// What C4 knows about the selected session, as a list of facts. Read-only by design: an observed
// session lives in another terminal and nothing here types into it.

import { el } from "../dom";
import {
  attachmentLabel,
  describeState,
  displayName,
  elapsed,
  hostLabel,
  plural,
  sourceLabel,
} from "../format";
import type { SessionView } from "../bridge/types";

export interface Inspector {
  el: HTMLElement;
  update(view: SessionView | null, now: number): void;
}

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString();
}

function noteFor(v: SessionView): string {
  switch (v.attachment) {
    case "observed":
      return `Observed through Claude Code's own files. The session runs in ${hostLabel(v.host)}; C4 does not type into it.`;
    case "disconnected":
      return "No live process. These are the last facts C4 had.";
    case "interactive_attached":
      return `Attached to a ${hostLabel(v.host)} pane.`;
    case "managed":
      return "A terminal that C4 owns.";
  }
}

function telemetry(v: SessionView): string {
  const parts = [v.coarse ? "coarse: no hook telemetry yet" : "hook telemetry"];
  if (v.hooks_seen) {
    parts.push("hooks seen");
  }
  if (v.osc_seen) {
    parts.push("OSC 7501 seen");
  }
  if (v.stalled) {
    parts.push("stalled?");
  }
  if (v.ended) {
    parts.push("ended");
  }
  return parts.join(", ");
}

/** The fact rows, in display order. */
export function facts(v: SessionView, now: number): [string, string][] {
  const running = v.agents.filter((a) => a.observed_running && a.ended_at === null).length;
  const open = v.background_tasks.filter((t) => t.completed_at === null).length;
  const rows: [string, string][] = [
    ["id", v.id],
    ["kind", v.session_kind],
    ["cwd", v.cwd ?? "unknown"],
    ["name", v.name ? `${v.name} (${v.name_source ?? "unknown source"})` : "none"],
    ["host", v.pane ? `${hostLabel(v.host)}, pane ${v.pane}` : hostLabel(v.host)],
    ["attachment", attachmentLabel(v.attachment)],
    [
      "process",
      v.process
        ? `pid ${v.process.pid}${v.process.proc_start === null ? "" : ` (started ${v.process.proc_start})`}, ${v.alive ? "alive" : "gone"}`
        : "unknown",
    ],
    ["version", v.version ?? "unknown"],
    ["started", v.started_at === null ? "unknown" : `${clock(v.started_at)}, ${elapsed(now - v.started_at)} ago`],
    ["last activity", `${elapsed(now - v.last_activity)} ago`],
    ["first seen by c4", `${elapsed(now - v.first_seen)} ago`],
    ["telemetry", telemetry(v)],
    ["agents", v.agents.length === 0 ? "none seen" : `${v.agents.length} seen, ${running} running`],
    ["background tasks", v.background_tasks.length === 0 ? "none known" : `${open} open of ${v.background_tasks.length}`],
    ["compactions", String(v.compactions)],
    ["transcript", v.transcript_path ?? "not found"],
  ];
  if (v.current_tool) {
    rows.splice(1, 0, ["tool", `${v.current_tool.name} for ${elapsed(now - v.current_tool.since)}`]);
  }
  if (v.daemon_short) {
    rows.push([
      "background job",
      `${v.daemon_short}, ${plural(v.daemon_in_flight, "task")} in flight${v.daemon_detail ? `: ${v.daemon_detail}` : ""}`,
    ]);
  }
  if (v.pending_background_agents > 0 || v.pending_workflows > 0) {
    rows.push(["pending after the last turn", `${v.pending_background_agents} agents, ${v.pending_workflows} workflows`]);
  }
  if (v.peer_socket) {
    rows.push(["peer messages", "socket present"]);
  }
  return rows;
}

export function createInspector(): Inspector {
  const root = el("section", "inspector");
  const eyebrow = el("p", "eyebrow", "session");
  const title = el("h1", "title");
  const state = el("p", "state");
  const note = el("p", "note");
  const list = el("dl", "facts");
  const hint = el("p", "hint", "Select a session to see what C4 knows about it.");
  root.append(eyebrow, title, state, note, list, hint);

  return {
    el: root,
    update(view, now) {
      const has = view !== null;
      title.hidden = !has;
      state.hidden = !has;
      note.hidden = !has;
      list.hidden = !has;
      hint.hidden = has;
      if (!view) {
        return;
      }
      const guessed = view.evidence.certainty === "heuristic";
      title.textContent = displayName(view);
      state.textContent = `${guessed ? "~" : ""}${describeState(view.state)} for ${elapsed(now - view.state_since)}, ${guessed ? "guessed" : "observed"} from ${sourceLabel(view.evidence.source)} at ${clock(view.evidence.at)}`;
      state.dataset.kind = view.state.state;
      state.dataset.certainty = view.evidence.certainty;
      note.textContent = noteFor(view);
      list.replaceChildren();
      for (const [key, value] of facts(view, now)) {
        list.append(el("dt", undefined, key), el("dd", undefined, value));
      }
    },
  };
}
