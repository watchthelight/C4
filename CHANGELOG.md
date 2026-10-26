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
  endpoint of the C4 home (a named pipe in the per-session namespace on Windows, a socket file
  elsewhere) and wait in `~/.c4/spool/<date>.jsonl` when C4 is not running. A test holds the endpoint
  rule equal to `c4-core::home`.

[Unreleased]: https://github.com/watchthelight/C4/compare/c3a0a47...HEAD
