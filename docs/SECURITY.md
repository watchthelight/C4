<!-- <wtl> project: c4 | file: docs/SECURITY.md | author: watchthelight </wtl> -->

# Security

## Threat model

C4 runs on the same machine and as the same user as the Claude Code sessions it watches. It reads
files that user can already read and runs commands that user can already run. The things it must
not do are: leak credentials that live next to the data it reads, give another process on the machine
a way to drive a session, publish private session data, or change a running session's behavior
without the user asking.

## What C4 never reads

- `~/.claude/.credentials.json`
- `~/.claude/sessions/*.key` (the peer-message tokens), except at the moment a peer message is sent,
  into a buffer that is zeroed afterwards and never persisted or logged
- The `rvAuth` and `ptyAuth` fields of the background session supervisor files
- IDE lock files beyond the fact that they exist
- `~/.claude/state/wtl/token`, `~/.claude/notify.json`, `~/.claude/channels/*/.env`
- Any environment value whose name contains TOKEN, KEY, SECRET or AUTHORIZATION
- HTTP headers and environment values in MCP server definitions, and URLs that embed credentials

These fields are on a deny list that is applied while parsing, before any struct exists, so they
cannot reach the SQLite store, the logs, an export or the screen.

## Redaction

Logs and exports pass through a redaction layer that rewrites bearer tokens, `sk-ant` keys, OAuth
token shapes and home paths. Config excerpts shown in the capability library render secret-looking
values as "redacted (N chars)". Prompt text and transcript content leave the machine only when the
user exports them on purpose.

## Local sockets

The ingest socket is a named pipe in the per-session `LOCAL` namespace with a DACL for the current
user on Windows and a Unix socket with mode 0600 in a private directory elsewhere. Remote clients are
rejected. Managed session hosts use the same rules. The shim connects only to the endpoint derived
from its own C4 home and sends one line per payload. Hook payloads include tool inputs, which can hold
whatever the session typed, so the spool that keeps undelivered lines (`~/.c4/spool/`) is created
with mode 0700 and its files with 0600 on Unix, lives under the user's own profile on Windows, and
each file is removed once C4 has replayed it.

## Control

Input to a session only ever goes through a verified handle: C4's own pty for managed sessions, or a
terminal host pane that was matched to the Claude Code process by pid ancestry or tty. No code path
accepts a raw pid or window handle for input. C4 does not use `WriteConsoleInput`, `SendInput`,
`ptrace`, DLL injection or any similar mechanism. One controller holds a session's lease at a time, so
two C4 windows cannot write to the same session at once.

C4 never answers a permission prompt, never accepts a workspace trust dialog, never sets
`hasTrustDialogAccepted`, never restarts or kills a session it did not start, and never offers its own
claude.ai login.

## Network

C4 makes no network requests by default. The About pane does not check for updates. Paired remote
hosts are an opt-in feature with a short-code pairing that derives X25519 keys which are then pinned;
a remote agent exposes the read-only discovery surface, and controls over it need the same lease and
validation as local ones.

## The webview

The Tauri content security policy is locked, no remote URLs are loaded, and the asset protocol is
scoped to the bundle. Terminal bytes are written to xterm.js, never interpreted as HTML.

## Reporting

Use GitHub's private vulnerability reporting on this repository. Include the C4 version, the platform
and what you observed; do not include tokens or transcripts.
