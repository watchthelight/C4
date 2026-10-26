// <wtl> project: c4 | file: apps/desktop/tests/shell.test.ts | author: watchthelight </wtl>
// The page's side of the shell: which window the query string names, the shell's words and how
// they are read, the real shell over a stand-in `window.c4`, the stand-in shell with its placement
// in memory and its opener and closer, and which of the two the page picks.

import { afterEach, describe, expect, it, vi } from "vitest";
import type { C4Window, CorePayload } from "../src/bridge/electron";
import { electronShell, mockShell, selectShell, shellMessage, windowRole } from "../src/shell";
import type { ShellMessage } from "../src/shell";

/** A `window.c4` whose shell channel answers with `answer` and records what it was asked. */
function fake(answer: (request: unknown) => unknown): C4Window & {
  requests: unknown[];
  push(payload: CorePayload): void;
} {
  const listeners = new Set<(payload: CorePayload) => void>();
  const requests: unknown[] = [];
  return {
    requests,
    push: (payload) => {
      for (const l of listeners) {
        l(payload);
      }
    },
    invoke: () => Promise.resolve(null),
    shell: (request) => {
      requests.push(request);
      try {
        return Promise.resolve(answer(request));
      } catch (error) {
        return Promise.reject(error);
      }
    },
    on: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    platform: "test",
    versions: { electron: "45.0.0", chrome: "156.0", node: "24.0.0" },
  };
}

afterEach(() => {
  delete window.c4;
  vi.unstubAllEnvs();
});

describe("windowRole", () => {
  it("names the sidebar window from the query string and the main window otherwise", () => {
    expect(windowRole("")).toBe("main");
    expect(windowRole("?window=sidebar")).toBe("sidebar");
    expect(windowRole("?x=1&window=sidebar")).toBe("sidebar");
    expect(windowRole("?window=main")).toBe("main");
    expect(windowRole("?window=")).toBe("main");
  });
});

describe("shellMessage", () => {
  it("reads the four kinds and refuses the rest", () => {
    expect(shellMessage({ kind: "windows", sidebar: "detached" })).toEqual({ kind: "windows", sidebar: "detached" });
    expect(shellMessage({ kind: "windows", sidebar: "attached" })).toEqual({ kind: "windows", sidebar: "attached" });
    expect(shellMessage({ kind: "windows", sidebar: "gone" })).toBeNull();
    expect(shellMessage({ kind: "windows" })).toBeNull();
    expect(shellMessage({ kind: "select", id: "abc" })).toEqual({ kind: "select", id: "abc" });
    expect(shellMessage({ kind: "select", id: null })).toEqual({ kind: "select", id: null });
    expect(shellMessage({ kind: "select", id: 7 })).toBeNull();
    expect(shellMessage({ kind: "prefs", doc: { schema: 1 } })).toEqual({ kind: "prefs", doc: { schema: 1 } });
    expect(shellMessage({ kind: "prefs", doc: "x" })).toBeNull();
    expect(shellMessage({ kind: "focus" })).toEqual({ kind: "focus" });
    expect(shellMessage({ kind: "other" })).toBeNull();
    expect(shellMessage(null)).toBeNull();
    expect(shellMessage([1])).toBeNull();
    expect(shellMessage("focus")).toBeNull();
  });
});

describe("the shell inside the app", () => {
  it("asks on the shell channel and reads the placement from the answer", async () => {
    const c4 = fake((request) => {
      const r = request as { kind: string; action?: string };
      return { sidebar: r.action === "detach" ? "detached" : "attached" };
    });
    const shell = electronShell(c4, "main");
    expect(shell.kind).toBe("electron");
    expect(shell.role).toBe("main");
    expect(await shell.windows()).toBe("attached");
    expect(await shell.sidebar("detach")).toBe("detached");
    expect(await shell.sidebar("attach")).toBe("attached");
    expect(await shell.sidebar("focus")).toBe("attached");
    shell.select("abc");
    shell.select(null);
    expect(c4.requests).toEqual([
      { kind: "windows" },
      { kind: "sidebar", action: "detach" },
      { kind: "sidebar", action: "attach" },
      { kind: "sidebar", action: "focus" },
      { kind: "select", id: "abc" },
      { kind: "select", id: null },
    ]);
  });

  it("hands the shell's words to the listener, read, and nothing else", () => {
    const c4 = fake(() => ({ sidebar: "attached" }));
    const shell = electronShell(c4, "sidebar");
    const seen: ShellMessage[] = [];
    const off = shell.onMessage((m) => seen.push(m));
    c4.push({ event: "sessions", data: { tick: 1 } });
    c4.push({ event: "shell", data: { kind: "select", id: "abc" } });
    c4.push({ event: "shell", data: { kind: "nothing" } });
    c4.push({ event: "shell", data: { kind: "focus" } });
    expect(seen).toEqual([
      { kind: "select", id: "abc" },
      { kind: "focus" },
    ]);
    off();
    c4.push({ event: "shell", data: { kind: "focus" } });
    expect(seen).toHaveLength(2);
  });

  it("rejects an answer without a placement and keeps the main process's words from an error", async () => {
    const empty = electronShell(
      fake(() => ({})),
      "main",
    );
    await expect(empty.windows()).rejects.toThrow("did not say where the sidebar is");
    const failing = electronShell(
      fake(() => {
        throw new Error("Error invoking remote method 'c4:shell': Error: not a request the shell takes");
      }),
      "main",
    );
    await expect(failing.sidebar("detach")).rejects.toThrow("not a request the shell takes");
    expect(() => electronShell(undefined, "main")).toThrow("window.c4 is missing");
  });

  it("lets a selection go without caring whether it arrived", async () => {
    const shell = electronShell(
      fake(() => Promise.reject(new Error("gone"))),
      "main",
    );
    shell.select("abc");
    await new Promise((resolve) => {
      setTimeout(resolve, 0);
    });
  });
});

describe("the stand-in shell", () => {
  it("keeps the placement, opens the sidebar once and closes it on attach", async () => {
    let opened = 0;
    let closed = 0;
    const shell = mockShell("main", {
      opener: () => {
        opened += 1;
        return {
          close: () => {
            closed += 1;
          },
        };
      },
    });
    const seen: ShellMessage[] = [];
    shell.onMessage((m) => seen.push(m));
    expect(shell.kind).toBe("mock");
    expect(shell.role).toBe("main");
    expect(await shell.windows()).toBe("attached");
    expect(await shell.sidebar("detach")).toBe("detached");
    expect(await shell.sidebar("detach")).toBe("detached");
    expect(opened).toBe(1);
    expect(await shell.sidebar("focus")).toBe("detached");
    expect(await shell.sidebar("attach")).toBe("attached");
    expect(closed).toBe(1);
    expect(await shell.sidebar("attach")).toBe("attached");
    expect(closed).toBe(1);
    expect(seen).toEqual([
      { kind: "windows", sidebar: "detached" },
      { kind: "windows", sidebar: "attached" },
    ]);
  });

  it("in the sidebar's own window starts detached and closes the window on attach", async () => {
    let closed = 0;
    const shell = mockShell("sidebar", {
      closer: () => {
        closed += 1;
      },
    });
    expect(await shell.windows()).toBe("detached");
    expect(await shell.sidebar("attach")).toBe("attached");
    expect(closed).toBe(1);
  });
});

describe("selectShell", () => {
  it("picks the real shell inside the app and the stand-in elsewhere", () => {
    expect(selectShell("main").kind).toBe("mock");
    window.c4 = fake(() => ({ sidebar: "attached" }));
    expect(selectShell("main").kind).toBe("electron");
    expect(selectShell("sidebar").role).toBe("sidebar");
    vi.stubEnv("VITE_C4_BRIDGE", "mock");
    expect(selectShell("main").kind).toBe("mock");
  });
});
