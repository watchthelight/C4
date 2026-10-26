<!-- <wtl> project: c4 | file: CHANGELOG.md | author: watchthelight </wtl> -->

# Changelog

Notable changes to C4, in the Keep a Changelog 1.1.0 format. Versions follow semantic versioning and
stay at 0.y.z while the interfaces settle.

## [Unreleased]

### Added

- Cargo workspace with a pinned Rust toolchain and the MIT license.
- `c4-core`: session, process, agent and host types; the nine-state session machine, which
  carries the source and certainty of every transition and never reports Failed from silence or
  Completed with work in flight; the versioned event envelope; redaction of paths and secrets.
- Synthetic fixtures: a scratch Claude home with registry entries, supervisor files, transcripts
  with subagents and workflows, a session list, and the state label contract shared by the Rust
  and TypeScript sides.
- `c4-discovery`: readers for the session registry, `claude agents --json`, the background
  session supervisor files and the process table; liveness by pid and creation time with pid reuse
  detected; terminal host identification from the process environment; one verdict per session
  when several name one pid, so a session whose process moved to a new id after `/clear` or
  `/resume` is completed instead of lingering.
- `c4-transcript`: a byte-offset tailer that leaves a partial last line alone, a tolerant record
  parser gated on the transcript version, a session follower with usage max-merged per message id,
  a metadata-only project index, a subagent and workflow scanner and a file watcher.
- `c4-testkit` and `scripts/fake_claude.py`: scratch homes and a stand-in Claude Code process
  registered with its own pid and creation time, so liveness tests run on a real process table.
- `c4 sessions` (table, JSON, watch), `c4 doctor` and `c4 telemetry --transcript`.
- The desktop shell: the `c4` executable opens a window that lists sessions in attention order
  with a read-only inspector, refreshed by a background pass every 2 s; any argument runs a headless
  command instead. A mock bridge serves synthetic sessions in a plain browser.
- Licensing: `cargo deny` over the four release targets, `cargo about` and a generated
  THIRD-PARTY-NOTICES.md that CI checks for staleness.
- Repository scripts: the pre-commit gate, the secret scan, the slop lint, the header check and
  the notices generator.
- CI: a pull-request gate and the verify workflow with a lint job and a four-runner test matrix
  that also builds the debug app.
- Documentation set under docs/ and the project directive and plan under docs/prompts/, including
  the finding that Claude Code 2.1.299 applies a hook added to a settings file to sessions that are
  already running.
- The design tokens stylesheet from docs/DESIGN-SYSTEM.md, Space Mono and JetBrains Mono bundled
  under assets/fonts/ with their OFL texts, and the starfield canvas behind the sidebar and the
  status strip: three drifting layers, one star in ten sage, a rare shooting star drawn as fading
  pixels, 30 fps inside a 1 ms budget, frozen while hidden or under reduced motion, seedable for
  screenshots.
- `c4-core::home`: where C4 keeps its own files (`C4_HOME`, else `~/.c4`) and the ingest endpoint
  for each home, a named pipe on Windows keyed by a hash of the home path and a socket file
  elsewhere. A `Statusline` observation carries the model, the context figures and the effort from
  a status line payload; it proves the process is alive and never changes the state on its own.
  The session view, the sidebar row (a context percentage in the status line's bands) and the
  inspector (model, context, effort, age of the last payload) show the new fields.
- `c4-shim`, the sidecar with no dependencies: `c4-shim statusline -- <command>` copies the status
  line payload to C4 and runs the original command with the same stdin, its output and exit code
  untouched; `c4-shim hook` copies a hook payload, prints nothing and exits 0. Lines go to the ingest
  endpoint of the C4 home (a named pipe on Windows, a socket file elsewhere) and wait in
  `~/.c4/spool/<date>.jsonl` when C4 is not running. A test holds the endpoint rule equal to
  `c4-core::home`.
- `c4-ingest`, the receiving end: a server on the ingest endpoint (a named pipe with a current-user
  DACL on Windows, a mode 0600 socket file with a peer uid check elsewhere, read with `getpeereid`
  on macOS and the BSDs because a shim has closed its end before the server looks) that turns each shim line
  into observations, a bounded inbox that drops its oldest item under pressure and counts the drop,
  the spool replay at start and every 30 s, a probe for `c4 doctor`, and a second server on the same
  home refused. Fixtures for every hook payload C4 listens to, two status line payloads and a spool
  file with two bad lines.
- `Discovery::fold` in `c4-discovery`: applies a batch of observations from outside a pass (the hook
  events and status line payloads the ingest server parses) in `recv_at` order, a stable sort so a
  payload's identity lands before its event, then runs the clock. It returns the transitions and the
  sessions touched. A session only such a payload names gets a view with no process. A test folds
  the hook and status line fixtures through the `c4-ingest` parser in reverse arrival order.
- `c4 install` and `c4 uninstall`: the shim copied to `<c4 home>/bin/`, `statusLine.command` wrapped
  after its leading assignments, one asynchronous hook group per event C4 listens to and a
  synchronous one for `SessionEnd`, every write of `settings.json` after a backup beside it and
  through a temporary file, with the file's key order, indentation, line endings and permissions
  kept, and a manifest under the C4 home that names the original's backup and hash. Uninstall
  restores the backup byte for byte while nothing else has written the file, and otherwise removes
  C4's entries one by one. A second install changes nothing. Test M: the agents repo's installer run
  twice over an installed file, with CI checking that repo out at a pinned commit;
  `fixtures/settings/full.json` is the settings file it starts from. `serde_json` keeps key order
  across the workspace now, which is what lets the rewrite leave a user's file in its own order.
- `c4 doctor` has a `c4` section: the C4 home (`--c4-home`), the shim copy against the manifest and
  against the `c4-shim` beside the executable, the copy's `--version` and `endpoint` (skipped by
  `--no-commands`) checked against the endpoint this C4 listens on, the install as the manifest
  records it against what `settings.json` holds now (hook groups present of 17, the status line
  wrapped or not, entries of another C4 home's shim), the ingest endpoint (listening, free or
  unknown) and the spool, with notes that name the command to run. The session list command type
  can carry environment variables for its child, which is how the shim copy is pointed at the same
  C4 home.
- The shim ships as a Tauri sidecar beside `c4`: `scripts/stage_shim.py` copies the built `c4-shim`
  to `src-tauri/binaries/c4-shim-<triple>`, `tauri.sidecar.conf.json` declares it for
  `tauri build --config`, and CI stages it before the debug app build on every runner. A plain cargo
  build or test never needs the staged file.
- The app runs the ingest server. A worker started when the window opens, on a tokio runtime of its
  own, listens on the C4 home's endpoint, replays the spool, and folds each batch of hook events and
  status line payloads into the session table on a blocking thread, sending the window the same
  `sessions` event a pass does, at most one every 40 ms. A line from inside a subagent folds its
  identity only, so a subagent's tool calls never move the session. When another C4 holds the
  endpoint the status strip says so and the worker tries again every 15 s; on exit the server is
  stopped and its socket file removed. The document and the status strip carry the server's state
  and counts, and `about` names the C4 home and the endpoint. Two tests send a hook line through
  the shim's own delivery code and watch it reach the window's document, and start a second server
  on the home to see the state reported and the endpoint taken over later.
- The sidebar is a virtual list: only the rows near the viewport exist, on pooled nodes placed with
  a transform, eight rows beyond each edge. Three densities (22, 34 and 56px rows) cycle on
  Ctrl+Shift+=; the compact row carries the state text in its tooltip, the standard row adds the
  state line and a 24px context bar, the expanded row adds the attachment chip, the model and
  effort, and the cwd. The arrow, page, Home and End keys move the selection and scroll to it. A
  reorder waits while the pointer is inside the list or the list has keyboard focus and applies on
  leave or after two seconds at rest. Secondary text at 10 and 11px uses `--dim`, as the design
  system says; `--mute` paints only the idle and completed squares.
- Grouping, filter and pins in the sidebar. The list groups by project, host or state (the button
  above it, or `g` in the list), with a 24px header per group carrying its number, name, member
  count and how many need you; a click or Left folds a group, Right opens it, and groups come in
  the order of their most urgent session. The filter box (`/`) narrows the list as you type: words
  match the name, project and cwd, and `state:`, `project:`, `tool:`, `mode:` and `host:` tokens
  match one field each. `p` pins the selected session to the top of the list or of its group. What
  the user does by hand applies at once; a reorder from the core still waits for the pointer and
  the keyboard, and a session that arrives meanwhile joins under its header.
- A hover card beside the row the pointer rests on for 350 ms: the attachment and why (the host
  and pane, or what ended the session), the state with its evidence, the model and effort, the
  context figure with its bar, the open tool, the agents with the last three named, the cwd, and
  when the session started, last did something and last sent a status line. It takes no pointer
  events, follows the pointer from row to row, and closes when the pointer leaves the rows, the
  list scrolls or changes by hand, or a key goes down, then stays closed until the pointer moves
  again. Tokens, the cache ratio, the branch and the last tool calls join it with the telemetry and
  transcript readers.
- Saved preferences and a sidebar splitter. The sidebar's pins, grouping, folded groups, density
  and width go to `prefs.json` under the C4 home a quarter of a second after each change by hand,
  and the next start builds the sidebar from them before the first pass (`prefs_get` and
  `prefs_set`; `crates/c4-cli/src/serve/prefs.rs`, `apps/desktop/src/prefs.ts`). The file is
  one JSON object with a schema number and a section per part of the window; the core replaces one
  section at a time through a temporary file and a rename, keeps a section or a schema number
  another version wrote, caps the file at 1 MiB, and moves a file it cannot read aside as
  `prefs.json.bad-<stamp>`, which the status strip reports. The window reads each value on its
  own, so a bad one falls back alone. The 6px splitter at the sidebar's right edge drags with the
  pointer (the starfield rests meanwhile), moves 10px per arrow key when focused, goes to its
  limits on Home and End, and puts the 320px default back on a double-click; the sidebar never goes
  under 220px and the workspace keeps 400px, so a narrower window shows less of the saved width and
  a wider one gives it back. In a browser the mock bridge keeps the preferences in local storage.
- A sidebar rail. The button at the right end of the filter line, or Ctrl+Shift+B, folds the
  sidebar to a 36px rail with one state square per session (pinned first, then the attention
  order, the attention stripe kept, the count that need you at the bottom); a square's tooltip and
  accessible name carry the session's name and state, a click selects, the arrow keys move the
  selection, and the pointer resting on a square opens the hover card. The button at the top of
  the rail, the chord again or `/` opens the sidebar back up. A reorder waits while the pointer or
  the keyboard is in the rail, as in the list. The fold is kept in `prefs.json` with the rest of
  the sidebar's organization (`apps/desktop/src/sessions/rail.ts`).
- `c4-cli serve`, the window's core as a child process. A shell starts it with pipes on stdin and
  stdout and talks JSON lines: a request `{"id", "method", "params"}` on stdin gets `{"id", "ok"}`
  or `{"id", "error"}` back with the same id, and every refresh pass and every batch the ingest
  server folds arrives as a `{"event": "sessions", "data"}` line, after a first `hello` line that
  names the protocol version, the output schema, the C4 version and the pid. The methods are the
  window's four (`list_sessions`, `about`, `prefs_get`, `prefs_set`); `--files-only` skips the
  session list command; a line that is not a request is answered with a null id; logs go to
  stderr, warnings and worse unless `--log` says otherwise. The process ends, with the ingest
  server stopped and the endpoint free, when stdin closes or stdout cannot be written. The session
  table, the ingest worker and the preferences file moved from the Tauri crate to
  `crates/c4-cli/src/serve/`, so the window and the child process share them. A test drives
  `c4-cli serve` as a child process through the whole protocol, including a hook line delivered
  through the shim's own code and the exit when stdin closes (`crates/c4-cli/tests/serve.rs`).
- The window runs in Electron, over `c4-cli serve`. Chromium renders the page on every platform
  (the Tauri window used the platform webview, WebView2 on Windows). `apps/desktop/electron/` is
  the main process: it starts the core as a child process and speaks its JSON lines, serves the
  page from the app's own `c4://app/` scheme with the same content security policy, and gives the
  sandboxed page `window.c4` through a preload script. A request waits for the core's hello; a
  core that exits is started again after a growing pause and the page hears about it as a `core`
  event; a stop closes the core's stdin first, after a grace for the preferences the page flushes
  on its way out. `c4 <args>` still runs the command line with no window. `pnpm -C apps/desktop
  dev` and `start` open the Electron window; the Tauri shell stays in the tree for one more
  commit. Tests cover the argument rule, the page origin check, the path check of the scheme, the
  sidecar against a stand-in core (ids, events, a restart, a protocol mismatch, stop and kill) and
  the renderer bridge (`apps/desktop/electron/tests/`, `apps/desktop/tests/electron.test.ts`).
  Electron is listed in THIRD-PARTY-NOTICES.md with a pointer to its own LICENSES.chromium.html.

[Unreleased]: https://github.com/watchthelight/C4/compare/c3a0a47...HEAD
