<!-- <wtl> project: c4 | file: docs/SESSION-INTEGRATION.md | author: watchthelight </wtl> -->

# Session integration

How C4 plugs into an existing Claude Code setup, what it changes, and how to take it out again.

Status: the shim (`crates/c4-shim`), the ingest server (`crates/c4-ingest`), `c4 install` and
`c4 uninstall` (`crates/c4-cli/src/install.rs`) and the core's end of the line
(`crates/c4-cli/src/serve/ingest.rs`, run by the window and by `c4-cli serve`) exist. The core
starts the server when it starts, replays what the shim spooled while no C4 ran, and folds every
hook event and status line payload into the session list as it arrives. A session the shim has not reported yet runs in the coarse mode described
in docs/HOW-IT-WORKS.md: registry, transcripts and the status line's cache files, no hooks. The
sidebar's organization is kept in `prefs.json` under the C4 home, never under the Claude home
(docs/HOW-IT-WORKS.md, "The window today"). Notifications come with a later Milestone 2 commit.

## What `c4 install` changes

```sh
c4 install --dry-run         # print what would change, write nothing
c4 install                   # copy the shim, wrap the status line, add the hook groups
c4 install --home DIR        # another Claude home (default: C4_CLAUDE_HOME, CLAUDE_CONFIG_DIR, ~/.claude)
c4 install --c4-home DIR     # another C4 home (default: C4_HOME, ~/.c4)
c4 install --shim FILE       # the shim to copy (default: the c4-shim beside the executable)
c4 uninstall [--dry-run]     # the way back
```

Install does three things and nothing else:

1. Copies `c4-shim` (`c4-shim.exe` on Windows) to `<c4 home>/bin/`, unless the copy there already has
   the same bytes. It never runs the shim.
2. Wraps `statusLine.command` in `settings.json` so that the payload reaches C4 before the original
   command runs.
3. Adds one hook group per event C4 listens to.

Two and three are the only changes to `~/.claude/settings.json`, and nothing else under the Claude
home is touched. Before:

```json
{
  "statusLine": {
    "type": "command",
    "command": "PYTHONIOENCODING=utf-8 python ~/.claude/statusline.py",
    "padding": 1
  },
  "hooks": {
    "Stop": [
      {"hooks": [{"type": "command", "command": "python ~/.claude/hooks/turn_end.py", "timeout": 60}]}
    ]
  }
}
```

After:

```json
{
  "statusLine": {
    "type": "command",
    "command": "PYTHONIOENCODING=utf-8 ~/.c4/bin/c4-shim statusline -- python ~/.claude/statusline.py",
    "padding": 1
  },
  "hooks": {
    "Stop": [
      {"hooks": [{"type": "command", "command": "python ~/.claude/hooks/turn_end.py", "timeout": 60}]},
      {"hooks": [{"type": "command", "command": "~/.c4/bin/c4-shim hook", "timeout": 5, "async": true}]}
    ],
    "UserPromptSubmit": [
      {"hooks": [{"type": "command", "command": "~/.c4/bin/c4-shim hook", "timeout": 5, "async": true}]}
    ]
  }
}
```

The same `c4-shim hook` group is added for SessionStart, UserPromptSubmit, PreToolUse, PostToolUse,
PostToolUseFailure, Notification, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop,
StopFailure, PreCompact, PostCompact, CwdChanged and ConfigChange, all asynchronous, plus a synchronous
SessionEnd hook with a 1 s timeout (Claude Code gives SessionEnd hooks 1.5 s). None of them print
anything or return a decision, so they cannot block a tool call, a permission prompt or a session.
C4 never installs PermissionRequest, Elicitation, PreModelSwitch or any other hook that can decide
something.

The wrapper goes in after any leading `NAME=value` words, so an environment the original command
relies on (`PYTHONIOENCODING` above) still reaches it. For the default C4 home the shim is named
`~/.c4/bin/c4-shim` (`c4-shim.exe` on Windows), which bash expands wherever the settings file is
read. For any other home the command carries the absolute path with forward slashes and a
`C4_HOME=<home>` assignment ahead of it, as in `C4_HOME=/srv/c4 /srv/c4/bin/c4-shim hook`, so the
shim and the app agree on the home; the app must then run with the same `C4_HOME`. A path bash would
split or expand is double-quoted; one that holds a double quote, a dollar sign, a backtick, a
backslash or a line break is refused, since every hook command runs through bash.

The edge cases, each reported in the command's output: a file without `statusLine` gets the hook
groups alone (the status line is the user's to configure; C4 does not add one). A `statusLine` that
is not a command, or whose command names no program, is left alone. A status line already wrapped by
this home's shim stays as it is; one wrapped by a `c4-shim` of another home is left alone, as is a
hook group that runs another home's shim. The match is on the exact command string, so running
`c4 install` twice changes nothing the second time. A file that is not valid JSON, or not a JSON
object, stops the command before anything is written, the shim copy included. The file is written
back with its own key order, indentation, line endings and byte order mark, through a temporary file
beside it and a rename, with the permissions of the file that was there; the `hooks` key, when it did
not exist, and new events under it go at the end.

The shim adds about 2 ms before the original status line command runs. The status line's own output,
cache files and everything that reads them keep working exactly as before.

## The shim

`c4-shim` is a small executable with no dependencies that `c4 install` copies to `~/.c4/bin/`. It has
four subcommands:

| Command | What it does |
|---|---|
| `c4-shim statusline -- <command> [args]` | Reads the status line payload from stdin, starts the original command at once with that same stdin, and lets its stdout, stderr and exit code through untouched. The copy to C4 runs on a thread beside the command, so it costs the status line nothing but the shim's own startup. |
| `c4-shim hook` | Reads the hook payload from stdin, copies it to C4, prints nothing and exits 0, whatever happened. A word after `hook` (an event name) is accepted and ignored. |
| `c4-shim endpoint` | Prints where this shim would connect, for `c4 doctor` to compare with the app's own answer. |
| `c4-shim --version` | Prints its version, so the installer can tell a stale copy from the current one. |

Each payload becomes one line of newline-delimited JSON:

```json
{"kind":"hook","recv_at":1792972800123,"pid":4242,"payload":{"session_id":"...","hook_event_name":"Stop","cwd":"..."}}
```

`kind` is `statusline` or `hook`, `recv_at` is when the shim read the payload in milliseconds since
the epoch, `pid` is the shim's own process id, and `payload` is Claude Code's JSON as it arrived, on
one line. A payload that is not a JSON object or array travels as `{"raw": "<text>"}`.

The line goes to the ingest endpoint of the C4 home (`C4_HOME`, else `~/.c4`): on Windows the named
pipe `\\.\pipe\LOCAL\c4-ingest-<hash>`, where the hash is the 64-bit FNV-1a of the home path in
lowercase with forward slashes, so two C4 homes on one machine never share a pipe; elsewhere the
socket file `~/.c4/ingest.sock`. The shim and `c4-core` each carry this rule and a test keeps them
equal. When nothing is listening the line is appended to `~/.c4/spool/<UTC date>.jsonl` instead
(one line per payload, at most 20 MiB per file, after which further lines for that day are dropped),
for the ingest server to replay when C4 starts. A payload over 1 MiB is passed to the original command whole
and not copied at all. The shim never waits long: a hook exits 900 ms after reading its payload at
the latest, inside the 1 s timeout of the synchronous SessionEnd hook, and the status line, once the
original command has finished, waits for a delivery still in progress only until 250 ms have passed
since the payload was read. Past those points the shim exits and leaves the delivery to fail on its
own. If the original command cannot be started, the reason becomes the status line text and
the shim exits 0, because a message is more use than an empty line. Setting `C4_SHIM_LOG=1` writes
one line per run to `~/.c4/logs/shim.log` with the byte count, the outcome and the exit code, never
the payload.

## The ingest server

`c4-ingest` listens on the endpoint above for as long as the app runs. On Windows it creates the pipe
with a DACL that admits the current user alone (`D:P(A;;GA;;;<user SID>)`, built from the SID of C4's
own token) and rejects remote clients; elsewhere it binds the socket file with mode 0600 inside a
0700 directory and compares the uid of every peer with the owner of the C4 home, refusing a mismatch.
The uid is the one the kernel recorded when the peer connected, because a shim has usually closed its
end before the server looks: on macOS and the BSDs the server reads it with `getpeereid`, since the
query tokio makes also asks for the peer's pid and fails once the peer is gone (the first macOS CI run
of the server tests found that). A second server on the same home is refused, so two C4 windows
cannot split one home's payloads.

One line is one payload. The parser takes `session_id`, `cwd`, `session_title` (hooks) or
`session_name` (status line), `transcript_path` and `version` into an identity observation, then the
hook event or the status line figures (model, context used, context size, effort) into a second one,
both stamped with the shim's `recv_at`. An event this version does not know is kept by name and moves
nothing; so is one whose required id is missing (the agent of a `SubagentStart`, the task of a
`TaskCreated`, the directory of a `CwdChanged`), so a malformed payload cannot create a nameless agent
or task. A hook that fires inside a subagent carries the subagent's `agent_id` beside its observations,
so the consumer can keep a subagent's tool calls from moving the session.

A line the parser refuses is counted and logged by size and reason, never by content, because a hook
payload carries whatever the session typed. A connection that stays silent for 5 s is closed; one that
sends a line over 2 MiB is dropped (the shim never sends more than 1 MiB). Parsed lines wait in a
bounded inbox of 4,096 items that drops its oldest item and counts the drop rather than hold up a
hook. The inbox keeps arrival order; after a replay that is not `recv_at` order, so `Discovery::fold`
in `c4-discovery` sorts each batch by `recv_at` before the reducer sees it. The sort is stable, so a
payload's identity stays ahead of its event, and a line more than a second older than what a view
already knows moves no state (the reducer allows that much clock skew). A session that only such a
line names gets a view with no process; nothing probes it, so it stays alive until its `SessionEnd`
arrives or a registry file names its process.

The spool is replayed once right after the server binds and every 30 s after that. A spool file is
claimed by renaming it to `<name>.replaying`, so a shim that spools its next line starts a fresh file;
it is read, checked once more for a line a shim appended during the read, and removed. A `.replaying`
file found at startup belongs to a C4 that died mid-replay and is read as it is. Replay is at least
once: a C4 that dies between reading a file and removing it reads the file again at the next start,
and the same line reaches the inbox twice. A file that cannot be renamed is left for the next pass.
A shim that opened a spool file just before the rename and finished its one write after the server's
last size check loses that line; the window is a few microseconds wide.

The app starts the server when its window opens, on a runtime of its own with two threads named
`c4-ingest`, so a slow refresh pass (the session list command can take seconds) never holds up a
connection. When another C4 already holds the home's endpoint the window's status strip says so
("ingest: another C4 holds this home's endpoint") and the app tries again every 15 s, which is how a
second window takes the endpoint over once the first closes; a server that cannot start for another
reason is reported with its error and retried at the same pace. Lines are taken from the inbox as
they arrive and folded on a blocking thread under the same lock as a refresh pass, at most one batch
every 40 ms, and each batch goes to the window as the same `sessions` event a pass emits, so a burst
of hooks is one update. A line from inside a subagent (its `agent_id` set) folds its identity only:
the subagent's own tool calls never move the session, which stays where the parent's events put it.
On exit the app stops the server, which removes its socket file on Unix. The status strip shows the
server's state and counts (lines taken in, lines replayed from the spool, drops, the age of the last
line) and `c4 doctor` reports whether something listens on the endpoint.

Not covered by a test on any platform: that a connection from another user is refused. It needs a
second account on the machine.

## Backups and removal

Before every write of `settings.json`, by install and by uninstall alike, the file is copied to
`settings.json.bak-c4-<UTC stamp>` beside it (`settings.json.bak-c4-20261026T051530Z`, with a
counter added when that name is taken). C4 never removes a backup.

`<c4 home>/install-manifest.json` records what install did:

| Field | Meaning |
|---|---|
| `schema` | 1 |
| `c4_version` | the C4 that wrote the manifest last |
| `installed_at`, `updated_at` | the first install and the last write, in milliseconds since the epoch |
| `settings_path` | the file that was installed into; another Claude home with the same C4 home is refused until this install is removed |
| `original` | the file before C4 first wrote it: `{"kind": "backup", "path": ..., "sha256": ...}`, `{"kind": "missing"}` when there was no file, or `{"kind": "unchanged", "sha256": ...}` when the first install found nothing to change |
| `after_sha256` | the SHA-256 of the file as C4 last left it |
| `restorable` | true while nothing but C4 has written the file since the original was taken |
| `hooks` | C4's hook entries, by event and command text |
| `hooks_created` | true when the `hooks` key did not exist before C4 added it |
| `statusline` | the status line command before and after the wrapper |
| `shim` | the path, the source and the SHA-256 of the shim copy |

`c4 uninstall` takes one of two routes. When `restorable` is true and the file's hash is still
`after_sha256`, nothing else has written the file since C4 did, so the original comes back byte for
byte: the backup's bytes are written over the file, or the file is removed when there was none before
C4. Otherwise, and whenever the backup is gone or no longer matches its hash, C4's entries come out
one by one: every hook handler whose command the manifest records (or that is this home's own), the
groups and events left empty by that, the `hooks` key itself only when C4 created it, and the status
line wrapper when the command is still the one the manifest wrote or still contains this home's
segment. Everything else stays, including what other installers added since. Without a manifest,
uninstall still finds this home's own shim text and takes it out. The shim copy and the manifest are
removed; the spool, the logs and the backups are kept. `--dry-run` prints the route and writes
nothing. A second install after another program rewrote the file turns `restorable` off, because the
backup no longer describes a file that C4 alone changed; from then on uninstall takes the one-by-one
route.

`c4 doctor` audits the install. Its `c4` section names the C4 home, says whether the shim copy is
the one the manifest recorded and the same bytes as the `c4-shim` beside the executable, runs the
copy with `--version` and `endpoint` (skipped by `--no-commands`) and checks that the copy would
connect where the app listens, reads the manifest, and opens `settings.json` for one purpose, C4's
own entries: how many of the 17 hook groups carry this home's shim, whether the status line is
wrapped, and whether a `c4-shim` of another C4 home is wired in. Nothing else in the file is
reported. It also says whether something listens on the endpoint and how much the spool holds, and
each finding that calls for a command names it in the notes: `c4 install` again for a wrapper
another installer removed, `c4 uninstall` with the right `--c4-home` for another home's leftovers.

## Living with other installers

Hook groups are matched by command string, so an installer that merges its own groups (such as the
one in watchthelight-labs/agents, or iTerm2's Claude Code integration) leaves C4's groups alone and C4
leaves theirs alone. An installer that overwrites `statusLine` wholesale removes the shim: the agents
installer does, on every run, because its template sets the whole `statusLine` object. C4 keeps
working from the status line's cache files and the hooks, and `c4 install` again puts the wrapper
back while keeping everything the other installer added. Test M
(`coexists_with_the_agents_installer` in `crates/c4-cli/tests/install.rs`) runs that installer twice
over a file C4 installed into (`python -I install/install.py --home <scratch>`), then `c4 install` and
`c4 uninstall` once more: C4's seventeen groups survive both runs, the agents' hooks and env keys
survive C4's uninstall, and the status line ends up as the agents template writes it. CI checks the
agents repo out at a pinned commit for that test; locally it uses `~/src/agents` or `C4_AGENTS_REPO`
and skips with a notice when neither holds the installer. `c4 doctor` reports the un-wrapped state
("the install is not whole: the status line is not wrapped; run c4 install again"); a watch on
`settings.json` that offers to re-apply the wrapper comes with a later commit.

Claude Code applies hook edits in a settings file to sessions that are already running. Tested
2026-10-25 with Claude Code 2.1.299 on Windows: a `UserPromptSubmit` command hook written to a
project's `.claude/settings.json` after a `claude -p --input-format stream-json` session had started
ran on that session's next turn, with no restart. The user-level file is documented as watched the
same way but was not part of the test. Until the first event with a session's id arrives, that session
shows "hook telemetry pending", which stays correct for a session that runs with `disableAllHooks` or
without the settings source C4 wrote to.

## Managed sessions

A session C4 starts gets the same hook groups and the status line wrapper through `--settings` on its
command line, so it is fully observed even when the global install was declined. Managed sessions run
in a detached host process that survives a C4 crash or exit; "stop sessions on quit" is an explicit
option, off by default.

Daemon-backed background sessions (`claude --bg`) are a first-class alternative: C4 lists them from
`claude agents --json` and attaches to them with `claude attach` inside a managed terminal.

## Attachment modes and what each host needs

| Host | Mode | Requirement | Verified on |
|---|---|---|---|
| C4 terminal | Managed | none | Windows (Milestone 3) |
| WezTerm | Interactive Attached | `wezterm cli` on PATH; the pane is matched through the `WEZTERM_PANE` variable of the Claude Code process | Windows |
| tmux | Interactive Attached | `tmux` on PATH; pane matched through `TMUX_PANE` | not yet |
| iTerm2 | Interactive Attached | Python API enabled in iTerm2; session matched through `ITERM_SESSION_ID` | not yet |
| kitty | Interactive Attached | remote control enabled; window matched through `KITTY_WINDOW_ID` | not yet |
| Windows Terminal | Observed | none; window focus only, there is no public tab API | Windows |
| Terminal.app, VS Code, GNOME Terminal, Alacritty, conhost | Observed | none | not yet |

An Observed session still gets a message channel: Claude Code's own peer-message socket, documented
under cross-session messaging. C4 labels such a message as "sent as a message from another process";
it is never presented as keyboard input and it cannot answer a permission prompt.

## What C4 asks of a running session

Nothing it cannot ask through a documented interface. C4 does not restart a session to pick up a new
MCP server or skill; it marks the session "restart required" and offers a restart only for sessions it
started itself. It does not approve permission prompts. It does not send keystrokes to a terminal it
has not verified.
