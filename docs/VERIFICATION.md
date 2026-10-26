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
| C | state changes show live | a hook line delivered through the shim's own code moves a session in the window's document (`apps/desktop/src-tauri/src/ingest.rs`); a stand-in that flips its registry status on a timeline and the Playwright side come with later Milestone 2 commits | in part: the hook path is verified, headless, on every runner, and seen in the window against a live session on Windows (Results, item 3) |
| D | subagents and background agents are listed with their totals | subagent transcript files appear mid-run | planned (Milestone 2, full in 5) |
| E | sidebar interactions | vitest components plus Playwright on the mock bridge | in part: vitest covers the virtual list (`apps/desktop/tests/list.test.ts`: the rows near the viewport, node reuse while scrolling, the three densities, keyboard selection, the reorder rule, headers that fold, the filter, pins, keyboard travel over headers and a row that arrives under its header while the order is held), the order rules (`apps/desktop/tests/arrange.test.ts`: the filter grammar, the attention sort with pins first, grouping by project, host and state) the filter bar (`apps/desktop/tests/filterbar.test.ts`) and the hover card (`apps/desktop/tests/hovercard.test.ts`: what the card says about a view, where it goes, the 350 ms rest and what closes it); the detached window and the Playwright side come with later Milestone 2 commits |
| F | token numbers match the status line | the status line's own parser run against the same transcript | planned (Milestone 4) |
| G | the REA toolkit can be assigned and reverted | scratch settings file, byte-identical revert | planned (Milestone 6) |
| H | a managed terminal is a real pty | real shell, echo, interrupt, terminate | planned (Milestone 3) |
| I | a session that cannot be attached is Observed with no input affordance | registry entry with an unmatched host; the window renders no input control for any session (`apps/desktop/tests/list.test.ts`) | in part: the window side is verified, the host match lands with the adapters (Milestone 3) |
| J | organization and layouts survive a restart | store round trip plus a relaunch smoke test | planned (Milestone 3) |
| K | performance targets | criterion benches, `c4 perf`, a 40-session load generator | planned (Milestone 7) |
| L | releases are published with checksums and attestations | `scripts/verify_release.py` after each tag | planned (Milestone 2) |
| M | an existing Claude Code setup keeps working after install | scratch home seeded with `fixtures/settings/full.json`; `c4 install`, the agents repo's installer twice (`python -I install/install.py --home <scratch>`; the repo is checked out at a pinned commit in CI, `~/src/agents` or `C4_AGENTS_REPO` locally), `c4 install` again, `c4 uninstall` (`crates/c4-cli/tests/install.rs`) | verified, headless, on every runner; the test skips with a notice where the agents repo is not reachable; run against the real home on Windows (Results, item 3) |

## Platform status

"Built in CI" covers the desktop app: `pnpm -C apps/desktop tauri build --debug --no-bundle`, which
links the window into the executable. The crates build as part of the tests on every runner. Nothing
in CI starts the app yet; a smoke test that does is planned.

| Platform | Built in CI | Tests in CI | Run on hardware |
|---|---|---|---|
| Windows x64 | debug app with the sidecar, verify run 40282296177 | `cargo test --workspace` and `pnpm -r test` green (windows-latest) | `c4 sessions`, `c4 doctor`, `c4 install` and `c4 uninstall` against the real home, and the window showing hook states and context figures from live sessions, see Results |
| macOS arm64 | debug app with the sidecar, verify run 40282296177 | `cargo test --workspace` and `pnpm -r test` green (macos-latest) | no hardware in reach |
| macOS x64 | debug app with the sidecar, verify run 40282296177 | `cargo test --workspace` and `pnpm -r test` green (macos-15-intel) | no hardware in reach |
| Linux x64 | debug app with the sidecar, verify run 40282296177 | `cargo test --workspace` and `pnpm -r test` green (ubuntu-latest) | not yet |

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
  and modification time, with no new backup beside it. The real install was left for the build in
  which the app starts the ingest server, so that nothing installed sits unread.
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
- 2026-10-26, the verify workflow for commit 84201f5 (run 40278916290): the lint job and the four
  test jobs green, the doctor audit tests included on every runner.
- 2026-10-26, Windows 11 Pro, the sidecar chain CI runs: `scripts/stage_shim.py --profile debug`
  staged the debug shim as `src-tauri/binaries/c4-shim-x86_64-pc-windows-msvc.exe`, and
  `tauri build --debug --no-bundle --config src-tauri/tauri.sidecar.conf.json` built the debug app.
  With the sidecar declared and the staged file moved away, the desktop crate's build script failed
  the build; with the file back and `target/debug/c4-shim.exe` moved aside, the build put a copy
  there with the staged file's hash. The built `c4.exe install --dry-run` named that copy as the
  shim it would install, and wrote nothing.
- 2026-10-26, the verify workflow for commit f798164 (run 40279629986): the lint job and the four
  test jobs green, each runner staging the debug shim and building the debug app with the sidecar
  declared.
- 2026-10-26, C4 0.0.1, Windows 11 Pro, the app's ingest worker, local: `cargo test -p c4-desktop
  --lib` runs two tests against a scratch Claude home and a scratch C4 home. The first starts the
  worker, sees the probe answer listening, delivers a `UserPromptSubmit` payload through the shim's
  own delivery code (sent, not spooled) and finds the session in the next emitted document as working
  with the hook seen; a second payload from inside a subagent leaves the state alone and is counted as
  a subagent line; a refresh pass carries the same counts; stopping the worker frees the endpoint.
  The second test starts a plain server on the home first, sees the worker report the endpoint as
  taken, naming it, and watches the worker listen once that server shuts down. Both pass serially and
  in parallel.
- 2026-10-26, the verify workflow for commit 194716b (run 40282296177): the lint job and the four
  test jobs green, the desktop crate's two ingest tests included on every runner.
- 2026-10-26, C4 0.0.1 (debug `c4.exe` built with the sidecar), Claude Code 2.1.299, Windows 11 Pro,
  Windows Terminal, display at 150 %. Item 3 (M, and the sidebar half of F): with the window open,
  `c4 install` against the real Claude home copied the shim to `~/.c4/bin/`, wrapped the status line
  and added the 17 hook groups, after a backup (`settings.json.bak-c4-20261026T065517Z`, the same
  16,887 bytes and SHA-256 as the file before) and with the manifest written, restorable. About two
  minutes later the window's row for the session running the check read "running PowerShell", a
  state only a hook delivers, with a context figure of 23 %, and the other live session's row 19 %.
  The status line's own cache file for that session, rewritten after the install, held 227,341 input
  tokens for its last turn, 22.7 % of a 1M window, which agrees with the sidebar; a digit-for-digit
  comparison with the meter in the terminal is left to a person at that terminal. `c4 doctor`
  reported the shim copy as the manifest recorded and the same as the sidecar, 17 of 17 hook groups,
  the status line wrapped, the endpoint listening and the spool empty, so every payload was delivered
  and none spooled. The agents installer's dry run (`python -I install/install.py --dry-run --home
  <copy>`) reported one settings change, "hook wiring, env and statusLine (python)", against the
  installed file and none against the file as it was before the install; its summary does not
  itemize, and test M covers what its merge keeps. `c4 uninstall --dry-run` named the restore route;
  `c4 uninstall` took a second backup, restored the file from the first one byte for byte (the same
  size and SHA-256 as before the install) and removed the shim copy and the manifest; both backups
  stay. Closing the window ended the process within 8 s and left the endpoint free, per
  `c4 doctor --no-commands`. The window was captured with `PrintWindow`; a capture from a process
  that is not DPI aware came back as the top-left 1295x837 physical pixels of a 1942x1256 window and
  so missed the status strip, which a capture sized by the window's own DPI showed: "refreshed 0s
  ago in 50 ms | session list: 3 entries, 5s ago | ingest: 0 lines | home from ~/.claude | c4 0.0.1
  | bridge: tauri" (taken after the uninstall, hence no lines). The ledger was not watched: no turn
  ended during the check.
- 2026-10-26, the virtual list, locally on Windows: vitest 25 tests green (the list tests give the
  listbox a viewport height and set its scroll position by hand, since jsdom lays nothing out),
  eslint and tsc clean. The layout itself was measured in Chromium against the mock bridge at
  1280x800: rows of 34, 56 and 22px as the chord walks the densities, the state square 8px and
  centered, the 24x3px context bar present at standard and expanded and gone at compact, the third
  line present only at expanded with the long model text clipped so the cwd keeps room, the compact
  row carrying the state text in its tooltip, every secondary color `--dim`, no console messages.
  With the eight synthetic sessions nothing scrolls; the 200-session case is the vitest test.
- 2026-10-26, the verify workflow for commit 87e194d (run 40285554095): the lint job and the four
  test jobs green.
- 2026-10-26, grouping, filter and pins, locally on Windows: vitest 39 tests green (the order rules
  without a DOM in `apps/desktop/tests/arrange.test.ts`, the filter bar in
  `apps/desktop/tests/filterbar.test.ts`, the list with headers, folding, filtering, pins, keyboard
  travel over headers and a row arriving under a header while the order is held in
  `apps/desktop/tests/list.test.ts`), eslint and tsc clean. Measured in Chromium against the mock
  bridge at 1280x800 with the eight synthetic sessions: the filter line's input and button 24px tall
  under the top bar; grouped by project, eight headers of 24px every 58px from the top with each row
  24px below its header, the fold glyph 12x8 at the same x as the rows' state squares, the number in
  `--sage`, the title in Space Mono 700, the rule 1px `--faint`, the count `--dim` and "1 needs you"
  `--amber`, the count span hidden when empty; `/` focused the box and its border turned `--sage-hi`;
  "state:wait" left one row under one header, "host:nowhere" none with "No sessions match the
  filter." shown, "mode:observed" six rows in six groups; Escape cleared the box and a second Escape
  moved the focus to the list; Down then `p` pinned the first session (aria-label ending ", pinned",
  a 6x6 `--sage` square after the name); a click on the first header folded it (seven rows left, the
  glyph a plus, the title `--dim`, the spacer 430px, the hidden selection no longer announced). No
  console errors or warnings. A code edit while the dev server runs reloads the page and resets the
  list, so every check was redone after the last edit.
- 2026-10-26, the verify workflow for commit fe1f779 (run 40288510275): the lint job and the four
  test jobs green.
- 2026-10-26, the hover card, locally on Windows: vitest 44 tests green
  (`apps/desktop/tests/hovercard.test.ts`: what the card says about the synthetic sessions, the
  agent lines and task counts, where it goes with stubbed geometry, the refresh that keeps it
  current, and the list's 350 ms wait with fake timers), eslint and tsc clean. Measured in Chromium
  against the mock bridge at 1036x850: the card opened beside the beta row after the pointer rested
  on it, 300px wide, 8px right of the row's edge and level with its top, `position: fixed`, a 1px
  `--faint` border on `--bg`, `pointer-events: none` (the element under the card's center is the
  workspace), no input or button inside it; the head's state square 8x8 in `--amber` beside the name
  and the OBSERVED chip, then "WezTerm, no input from C4", "waiting for permission: Bash for 3m 28s"
  in `--amber`, "observed from hook Notification", the eight facts in order (model, context "78% of
  200K" with a 24x3px bar filled to 78% in `--amber`, agents "none seen", cwd, started, last
  activity, status line, telemetry) and the agent list hidden. Moving to the nightly row switched
  the card at once, with its two agent lines; the docs card carried the tool line "Read for 3m 23s"
  and the context "91% of 1.0M" in `--red`. At 620px wide the card went to the left of the row,
  held 8px from the window's edge. A click on a row closed the card and focused the list; a hover
  opened it for another row and ArrowDown closed it, and it stayed closed through the next three
  seconds of refreshes while the pointer rested. Before the pointer-rest rule it came back after
  350 ms on Chromium's own pointer event for the resting pointer, which is why the rule exists. No
  console errors or warnings. A Playwright screenshot that changes the device scale factor resets
  Chromium's hover state and closed the card, so the screenshots were taken at device scale.
