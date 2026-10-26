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
  and counts, and `about` names the C4 home and the endpoint. Two desktop tests send a hook line
  through the shim's own delivery code and watch it reach the window's document, and start a second
  server on the home to see the state reported and the endpoint taken over later.

[Unreleased]: https://github.com/watchthelight/C4/compare/c3a0a47...HEAD
