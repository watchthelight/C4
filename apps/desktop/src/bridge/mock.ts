// <wtl> project: c4 | file: apps/desktop/src/bridge/mock.ts | author: watchthelight </wtl>
// A synthetic core for the browser and the tests: eight invented sessions with fake paths, the same
// command and event names as the real bridge, and a scripted state rotation so the window can be
// watched changing. Nothing here comes from a real Claude home.

import { attentionOrder } from "../format";
import type {
  AgentsRun,
  Bridge,
  Evidence,
  SessionState,
  SessionView,
  Snapshot,
  SnapshotReport,
  Source,
  TransitionOut,
} from "./types";

export interface MockOptions {
  /** Milliseconds between scripted passes once a listener is attached; 0 means advance by hand. */
  period?: number;
  /** The clock, for deterministic tests. */
  now?: () => number;
}

export interface MockBridge extends Bridge {
  /** Run one scripted pass and notify the listeners. */
  advance(): Snapshot;
  /** The pass counter. */
  readonly tick: number;
}

const MINUTE = 60_000;

function observed(source: Source, at: number): Evidence {
  return { source, certainty: "observed", at };
}

function heuristic(source: Source, at: number): Evidence {
  return { source, certainty: "heuristic", at };
}

function hook(name: string, at: number): Evidence {
  return observed({ source: "hook", detail: name }, at);
}

type Seed = Partial<SessionView> & Pick<SessionView, "id" | "state" | "evidence">;

function view(now: number, seed: Seed): SessionView {
  const base: SessionView = {
    id: seed.id,
    session_kind: "interactive",
    process: null,
    alive: true,
    cwd: null,
    name: null,
    name_source: null,
    version: "2.1.299",
    host: "unknown",
    attachment: "observed",
    pane: null,
    state: seed.state,
    evidence: seed.evidence,
    coarse: true,
    hooks_seen: false,
    osc_seen: false,
    stalled: false,
    current_tool: null,
    agents: [],
    background_tasks: [],
    pending_background_agents: 0,
    pending_workflows: 0,
    daemon_short: null,
    daemon_in_flight: 0,
    daemon_detail: null,
    compactions: 0,
    first_seen: now - 40 * MINUTE,
    started_at: now - 45 * MINUTE,
    last_activity: now - MINUTE,
    state_since: now - MINUTE,
    waiting_since: null,
    ended: false,
    peer_socket: null,
    transcript_path: null,
    seq: 1,
  };
  return { ...base, ...seed };
}

/** The invented sessions at a given moment. */
export function syntheticSessions(now: number): SessionView[] {
  return [
    view(now, {
      id: "0a1b2c3d-0001-4000-8000-000000000001",
      name: "alpha build",
      name_source: "user",
      cwd: "C:\\Users\\example\\src\\alpha",
      host: "windows_terminal",
      process: { pid: 41001, proc_start: 133_900_000_000_000_000 },
      state: { state: "working" },
      evidence: observed({ source: "registry" }, now - 20_000),
      state_since: now - 20_000,
      last_activity: now - 20_000,
      transcript_path: "C:\\Users\\example\\.claude\\projects\\C--Users-example-src-alpha\\0a1b2c3d-0001-4000-8000-000000000001.jsonl",
      peer_socket: "\\\\.\\pipe\\LOCAL\\cc-msg-example",
    }),
    view(now, {
      id: "0a1b2c3d-0001-4000-8000-000000000002",
      name: "beta",
      name_source: "auto",
      cwd: "/home/example/proj/beta",
      host: "wez_term",
      pane: "7",
      process: { pid: 41002, proc_start: 1_792_826_400 },
      state: { state: "waiting_permission", what: "Bash" },
      evidence: hook("Notification", now - 3 * MINUTE),
      coarse: false,
      hooks_seen: true,
      state_since: now - 3 * MINUTE,
      waiting_since: now - 3 * MINUTE,
      last_activity: now - 3 * MINUTE,
    }),
    view(now, {
      id: "0a1b2c3d-0001-4000-8000-000000000003",
      cwd: "/home/example/proj/docs",
      host: "tmux",
      pane: "%3",
      process: { pid: 41003, proc_start: 1_792_826_500 },
      state: { state: "running_tool", tool: "Read" },
      evidence: hook("PreToolUse", now - 4_000),
      coarse: false,
      hooks_seen: true,
      current_tool: { name: "Read", tool_use_id: "toolu_synthetic_0001", since: now - 4_000 },
      state_since: now - 4_000,
      last_activity: now - 4_000,
      compactions: 1,
    }),
    view(now, {
      id: "0a1b2c3d-0001-4000-8000-000000000004",
      cwd: "/home/example/proj/gamma",
      host: "vs_code",
      process: { pid: 41004, proc_start: 1_792_826_600 },
      state: { state: "idle" },
      evidence: observed({ source: "registry" }, now - 12 * MINUTE),
      state_since: now - 12 * MINUTE,
      last_activity: now - 12 * MINUTE,
    }),
    view(now, {
      id: "0b2c3d4e-0002-4000-8000-000000000011",
      session_kind: "background",
      name: "nightly",
      name_source: "user",
      cwd: "/home/example/proj/nightly",
      process: { pid: 41011, proc_start: 1_792_827_500 },
      state: { state: "background_agents_active", count: 2 },
      evidence: observed({ source: "daemon" }, now - 2 * MINUTE),
      daemon_short: "0b2c3d4e",
      daemon_in_flight: 2,
      daemon_detail: "two fan-out workers running",
      agents: [
        {
          id: "agent-synthetic-01",
          agent_type: "Explore",
          description: "find the config readers",
          transcript_path: null,
          started_at: now - 6 * MINUTE,
          ended_at: null,
          observed_running: true,
        },
        {
          id: "agent-synthetic-02",
          agent_type: "Plan",
          description: "plan the migration",
          transcript_path: null,
          started_at: now - 5 * MINUTE,
          ended_at: null,
          observed_running: true,
        },
      ],
      state_since: now - 2 * MINUTE,
      last_activity: now - 2 * MINUTE,
    }),
    view(now, {
      id: "0c3d4e5f-0002-4000-8000-000000000012",
      session_kind: "background",
      name: "old worker",
      name_source: "user",
      cwd: "/home/example/proj/old",
      alive: false,
      attachment: "disconnected",
      process: { pid: 41012, proc_start: 1_792_820_000 },
      state: { state: "disconnected", reason: "process_gone" },
      evidence: observed({ source: "process" }, now - 50 * MINUTE),
      daemon_short: "0c3d4e5f",
      daemon_detail: "blocked, supervisor gone",
      state_since: now - 50 * MINUTE,
      last_activity: now - 50 * MINUTE,
    }),
    view(now, {
      id: "0a1b2c3d-0001-4000-8000-000000000007",
      cwd: "C:\\Users\\example\\scratch",
      host: "windows_terminal",
      process: { pid: 41007, proc_start: 133_900_000_100_000_000 },
      state: { state: "failed", error: "rate_limit" },
      evidence: hook("StopFailure", now - 7 * MINUTE),
      coarse: false,
      hooks_seen: true,
      state_since: now - 7 * MINUTE,
      waiting_since: now - 7 * MINUTE,
      last_activity: now - 7 * MINUTE,
    }),
    view(now, {
      id: "0a1b2c3d-0001-4000-8000-000000000008",
      name: "review",
      name_source: "derived",
      cwd: "/home/example/proj/review",
      host: "kitty",
      alive: false,
      attachment: "disconnected",
      process: { pid: 41008, proc_start: 1_792_826_900 },
      state: { state: "completed", reason: "logout" },
      evidence: heuristic({ source: "timer" }, now - 30 * MINUTE),
      ended: true,
      state_since: now - 30 * MINUTE,
      last_activity: now - 30 * MINUTE,
    }),
  ];
}

const ALPHA = "0a1b2c3d-0001-4000-8000-000000000001";
const GAMMA = "0a1b2c3d-0001-4000-8000-000000000004";

/** What alpha cycles through, one step per pass. */
const ALPHA_SCRIPT: readonly { state: SessionState; evidence: (at: number) => Evidence }[] = [
  { state: { state: "running_tool", tool: "Bash" }, evidence: (at) => hook("PreToolUse", at) },
  { state: { state: "working" }, evidence: (at) => hook("PostToolUse", at) },
  { state: { state: "idle" }, evidence: (at) => observed({ source: "registry" }, at) },
  { state: { state: "working" }, evidence: (at) => observed({ source: "registry" }, at) },
];

export function createMock(options: MockOptions = {}): MockBridge {
  const now = options.now ?? Date.now;
  const period = options.period ?? 2000;
  const listeners = new Set<(snapshot: Snapshot) => void>();
  let timer: ReturnType<typeof setInterval> | null = null;
  let tick = 0;
  let sessions = syntheticSessions(now());
  let agents: AgentsRun | null = null;

  function report(): SnapshotReport {
    return {
      registry_entries: 5,
      registry_skipped: 2,
      agents_entries: null,
      agents_error: null,
      roster_workers: 2,
      jobs: 2,
      transcripts: 5,
      probed: 7,
      alive: sessions.filter((s) => s.alive).length,
      problems: [],
      elapsed_ms: 11,
    };
  }

  function snapshot(transitions: TransitionOut[]): Snapshot {
    return {
      schema: 1,
      at: now(),
      home: "/home/example/.claude",
      sessions: attentionOrder(sessions),
      report: report(),
      tick,
      agents,
      transitions,
    };
  }

  function change(id: string, state: SessionState, evidence: Evidence, at: number): TransitionOut | null {
    const current = sessions.find((s) => s.id === id);
    if (!current) {
      return null;
    }
    const from = current.state;
    const attention =
      state.state === "waiting_permission" || state.state === "waiting_user_input" || state.state === "failed";
    const next: SessionView = {
      ...current,
      state,
      evidence,
      state_since: at,
      last_activity: at,
      waiting_since: attention ? at : null,
      current_tool: state.state === "running_tool" ? { name: state.tool, tool_use_id: null, since: at } : null,
      seq: current.seq + 1,
    };
    sessions = sessions.map((s) => (s.id === id ? next : s));
    return { session: id, from, to: state, describe: "", evidence };
  }

  function advance(): Snapshot {
    tick += 1;
    const at = now();
    const transitions: TransitionOut[] = [];
    const step = ALPHA_SCRIPT[(tick - 1) % ALPHA_SCRIPT.length];
    if (step) {
      const t = change(ALPHA, step.state, step.evidence(at), at);
      if (t) {
        transitions.push(t);
      }
    }
    if (tick % 3 === 0) {
      const gamma = sessions.find((s) => s.id === GAMMA);
      const waiting = gamma?.state.state === "waiting_user_input";
      const t = waiting
        ? change(GAMMA, { state: "idle" }, hook("Stop", at), at)
        : change(GAMMA, { state: "waiting_user_input", reason: "question", detail: null }, hook("Notification", at), at);
      if (t) {
        transitions.push(t);
      }
    }
    if (tick % 8 === 1) {
      agents = { at, entries: 3, error: null };
    }
    const snap = snapshot(transitions);
    for (const listener of listeners) {
      listener(snap);
    }
    return snap;
  }

  return {
    kind: "mock",
    get tick() {
      return tick;
    },
    advance,
    listSessions: () => Promise.resolve(snapshot([])),
    about: () =>
      Promise.resolve({
        version: "0.0.1",
        home: "/home/example/.claude",
        home_source: "synthetic",
        refresh_ms: period,
        agents_every: 8,
        os: "mock",
      }),
    onSessions: (listener) => {
      listeners.add(listener);
      if (period > 0 && timer === null) {
        timer = setInterval(advance, period);
      }
      return Promise.resolve(() => {
        listeners.delete(listener);
        if (listeners.size === 0 && timer !== null) {
          clearInterval(timer);
          timer = null;
        }
      });
    },
  };
}
