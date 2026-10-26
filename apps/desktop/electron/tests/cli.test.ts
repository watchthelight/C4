// <wtl> project: c4 | file: apps/desktop/electron/tests/cli.test.ts | author: watchthelight </wtl>
// @vitest-environment node

import { describe, expect, it } from "vitest";
import { relayArgs } from "../cli.js";
import { fromPage } from "../ipc.js";
import { fileFor } from "../protocol.js";

describe("the command line rule", () => {
  it("opens the window when there is no command", () => {
    expect(relayArgs(["c4"], false)).toBeNull();
    expect(relayArgs(["electron", "."], true)).toBeNull();
    expect(relayArgs(["C4.app/Contents/MacOS/c4", "-psn_0_12345"], false)).toBeNull();
    expect(relayArgs(["c4", "--remote-debugging-port=9333"], false)).toBeNull();
    expect(relayArgs(["electron", ".", "--inspect=0", "--remote-debugging-pipe"], true)).toBeNull();
    expect(relayArgs(["c4", "--no-sandbox", "--enable-logging"], false)).toBeNull();
    expect(relayArgs(["c4", "--log", "debug"], false)).toBeNull();
    expect(relayArgs(["c4", "--log=debug", "--no-sandbox"], false)).toBeNull();
  });

  it("relays everything from the command on, as it is", () => {
    expect(relayArgs(["c4", "sessions", "--json"], false)).toEqual(["sessions", "--json"]);
    expect(relayArgs(["c4", "--version"], false)).toEqual(["--version"]);
    expect(relayArgs(["c4", "-h"], false)).toEqual(["-h"]);
    expect(relayArgs(["c4", "doctor", "--no-sandbox"], false)).toEqual(["doctor", "--no-sandbox"]);
    expect(relayArgs(["electron", ".", "doctor", "--no-commands"], true)).toEqual(["doctor", "--no-commands"]);
    expect(relayArgs(["c4", "install", "--home", "C:\\scratch\\claude"], false)).toEqual([
      "install",
      "--home",
      "C:\\scratch\\claude",
    ]);
  });

  it("drops the window's switches before the command and keeps the core's", () => {
    expect(relayArgs(["c4", "--no-sandbox", "doctor"], false)).toEqual(["doctor"]);
    expect(relayArgs(["c4", "--no-sandbox", "--version"], false)).toEqual(["--version"]);
    expect(relayArgs(["c4", "-psn_0_12345", "-h"], false)).toEqual(["-h"]);
    expect(relayArgs(["electron", ".", "--inspect=0", "sessions", "--json"], true)).toEqual(["sessions", "--json"]);
    expect(relayArgs(["c4", "--log", "debug", "sessions"], false)).toEqual(["--log", "debug", "sessions"]);
    expect(relayArgs(["c4", "--log=c4_discovery=trace", "doctor"], false)).toEqual([
      "--log=c4_discovery=trace",
      "doctor",
    ]);
    expect(relayArgs(["c4", "--no-sandbox", "--log", "debug", "--enable-logging", "doctor"], false)).toEqual([
      "--log",
      "debug",
      "doctor",
    ]);
  });
});

describe("the page check", () => {
  it("accepts the app's own scheme and the dev server, nothing else", () => {
    const origins = ["c4://app", "http://127.0.0.1:1420"];
    expect(fromPage("c4://app/index.html", origins)).toBe(true);
    expect(fromPage("c4://app", origins)).toBe(true);
    expect(fromPage("http://127.0.0.1:1420/", origins)).toBe(true);
    expect(fromPage("http://127.0.0.1:14200/", origins)).toBe(false);
    expect(fromPage("c4://apps/index.html", origins)).toBe(false);
    expect(fromPage("https://example.com/c4://app/", origins)).toBe(false);
    expect(fromPage(undefined, origins)).toBe(false);
  });
});

describe("the file a request names", () => {
  const root = process.platform === "win32" ? "C:\\app\\dist" : "/app/dist";
  const sep = process.platform === "win32" ? "\\" : "/";

  it("serves index.html for the root and files under the directory", () => {
    expect(fileFor(root, "/")).toBe(`${root}${sep}index.html`);
    expect(fileFor(root, "")).toBe(`${root}${sep}index.html`);
    expect(fileFor(root, "/assets/index-abc.js")).toBe(`${root}${sep}assets${sep}index-abc.js`);
    expect(fileFor(root, "/assets/Space%20Mono.woff2")).toBe(`${root}${sep}assets${sep}Space Mono.woff2`);
  });

  it("refuses a path that leaves the directory or cannot be decoded", () => {
    expect(fileFor(root, "/../secrets.json")).toBeNull();
    expect(fileFor(root, "/assets/../../x")).toBeNull();
    expect(fileFor(root, "/..%2F..%2Fx")).toBeNull();
    expect(fileFor(root, "/%E0%A4%A")).toBeNull();
  });
});
