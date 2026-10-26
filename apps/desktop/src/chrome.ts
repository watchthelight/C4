// <wtl> project: c4 | file: apps/desktop/src/chrome.ts | author: watchthelight </wtl>
// The top bar (mark, counts, home) and the status strip (refresh timing, session list command,
// version, bridge).

import { el } from "./dom";
import { elapsed, needsAttention, plural } from "./format";
import type { About, SessionView, Snapshot } from "./bridge/types";

export interface TopBar {
  el: HTMLElement;
  update(views: readonly SessionView[], about: About | null): void;
}

export interface Strip {
  el: HTMLElement;
  update(snapshot: Snapshot | null, about: About | null, now: number): void;
}

export function createTopBar(): TopBar {
  const root = el("header", "topbar");
  const mark = el("span", "mark");
  mark.setAttribute("aria-hidden", "true");
  const word = el("span", "word", "c4");
  const counts = el("span", "counts");
  const home = el("span", "home");
  root.append(mark, word, counts, home);
  return {
    el: root,
    update(views, about) {
      const alive = views.filter((v) => v.alive).length;
      const attention = views.filter((v) => needsAttention(v.state.state)).length;
      const parts = views.length === 0 ? ["no sessions"] : [plural(views.length, "session"), `${alive} alive`];
      if (attention > 0) {
        parts.push(`${attention} need${attention === 1 ? "s" : ""} you`);
      }
      counts.textContent = parts.join(", ");
      home.textContent = about ? about.home : "";
    },
  };
}

export function createStrip(bridge: string): Strip {
  const root = el("footer", "strip");
  return {
    el: root,
    update(snapshot, about, now) {
      const parts: string[] = [];
      if (snapshot === null) {
        parts.push("waiting for the first pass");
      } else {
        parts.push(`refreshed ${elapsed(now - snapshot.at)} ago in ${snapshot.report.elapsed_ms} ms`);
        const agents = snapshot.agents;
        if (agents === null) {
          parts.push("session list: not run yet");
        } else if (agents !== undefined) {
          parts.push(
            agents.error
              ? `session list: ${agents.error}`
              : `session list: ${plural(agents.entries ?? 0, "entry").replace("entrys", "entries")}, ${elapsed(now - agents.at)} ago`,
          );
        }
        if (snapshot.report.problems.length > 0) {
          parts.push(plural(snapshot.report.problems.length, "unreadable file"));
        }
      }
      if (about) {
        parts.push(`home from ${about.home_source}`, `c4 ${about.version}`);
      }
      parts.push(`bridge: ${bridge}`);
      root.textContent = parts.join("  |  ");
    },
  };
}
