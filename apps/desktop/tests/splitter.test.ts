// <wtl> project: c4 | file: apps/desktop/tests/splitter.test.ts | author: watchthelight </wtl>
// The sidebar splitter: what it tells assistive technology, a pointer drag with its limits, the
// keys, the double-click reset, and that nothing leaks after dispose. Pointer events are plain
// MouseEvents here, since jsdom gives a plain Event no coordinates.

import { describe, expect, it, vi } from "vitest";
import { SPLITTER_STEP, createSplitter } from "../src/splitter";
import type { SplitterOptions } from "../src/splitter";

function make(over: Partial<SplitterOptions> = {}) {
  const onChange = vi.fn();
  const onCommit = vi.fn();
  const onDrag = vi.fn();
  const splitter = createSplitter({
    width: 320,
    reset: 320,
    limits: () => ({ min: 220, max: 600 }),
    onChange,
    onCommit,
    onDrag,
    ...over,
  });
  document.body.append(splitter.el);
  return { splitter, onChange, onCommit, onDrag };
}

function pointer(el: HTMLElement, type: string, clientX: number, button = 0): void {
  el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX, button }));
}

function key(el: HTMLElement, k: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
  el.dispatchEvent(event);
  return event;
}

describe("splitter", () => {
  it("is a vertical separator with its limits and value, reachable by Tab", () => {
    const { splitter } = make();
    const el = splitter.el;
    expect(el.getAttribute("role")).toBe("separator");
    expect(el.getAttribute("aria-orientation")).toBe("vertical");
    expect(el.getAttribute("aria-label")).toBe("sidebar width");
    expect(el.getAttribute("aria-valuemin")).toBe("220");
    expect(el.getAttribute("aria-valuemax")).toBe("600");
    expect(el.getAttribute("aria-valuenow")).toBe("320");
    expect(el.tabIndex).toBe(0);
    expect(splitter.width).toBe(320);
    expect(splitter.dragging).toBe(false);
    splitter.dispose();
  });

  it("follows a drag live and commits once the pointer is up", () => {
    const { splitter, onChange, onCommit, onDrag } = make();
    pointer(splitter.el, "pointerdown", 320);
    expect(splitter.dragging).toBe(true);
    expect(splitter.el.dataset.dragging).toBe("true");
    expect(onDrag).toHaveBeenLastCalledWith(true);
    pointer(splitter.el, "pointermove", 350);
    pointer(splitter.el, "pointermove", 380);
    expect(onChange.mock.calls).toEqual([[350], [380]]);
    expect(splitter.width).toBe(380);
    expect(splitter.el.getAttribute("aria-valuenow")).toBe("380");
    expect(onCommit).not.toHaveBeenCalled();
    pointer(splitter.el, "pointerup", 380);
    expect(splitter.dragging).toBe(false);
    expect(splitter.el.dataset.dragging).toBeUndefined();
    expect(onDrag).toHaveBeenLastCalledWith(false);
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(380);
    // a move after the drag does nothing
    pointer(splitter.el, "pointermove", 500);
    expect(splitter.width).toBe(380);
    splitter.dispose();
  });

  it("holds the width inside the limits and only reports real changes", () => {
    const { splitter, onChange } = make();
    pointer(splitter.el, "pointerdown", 320);
    pointer(splitter.el, "pointermove", 2000);
    expect(splitter.width).toBe(600);
    pointer(splitter.el, "pointermove", 1900);
    expect(onChange).toHaveBeenCalledTimes(1);
    pointer(splitter.el, "pointermove", -500);
    expect(splitter.width).toBe(220);
    expect(onChange).toHaveBeenCalledTimes(2);
    pointer(splitter.el, "pointerup", -500);
    splitter.dispose();
  });

  it("ignores any button but the first, and a cancelled pointer still commits", () => {
    const { splitter, onCommit } = make();
    pointer(splitter.el, "pointerdown", 320, 2);
    expect(splitter.dragging).toBe(false);
    pointer(splitter.el, "pointerdown", 320);
    pointer(splitter.el, "pointermove", 300);
    pointer(splitter.el, "pointercancel", 300);
    expect(splitter.dragging).toBe(false);
    expect(onCommit).toHaveBeenCalledWith(300);
    splitter.dispose();
  });

  it("moves by the step on the arrows and to the limits on Home and End, each a commit", () => {
    const { splitter, onChange, onCommit } = make();
    expect(key(splitter.el, "ArrowRight").defaultPrevented).toBe(true);
    expect(splitter.width).toBe(320 + SPLITTER_STEP);
    key(splitter.el, "ArrowLeft");
    key(splitter.el, "ArrowLeft");
    expect(splitter.width).toBe(320 - SPLITTER_STEP);
    key(splitter.el, "End");
    expect(splitter.width).toBe(600);
    key(splitter.el, "Home");
    expect(splitter.width).toBe(220);
    expect(onChange).toHaveBeenCalledTimes(5);
    expect(onCommit.mock.calls.map((c) => c[0])).toEqual([330, 320, 310, 600, 220]);
    // at the limit already: nothing to report
    key(splitter.el, "ArrowLeft");
    expect(onCommit).toHaveBeenCalledTimes(5);
    // a chord or another key is not the splitter's
    expect(key(splitter.el, "ArrowRight", { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(key(splitter.el, "a").defaultPrevented).toBe(false);
    expect(splitter.width).toBe(220);
    splitter.dispose();
  });

  it("puts the default back on a double-click, once", () => {
    const { splitter, onCommit } = make({ width: 500 });
    splitter.el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(splitter.width).toBe(320);
    expect(onCommit).toHaveBeenCalledWith(320);
    splitter.el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(onCommit).toHaveBeenCalledTimes(1);
    splitter.dispose();
  });

  it("takes a width from outside within the limits, without a report", () => {
    const { splitter, onChange, onCommit } = make();
    splitter.setWidth(1000);
    expect(splitter.width).toBe(600);
    expect(splitter.el.getAttribute("aria-valuenow")).toBe("600");
    splitter.setWidth(260.4);
    expect(splitter.width).toBe(260);
    expect(onChange).not.toHaveBeenCalled();
    expect(onCommit).not.toHaveBeenCalled();
    splitter.dispose();
  });

  it("asks for the limits each time, so a narrower window lowers the ceiling", () => {
    let max = 600;
    const { splitter } = make({ limits: () => ({ min: 220, max }) });
    key(splitter.el, "End");
    expect(splitter.width).toBe(600);
    max = 400;
    key(splitter.el, "ArrowRight");
    expect(splitter.width).toBe(400);
    expect(splitter.el.getAttribute("aria-valuemax")).toBe("400");
    splitter.dispose();
  });

  it("ends a drag and leaves the document on dispose", () => {
    const { splitter, onCommit } = make();
    pointer(splitter.el, "pointerdown", 320);
    pointer(splitter.el, "pointermove", 340);
    splitter.dispose();
    expect(splitter.dragging).toBe(false);
    expect(onCommit).toHaveBeenCalledWith(340);
    expect(splitter.el.isConnected).toBe(false);
    key(splitter.el, "ArrowRight");
    expect(splitter.width).toBe(340);
  });
});
