// <wtl> project: c4 | file: apps/desktop/src/bridge/types.ts | author: watchthelight </wtl>
// The JSON the core sends, field for field. The Rust definitions are the source of truth:
// crates/c4-core/src/{state,ids,evidence,observation,view}.rs and crates/c4-discovery/src/snapshot.rs.
// Timestamps are milliseconds since the Unix epoch.

export type StateKind =
  | "waiting_permission"
  | "waiting_user_input"
  | "failed"
  | "disconnected"
  | "running_tool"
  | "background_agents_active"
  | "working"
  | "idle"
  | "completed";

export type InputReason = "question" | "elicitation" | "agent_needs_input" | "auth" | "dialog" | "other";

export type DisconnectReason = "process_gone" | "pid_reused" | "registry_gone" | "host_gone" | "host_exited";

export type SessionState =
  | { state: "working" }
  | { state: "running_tool"; tool: string }
  | { state: "waiting_permission"; what: string | null }
  | { state: "waiting_user_input"; reason: InputReason; detail: string | null }
  | { state: "background_agents_active"; count: number }
  | { state: "idle" }
  | { state: "failed"; error: string }
  | { state: "disconnected"; reason: DisconnectReason }
  | { state: "completed"; reason: string };

export type HostKind =
  | "c4"
  | "wez_term"
  | "tmux"
  | "i_term2"
  | "kitty"
  | "windows_terminal"
  | "terminal_app"
  | "vs_code"
  | "gnome_terminal"
  | "alacritty"
  | "conhost"
  | "unknown";

export type Attachment = "managed" | "interactive_attached" | "observed" | "disconnected";

export type SessionKind = "interactive" | "background";

export type Certainty = "observed" | "heuristic";

export type Source =
  | { source: "registry" }
  | { source: "agents_json" }
  | { source: "hook"; detail: string }
  | { source: "statusline" }
  | { source: "transcript" }
  | { source: "osc7501" }
  | { source: "osc21337" }
  | { source: "process" }
  | { source: "daemon" }
  | { source: "host"; detail: HostKind }
  | { source: "managed" }
  | { source: "timer" };

export interface Evidence {
  source: Source;
  certainty: Certainty;
  at: number;
}

export interface ProcessRef {
  pid: number;
  proc_start: number | null;
}

export interface ToolInfo {
  name: string;
  tool_use_id: string | null;
  since: number;
}

export interface AgentInfo {
  id: string;
  agent_type: string | null;
  description: string | null;
  transcript_path: string | null;
  started_at: number;
  ended_at: number | null;
  observed_running: boolean;
}

export interface TaskInfo {
  id: string;
  kind: string;
  status: string;
  description: string | null;
  created_at: number;
  completed_at: number | null;
}

export interface SessionView {
  id: string;
  session_kind: SessionKind;
  process: ProcessRef | null;
  alive: boolean;
  cwd: string | null;
  name: string | null;
  name_source: string | null;
  version: string | null;
  host: HostKind;
  attachment: Attachment;
  pane: string | null;
  state: SessionState;
  evidence: Evidence;
  coarse: boolean;
  hooks_seen: boolean;
  osc_seen: boolean;
  stalled: boolean;
  current_tool: ToolInfo | null;
  agents: AgentInfo[];
  background_tasks: TaskInfo[];
  pending_background_agents: number;
  pending_workflows: number;
  daemon_short: string | null;
  daemon_in_flight: number;
  daemon_detail: string | null;
  compactions: number;
  first_seen: number;
  started_at: number | null;
  last_activity: number;
  state_since: number;
  waiting_since: number | null;
  ended: boolean;
  peer_socket: string | null;
  transcript_path: string | null;
  seq: number;
}

export interface Problem {
  path: string;
  what: string;
}

export interface SnapshotReport {
  registry_entries: number;
  registry_skipped: number;
  agents_entries: number | null;
  agents_error: string | null;
  roster_workers: number;
  jobs: number;
  transcripts: number;
  probed: number;
  alive: number;
  problems: Problem[];
  elapsed_ms: number;
}

/** The last run of the session list command, as the app reports it. */
export interface AgentsRun {
  at: number;
  entries: number | null;
  error: string | null;
}

/** A state change of one pass. */
export interface TransitionOut {
  session: string;
  from: SessionState;
  to: SessionState;
  describe: string;
  evidence: Evidence;
}

/** What `c4 sessions --json` prints; the app adds the last three fields. */
export interface Snapshot {
  schema: number;
  at: number;
  home: string;
  sessions: SessionView[];
  report: SnapshotReport;
  tick?: number;
  agents?: AgentsRun | null;
  transitions?: TransitionOut[];
}

export interface About {
  version: string;
  home: string;
  home_source: string;
  refresh_ms: number;
  agents_every: number;
  os: string;
}

/** What the window needs from the core, however it is reached. */
export interface Bridge {
  readonly kind: "tauri" | "mock";
  /** One files-only pass, then the snapshot. */
  listSessions(): Promise<Snapshot>;
  /** Called after every refresh. Resolves to the function that stops the calls. */
  onSessions(listener: (snapshot: Snapshot) => void): Promise<() => void>;
  about(): Promise<About>;
}
