// <wtl> project: c4 | file: apps/desktop/tests/filterbar.test.ts | author: watchthelight </wtl>
// The filter box and the grouping button above the list: typing narrows at once, Enter and the down
// arrow hand focus back to the list, Escape clears first and leaves second, the button walks the
// grouping modes and shows one chosen elsewhere.

import { describe, expect, it } from "vitest";
import { createFilterBar, groupLabel } from "../src/sessions/filter";
import type { GroupBy } from "../src/sessions/arrange";

describe("filter bar", () => {
  it("narrows as you type, leaves on Enter, clears then leaves on Escape, and cycles the grouping", () => {
    const texts: string[] = [];
    const groups: GroupBy[] = [];
    let left = 0;
    const bar = createFilterBar({
      groupBy: "none",
      onFilter: (t) => texts.push(t),
      onGroupBy: (g) => groups.push(g),
      onLeave: () => {
        left += 1;
      },
    });
    document.body.append(bar.el);
    const input = bar.el.querySelector<HTMLInputElement>("input.filter");
    const button = bar.el.querySelector<HTMLButtonElement>("button.groupby");
    if (!input || !button) {
      throw new Error("the bar is missing its box or its button");
    }
    expect(input.getAttribute("aria-label")).toBe("filter sessions");
    expect(input.type).toBe("text");
    expect(input.spellcheck).toBe(false);

    bar.focus();
    expect(document.activeElement).toBe(input);
    input.value = "state:wait";
    input.dispatchEvent(new Event("input"));
    expect(texts).toEqual(["state:wait"]);
    expect(bar.text).toBe("state:wait");

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    expect(left).toBe(1);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    expect(left).toBe(2);
    expect(input.value).toBe("state:wait");

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(input.value).toBe("");
    expect(texts).toEqual(["state:wait", ""]);
    expect(left).toBe(2);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(left).toBe(3);
    bar.clear();
    expect(texts).toHaveLength(2);

    expect(button.textContent).toBe("no groups");
    expect(button.dataset.group).toBe("none");
    button.click();
    expect(groups).toEqual(["project"]);
    expect(button.textContent).toBe("by project");
    bar.setGroupBy("state");
    expect(button.textContent).toBe("by state");
    button.click();
    expect(groups).toEqual(["project", "none"]);
    expect(groupLabel("host")).toBe("by host");
    expect(bar.el.querySelector("button.fold")).toBeNull();
    bar.el.remove();
  });

  it("gets a fold button only when the window has a rail to fold to", () => {
    let folded = 0;
    const bar = createFilterBar({
      groupBy: "none",
      onFilter: () => {},
      onGroupBy: () => {},
      onLeave: () => {},
      onFold: () => {
        folded += 1;
      },
    });
    const fold = bar.el.querySelector<HTMLButtonElement>("button.fold");
    if (!fold) {
      throw new Error("the bar has no fold button");
    }
    expect(fold.type).toBe("button");
    expect(fold.getAttribute("aria-label")).toBe("fold the sidebar to its rail");
    expect(fold.title).toContain("Ctrl+Shift+B");
    expect(fold.querySelector(".glyph-sidebar")?.getAttribute("aria-hidden")).toBe("true");
    fold.click();
    expect(folded).toBe(1);
  });
});
