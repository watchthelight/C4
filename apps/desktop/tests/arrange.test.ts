// <wtl> project: c4 | file: apps/desktop/tests/arrange.test.ts | author: watchthelight </wtl>
// The sidebar's order rules without a DOM: the filter grammar, what each token matches, the
// attention sort with pins, and the grouping with its headers, counts and collapsed groups.

import { describe, expect, it } from "vitest";
import { syntheticSessions } from "../src/bridge/mock";
import type { SessionView, StateKind } from "../src/bridge/types";
import { displayName } from "../src/format";
import { arrange, matchesQuery, nextGroupBy, parseQuery, projectTitle } from "../src/sessions/arrange";
import type { ArrangeOptions, Arranged, HeadItem } from "../src/sessions/arrange";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

const FLAT: ArrangeOptions = {
  groupBy: "none",
  query: parseQuery(""),
  pinned: new Set<string>(),
  collapsed: new Set<string>(),
};

function heads(a: Arranged): HeadItem[] {
  return a.items.filter((i): i is HeadItem => i.kind === "head");
}

function states(a: Arranged): (StateKind | "head")[] {
  return a.items.map((i) => (i.kind === "row" ? i.view.state.state : "head"));
}

describe("filter query", () => {
  it("splits words from field tokens, lowercases, and lets an empty value through", () => {
    expect(parseQuery("  Beta state:wait HOST:Wez foo:bar tool: ")).toEqual({
      words: ["beta", "foo:bar"],
      fields: { state: ["wait"], host: ["wez"] },
    });
    expect(parseQuery("")).toEqual({ words: [], fields: {} });
    expect(parseQuery("   ")).toEqual({ words: [], fields: {} });
  });

  it("matches words against the name, project and cwd, and tokens against one field each", () => {
    const views = syntheticSessions(NOW);
    const names = (text: string): string[] =>
      views.filter((v) => matchesQuery(v, parseQuery(text))).map((v) => displayName(v));
    expect(names("beta")).toEqual(["beta"]);
    expect(names("proj beta")).toEqual(["beta"]);
    expect(names("example")).toHaveLength(views.length);
    expect(names("foo:bar")).toEqual([]);
    expect(names("state:")).toHaveLength(views.length);
    expect(names("state:wait")).toEqual(["beta"]);
    expect(names("state:permission")).toEqual(["beta"]);
    // two values of one key: either matches
    expect(names("state:waiting state:failed")).toEqual(["beta", "scratch"]);
    // two keys: both have to
    expect(names("state:wait host:tmux")).toEqual([]);
    expect(names("host:wez")).toEqual(["beta"]);
    expect(names("host:windows")).toEqual(["alpha build", "scratch"]);
    expect(names("tool:read")).toEqual(["docs"]);
    expect(names("mode:disconnected")).toEqual(["old worker", "review"]);
    expect(names("mode:observed")).toHaveLength(6);
    expect(names("project:alpha")).toEqual(["alpha build"]);
  });

  it("names a project by its last folder, with either separator", () => {
    expect(projectTitle("C:\\Users\\example\\src\\alpha")).toBe("alpha");
    expect(projectTitle("/home/example/proj/beta/")).toBe("beta");
    expect(projectTitle("/home/example/proj/beta", 2)).toBe("proj/beta");
    expect(projectTitle(null)).toBe("no cwd");
    expect(projectTitle("/")).toBe("no cwd");
  });
});

describe("arrange", () => {
  it("sorts by attention with pinned sessions first and no headers when ungrouped", () => {
    const views = syntheticSessions(NOW);
    const flat = arrange(views, FLAT);
    expect(states(flat)).toEqual([
      "waiting_permission",
      "failed",
      "disconnected",
      "running_tool",
      "background_agents_active",
      "working",
      "idle",
      "completed",
    ]);
    expect(flat.matched).toBe(8);
    expect(flat.rows).toBe(8);
    expect(flat.items.every((i) => i.kind === "row" && !i.pinned && i.head === null)).toBe(true);

    const review = views.find((v) => v.name === "review")?.id ?? "";
    const pinned = arrange(views, { ...FLAT, pinned: new Set([review]) });
    expect(pinned.items[0]).toMatchObject({ kind: "row", id: review, pinned: true });
    expect(states(pinned).slice(1)).toEqual(states(flat).slice(0, -1));

    const narrowed = arrange(views, { ...FLAT, query: parseQuery("state:wait") });
    expect(narrowed.matched).toBe(1);
    expect(narrowed.items).toHaveLength(1);
  });

  it("groups by project with a numbered header per group in the order of their most urgent session", () => {
    const views = syntheticSessions(NOW);
    const byProject = arrange(views, { ...FLAT, groupBy: "project" });
    const h = heads(byProject);
    expect(h.map((x) => x.title)).toEqual(["beta", "scratch", "old", "docs", "nightly", "alpha", "gamma", "review"]);
    expect(h.map((x) => x.index)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(h[0]).toMatchObject({
      key: "g:project:/home/example/proj/beta",
      group: "project:/home/example/proj/beta",
      count: 1,
      attention: 1,
      collapsed: false,
    });
    expect(byProject.items[1]).toMatchObject({ kind: "row", head: h[0]?.key });
    expect(byProject.rows).toBe(8);
    expect(byProject.items).toHaveLength(16);
  });

  it("tells two projects with the same folder name apart by their parent", () => {
    const base = syntheticSessions(NOW)[0];
    if (!base) {
      throw new Error("no synthetic session");
    }
    const views: SessionView[] = [
      { ...base, id: "00000000-0000-4000-8000-000000000001", cwd: "/home/example/a/src" },
      { ...base, id: "00000000-0000-4000-8000-000000000002", cwd: "/home/example/b/src" },
      { ...base, id: "00000000-0000-4000-8000-000000000003", cwd: "/home/example/c/alpha" },
      { ...base, id: "00000000-0000-4000-8000-000000000004", cwd: null },
    ];
    const titles = heads(arrange(views, { ...FLAT, groupBy: "project" })).map((h) => h.title);
    expect(titles).toEqual(["a/src", "b/src", "alpha", "no cwd"]);
  });

  it("groups by host and by state, counts who needs you, and hides the rows of a collapsed group", () => {
    const views = syntheticSessions(NOW);
    const byHost = arrange(views, { ...FLAT, groupBy: "host" });
    expect(heads(byHost).map((h) => h.title)).toEqual([
      "WezTerm",
      "Windows Terminal",
      "unknown host",
      "tmux",
      "VS Code",
      "kitty",
    ]);
    expect(heads(byHost)[1]).toMatchObject({ group: "host:windows_terminal", count: 2, attention: 1 });

    const byState = arrange(views, {
      ...FLAT,
      groupBy: "state",
      collapsed: new Set(["state:waiting_permission", "host:tmux"]),
    });
    expect(heads(byState).map((h) => h.title)).toEqual([
      "waiting for permission",
      "failed",
      "disconnected",
      "running a tool",
      "background agents active",
      "working",
      "idle",
      "completed",
    ]);
    expect(heads(byState)[0]?.collapsed).toBe(true);
    expect(byState.rows).toBe(7);
    expect(byState.matched).toBe(8);
    expect(byState.items[1]?.kind).toBe("head");
  });

  it("walks the grouping modes", () => {
    expect(nextGroupBy("none")).toBe("project");
    expect(nextGroupBy("project")).toBe("host");
    expect(nextGroupBy("host")).toBe("state");
    expect(nextGroupBy("state")).toBe("none");
  });
});
