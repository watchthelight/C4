// <wtl> project: c4 | file: apps/desktop/tests/electron.test.ts | author: watchthelight </wtl>

import { afterEach, describe, expect, it, vi } from "vitest";
import { selectBridge } from "../src/bridge";
import { electronBridge, insideElectron } from "../src/bridge/electron";
import type { C4Window, CorePayload } from "../src/bridge/electron";

/** A `window.c4` that records what it was asked and lets a test push event lines. */
function fake(answer: (method: string, params: unknown) => unknown): C4Window & {
  calls: [string, unknown][];
  push(payload: CorePayload): void;
  listeners: number;
} {
  const listeners = new Set<(payload: CorePayload) => void>();
  const calls: [string, unknown][] = [];
  return {
    calls,
    get listeners() {
      return listeners.size;
    },
    push: (payload) => {
      for (const l of listeners) {
        l(payload);
      }
    },
    invoke: (method, params) => {
      calls.push([method, params]);
      try {
        return Promise.resolve(answer(method, params));
      } catch (error) {
        return Promise.reject(error);
      }
    },
    shell: (request) => {
      calls.push(["shell", request]);
      return Promise.resolve({ sidebar: "attached" });
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

describe("electron bridge", () => {
  it("is chosen when window.c4 exists and not otherwise", () => {
    expect(insideElectron()).toBe(false);
    expect(selectBridge().kind).toBe("mock");
    window.c4 = fake(() => null);
    expect(insideElectron()).toBe(true);
    expect(selectBridge().kind).toBe("electron");
    vi.stubEnv("VITE_C4_BRIDGE", "mock");
    expect(selectBridge().kind).toBe("mock");
  });

  it("names the four methods, passes prefs_set its section and value, and never asks the shell", async () => {
    const c4 = fake((method) => ({ method }));
    const bridge = electronBridge(c4);
    await bridge.listSessions();
    await bridge.about();
    await bridge.prefsGet();
    await bridge.prefsSet("sidebar", { width: 300 });
    expect(c4.calls).toEqual([
      ["list_sessions", undefined],
      ["about", undefined],
      ["prefs_get", undefined],
      ["prefs_set", { section: "sidebar", value: { width: 300 } }],
    ]);
  });

  it("hands sessions events to the listener and stops on the returned function", async () => {
    const c4 = fake(() => null);
    const bridge = electronBridge(c4);
    const seen: unknown[] = [];
    const off = await bridge.onSessions((s) => seen.push(s));
    expect(c4.listeners).toBe(1);
    c4.push({ event: "core", data: { state: "running" } });
    c4.push({ event: "sessions", data: { tick: 3 } });
    expect(seen).toEqual([{ tick: 3 }]);
    off();
    expect(c4.listeners).toBe(0);
    c4.push({ event: "sessions", data: { tick: 4 } });
    expect(seen).toHaveLength(1);
  });

  it("strips Electron's wrapper from an error and keeps the core's words", async () => {
    const c4 = fake(() => {
      throw new Error("Error invoking remote method 'c4:invoke': Error: \"schema\" is not a section name");
    });
    const bridge = electronBridge(c4);
    await expect(bridge.prefsSet("schema", 2)).rejects.toThrow('"schema" is not a section name');
    const plain = electronBridge(fake(() => Promise.reject(new Error("the core is not running"))));
    await expect(plain.about()).rejects.toThrow("the core is not running");
  });

  it("refuses to build without window.c4", () => {
    expect(() => electronBridge(undefined)).toThrow("window.c4 is missing");
  });
});
