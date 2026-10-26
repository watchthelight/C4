// <wtl> project: c4 | file: apps/desktop/src/format.ts | author: watchthelight </wtl>
// Labels and text for the session views. Every string here mirrors the Rust side (c4-core's
// state.rs, ids.rs, evidence.rs and view.rs); fixtures/labels.json is the contract both sides are
// tested against, so the window never shows a label the command line would contradict.

import type {
  Attachment,
  DisconnectReason,
  HostKind,
  InputReason,
  SessionState,
  SessionView,
  Source,
  StateKind,
} from "./bridge/types";

/** The nine states in default attention order, highest first. */
export const STATE_ORDER: readonly StateKind[] = [
  "waiting_permission",
  "waiting_user_input",
  "failed",
  "disconnected",
  "running_tool",
  "background_agents_active",
  "working",
  "idle",
  "completed",
];

const STATE_LABEL: Record<StateKind, string> = {
  waiting_permission: "waiting for permission",
  waiting_user_input: "waiting for input",
  failed: "failed",
  disconnected: "disconnected",
  running_tool: "running a tool",
  background_agents_active: "background agents active",
  working: "working",
  idle: "idle",
  completed: "completed",
};

const STATE_WEIGHT: Record<StateKind, number> = {
  waiting_permission: 100,
  waiting_user_input: 90,
  failed: 80,
  disconnected: 70,
  running_tool: 40,
  background_agents_active: 35,
  working: 30,
  idle: 10,
  completed: 0,
};

const INPUT_REASON: Record<InputReason, string> = {
  question: "question",
  elicitation: "elicitation",
  agent_needs_input: "agent needs input",
  auth: "sign-in",
  dialog: "dialog",
  other: "input",
};

const DISCONNECT_REASON: Record<DisconnectReason, string> = {
  process_gone: "process exited",
  pid_reused: "pid reused",
  registry_gone: "registry entry gone",
  host_gone: "terminal pane gone",
  host_exited: "session host exited",
};

const HOST: Record<HostKind, string> = {
  c4: "C4",
  wez_term: "WezTerm",
  tmux: "tmux",
  i_term2: "iTerm2",
  kitty: "kitty",
  windows_terminal: "Windows Terminal",
  terminal_app: "Terminal.app",
  vs_code: "VS Code",
  gnome_terminal: "GNOME Terminal",
  alacritty: "Alacritty",
  conhost: "console",
  unknown: "unknown host",
};

const ATTACHMENT: Record<Attachment, string> = {
  managed: "managed",
  interactive_attached: "attached",
  observed: "observed",
  disconnected: "disconnected",
};

export function stateLabel(kind: StateKind): string {
  return STATE_LABEL[kind];
}

export function stateWeight(kind: StateKind): number {
  return STATE_WEIGHT[kind];
}

/** The states that get the attention stripe. */
export function needsAttention(kind: StateKind): boolean {
  return kind === "waiting_permission" || kind === "waiting_user_input" || kind === "failed";
}

/** False for the two states that mean no live process. */
export function isLive(kind: StateKind): boolean {
  return kind !== "disconnected" && kind !== "completed";
}

export function inputReasonLabel(reason: InputReason): string {
  return INPUT_REASON[reason];
}

export function disconnectReasonLabel(reason: DisconnectReason): string {
  return DISCONNECT_REASON[reason];
}

export function hostLabel(host: HostKind): string {
  return HOST[host];
}

export function attachmentLabel(attachment: Attachment): string {
  return ATTACHMENT[attachment];
}

/** The sidebar label with the payload folded in, as `SessionState::describe` in Rust. */
export function describeState(s: SessionState): string {
  switch (s.state) {
    case "running_tool":
      return `running ${s.tool}`;
    case "waiting_permission":
      return s.what === null ? STATE_LABEL[s.state] : `waiting for permission: ${s.what}`;
    case "waiting_user_input": {
      const reason = INPUT_REASON[s.reason];
      return s.detail ? `waiting for input: ${reason} (${s.detail})` : `waiting for input: ${reason}`;
    }
    case "background_agents_active":
      return `${s.count} background agent${s.count === 1 ? "" : "s"} active`;
    case "failed":
      return `failed: ${s.error}`;
    case "disconnected":
      return `disconnected: ${DISCONNECT_REASON[s.reason]}`;
    case "completed":
      return `completed (${s.reason})`;
    case "working":
    case "idle":
      return STATE_LABEL[s.state];
  }
}

/** A short name for where a fact came from, as `Source::label` in Rust. */
export function sourceLabel(s: Source): string {
  switch (s.source) {
    case "registry":
      return "registry";
    case "agents_json":
      return "claude agents";
    case "hook":
      return `hook ${s.detail}`;
    case "statusline":
      return "status line";
    case "transcript":
      return "transcript";
    case "osc7501":
      return "OSC 7501";
    case "osc21337":
      return "OSC 21337";
    case "process":
      return "process table";
    case "daemon":
      return "background supervisor";
    case "host":
      return HOST[s.detail];
    case "managed":
      return "managed host";
    case "timer":
      return "timer";
  }
}

/** The 8-character prefix the status line and the ledger use. */
export function shortId(id: string): string {
  return id.slice(0, 8);
}

/** The last path segment of a working directory, with either separator. */
export function cwdTail(cwd: string | null): string | null {
  if (cwd === null) {
    return null;
  }
  const trimmed = cwd.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/);
  const tail = parts[parts.length - 1];
  return tail ? tail : null;
}

/** Claude Code's name, else the cwd tail, else the short id, as `SessionView::display_name`. */
export function displayName(v: Pick<SessionView, "id" | "name" | "cwd">): string {
  const name = v.name?.trim();
  if (name) {
    return name;
  }
  return cwdTail(v.cwd) ?? shortId(v.id);
}

/** A duration for the sidebar: "12s", "3m 04s", "1h 12m", "2d 3h". Negative or odd input reads 0s. */
export function elapsed(ms: number): string {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0;
  if (total < 60) {
    return `${total}s`;
  }
  const minutes = Math.floor(total / 60);
  if (minutes < 60) {
    return `${minutes}m ${String(total % 60).padStart(2, "0")}s`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 48) {
    return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`;
  }
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

/** "1 session", "3 sessions". */
export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

/** The attention order the core uses: weight down, the longest wait first, then the latest activity. */
export function attentionOrder<T extends Pick<SessionView, "state" | "waiting_since" | "last_activity">>(
  views: readonly T[],
): T[] {
  return [...views].sort((a, b) => {
    const weight = stateWeight(b.state.state) - stateWeight(a.state.state);
    if (weight !== 0) {
      return weight;
    }
    const wa = a.waiting_since ?? Number.MAX_SAFE_INTEGER;
    const wb = b.waiting_since ?? Number.MAX_SAFE_INTEGER;
    if (wa !== wb) {
      return wa - wb;
    }
    return b.last_activity - a.last_activity;
  });
}
