<!-- <wtl> project: c4 | file: fixtures/README.md | author: watchthelight </wtl> -->
# Fixtures

Everything under this directory is synthetic. The files copy the key names and shapes that Claude Code
2.1.299 writes, with invented values: session ids of the form `0a1b2c3d-0001-4000-8000-00000000000N`,
project paths under `/home/example` and `C:\Users\example`, pids in the 41000 range, process start
times that belong to no real process, and auth fields that read `fixture-not-a-secret`. No transcript,
prompt, token or key from a real session is in here, and the test suites never read a real Claude home.

`home/` is a complete scratch Claude home. Tests point `C4_CLAUDE_HOME` at it, or copy it into a
temporary directory when they need to write.

| Path | Mirrors | Used by |
|---|---|---|
| `home/sessions/` | `~/.claude/sessions/<pid>.json` and the `<pid>.<hash>.key` siblings | `c4-discovery` registry reader |
| `home/daemon/roster.json` | the background session supervisor roster | `c4-discovery` daemon reader |
| `home/jobs/<short>/state.json`, `timeline.jsonl` | one background job's state and history | `c4-discovery` daemon reader |
| `home/projects/<encoded cwd>/` | `~/.claude/projects/<encoded cwd>/<session>.jsonl` and `<session>/subagents/` | `c4-transcript` tailer and scanner |
| `agents-json/` | the output of `claude agents --json` | `c4-discovery` agents poller |

Files that exist to prove tolerance: `home/sessions/broken.json` (truncated JSON), `home/sessions/notes.txt`
(not a record), `home/sessions/41004.json` (an older record missing optional fields), the empty
transcript for session 4, the transcript for session 2 that ends mid-line, and the `agents-json` entry
with a kind and status no version of C4 knows.

Rules for adding to this directory:

- Generate values; never paste from a real file, even with edits.
- Keep the key names exactly as Claude Code writes them, including the camel case.
- `.key` files here contain a sentence, not a key. The scanner must never open them; a test checks
  that it does not.
- `.scanignore` exempts only `home/sessions`, `home/daemon` and `home/jobs` from `scripts/scan.py`,
  because their fake socket names, start times and auth fields look like the real thing. Everything
  else is scanned.
