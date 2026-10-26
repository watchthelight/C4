// <wtl> project: c4 | file: apps/desktop/electron/sidecar.ts | author: watchthelight </wtl>
// The core as a child process: `c4-cli serve`, one line of JSON each way, as
// crates/c4-cli/src/serve/mod.rs describes it. A request gets a number for its id and a promise
// the matching answer settles; event lines go out to whoever listens. A core that exits on its
// own starts again after a pause that grows while it keeps failing; a core that speaks another
// protocol version is fatal.

import { spawn } from "node:child_process";
import type { ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { createInterface } from "node:readline";
import { setTimeout as sleep } from "node:timers/promises";
import { log } from "./log.js";

/** The protocol version this shell speaks; the core names its own in the hello line. */
export const PROTOCOL = 1;
/** How long a stop waits for the core to leave on its own once stdin is closed. */
export const STOP_WAIT_MS = 2000;
/** A stop first lets a request the window sent in its last moments land and be answered. */
export const STOP_GRACE_MS = 50;
/** A core that lived this long before exiting starts again after the shortest pause. */
export const STEADY_MS = 60_000;
/** The pauses before each start after an exit, the last one repeated. */
export const RETRY_MS: readonly number[] = [1000, 2000, 4000, 8000, 16_000, 30_000];

export interface Hello {
  protocol: number;
  schema: number;
  version: string;
  pid: number;
}

/** A line the core sends on its own. */
export interface CoreEvent {
  event: string;
  data: unknown;
}

/** What the shell knows about the child, sent to the windows as the `core` event. */
export type CoreState =
  | { state: "starting"; attempt: number }
  | { state: "running"; pid: number; version: string; schema: number }
  | {
      state: "stopped";
      code: number | null;
      signal: string | null;
      error: string | null;
      /** The pause before the next start; null when none is coming. */
      retry_ms: number | null;
    };

/** What the core answered with `error`, or why it could not answer. */
export class CoreError extends Error {}

export interface SidecarOptions {
  retryMs?: readonly number[];
  stopWaitMs?: number;
}

interface Pending {
  method: string;
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
}

interface Events {
  hello: [Hello];
  event: [CoreEvent];
  state: [CoreState];
  fatal: [string];
}

export class Sidecar extends EventEmitter<Events> {
  private child: ChildProcess | null = null;
  private hello: Hello | null = null;
  private readonly pending = new Map<number, Pending>();
  private nextId = 1;
  private attempt = 0;
  private startedAt = 0;
  private retry: NodeJS.Timeout | null = null;
  private stopping = false;
  private readonly retryMs: readonly number[];
  private readonly stopWaitMs: number;

  constructor(
    private readonly exe: string,
    private readonly args: readonly string[] = ["serve"],
    options: SidecarOptions = {},
  ) {
    super();
    this.retryMs = options.retryMs ?? RETRY_MS;
    this.stopWaitMs = options.stopWaitMs ?? STOP_WAIT_MS;
  }

  /** The hello line of the running core, or null before it arrives and after an exit. */
  get running(): Hello | null {
    return this.hello;
  }

  /** The child's pid while it runs. */
  get pid(): number | undefined {
    return this.child?.pid;
  }

  /** Start the child. Nothing happens while one runs or after `stop`. */
  start(): void {
    if (this.child !== null || this.stopping) {
      return;
    }
    this.attempt += 1;
    this.emit("state", { state: "starting", attempt: this.attempt });
    log("info", "starting the core", { exe: this.exe, args: this.args, attempt: this.attempt });
    const child = spawn(this.exe, this.args, { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
    this.child = child;
    this.startedAt = Date.now();
    // spawn reports a missing executable through `error`; `exit` may or may not follow it
    child.on("error", (error) => {
      this.ended(child, null, null, error.message);
    });
    child.on("exit", (code, signal) => {
      this.ended(child, code, signal, null);
    });
    child.stdin?.on("error", (error) => {
      log("warn", "writing to the core failed", { error: error.message });
    });
    if (child.stdout !== null) {
      createInterface({ input: child.stdout, crlfDelay: Infinity }).on("line", (line) => {
        if (this.child === child) {
          this.took(line);
        }
      });
    }
    if (child.stderr !== null) {
      createInterface({ input: child.stderr, crlfDelay: Infinity }).on("line", (line) => {
        if (line.trim() !== "") {
          process.stderr.write(`core: ${line}\n`);
        }
      });
    }
  }

  /** Resolves once the core has greeted; rejects on a fatal core or when `timeoutMs` passes first. */
  whenRunning(timeoutMs: number): Promise<Hello> {
    if (this.hello !== null) {
      return Promise.resolve(this.hello);
    }
    if (this.stopping) {
      return Promise.reject(new CoreError("the core is stopping"));
    }
    return new Promise((resolve, reject) => {
      const done = (): void => {
        clearTimeout(timer);
        this.off("hello", onHello);
        this.off("fatal", onFatal);
      };
      const onHello = (hello: Hello): void => {
        done();
        resolve(hello);
      };
      const onFatal = (why: string): void => {
        done();
        reject(new CoreError(why));
      };
      const timer = setTimeout(() => {
        done();
        reject(new CoreError(`the core did not start within ${timeoutMs} ms`));
      }, timeoutMs);
      this.on("hello", onHello);
      this.on("fatal", onFatal);
    });
  }

  /** Send a request; resolves with the `ok` value, rejects with the `error` text. */
  invoke(method: string, params: unknown): Promise<unknown> {
    const child = this.child;
    if (child === null || child.stdin === null || !child.stdin.writable) {
      return Promise.reject(new CoreError("the core is not running"));
    }
    const stdin = child.stdin;
    const id = this.nextId;
    this.nextId += 1;
    const line = `${JSON.stringify({ id, method, params })}\n`;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { method, resolve, reject });
      stdin.write(line, (error) => {
        if (error && this.pending.delete(id)) {
          reject(new CoreError(`could not send ${method} to the core: ${error.message}`));
        }
      });
    });
  }

  /** Close stdin, which ends the core, and wait for it; kill it when it lingers. */
  async stop(): Promise<void> {
    this.stopping = true;
    if (this.retry !== null) {
      clearTimeout(this.retry);
      this.retry = null;
    }
    const child = this.child;
    if (child === null) {
      return;
    }
    const gone = new Promise<void>((resolve) => {
      child.once("exit", () => {
        resolve();
      });
      child.once("error", () => {
        resolve();
      });
    });
    // a request the window sent in its last moments (the preferences it flushes on pagehide) may
    // still be on its way: let it land and be answered before the end of stdin ends the core
    const deadline = Date.now() + this.stopWaitMs;
    await sleep(STOP_GRACE_MS);
    while (this.pending.size > 0 && this.child === child && Date.now() < deadline) {
      await sleep(10);
    }
    if (this.child !== child) {
      return;
    }
    child.stdin?.end();
    const timer = setTimeout(() => {
      log("warn", "the core did not stop in time; killing it", { pid: child.pid });
      child.kill();
    }, this.stopWaitMs);
    await gone;
    clearTimeout(timer);
  }

  private ended(
    child: ChildProcess,
    code: number | null,
    signal: NodeJS.Signals | null,
    error: string | null,
  ): void {
    if (this.child !== child) {
      return;
    }
    this.child = null;
    this.hello = null;
    const why = error ?? (signal !== null ? `signal ${signal}` : `exit ${code ?? "?"}`);
    for (const p of this.pending.values()) {
      p.reject(new CoreError(`the core stopped before answering ${p.method} (${why})`));
    }
    this.pending.clear();
    if (this.stopping) {
      this.emit("state", { state: "stopped", code, signal, error, retry_ms: null });
      return;
    }
    if (Date.now() - this.startedAt >= STEADY_MS) {
      this.attempt = 0;
    }
    const last = this.retryMs[this.retryMs.length - 1] ?? STOP_WAIT_MS;
    const wait = this.retryMs[Math.max(0, this.attempt - 1)] ?? last;
    log("warn", "the core exited; starting it again", { code, signal, error, retry_ms: wait });
    this.emit("state", { state: "stopped", code, signal, error, retry_ms: wait });
    this.retry = setTimeout(() => {
      this.retry = null;
      this.start();
    }, wait);
  }

  private took(line: string): void {
    let message: unknown;
    try {
      message = JSON.parse(line);
    } catch {
      log("warn", "a line from the core is not JSON", { line: line.slice(0, 200) });
      return;
    }
    if (typeof message !== "object" || message === null || Array.isArray(message)) {
      log("warn", "a line from the core is not an object", { line: line.slice(0, 200) });
      return;
    }
    const m = message as Record<string, unknown>;
    if (typeof m.event === "string") {
      if (m.event === "hello") {
        this.greeted(m.data);
      } else {
        this.emit("event", { event: m.event, data: m.data });
      }
      return;
    }
    if (typeof m.id === "number") {
      const p = this.pending.get(m.id);
      if (p === undefined) {
        log("warn", "an answer for no request", { id: m.id });
        return;
      }
      this.pending.delete(m.id);
      if (Object.hasOwn(m, "ok")) {
        p.resolve(m.ok);
      } else {
        p.reject(new CoreError(typeof m.error === "string" ? m.error : "an answer with neither ok nor error"));
      }
      return;
    }
    // a line the core could not read as a request carries a null id; this shell sends only JSON
    log("warn", "the core answered a request this shell never sent", {
      id: m.id ?? null,
      error: m.error ?? null,
    });
  }

  private greeted(data: unknown): void {
    const d = (typeof data === "object" && data !== null ? data : {}) as Record<string, unknown>;
    const hello: Hello = {
      protocol: typeof d.protocol === "number" ? d.protocol : -1,
      schema: typeof d.schema === "number" ? d.schema : -1,
      version: typeof d.version === "string" ? d.version : "?",
      pid: typeof d.pid === "number" ? d.pid : -1,
    };
    if (hello.protocol !== PROTOCOL) {
      const why = `${this.exe} speaks protocol ${hello.protocol} and this window speaks ${PROTOCOL}`;
      log("error", why);
      this.emit("fatal", why);
      void this.stop();
      return;
    }
    this.hello = hello;
    log("info", "the core is up", { pid: hello.pid, version: hello.version, schema: hello.schema });
    this.emit("hello", hello);
    this.emit("state", { state: "running", pid: hello.pid, version: hello.version, schema: hello.schema });
  }
}
