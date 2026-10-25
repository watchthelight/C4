<!-- <wtl> project: c4 | file: CONTRIBUTING.md | author: watchthelight </wtl> -->

# Contributing

## Setup

1. Install the prerequisites in docs/BUILDING.md (Rust from `rust-toolchain.toml`, Node 22 or later,
   pnpm, Python 3.11 or later, and the platform packages).
2. Clone the repo and point git at the repo's hooks:

   ```sh
   git clone https://github.com/watchthelight/C4.git
   cd C4
   git config core.hooksPath .githooks
   git config commit.template .github/commit-template.txt
   ```

3. Build and test once to warm the caches:

   ```sh
   cargo test --workspace
   pnpm install
   pnpm -r test
   ```

The hooks run `scripts/precommit.py` before every commit and `scripts/slop_lint.py --commit-msg` on the
message. Both are plain Python with no dependencies.

## Style

- Plain ASCII in every file. Straight quotes, `->` for arrows, no em or en dashes, no emoji.
- Every text file starts with a `<wtl>` header naming its own path. JSON, lock files, fonts and
  fixtures are exempt; `scripts/check_headers.py` lists the rules.
- Prose passes `scripts/slop_lint.py`: no filler phrases, no self-answered questions, no stacked short
  fragments, no stock section names. Write the way you would explain the code to a colleague.
- Rust: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, no `unsafe` without a
  comment that says why, no panics on data that comes from disk or from another process.
- TypeScript: strict mode, no `any`, no framework; components are factory functions that return
  `{el, dispose}`.
- Python (hooks and scripts): standard library only, `python -I` safe, every exception handled so a
  hook never breaks a Claude Code session.

## Commits

- Subject: lowercase `area: what changed`, under 72 characters, no trailing period. Areas are listed
  in `.github/commit-template.txt`.
- One change per commit. Push after every green commit. Never force-push, never amend a pushed
  commit, never skip the hooks.
- No trailers of any kind: no co-author lines, no sign-off lines, no attribution lines.
- Commit prompts and design notes too; nothing about how the project is built is hidden.

## Tests

| Area | Command |
|---|---|
| Rust, one crate | `cargo test -p c4-core` |
| Rust, everything | `cargo test --workspace` |
| Frontend unit | `pnpm -C apps/desktop test` |
| Frontend end to end (mock bridge) | `pnpm -C apps/desktop e2e` |
| Hooks | `python -m unittest discover -s hooks/tests -t .` |
| Lint and scans | `python scripts/check_headers.py && python scripts/slop_lint.py && python scripts/scan.py` |

Tests never touch a real `~/.claude`. The `c4-testkit` crate builds a scratch Claude home from the
synthetic fixtures under `fixtures/`; set `C4_CLAUDE_HOME` to point the core at it.

## Pull requests

Fill in the template. Say what changed and why, list the tests you ran, and note any behavior that
is only verified on one platform so the docs can say so. A PR that adds a host adapter must add a
`docs/adapters/<host>.md` page with the exact commands it runs and how it verifies the target pane.

## Reporting a security problem

Use GitHub's private vulnerability reporting on this repository. docs/SECURITY.md describes the
threat model and what C4 never reads.
