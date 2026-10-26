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
| I | a session that cannot be attached is Observed with no input affordance | registry entry with an unmatched host; the window renders no input control for any session (`apps/desktop/tests/list.test.ts`) | in part: the window side is verified, the host match lands with the adapters (Milestone 3) |
| J | organization and layouts survive a restart | store round trip plus a relaunch smoke test | planned (Milestone 3) |
| K | performance targets | criterion benches, `c4 perf`, a 40-session load generator | planned (Milestone 7) |
| L | releases are published with checksums and attestations | `scripts/verify_release.py` after each tag | planned (Milestone 2) |
| M | an existing Claude Code setup keeps working after install | scratch home seeded with `fixtures/settings/full.json`; `c4 install`, the agents repo's installer twice (`python -I install/install.py --home <scratch>`; the repo is checked out at a pinned commit in CI, `~/src/agents` or `C4_AGENTS_REPO` locally), `c4 install` again, `c4 uninstall` (`crates/c4-cli/tests/install.rs`) | verified, headless, on every runner; the test skips with a notice where the agents repo is not reachable; the real home is manual item 3 |

## Platform status

"Built in CI" covers the desktop app: `pnpm -C apps/desktop tauri build --debug --no-bundle`, which
links the window into the executable. The crates build as part of the tests on every runner. Nothing
in CI starts the app yet; a smoke test that does is planned.

| Platform | Built in CI | Tests in CI | Run on hardware |
|---|---|---|---|
| Windows x64 | debug app, verify run 40276837007 | `cargo test --workspace` and `pnpm -r test` green (windows-latest) | `c4 sessions`, `c4 doctor`, `c4 install --dry-run` and the window against a real home, see Results |
| macOS arm64 | debug app, verify run 40276837007 | `cargo test --workspace` and `pnpm -r test` green (macos-latest) | no hardware in reach |
| macOS x64 | debug app, verify run 40276837007 | `cargo test --workspace` and `pnpm -r test` green (macos-15-intel) | no hardware in reach |
| Linux x64 | debug app, verify run 40276837007 | `cargo test --workspace` and `pnpm -r test` green (ubuntu-latest) | not yet |

## Manual checklist

Each item names the test it supports. Record the date, the C4 version, the Claude Code version and the
result below the list when you run it.

1. (A) Start `c4` while two interactive sessions run in Windows Terminal: both rows appear within 2 s
   with name, cwd, host "Windows Terminal", mode Observed and the state from the registry. A stale
   background job whose supervisor is gone shows Disconnected, not blocked.
2. (B, I) Open a new `claude` in WezTerm: it appears within 2 s as Interactive Attached with its pane
   id; "send message" delivers to a throwaway session only.
3. (F, M) `c4 install --dry-run` first, then `c4 install`: the sidebar context percentage equals the
   status line's meter in the same terminal; the status line's cache files and ledger keep updating;
   `c4 uninstall` restores settings.json byte for byte when nothing else has written it since, and
   otherwise removes C4's entries alone; the agents installer's dry run reports no change beyond
   statusLine.
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
- 2026-10-25, C4 0.0.1 (debug build of the desktop shell), Claude Code 2.1.299, Windows 11 Pro.
  Desktop part of item 1 (A): `c4` with no arguments opened its window and listed the four sessions
  discovery sees in the real Claude home: the two interactive sessions in Windows Terminal (observed,
  working), a daemon-backed background session (observed, idle) and the background job whose
  supervisor had died (disconnected). The first pass took 37 ms; the session list command's three
  entries arrived on the same pass. `c4 sessions --json --files-only` from the same executable
  returned the same four sessions; the window closed with exit code 0 and nothing on stderr. Checked
  by screenshot, not by a test; the Playwright suite arrives in Milestone 2.
- 2026-10-26, the verify workflow for commit 923519b (run 40257450521): the lint job and the four
  test jobs green. Each runner built the debug app with `tauri build --debug --no-bundle` after
  `cargo test --workspace` and `pnpm -r test`. The app was built, not started, on every runner.
- 2026-10-26, the verify workflow for commits 1cf88c6 and 08d7953 (run 40271184171): the lint job
  and the four test jobs green, the ingest server's tests included on both macOS runners after the
  `getpeereid` fix of 1cf88c6.
- 2026-10-26, C4 0.0.1 (debug `c4-cli`), Claude Code 2.1.299, Windows 11 Pro. The dry-run half of
  item 3 (M): `c4-cli install --dry-run` against the real Claude home named the shim it would copy to
  `~/.c4/bin/`, the wrapped status line command (the original `PYTHONIOENCODING=utf-8 python
  ~/.claude/statusline.py` with `~/.c4/bin/c4-shim.exe statusline --` in front of the program), the
  17 hook groups it would add, the backup it would take and the manifest it would write, restorable.
  `c4-cli uninstall --dry-run` reported nothing of C4's in the file, no shim copy and no manifest.
  Afterwards `~/.c4/install-manifest.json` did not exist and `settings.json` had its previous size
  and modification time, with no new backup beside it. The real install waits for the build in which
  the app starts the ingest server, so that nothing installed sits unread.
- 2026-10-26, the verify workflow for commit 3238e80 (run 40276837007): the lint job and the four
  test jobs green, with the agents repo checked out at its pinned commit for test M on every runner.
- 2026-10-26, C4 0.0.1 (debug `c4-cli`), Claude Code 2.1.299, Windows 11 Pro. `c4-cli doctor`
  against the real Claude home, before any install: the home's files (two registry records, the
  roster, one job), `claude.cmd` on PATH with its version in 81 ms, three entries from the session
  list command in 543 ms, the process table answering for the command's own pid; then C4's own
  parts: no C4 home, no shim copy, the sidecar beside the executable present, no manifest,
  `settings.json` present with 0 of 17 hook groups and the status line not wrapped, the ingest
  endpoint free, the spool empty, and the one note that `c4 install` has not run. Afterwards
  `~/.c4/install-manifest.json` did not exist and `settings.json` had its previous size and
  modification time.
