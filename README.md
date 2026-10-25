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

## License

MIT. Third-party licenses are listed in THIRD-PARTY-NOTICES.md.
