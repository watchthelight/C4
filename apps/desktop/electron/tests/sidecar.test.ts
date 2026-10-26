// <wtl> project: c4 | file: apps/desktop/electron/tests/sidecar.test.ts | author: watchthelight </wtl>
// @vitest-environment node
// The sidecar against fake-core.mjs, a stand-in that speaks the serve protocol.

import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { CoreError, PROTOCOL, Sidecar } from "../sidecar.js";
import type { CoreEvent, CoreState, Hello } from "../sidecar.js";

const FAKE = fileURLToPath(new URL("./fake-core.mjs", import.meta.url));
const cores: Sidecar[] = [];

function core(...args: string[]): Sidecar {
  const s = new Sidecar(process.execPath, [FAKE, ...args], { retryMs: [50, 100], stopWaitMs: 300 });
  cores.push(s);
  return s;
}

function hello(s: Sidecar): Promise<Hello> {
  return new Promise((resolve) => {
    s.once("hello", resolve);
  });
}

/** The next state event of the given kind. */
function nextState(s: Sidecar, want: CoreState["state"]): Promise<CoreState> {
  return new Promise((resolve) => {
    const on = (state: CoreState): void => {
      if (state.state === want) {
        s.off("state", on);
        resolve(state);
      }
    };
    s.on("state", on);
  });
}

afterEach(async () => {
  await Promise.all(cores.splice(0).map((s) => s.stop()));
});

describe("sidecar", () => {
  it("greets, answers by id in any order and relays events", async () => {
    const s = core();
    const states: CoreState[] = [];
    const events: CoreEvent[] = [];
    s.on("state", (state) => states.push(state));
    s.on("event", (event) => events.push(event));
    const greeted = hello(s);
    s.start();
    const h = await greeted;
    expect(h).toEqual({ protocol: PROTOCOL, schema: 1, version: "fake", pid: s.pid });
    expect(s.running).toEqual(h);
    expect(states).toEqual([
      { state: "starting", attempt: 1 },
      { state: "running", pid: h.pid, version: "fake", schema: 1 },
    ]);

    const first = s.invoke("echo", { n: 1 });
    const second = s.invoke("echo", { n: 2 });
    expect(await second).toEqual({ n: 2 });
    expect(await first).toEqual({ n: 1 });
    await expect(s.invoke("frobnicate", null)).rejects.toThrow(new CoreError('unknown method "frobnicate"'));

    await s.invoke("emit", { event: "sessions", data: { tick: 7 } });
    expect(events).toEqual([{ event: "sessions", data: { tick: 7 } }]);
    await s.invoke("garbage", null);
    expect(await s.invoke("echo", "still here")).toBe("still here");

    await s.stop();
    expect(s.running).toBeNull();
    expect(states.at(-1)).toEqual({ state: "stopped", code: 0, signal: null, error: null, retry_ms: null });
    expect(events).toHaveLength(1);
    await expect(s.invoke("echo", null)).rejects.toThrow("the core is not running");
  });

  it("holds a request until the core has greeted", async () => {
    const s = core();
    const waiting = s.whenRunning(2000).then((h) => s.invoke("echo", h.version));
    s.start();
    expect(await waiting).toBe("fake");
    expect(await s.whenRunning(1)).toEqual(s.running);

    const idle = core();
    await expect(idle.whenRunning(30)).rejects.toThrow("the core did not start within 30 ms");
  });

  it("lets a request sent during the stop reach the core", async () => {
    const s = core();
    s.start();
    await hello(s);
    const stopping = s.stop();
    const late = s.invoke("echo", "last words");
    expect(await late).toBe("last words");
    await stopping;
    expect(s.pid).toBeUndefined();
  });

  it("starts the core again after it exits and fails what was in flight", async () => {
    const s = core();
    const states: CoreState[] = [];
    s.on("state", (state) => states.push(state));
    s.start();
    const first = await hello(s);
    const again = hello(s);
    await expect(s.invoke("die", { code: 3 })).rejects.toThrow(/the core stopped before answering die \(exit 3\)/);
    expect(states.at(-1)).toEqual({ state: "stopped", code: 3, signal: null, error: null, retry_ms: 50 });
    const second = await again;
    expect(second.pid).not.toBe(first.pid);
    expect(states.filter((x) => x.state === "starting")).toEqual([
      { state: "starting", attempt: 1 },
      { state: "starting", attempt: 2 },
    ]);
    expect(await s.invoke("echo", "back")).toBe("back");
  });

  it("reports a core that cannot start and keeps trying", async () => {
    const s = new Sidecar("/no/such/c4-cli", ["serve"], { retryMs: [20], stopWaitMs: 100 });
    cores.push(s);
    const seen: CoreState[] = [];
    s.on("state", (state) => seen.push(state));
    const stopped = nextState(s, "stopped");
    s.start();
    const state = await stopped;
    if (state.state !== "stopped") {
      throw new Error("expected a stopped state");
    }
    expect(state.error).toMatch(/ENOENT/);
    expect(state.retry_ms).toBe(20);
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(seen.filter((x) => x.state === "starting").length).toBeGreaterThanOrEqual(2);
    await s.stop();
    expect(s.pid).toBeUndefined();
  });

  it("treats another protocol version as fatal", async () => {
    const s = core("--protocol", "2");
    const fatal = new Promise<string>((resolve) => {
      s.once("fatal", resolve);
    });
    const waiting = s.whenRunning(2000);
    s.start();
    expect(await fatal).toMatch(/speaks protocol 2 and this window speaks 1/);
    await expect(waiting).rejects.toThrow(/speaks protocol 2/);
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(s.running).toBeNull();
    expect(s.pid).toBeUndefined();
  });

  it("kills a core that ignores the end of stdin", async () => {
    const s = core("--linger");
    s.start();
    await hello(s);
    const before = Date.now();
    await s.stop();
    expect(Date.now() - before).toBeGreaterThanOrEqual(250);
    expect(s.pid).toBeUndefined();
  });
});
