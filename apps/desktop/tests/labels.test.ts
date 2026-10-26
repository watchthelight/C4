// <wtl> project: c4 | file: apps/desktop/tests/labels.test.ts | author: watchthelight </wtl>
// The TypeScript side of the label contract in fixtures/labels.json. crates/c4-core/tests/labels.rs
// checks the Rust side against the same file.

import { describe, expect, it } from "vitest";
import raw from "../../../fixtures/labels.json";
import type {
  Attachment,
  DisconnectReason,
  HostKind,
  InputReason,
  SessionState,
  Source,
  StateKind,
} from "../src/bridge/types";
import {
  STATE_ORDER,
  attachmentLabel,
  describeState,
  disconnectReasonLabel,
  hostLabel,
  inputReasonLabel,
  isLive,
  needsAttention,
  sourceLabel,
  stateLabel,
  stateWeight,
} from "../src/format";

interface Contract {
  states: Record<StateKind, { label: string; weight: number; attention: boolean; live: boolean }>;
  input_reasons: Record<InputReason, string>;
  disconnect_reasons: Record<DisconnectReason, string>;
  hosts: Record<HostKind, string>;
  attachments: Record<Attachment, string>;
  sources: { source: Source; label: string }[];
  describe: { state: SessionState; text: string }[];
}

const contract = raw as unknown as Contract;

function entries<K extends string, V>(record: Record<K, V>): [K, V][] {
  return Object.entries(record) as [K, V][];
}

describe("labels contract", () => {
  it("names every state with its weight, attention rule and liveness", () => {
    expect(Object.keys(contract.states).sort()).toEqual([...STATE_ORDER].sort());
    for (const [kind, entry] of entries(contract.states)) {
      expect(stateLabel(kind), kind).toBe(entry.label);
      expect(stateWeight(kind), kind).toBe(entry.weight);
      expect(needsAttention(kind), kind).toBe(entry.attention);
      expect(isLive(kind), kind).toBe(entry.live);
    }
    const weights = STATE_ORDER.map(stateWeight);
    expect(weights).toEqual([...weights].sort((a, b) => b - a));
  });

  it("names the reasons, hosts and attachments", () => {
    expect(entries(contract.input_reasons)).toHaveLength(6);
    for (const [reason, label] of entries(contract.input_reasons)) {
      expect(inputReasonLabel(reason)).toBe(label);
    }
    expect(entries(contract.disconnect_reasons)).toHaveLength(5);
    for (const [reason, label] of entries(contract.disconnect_reasons)) {
      expect(disconnectReasonLabel(reason)).toBe(label);
    }
    expect(entries(contract.hosts)).toHaveLength(12);
    for (const [host, label] of entries(contract.hosts)) {
      expect(hostLabel(host)).toBe(label);
    }
    expect(entries(contract.attachments)).toHaveLength(4);
    for (const [attachment, label] of entries(contract.attachments)) {
      expect(attachmentLabel(attachment)).toBe(label);
    }
  });

  it("describes states and sources the way the core does", () => {
    expect(contract.describe.length).toBeGreaterThanOrEqual(12);
    for (const c of contract.describe) {
      expect(describeState(c.state), JSON.stringify(c.state)).toBe(c.text);
    }
    expect(contract.sources).toHaveLength(12);
    for (const c of contract.sources) {
      expect(sourceLabel(c.source), JSON.stringify(c.source)).toBe(c.label);
    }
  });
});
