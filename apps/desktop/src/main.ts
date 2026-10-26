// <wtl> project: c4 | file: apps/desktop/src/main.ts | author: watchthelight </wtl>
// Wires the bridge to the window: a snapshot signal, a selection signal and a one-second clock feed
// one render per animation frame. Nothing renders while the window is hidden. The starfield canvas
// goes in first and runs on its own clock behind the sidebar and the status strip.

import { effect, signal } from "@preact/signals-core";
import { selectBridge } from "./bridge";
import type { About, Snapshot } from "./bridge/types";
import { createStrip, createTopBar } from "./chrome";
import { el } from "./dom";
import { createFilterBar } from "./sessions/filter";
import { createInspector } from "./sessions/inspector";
import { createSessionList, nextDensity } from "./sessions/list";
import { createStarfield } from "./starfield";
import type { StarfieldOptions } from "./starfield";
import "./tokens.css";
import "./style.css";

interface Frame {
  snapshot: Snapshot | null;
  about: About | null;
  selected: string | null;
  now: number;
}

const root = document.getElementById("app");
if (!root) {
  throw new Error("index.html has no #app element");
}

const bridge = selectBridge();
const snapshot = signal<Snapshot | null>(null);
const about = signal<About | null>(null);
const selected = signal<string | null>(null);
const clock = signal(Date.now());

const topBar = createTopBar();
const list = createSessionList({
  onSelect: (id) => {
    selected.value = id;
  },
  // nothing is persisted yet; the bar only mirrors a grouping the `g` key chose
  onPrefs: (prefs) => {
    filterBar.setGroupBy(prefs.groupBy);
  },
});
const filterBar = createFilterBar({
  groupBy: list.groupBy,
  onFilter: (text) => {
    list.setFilter(text);
  },
  onGroupBy: (groupBy) => {
    list.setGroupBy(groupBy);
  },
  onLeave: () => {
    list.el.focus();
  },
});
const inspector = createInspector();
const strip = createStrip(bridge.kind);

const sidebar = el("aside", "sidebar");
sidebar.append(filterBar.el, list.el);
const workspace = el("main", "workspace");
workspace.append(inspector.el);
root.append(topBar.el, sidebar, workspace, strip.el);
createStarfield(root, starfieldOptions());

/** A fixed seed, for screenshots, gives the same sky every run and no shooting stars. */
function starfieldOptions(): StarfieldOptions {
  const seed = import.meta.env.VITE_C4_SEED;
  if (seed === undefined || seed === "") {
    return {};
  }
  return { seed: Number(seed) || 0, shootingStars: false };
}

let frame: Frame | null = null;
let scheduled = false;

function render(f: Frame): void {
  const views = f.snapshot?.sessions ?? [];
  topBar.update(views, f.about);
  list.update(views, f.selected, f.now);
  inspector.update(views.find((v) => v.id === f.selected) ?? null, f.now);
  strip.update(f.snapshot, f.about, f.now);
}

function schedule(): void {
  if (scheduled) {
    return;
  }
  scheduled = true;
  requestAnimationFrame(() => {
    scheduled = false;
    if (frame) {
      render(frame);
    }
  });
}

effect(() => {
  frame = { snapshot: snapshot.value, about: about.value, selected: selected.value, now: clock.value };
  schedule();
});

setInterval(() => {
  clock.value = Date.now();
}, 1000);

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    schedule();
  }
});

/** True for a text field, where a plain key is typing and not a command. */
function typing(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

// Ctrl+Shift+= (Cmd+Shift+= on macOS) walks the row density: compact, standard, expanded.
// `/` outside a text field focuses the filter box.
document.addEventListener("keydown", (event) => {
  const chord = (event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey;
  if (chord && event.code === "Equal") {
    event.preventDefault();
    list.setDensity(nextDensity(list.density));
    return;
  }
  const plain = !event.ctrlKey && !event.metaKey && !event.altKey;
  if (plain && event.key === "/" && !typing(event.target)) {
    event.preventDefault();
    filterBar.focus();
  }
});

void bridge.about().then((a) => {
  about.value = a;
});
void bridge.listSessions().then((s) => {
  snapshot.value = s;
});
void bridge.onSessions((s) => {
  snapshot.value = s;
});
