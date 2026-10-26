// <wtl> project: c4 | file: apps/desktop/src/sessions/filter.ts | author: watchthelight </wtl>
// The line above the session list: the filter box, the grouping button and, in the main window,
// the button that folds the sidebar to its rail. `/` anywhere in the window focuses the box;
// Escape clears it, then leaves it; Enter and the down arrow leave it for the list with the filter
// kept. The box is the only text input in the sidebar and it reaches no session: it only narrows
// what the list shows.

import { el } from "../dom";
import { GROUP_BYS, nextGroupBy } from "./arrange";
import type { GroupBy } from "./arrange";

const GROUP_LABEL: Readonly<Record<GroupBy, string>> = {
  none: "no groups",
  project: "by project",
  host: "by host",
  state: "by state",
};

export function groupLabel(g: GroupBy): string {
  return GROUP_LABEL[g];
}

export interface FilterBarOptions {
  groupBy: GroupBy;
  onFilter(text: string): void;
  onGroupBy(groupBy: GroupBy): void;
  /** Focus should go back to the list. */
  onLeave(): void;
  /** When given, a button at the right end folds the sidebar to its rail. */
  onFold?(): void;
}

export interface FilterBar {
  el: HTMLElement;
  readonly text: string;
  focus(): void;
  clear(): void;
  /** Reflect a grouping chosen elsewhere, such as by the `g` key in the list. */
  setGroupBy(groupBy: GroupBy): void;
}

export function createFilterBar(options: FilterBarOptions): FilterBar {
  const root = el("div", "filterbar");
  const input = el("input", "filter");
  input.type = "text";
  input.placeholder = "filter (/)";
  input.spellcheck = false;
  input.autocomplete = "off";
  input.setAttribute("aria-label", "filter sessions");
  input.title = "words match the name, project and cwd; state:, project:, tool:, mode: and host: match one field";
  const button = el("button", "groupby");
  button.type = "button";
  let groupBy = options.groupBy;

  function show(): void {
    button.textContent = GROUP_LABEL[groupBy];
    button.dataset.group = groupBy;
    button.title = `group the list: ${GROUP_BYS.join(", ")} (g in the list)`;
  }
  show();

  function clear(): void {
    if (input.value !== "") {
      input.value = "";
      options.onFilter("");
    }
  }

  input.addEventListener("input", () => {
    options.onFilter(input.value);
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (input.value !== "") {
        clear();
      } else {
        options.onLeave();
      }
    } else if (event.key === "Enter" || event.key === "ArrowDown") {
      event.preventDefault();
      options.onLeave();
    }
  });
  button.addEventListener("click", () => {
    groupBy = nextGroupBy(groupBy);
    show();
    options.onGroupBy(groupBy);
  });
  root.append(input, button);
  const onFold = options.onFold;
  if (onFold) {
    const fold = el("button", "fold");
    fold.type = "button";
    fold.setAttribute("aria-label", "fold the sidebar to its rail");
    fold.title = "fold the sidebar to its rail (Ctrl+Shift+B)";
    const glyph = el("span", "glyph-sidebar");
    glyph.setAttribute("aria-hidden", "true");
    fold.append(glyph);
    fold.addEventListener("click", () => {
      onFold();
    });
    root.append(fold);
  }

  return {
    el: root,
    get text() {
      return input.value;
    },
    focus() {
      input.focus();
      input.select();
    },
    clear,
    setGroupBy(g) {
      if (g !== groupBy) {
        groupBy = g;
        show();
      }
    },
  };
}
