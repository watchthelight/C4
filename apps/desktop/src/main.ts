// <wtl> project: c4 | file: apps/desktop/src/main.ts | author: watchthelight </wtl>
// Wires the bridge to the window: a snapshot signal, a selection signal and a one-second clock feed
// one render per animation frame. Nothing renders while the window is hidden. The starfield canvas
// goes in first and runs on its own clock behind the sidebar and the status strip. The saved
// preferences come before anything else: the sidebar is built from them, and every change made by
// hand (a pin, the grouping, a folded group, the density, the sidebar's width, the fold to the
// rail) goes back to the core after a short quiet. The same page serves two windows: the main one,
// and the sidebar's own when the sidebar is detached (`?window=sidebar`), which holds the sidebar
// alone. The shell (src/shell.ts) says which window this is, moves the sidebar between them, and
// passes the chosen session and the written preferences from one window to the other.

import { effect, signal } from "@preact/signals-core";
import { selectBridge } from "./bridge";
import type { About, PrefsDoc, SessionView, Snapshot } from "./bridge/types";
import { createStrip, createTopBar } from "./chrome";
import { el } from "./dom";
import {
  SIDEBAR_MIN,
  SIDEBAR_WIDTH,
  WORKSPACE_MIN,
  clampWidth,
  createPrefsStore,
  readDetached,
  readSidebar,
  sidebarSection,
} from "./prefs";
import type { PrefsStore, SidebarPrefs, SidebarView } from "./prefs";
import { createFilterBar } from "./sessions/filter";
import type { FilterBar, WindowButton } from "./sessions/filter";
import { createHoverCard } from "./sessions/hovercard";
import type { HoverCard } from "./sessions/hovercard";
import { createInspector } from "./sessions/inspector";
import { createSessionList, nextDensity } from "./sessions/list";
import type { SessionList } from "./sessions/list";
import { createRail } from "./sessions/rail";
import { selectShell, windowRole } from "./shell";
import type { Placement } from "./shell";
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
const role = windowRole(window.location.search);
const bridge = selectBridge();
const shell = selectShell(role);
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

/** An app chord: Ctrl+Shift on Windows and Linux, Cmd+Shift on macOS, never with Alt. */
function chord(event: KeyboardEvent): boolean {
  return (event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey;
}

function plain(event: KeyboardEvent): boolean {
  return !event.ctrlKey && !event.metaKey && !event.altKey;
}

/** A session picked by hand in this window: shown here, and told to the other window. */
function choose(id: string): void {
  selected.value = id;
  shell.select(id);
}

interface SidebarOptions {
  /** In the main window: the button that folds the sidebar to its rail. */
  onFold?(): void;
  /** The button that moves the sidebar between the windows. */
  windowButton: WindowButton;
  /** The rest of the saved section, which the main window owns: the width and the view. */
  rest(): { width: number; view: SidebarView };
}

interface Sidebar {
  el: HTMLElement;
  list: SessionList;
  filterBar: FilterBar;
  hover: HoverCard;
  hoverFor(id: string | null, anchor: HTMLElement | null): void;
  /** Write the section: the list's preferences and the rest. */
  save(): void;
  /** Take the section of a document the other window wrote; what it held. */
  take(doc: PrefsDoc): SidebarPrefs;
  render(views: readonly SessionView[], selected: string | null, now: number): void;
}

/** The sidebar itself, the same in both windows: the filter line, the list and the hover card. */
function buildSidebar(saved: SidebarPrefs, store: PrefsStore, o: SidebarOptions): Sidebar {
  const hover = createHoverCard();

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
    onSelect: choose,
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
    onFold: o.onFold,
    windowButton: o.windowButton,
  });

  function save(): void {
    store.set("sidebar", sidebarSection({ ...list.prefs(), ...o.rest() }));
  }

  const aside = el("aside", "sidebar");
  aside.append(filterBar.el, list.el);

  return {
    el: aside,
    list,
    filterBar,
    hover,
    hoverFor,
    save,
    take(doc) {
      const prefs = readSidebar(doc);
      list.setPrefs(prefs);
      filterBar.setGroupBy(prefs.groupBy);
      return prefs;
    },
    render(views, sel, now) {
      list.update(views, sel, now);
      hover.refresh(views, now);
    },
  };
}

/** The frame: the signals folded into one render per animation frame, the clock, and the feeds. */
function run(render: (f: Frame) => void, store: PrefsStore): void {
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

  // what waits in the store goes out before the page does
  window.addEventListener("pagehide", () => {
    void store.flush();
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

/** The main window: the top bar, the sidebar or its rail, the workspace, the strip. */
function mountMain(saved: SidebarPrefs, store: PrefsStore, startDetached: boolean): { setPlacement(p: Placement): void } {
  const topBar = createTopBar();
  const stars = createStarfield(root, starfieldOptions());
  let view: SidebarView = saved.view;
  let wanted = saved.width;
  let placement: Placement = startDetached ? "detached" : "attached";

  const sidebar = buildSidebar(saved, store, {
    onFold: () => {
      setView("rail");
      rail.focus();
    },
    windowButton: {
      kind: "detach",
      onClick: () => {
        void shell.sidebar("detach");
      },
    },
    rest: () => ({ width: wanted, view }),
  });
  const rail = createRail({
    onSelect: choose,
    onExpand: () => {
      if (placement === "detached") {
        void shell.sidebar("attach");
      } else {
        setView("full");
        sidebar.list.el.focus();
      }
    },
    onHover: sidebar.hoverFor,
  });
  const inspector = createInspector();
  const strip = createStrip(bridge.kind);

  // the app root says what stands at the left (the sidebar, its rail, or the rail while the
  // sidebar is away in its own window) and the grid follows
  function layout(): void {
    const away = placement === "detached";
    root.dataset.sidebar = away ? "away" : view;
    root.dataset.detached = String(away);
    rail.setDetached(away);
  }
  layout();

  // the list is painted again when it comes back, since it had no height while hidden
  function setView(next: SidebarView): void {
    if (next === view) {
      return;
    }
    view = next;
    sidebar.hover.hide();
    layout();
    if (next === "full") {
      sidebar.list.paint();
    }
    sidebar.save();
  }

  // the sidebar comes back in full, with the focus the main window was just given
  function setPlacement(next: Placement): void {
    if (next === placement) {
      return;
    }
    placement = next;
    sidebar.hover.hide();
    if (next === "attached" && view !== "full") {
      view = "full";
      sidebar.save();
    }
    layout();
    if (next === "attached") {
      sidebar.list.paint();
      sidebar.list.el.focus();
    }
  }

  // the sidebar's width is a custom property on the app root; the splitter moves it live and the
  // store gets the width once the drag ends. `wanted` is the width the user set; a narrower window
  // shows less of it and a wider one gives it back, and it is what the file keeps
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
      sidebar.save();
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

  const workspace = el("main", "workspace");
  workspace.append(inspector.el);
  root.append(topBar.el, sidebar.el, rail.el, workspace, strip.el, splitter.el, sidebar.hover.el);

  function render(f: Frame): void {
    const views = f.snapshot?.sessions ?? [];
    topBar.update(views, f.about);
    sidebar.render(views, f.selected, f.now);
    rail.update(views, f.selected, sidebar.list.isPinned);
    inspector.update(views.find((v) => v.id === f.selected) ?? null, f.now);
    const facts = f.about === null ? null : { ...f.about, prefs_problem: f.note ?? f.about.prefs_problem };
    strip.update(f.snapshot, facts, f.now);
  }

  // the shell's words: the placement changed, the sidebar window chose a session or wrote the
  // section, or the filter box is wanted
  shell.onMessage((message) => {
    switch (message.kind) {
      case "windows":
        setPlacement(message.sidebar);
        break;
      case "select":
        selected.value = message.id;
        break;
      case "prefs":
        sidebar.take(message.doc);
        break;
      case "focus":
        if (placement === "attached") {
          setView("full");
          sidebar.filterBar.focus();
        }
        break;
    }
  });

  // a narrower window pulls the sidebar in, without a write; a wider one gives the width back
  window.addEventListener("resize", () => {
    const width = clampWidth(wanted, window.innerWidth);
    splitter.setWidth(width);
    applyWidth(width);
  });

  // Ctrl+Shift+= (Cmd+Shift+= on macOS) walks the row density: compact, standard, expanded.
  // Ctrl+Shift+B folds the sidebar to its rail and opens it again, with focus following, or brings
  // it back from its own window. Ctrl+Shift+O moves it out into its own window and back. `/`
  // outside a text field focuses the filter box, opening the sidebar first if it is folded, or
  // bringing its window up.
  document.addEventListener("keydown", (event) => {
    if (chord(event)) {
      switch (event.code) {
        case "Equal":
          event.preventDefault();
          sidebar.list.setDensity(nextDensity(sidebar.list.density));
          break;
        case "KeyB":
          event.preventDefault();
          if (placement === "detached") {
            void shell.sidebar("attach");
          } else if (view === "full") {
            setView("rail");
            rail.focus();
          } else {
            setView("full");
            sidebar.list.el.focus();
          }
          break;
        case "KeyO":
          event.preventDefault();
          void shell.sidebar(placement === "detached" ? "attach" : "detach");
          break;
        default:
          break;
      }
      return;
    }
    if (plain(event) && event.key === "/" && !typing(event.target)) {
      event.preventDefault();
      if (placement === "detached") {
        void shell.sidebar("focus");
      } else {
        setView("full");
        sidebar.filterBar.focus();
      }
    }
  });

  run(render, store);
  return { setPlacement };
}

/** The sidebar's own window: the sidebar alone, edge to edge, over the starfield. */
function mountSidebarWindow(saved: SidebarPrefs, store: PrefsStore): void {
  document.title = "c4 sessions";
  createStarfield(root, starfieldOptions());
  // the width and the view belong to the main window; this window writes back what it last read
  let rest = { width: saved.width, view: saved.view };
  const sidebar = buildSidebar(saved, store, {
    windowButton: {
      kind: "attach",
      onClick: () => {
        void shell.sidebar("attach");
      },
    },
    rest: () => rest,
  });
  root.append(sidebar.el, sidebar.hover.el);

  shell.onMessage((message) => {
    switch (message.kind) {
      case "select":
        selected.value = message.id;
        break;
      case "prefs": {
        const prefs = sidebar.take(message.doc);
        rest = { width: prefs.width, view: prefs.view };
        break;
      }
      case "focus":
        sidebar.filterBar.focus();
        break;
      default:
        break;
    }
  });

  // the density chord as in the main window; Ctrl+Shift+B and Ctrl+Shift+O both put the sidebar
  // back, since there is no rail here to fold to; `/` focuses the filter box
  document.addEventListener("keydown", (event) => {
    if (chord(event)) {
      if (event.code === "Equal") {
        event.preventDefault();
        sidebar.list.setDensity(nextDensity(sidebar.list.density));
      } else if (event.code === "KeyB" || event.code === "KeyO") {
        event.preventDefault();
        void shell.sidebar("attach");
      }
      return;
    }
    if (plain(event) && event.key === "/" && !typing(event.target)) {
      event.preventDefault();
      sidebar.filterBar.focus();
    }
  });

  run((f) => {
    sidebar.render(f.snapshot?.sessions ?? [], f.selected, f.now);
  }, store);
}

/** Read the preferences, then build the window; a core that cannot give them yields the defaults. */
async function start(): Promise<void> {
  root.dataset.window = role;
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
  const saved = readSidebar(doc);
  if (role === "sidebar") {
    mountSidebarWindow(saved, store);
    return;
  }
  const main = mountMain(saved, store, readDetached(doc));
  // the shell's own word on where the sidebar is, once the last run's placement is back
  void shell.windows().then(
    (placement) => {
      main.setPlacement(placement);
    },
    () => undefined,
  );
}

void start();
