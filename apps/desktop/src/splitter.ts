// <wtl> project: c4 | file: apps/desktop/src/splitter.ts | author: watchthelight </wtl>
// The handle between the sidebar and the workspace: a 6px strip over the sidebar's right edge that
// the pointer drags and the keyboard nudges, which sets the sidebar's width within its limits. It
// reports every change for the live layout and, once the pointer is up or a key has moved it, the
// width to keep. A double-click puts the default back. Nothing in it reaches a session.

import { el } from "./dom";

/** How far one arrow key moves the edge. */
export const SPLITTER_STEP = 10;

export interface SplitterOptions {
  /** The width to start from. */
  width: number;
  /** The width a double-click puts back. */
  reset: number;
  /** The limits as they are now; asked on every change, since the window's width moves the upper one. */
  limits(): { min: number; max: number };
  /** Every change, for the live layout. */
  onChange(width: number): void;
  /** The width to keep: after a drag, a key or a reset. */
  onCommit(width: number): void;
  /** A drag starts (true) and ends (false); the starfield rests in between. */
  onDrag?(active: boolean): void;
}

export interface Splitter {
  el: HTMLElement;
  readonly width: number;
  readonly dragging: boolean;
  /** Move the edge from outside, without a commit: the window shrank, or a saved width applies. */
  setWidth(width: number): void;
  dispose(): void;
}

export function createSplitter(options: SplitterOptions): Splitter {
  const root = el("div", "splitter");
  root.setAttribute("role", "separator");
  root.setAttribute("aria-orientation", "vertical");
  root.setAttribute("aria-label", "sidebar width");
  root.title = "drag to resize the sidebar; a double-click puts the default back";
  root.tabIndex = 0;
  let width = options.width;
  let dragging = false;
  let startX = 0;
  let startWidth = 0;
  let pointerId: number | null = null;

  function clamp(w: number): number {
    const { min, max } = options.limits();
    return Math.round(Math.min(Math.max(min, max), Math.max(min, w)));
  }

  function show(): void {
    const { min, max } = options.limits();
    root.setAttribute("aria-valuemin", String(min));
    root.setAttribute("aria-valuemax", String(Math.max(min, max)));
    root.setAttribute("aria-valuenow", String(width));
  }

  /** Move to `w` within the limits; true when the width changed. */
  function move(w: number): boolean {
    const next = clamp(w);
    if (next === width) {
      show();
      return false;
    }
    width = next;
    show();
    options.onChange(width);
    return true;
  }

  function onPointerDown(event: MouseEvent): void {
    if (event.button !== 0 || dragging) {
      return;
    }
    event.preventDefault();
    dragging = true;
    startX = event.clientX;
    startWidth = width;
    pointerId = typeof PointerEvent !== "undefined" && event instanceof PointerEvent ? event.pointerId : null;
    if (pointerId !== null && typeof root.setPointerCapture === "function") {
      try {
        root.setPointerCapture(pointerId);
      } catch {
        // a pointer that is already gone; the drag still follows the events that reach the strip
      }
    }
    root.dataset.dragging = "true";
    options.onDrag?.(true);
  }

  function onPointerMove(event: MouseEvent): void {
    if (dragging) {
      move(startWidth + (event.clientX - startX));
    }
  }

  function end(): void {
    if (!dragging) {
      return;
    }
    dragging = false;
    if (pointerId !== null && typeof root.releasePointerCapture === "function") {
      try {
        root.releasePointerCapture(pointerId);
      } catch {
        // already released
      }
    }
    pointerId = null;
    delete root.dataset.dragging;
    options.onDrag?.(false);
    options.onCommit(width);
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    const { min, max } = options.limits();
    let next: number;
    switch (event.key) {
      case "ArrowLeft":
        next = width - SPLITTER_STEP;
        break;
      case "ArrowRight":
        next = width + SPLITTER_STEP;
        break;
      case "Home":
        next = min;
        break;
      case "End":
        next = max;
        break;
      default:
        return;
    }
    event.preventDefault();
    if (move(next)) {
      options.onCommit(width);
    }
  }

  function onDoubleClick(): void {
    if (move(options.reset)) {
      options.onCommit(width);
    }
  }

  root.addEventListener("pointerdown", onPointerDown);
  root.addEventListener("pointermove", onPointerMove);
  root.addEventListener("pointerup", end);
  root.addEventListener("pointercancel", end);
  root.addEventListener("lostpointercapture", end);
  root.addEventListener("keydown", onKeyDown);
  root.addEventListener("dblclick", onDoubleClick);
  show();

  return {
    el: root,
    get width() {
      return width;
    },
    get dragging() {
      return dragging;
    },
    setWidth(w) {
      width = clamp(w);
      show();
    },
    dispose() {
      end();
      root.removeEventListener("pointerdown", onPointerDown);
      root.removeEventListener("pointermove", onPointerMove);
      root.removeEventListener("pointerup", end);
      root.removeEventListener("pointercancel", end);
      root.removeEventListener("lostpointercapture", end);
      root.removeEventListener("keydown", onKeyDown);
      root.removeEventListener("dblclick", onDoubleClick);
      root.remove();
    },
  };
}
