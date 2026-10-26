// <wtl> project: c4 | file: apps/desktop/src/main.ts | author: watchthelight </wtl>
// Wires the bridge to the window: a snapshot signal, a selection signal and a one-second clock feed
// one render per animation frame. Nothing renders while the window is hidden.

import { effect, signal } from "@preact/signals-core";
import { selectBridge } from "./bridge";
import type { About, Snapshot } from "./bridge/types";
import { createStrip, createTopBar } from "./chrome";
import { el } from "./dom";
import { createInspector } from "./sessions/inspector";
import { createSessionList } from "./sessions/list";
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
});
const inspector = createInspector();
const strip = createStrip(bridge.kind);

const sidebar = el("aside", "sidebar");
sidebar.append(list.el);
const workspace = el("main", "workspace");
workspace.append(inspector.el);
root.append(topBar.el, sidebar, workspace, strip.el);

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

void bridge.about().then((a) => {
  about.value = a;
});
void bridge.listSessions().then((s) => {
  snapshot.value = s;
});
void bridge.onSessions((s) => {
  snapshot.value = s;
});
