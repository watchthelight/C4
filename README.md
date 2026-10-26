<!-- <wtl> project: c4 | file: README.md | author: watchthelight </wtl> -->

# C4

C4 is a command center for Claude Code sessions. It is a desktop application
(Windows, macOS, Linux) that finds every Claude Code session you can reach,
shows what each one is doing and whether it needs you, hosts real terminals for
the sessions it starts, and only ever controls a session through a verified
channel.

It reads the data Claude Code already writes (the session registry, the
transcripts, the status line payload and hook events) and adds a sidebar in the
style of iTerm2's Session Status tool, a terminal workspace with tabs and
splits, a workflow inspector for subagents and tool calls, token and quota
views that agree with the status line, and a library for skills, MCP servers
and toolkits.

This repository is under construction. The first tagged release carries the
working sidebar; see CHANGELOG.md for what each version adds.

## Quick start, headless

Until the first release the headless commands are the way in. `c4-cli` is the
same command line as the `c4` executable without the window:

```sh
cargo build --release -p c4-cli -p c4-shim
target/release/c4-cli install --dry-run   # what would change; nothing is written
target/release/c4-cli install             # copy the shim to ~/.c4/bin and wire settings.json
target/release/c4-cli doctor              # the Claude home, claude, and C4's shim, install and endpoint
target/release/c4-cli sessions            # every session discovery can see
target/release/c4-cli uninstall           # the way back
```

`c4 install` changes one file, `~/.claude/settings.json`: the status line command
gets the shim in front of it and one hook group per event C4 listens to is added.
The file is copied to a backup beside it first, written back in its own layout,
and what changed is recorded under `~/.c4/` so that `c4 uninstall` can restore
the backup byte for byte, or take C4's entries out one by one when something
else has written the file since. The app does not start the ingest server yet,
so a shim installed today spools what it receives under `~/.c4/spool/` until a
later build replays it. docs/SESSION-INTEGRATION.md has the details.

## License

MIT. Third-party licenses are listed in THIRD-PARTY-NOTICES.md.
