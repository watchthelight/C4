<!-- <wtl> project: c4 | file: CLAUDE.md | author: watchthelight </wtl> -->

# C4 repo rules for Claude Code sessions

Read docs/prompts/c4-directive.md and docs/prompts/c4-plan.md before changing anything; they are the
source of every decision here.

## Identity and commits

- Author is `watchthelight <admin@watchthelight.org>`, set locally in this clone. Check
  `git config user.email` before the first commit of a session.
- Subjects are lowercase `area: what changed`, under 72 characters. Small commits, pushed after every
  green pre-commit run. Never force-push, never `--no-verify`, never amend a pushed commit.
- No trailers: no co-author lines, no lines saying a tool wrote the code, no AI attribution in
  commits, PR bodies, issues or files. No AI disclaimers in README or docs.

## Text

- Plain ASCII everywhere. No em dashes, no unicode arrows, no curly quotes, no emoji.
- Every text file carries `<wtl> project: c4 | file: <its path> | author: watchthelight </wtl>` in a
  comment near the top. `scripts/check_headers.py` enforces it.
- Prose must pass `scripts/slop_lint.py`; the phrases listed in that script are banned. Describe the
  product as "C4, a command center for Claude Code sessions", never as a product
  named "Claude Code Command Center".

## Data

- Never commit anything from `~/.claude`, a transcript, a ledger, a key file or a home path with a
  user name. `scripts/scan.py` and the pre-commit denylist catch most of it; think before staging.
- Fixtures are synthetic. Tests run against a scratch Claude home (`C4_CLAUDE_HOME`), never the real
  one. Throwaway Claude sessions only; never restart or kill an existing session.
- Config edits to a user's `~/.claude/settings.json` go through `c4 install` with a backup and a
  manifest, nothing else.

## Build notes

- On this Windows machine the user env var `CC` points at cl.exe, which breaks cc-rs outside a
  developer prompt. Run cargo with `$env:CC = $null;` in PowerShell. `scripts/precommit.py` drops it
  for its own child processes.
- `cargo test --workspace`, `pnpm -r test`, `python -m unittest discover -s hooks/tests -t .` are the
  suites. `python scripts/precommit.py` runs what the staged files need.
