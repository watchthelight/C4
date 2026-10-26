// <wtl> project: c4 | file: apps/desktop/tests/chrome.test.ts | author: watchthelight </wtl>
// The status strip's ingest phrase: what it says in each server state, and that the strip carries
// it from the snapshot.

import { describe, expect, it } from "vitest";
import { createMock } from "../src/bridge/mock";
import type { IngestReport } from "../src/bridge/types";
import { createStrip, ingestText } from "../src/chrome";

const NOW = Date.UTC(2026, 9, 25, 12, 0, 0);

function report(over: Partial<IngestReport>): IngestReport {
  return {
    home: "/home/example/.c4",
    home_source: "synthetic",
    endpoint: "/home/example/.c4/ingest.sock",
    state: "listening",
    detail: null,
    since: NOW - 60_000,
    connections: 0,
    refused: 0,
    lines: 0,
    bad_lines: 0,
    oversize: 0,
    idle_closed: 0,
    replayed_files: 0,
    replayed_lines: 0,
    replay_bad_lines: 0,
    received: 0,
    dropped: 0,
    queued: 0,
    folded: 0,
    subagent_lines: 0,
    last_line_at: null,
    ...over,
  };
}

describe("ingest text", () => {
  it("counts the lines while listening and names the spool and the drops", () => {
    expect(ingestText(report({}), NOW)).toBe("ingest: 0 lines");
    expect(ingestText(report({ received: 1, last_line_at: NOW - 4_000 }), NOW)).toBe("ingest: 1 line, last 4s ago");
    expect(
      ingestText(report({ received: 12, replayed_lines: 3, dropped: 2, last_line_at: NOW - 90_000 }), NOW),
    ).toBe("ingest: 12 lines, 3 from the spool, last 1m 30s ago, 2 dropped");
  });

  it("says when another C4 holds the endpoint, or why the server could not start", () => {
    expect(ingestText(report({ state: "taken", detail: "another process listens on /x" }), NOW)).toBe(
      "ingest: another C4 holds this home's endpoint",
    );
    expect(ingestText(report({ state: "error", detail: "could not listen on /x: denied" }), NOW)).toBe(
      "ingest: could not listen on /x: denied",
    );
    expect(ingestText(report({ state: "error" }), NOW)).toBe("ingest: could not start");
    expect(ingestText(report({ state: "starting" }), NOW)).toBe("ingest: starting");
    expect(ingestText(report({ state: "stopped" }), NOW)).toBe("ingest: stopped");
  });
});

describe("status strip", () => {
  it("carries the ingest phrase from the snapshot", async () => {
    const bridge = createMock({ period: 0, now: () => NOW });
    const strip = createStrip(bridge.kind);
    strip.update(null, null, NOW);
    expect(strip.el.textContent).toContain("waiting for the first pass");
    strip.update(await bridge.listSessions(), await bridge.about(), NOW);
    expect(strip.el.textContent).toContain("ingest: 0 lines");
    const one = bridge.advance();
    strip.update(one, await bridge.about(), NOW);
    expect(strip.el.textContent).toContain("ingest: 2 lines, last 0s ago");
    expect(strip.el.textContent).toContain("bridge: mock");
  });
});
