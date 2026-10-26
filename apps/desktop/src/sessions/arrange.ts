// <wtl> project: c4 | file: apps/desktop/src/sessions/arrange.ts | author: watchthelight </wtl>
// What the sidebar shows and in which order: the filter (plain words and `key:value` tokens), the
// attention sort with pinned sessions first, and the grouping by project, host or state with a
// header per group. Pure functions over the session views; the list only lays out the items this
// returns, so every rule about order lives here and is tested without a DOM.

import { attachmentLabel, compareAttention, displayName, hostLabel, needsAttention, stateLabel } from "../format";
import type { SessionView } from "../bridge/types";

export type GroupBy = "none" | "project" | "host" | "state";

export const GROUP_BYS: readonly GroupBy[] = ["none", "project", "host", "state"];

/** The grouping after `g` in the cycle the `g` key walks. */
export function nextGroupBy(g: GroupBy): GroupBy {
  const i = GROUP_BYS.indexOf(g);
  return GROUP_BYS[(i + 1) % GROUP_BYS.length] ?? "none";
}

export type QueryKey = "state" | "project" | "tool" | "mode" | "host";

const QUERY_KEYS: readonly QueryKey[] = ["state", "project", "tool", "mode", "host"];

function isQueryKey(s: string): s is QueryKey {
  return (QUERY_KEYS as readonly string[]).includes(s);
}

export interface Query {
  /** Plain words, lowercased; each must appear in the name, the project or the cwd. */
  words: string[];
  /** Field tokens by key; a session matches a key when any of the key's values matches. */
  fields: Partial<Record<QueryKey, string[]>>;
}

/**
 * Split filter text into words and `key:value` tokens. A known key with nothing after the colon
 * (still being typed) constrains nothing; an unknown key is an ordinary word. Everything is matched
 * lowercase.
 */
export function parseQuery(text: string): Query {
  const words: string[] = [];
  const fields: Partial<Record<QueryKey, string[]>> = {};
  for (const raw of text.trim().split(/\s+/)) {
    if (raw === "") {
      continue;
    }
    const word = raw.toLowerCase();
    const colon = word.indexOf(":");
    const key = colon === -1 ? null : word.slice(0, colon);
    if (key !== null && isQueryKey(key)) {
      const value = word.slice(colon + 1);
      if (value !== "") {
        (fields[key] ??= []).push(value);
      }
      continue;
    }
    words.push(word);
  }
  return { words, fields };
}

function lower(s: string): string {
  return s.toLowerCase();
}

/** The state and host kinds use underscores; the filter sees them as spaces. */
function spaced(s: string): string {
  return s.replace(/_/g, " ");
}

function has(haystack: readonly string[], needle: string): boolean {
  return haystack.some((h) => h.includes(needle));
}

/** The project a cwd names: its last folder, or the last `depth` folders joined with a slash. */
export function projectTitle(cwd: string | null, depth = 1): string {
  if (cwd === null) {
    return "no cwd";
  }
  const parts = cwd.split(/[\\/]/).filter((p) => p !== "");
  if (parts.length === 0) {
    return "no cwd";
  }
  return parts.slice(-depth).join("/");
}

/** What one field token is matched against, lowercased. */
function fieldText(v: SessionView, key: QueryKey): string[] {
  switch (key) {
    case "state":
      return [spaced(v.state.state), stateLabel(v.state.state)].map(lower);
    case "project":
      return [projectTitle(v.cwd), v.cwd ?? ""].map(lower);
    case "tool": {
      const tool = v.state.state === "running_tool" ? v.state.tool : v.current_tool?.name;
      return tool ? [lower(tool)] : [];
    }
    case "mode":
      return [spaced(v.attachment), attachmentLabel(v.attachment)].map(lower);
    case "host":
      return [spaced(v.host), hostLabel(v.host)].map(lower);
  }
}

/** Words all have to match; within a key any value may; across keys all have to. */
export function matchesQuery(v: SessionView, q: Query): boolean {
  if (q.words.length > 0) {
    const text = [displayName(v), projectTitle(v.cwd), v.cwd ?? ""].map(lower);
    if (!q.words.every((w) => has(text, w))) {
      return false;
    }
  }
  for (const key of QUERY_KEYS) {
    const values = q.fields[key];
    if (!values || values.length === 0) {
      continue;
    }
    const hay = fieldText(v, key);
    if (!values.some((value) => has(hay, value))) {
      return false;
    }
  }
  return true;
}

export interface RowItem {
  kind: "row";
  key: string;
  id: string;
  view: SessionView;
  pinned: boolean;
  /** The key of the header above this row; null when the list is flat. */
  head: string | null;
}

export interface HeadItem {
  kind: "head";
  key: string;
  /** What a collapsed set stores: `<groupBy>:<group key>`. */
  group: string;
  title: string;
  /** 1-based position among the groups. */
  index: number;
  count: number;
  /** Members in a state that needs the user. */
  attention: number;
  collapsed: boolean;
}

export type Item = RowItem | HeadItem;

export interface ArrangeOptions {
  groupBy: GroupBy;
  query: Query;
  pinned: ReadonlySet<string>;
  collapsed: ReadonlySet<string>;
}

export interface Arranged {
  items: Item[];
  /** Sessions that passed the filter. */
  matched: number;
  /** Rows among the items; a collapsed group hides its rows. */
  rows: number;
}

export function rowKey(id: string): string {
  return `r:${id}`;
}

function headKey(group: string): string {
  return `g:${group}`;
}

/** The group id of a session under a grouping, as the collapsed set stores it; null when flat. */
export function groupOf(groupBy: GroupBy, v: SessionView): string | null {
  switch (groupBy) {
    case "none":
      return null;
    case "project":
      return `project:${v.cwd ?? ""}`;
    case "host":
      return `host:${v.host}`;
    case "state":
      return `state:${v.state.state}`;
  }
}

function groupTitle(groupBy: GroupBy, v: SessionView): string {
  switch (groupBy) {
    case "none":
      return "";
    case "project":
      return projectTitle(v.cwd);
    case "host":
      return hostLabel(v.host);
    case "state":
      return stateLabel(v.state.state);
  }
}

interface Bucket {
  title: string;
  first: SessionView;
  members: SessionView[];
}

function row(v: SessionView, pinned: boolean, head: string | null): RowItem {
  return { kind: "row", key: rowKey(v.id), id: v.id, view: v, pinned, head };
}

/**
 * Filter, sort and group the views. Rows sort by pinned first, then the core's attention order.
 * Groups come in the order of their first row, so the group holding the most urgent session leads;
 * two projects whose folders share a name get their parent folder in front.
 */
export function arrange(views: readonly SessionView[], o: ArrangeOptions): Arranged {
  const matched = views.filter((v) => matchesQuery(v, o.query));
  const sorted = [...matched].sort((a, b) => {
    const pa = o.pinned.has(a.id) ? 1 : 0;
    const pb = o.pinned.has(b.id) ? 1 : 0;
    return pb - pa || compareAttention(a, b);
  });
  if (o.groupBy === "none") {
    return {
      items: sorted.map((v) => row(v, o.pinned.has(v.id), null)),
      matched: matched.length,
      rows: sorted.length,
    };
  }
  const buckets = new Map<string, Bucket>();
  for (const v of sorted) {
    const group = groupOf(o.groupBy, v) ?? "";
    const bucket = buckets.get(group);
    if (bucket) {
      bucket.members.push(v);
    } else {
      buckets.set(group, { title: groupTitle(o.groupBy, v), first: v, members: [v] });
    }
  }
  if (o.groupBy === "project") {
    const seen = new Map<string, number>();
    for (const b of buckets.values()) {
      seen.set(b.title, (seen.get(b.title) ?? 0) + 1);
    }
    for (const b of buckets.values()) {
      if ((seen.get(b.title) ?? 0) > 1) {
        b.title = projectTitle(b.first.cwd, 2);
      }
    }
  }
  const items: Item[] = [];
  let rows = 0;
  let index = 0;
  for (const [group, bucket] of buckets) {
    index += 1;
    const key = headKey(group);
    const collapsed = o.collapsed.has(group);
    items.push({
      kind: "head",
      key,
      group,
      title: bucket.title,
      index,
      count: bucket.members.length,
      attention: bucket.members.filter((m) => needsAttention(m.state.state)).length,
      collapsed,
    });
    if (!collapsed) {
      for (const v of bucket.members) {
        items.push(row(v, o.pinned.has(v.id), key));
        rows += 1;
      }
    }
  }
  return { items, matched: matched.length, rows };
}
