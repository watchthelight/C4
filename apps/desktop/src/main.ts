// <wtl> project: c4 | file: apps/desktop/src/main.ts | author: watchthelight </wtl>
// Wires the bridge to the window: a snapshot signal, a selection signal and a one-second clock feed
// one render per animation frame. Nothing renders while the window is hidden. The starfield canvas
// goes in first and runs on its own clock behind the sidebar and the status strip. The saved
// preferences come before anything else: the sidebar is built from them, and every change made by
// hand (a pin, the grouping, a folded group, the density, the sidebar's width, the fold to the
// rail) goes back to the core after a short quiet.

import { effect, signal } from "@preact/signals-core";
import { selectBridge } from "./bridge";
import type { About, PrefsDoc, SessionView, Snapshot } from "./bridge/types";
import { createStrip, createTopBar } from "./chrome";
import { el } from "./dom";
import { SIDEBAR_MIN, SIDEBAR_WIDTH, WORKSPACE_MIN, clampWidth, createPrefsStore, readSidebar, sidebarSection } from "./prefs";
import type { PrefsStore, SidebarPrefs, SidebarView } from "./prefs";
import { createFilterBar } from "./sessions/filter";
import { createHoverCard } from "./sessions/hovercard";
import { createInspector } from "./sessions/inspector";
import { createSessionList, nextDensity } from "./sessions/list";
import { createRail } from "./sessions/rail";
import { createSplitter } from "./splitter";
import { createStarfield } from "./starfield";
import type { StarfieldOptions } from "./starfield";
import "./tokens.css";
import "./style.css";

interface Frame {
  snapshot: Snapshot | null;
  about: About | null;
  selected: string | null;
  /** What went wrong with the preferences in this run, for the strip; null while nothing has. */
  note: string | null;
  now: number;
}

function appRoot(): HTMLElement {
  const root = document.getElementById("app");
  if (!root) {
    throw new Error("index.html has no #app element");
  }
  return root;
}

const root = appRoot();
const bridge = selectBridge();
const snapshot = signal<Snapshot | null>(null);
const about = signal<About | null>(null);
const selected = signal<string | null>(null);
const note = signal<string | null>(null);
const clock = signal(Date.now());

let frame: Frame | null = null;
let scheduled = false;

/** The view the last frame carried for `id`, if any. */
function viewOf(id: string): SessionView | undefined {
  return frame?.snapshot?.sessions.find((v) => v.id === id);
}

/** A fixed seed, for screenshots, gives the same sky every run and no shooting stars. */
function starfieldOptions(): StarfieldOptions {
  const seed = import.meta.env.VITE_C4_SEED;
  if (seed === undefined || seed === "") {
    return {};
  }
  return { seed: Number(seed) || 0, shootingStars: false };
}

/** True for a text field, where a plain key is typing and not a command. */
function typing(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

/** Build the window from the saved preferences and start the feeds. */
function mount(saved: SidebarPrefs, store: PrefsStore): void {
  const topBar = createTopBar();
  const hover = createHoverCard();
  const stars = createStarfield(root, starfieldOptions());

  // the card is drawn outside the list and the rail, beside the row or square the pointer rests
  // on, on the frame's clock so its times agree with the rows
  function hoverFor(id: string | null, anchor: HTMLElement | null): void {
    const view = id === null ? undefined : viewOf(id);
    if (view && anchor && frame) {
      hover.show(view, anchor, frame.now);
    } else {
      hover.hide();
    }
  }

  const list = createSessionList({
    density: saved.density,
    groupBy: saved.groupBy,
    pinned: saved.pinned,
    collapsed: saved.collapsed,
    onSelect: (id) => {
      selected.value = id;
    },
    // every change by hand: the bar mirrors the grouping, the store gets the section
    onPrefs: (prefs) => {
      filterBar.setGroupBy(prefs.groupBy);
      save();
    },
    onHover: hoverFor,
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
    onFold: () => {
      setView("rail");
      rail.focus();
    },
  });
  const rail = createRail({
    onSelect: (id) => {
      selected.value = id;
    },
    onExpand: () => {
      setView("full");
      list.el.focus();
    },
    onHover: hoverFor,
  });
  const inspector = createInspector();
  const strip = createStrip(bridge.kind);

  // the sidebar shows in full or folded to its rail; the app root carries which, and the layout
  // follows. The list is painted again when it comes back, since it had no height while hidden.
  let view: SidebarView = saved.view;
  root.dataset.sidebar = view;
  function setView(next: SidebarView): void {
    if (next === view) {
      return;
    }
    view = next;
    hover.hide();
    root.dataset.sidebar = next;
    if (next === "full") {
      list.paint();
    }
    save();
  }

  // the sidebar's width is a custom property on the app root; the splitter moves it live and the
  // store gets the width once the drag ends. `wanted` is the width the user set; a narrower window
  // shows less of it and a wider one gives it back, and it is what the file keeps
  let wanted = saved.width;
  function applyWidth(width: number): void {
    root.style.setProperty("--sidebar-w", `${width}px`);
  }
  const splitter = createSplitter({
    width: clampWidth(wanted, window.innerWidth),
    reset: SIDEBAR_WIDTH,
    limits: () => ({ min: SIDEBAR_MIN, max: Math.max(SIDEBAR_MIN, window.innerWidth - WORKSPACE_MIN) }),
    onChange: applyWidth,
    onCommit: (width) => {
      wanted = width;
      save();
    },
    onDrag: (active) => {
      if (active) {
        stars.freeze("splitter");
      } else {
        stars.thaw("splitter");
      }
    },
  });
  applyWidth(splitter.width);

  function save(): void {
    store.set("sidebar", sidebarSection({ ...list.prefs(), width: wanted, view }));
  }

  const sidebar = el("aside", "sidebar");
  sidebar.append(filterBar.el, list.el);
  const workspace = el("main", "workspace");
  workspace.append(inspector.el);
  root.append(topBar.el, sidebar, rail.el, workspace, strip.el, splitter.el, hover.el);

  function render(f: Frame): void {
    const views = f.snapshot?.sessions ?? [];
    topBar.update(views, f.about);
    list.update(views, f.selected, f.now);
    rail.update(views, f.selected, list.isPinned);
    hover.refresh(views, f.now);
    inspector.update(views.find((v) => v.id === f.selected) ?? null, f.now);
    const facts = f.about === null ? null : { ...f.about, prefs_problem: f.note ?? f.about.prefs_problem };
    strip.update(f.snapshot, facts, f.now);
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
    frame = {
      snapshot: snapshot.value,
      about: about.value,
      selected: selected.value,
      note: note.value,
      now: clock.value,
    };
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

  // a narrower window pulls the sidebar in, without a write; a wider one gives the width back
  window.addEventListener("resize", () => {
    const width = clampWidth(wanted, window.innerWidth);
    splitter.setWidth(width);
    applyWidth(width);
  });

  // what waits in the store goes out before the page does
  window.addEventListener("pagehide", () => {
    void store.flush();
  });

  // Ctrl+Shift+= (Cmd+Shift+= on macOS) walks the row density: compact, standard, expanded.
  // Ctrl+Shift+B folds the sidebar to its rail and opens it again, with focus following. `/`
  // outside a text field focuses the filter box, opening the sidebar first if it is folded.
  document.addEventListener("keydown", (event) => {
    const chord = (event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey;
    if (chord && event.code === "Equal") {
      event.preventDefault();
      list.setDensity(nextDensity(list.density));
      return;
    }
    if (chord && event.code === "KeyB") {
      event.preventDefault();
      if (view === "full") {
        setView("rail");
        rail.focus();
      } else {
        setView("full");
        list.el.focus();
      }
      return;
    }
    const plain = !event.ctrlKey && !event.metaKey && !event.altKey;
    if (plain && event.key === "/" && !typing(event.target)) {
      event.preventDefault();
      setView("full");
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
}

/** Read the preferences, then build the window; a core that cannot give them yields the defaults. */
async function start(): Promise<void> {
  let doc: PrefsDoc = {};
  try {
    doc = await bridge.prefsGet();
  } catch (error) {
    note.value = `preferences: could not read them: ${String(error)}`;
  }
  const store = createPrefsStore(bridge, doc, {
    onError: (section, error) => {
      note.value = `preferences: could not save ${section}: ${String(error)}`;
    },
  });
  mount(readSidebar(doc), store);
}

void start();
