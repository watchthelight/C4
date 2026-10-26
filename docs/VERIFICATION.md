<!-- <wtl> project: c4 | file: docs/VERIFICATION.md | author: watchthelight </wtl> -->

# Verification

The directive names thirteen tests, A to M. This page maps each one to what runs in CI and what has
to be run by a person, and records results as they happen. "Verified" means the automated test exists
and passes; "manual" means someone ran the checklist on real hardware; "unverified" means neither.

## Tests A to M

| Test | What it checks | Automation | Status |
|---|---|---|---|
| A | sessions that were running before C4 started appear | three stand-in sessions started before `c4 sessions --json` (`crates/c4-testkit/tests/discovery.rs`, `crates/c4-cli/tests/cli.rs`) | verified, headless; the desktop app and real terminals are manual item 1 |
| B | a session started after C4 appears within 2 s | a stand-in started after the first discovery pass, and the Python stand-in through the binary | verified, headless |
| C | state changes show live | fake session flips its registry status on a timeline | planned (Milestone 2) |
| D | subagents and background agents are listed with their totals | subagent transcript files appear mid-run | planned (Milestone 2, full in 5) |
| E | sidebar interactions | vitest components plus Playwright on the mock bridge | planned (Milestone 2) |
| F | token numbers match the status line | the status line's own parser run against the same transcript | planned (Milestone 4) |
| G | the REA toolkit can be assigned and reverted | scratch settings file, byte-identical revert | planned (Milestone 6) |
| H | a managed terminal is a real pty | real shell, echo, interrupt, terminate | planned (Milestone 3) |
| I | a session that cannot be attached is Observed with no input affordance | registry entry with an unmatched host | planned (Milestone 2) |
| J | organization and layouts survive a restart | store round trip plus a relaunch smoke test | planned (Milestone 3) |
| K | performance targets | criterion benches, `c4 perf`, a 40-session load generator | planned (Milestone 7) |
| L | releases are published with checksums and attestations | `scripts/verify_release.py` after each tag | planned (Milestone 2) |
| M | an existing Claude Code setup keeps working after install | scratch home seeded with a full settings file, then the agents installer twice | planned (Milestone 2) |

## Platform status

"Built in CI" covers the desktop app; the crates build as part of the tests on every runner.

| Platform | Built in CI | Tests in CI | Run on hardware |
|---|---|---|---|
| Windows x64 | crates only | `cargo test --workspace` green (windows-latest) | `c4 sessions` and `c4 doctor` against a real home, see Results |
| macOS arm64 | crates only | `cargo test --workspace` green (macos-latest) | no hardware in reach |
| macOS x64 | crates only | `cargo test --workspace` green (macos-15-intel) | no hardware in reach |
| Linux x64 | crates only | `cargo test --workspace` green (ubuntu-latest) | not yet |

## Manual checklist

Each item names the test it supports. Record the date, the C4 version, the Claude Code version and the
result below the list when you run it.

1. (A) Start `c4` while two interactive sessions run in Windows Terminal: both rows appear within 2 s
   with name, cwd, host "Windows Terminal", mode Observed and the state from the registry. A stale
   background job whose supervisor is gone shows Disconnected, not blocked.
2. (B, I) Open a new `claude` in WezTerm: it appears within 2 s as Interactive Attached with its pane
   id; "send message" delivers to a throwaway session only.
3. (F, M) After `c4 install`: the sidebar context percentage equals the status line's meter in the same
   terminal; the status line's cache files and ledger keep updating; `c4 uninstall` restores
   settings.json byte for byte apart from the removed keys; the agents installer's dry run reports no
   change beyond statusLine.
4. (H, J) Start a managed `claude` in a scratch directory, type, interrupt with ESC, close the C4
   window, reopen: the session is still there with its scrollback.
5. (K) Run the load generator with 40 sessions at 3,000 events per minute and `c4 perf`; record
   discovery time, hook-to-UI p95, FPS, idle CPU and RSS.
6. (L) After each tag: `scripts/verify_release.py <tag>`, then `sha256sum -c SHA256SUMS` and
   `gh attestation verify` on a downloaded asset.
7. (G) In a scratch project: Suggest shows REA with its reason, Prepare shows the exact diffs and
   backup paths, Apply writes them, the session shows restart required, Revert restores.

## Results

- 2026-10-25, C4 0.0.1, Claude Code 2.1.299, Windows 11 Pro, Windows Terminal. Headless part of
  item 1 (A): `c4 sessions` against the real Claude home listed both live interactive sessions as
  Observed in Windows Terminal with their registry states, a daemon-backed background session, and
  a background job whose supervisor had died as disconnected rather than blocked. 611 ms including
  the session list command. `c4 doctor` found the home, the `claude.cmd` shim on PATH (version in
  80 ms) and three entries from `claude agents --json` in 527 ms. The desktop app part of item 1 is
  still open.
- 2026-10-25, the verify workflow for commit 561dc2c: `cargo test --workspace` green on
  windows-latest, ubuntu-latest, macos-latest and macos-15-intel. Tests A and B are in that suite.
