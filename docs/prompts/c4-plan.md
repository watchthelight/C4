<!-- <wtl> project: c4 | file: docs/prompts/c4-plan.md | author: watchthelight </wtl> -->

This is the implementation plan that answered docs/prompts/c4-directive.md, as approved on 2026-10-25, with machine-specific paths and session identifiers redacted. The living versions of its sections are the other files under docs/.

# C4: Claude Code Command Center. Implementation plan

Status: ready for review. Research complete; one background check of the Claude Code source mirror (registry status enum, peer-message wire format, daemon file formats) is still running and its findings land in Open questions 2 and 4, which tests resolve either way.

## Context

Bash runs many Claude Code sessions at once, across Windows Terminal, WezTerm, background `claude --bg` workers and, on the Legion, iTerm2 and tmux. Nothing today shows all of them in one place with their real state, what each is doing, how much context and quota they burn, which subagents they spawned, or which skills, MCP servers and toolkits they have. The statusline (`~/.claude/statusline.py`), the hook suite (`~/src/agents`), the ledger and the usage snapshot already compute most of the numbers, but only per session and only inside that session's terminal.

C4 is a real desktop application (executable `c4`, Rust core, Tauri 2 shell, xterm.js terminal panes) that discovers every reachable Claude Code session on startup, observes it through first-party data (Claude Code's own session registry, transcripts, the statusline payload and hook events), organizes sessions in an iTerm2-style Session Status sidebar, hosts real terminals for managed sessions, and controls sessions only through verified channels. It looks like an uprooted.sh product (near-black, sage, square geometry, Space Mono, pixel starfield) and behaves like an iTerm2 workstation. It ships as a public repo under the watchthelight identity with cross-platform CI and signed-where-possible releases.

The intended outcome: launch `c4` and immediately see every accessible session, what it is doing, whether it needs attention, its agents and context; open a terminal to it, inspect its workflow, watch tokens and quota, and suggest or assign the REA toolkit, without breaking anything that already works.

## Research record (verified 2026-10-25 on this machine unless marked)

### Claude Code 2.1.299 surfaces C4 can rely on

| Source | What it gives | Verified how |
|---|---|---|
| `~/.claude/sessions/<pid>.json` | Claude Code's own per-process registry: `pid, sessionId, cwd, startedAt, procStart, version, peerProtocol 1, peerFeatures [notify_idle, artifact_yield], kind interactive, entrypoint cli, pidDomain, messagingSocketPath (\\.\pipe\LOCAL\cc-msg-<hash>), name, nameSource derived/auto, nameSince, status busy, statusUpdatedAt, updatedAt`; sibling `<pid>.<hash>.key` is a secret. Records disappear when the process exits. | Read live records for PIDs 4432 and 50112 |
| `claude agents --json [--all] [--cwd]` | Scriptable list of active sessions, interactive `{pid, cwd, kind, startedAt, sessionId, name, status}` and background `{id, cwd, kind, startedAt, sessionId, name, state}`. Background entries come from state files: a dead worker (PID 6436, supervisor 22128 dead) was still listed as `blocked`. C4 must check liveness itself. | Ran it, cross-checked with System Informer |
| `~/.claude/daemon/roster.json`, `daemon.status.json`, `daemon/pty-pids`, `jobs/<short>/state.json`, `jobs/<short>/timeline.jsonl` | Background session supervisor state: pid, procStart, sessionId, cwd, launch mode (resume/prompt, fork, flags), `state/detail/tempo/inFlight/fan/tokens/needs/linkScanPath`, timeline `{at, state, detail, text}`. Contains `rvAuth`/`ptyAuth` secrets, never copied or logged. | Read files |
| CLI (`claude --help`) | `--session-id`, `-r/--resume`, `-c`, `--fork-session`, `-n/--name` (also sets terminal title), `--bg` (+ `agents`, `attach`, `logs`, `stop`, `rm`, `respawn`), `-p` with `--output-format stream-json`, `--input-format stream-json`, `--include-hook-events`, `--include-partial-messages`, `--permission-mode`, `--permission-prompts host|none`, `--remote-control`, `--tmux`, `-w/--worktree`, `--bare`, `--autocompact`, `--effort`, `--mcp-config`, `--strict-mcp-config`, `--settings`, `--add-dir`, `--plugin-dir` | Help text of the installed binary |
| Transcripts `~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl`, `<sessionId>/subagents/agent-<agentId>.jsonl` (+ `.meta.json`), `<sessionId>/subagents/workflows/wf_<id>/agent-*.jsonl` (+ `journal.jsonl`), `<sessionId>/workflows/wf_<id>.json`, `<sessionId>/tool-results/` | Shared fields `uuid, parentUuid, timestamp (ISO), sessionId, cwd, gitBranch, version, isSidechain, entrypoint, slug, promptId, requestId, agentId (agent files only), sessionKind "bg"`. `sessionId` always equals the file stem (0 mismatches in 88 main files); `cwd`, `gitBranch`, `version` can change inside a file. Content records: `user` (string or blocks `tool_result/text/image`, plus `toolUseResult` whose shape depends on the tool: Agent gives `{agentId, status "async_launched", description, resolvedModel, isAsync, outputFile}`, Bash gives `{stdout, stderr, interrupted, backgroundTaskId, persistedOutputPath, timedOutAfterMs}`, Workflow gives `{runId, transcriptDir, status}`; `permissionMode`, `promptSource typed/system/queued/sdk`, `origin.kind human/task-notification/coordinator/peer`, `isMeta`, `isCompactSummary`), `assistant` (`message {id, model, content [thinking, text, tool_use], stop_reason tool_use/null/end_turn/stop_sequence/refusal, usage {input_tokens, cache_creation_input_tokens, cache_read_input_tokens, output_tokens, cache_creation.ephemeral_1h/5m, service_tier}}`, `requestId, effort, attributionAgent/Skill/McpServer, isApiErrorMessage`; one API response spans several records sharing `message.id`, `model` can be `<synthetic>`), `system` (subtypes `compact_boundary {compactMetadata {trigger auto/manual, preTokens, postTokens, cumulativeDroppedTokens, durationMs}}`, `turn_duration {durationMs, messageCount, pendingBackgroundAgentCount, pendingWorkflowCount}` = cheapest turn-end marker, `stop_hook_summary {hookInfos [{command, durationMs}], hookErrors, preventedContinuation}`, `scheduled_task_fire`, `local_command`, `model_refusal_fallback`, `away_summary`, `informational`), `attachment` (`hook_success/hook_additional_context/hook_cancelled` for SessionStart, UserPromptSubmit, PostToolUse; `environment {platform, shell, osVersion, isGitRepo, isWorktree}`; no trace of Notification hooks). Metadata records without timestamps, re-appended each turn, last one wins: `last-prompt`, `mode`, `permission-mode {permissionMode}`, `ai-title {aiTitle}`, `agent-name {agentName}`, `atis-latch`, `cost-state {totalCostUSD, totalAPIDuration, totalLinesAdded/Removed, modelUsage {<model>: {inputTokens, outputTokens, thinkingTokens, cacheReadInputTokens, cacheCreationInputTokens, costUSD}}}`; timestamped: `pr-link {prNumber, prUrl, prRepository}`, `queue-operation`, `continued-in {continuedInSessionId}`, `file-history-*`. First line of a main file is usually `last-prompt`, `queue-operation` or `mode`, not a user message; 0-byte files exist. Agent files: every record `isSidechain true` with `agentId` = file name; `meta.json {agentType, description, toolUseId, spawnDepth, requestShape}`; depth-1 `toolUseId` resolves to a `tool_use` in the parent transcript (410 of 410); agent files can contain `compact_boundary`; workflow agents use `agentType workflow-subagent`. Retention: `cleanupOldSessionFiles` deletes transcripts older than `cleanupPeriodDays` (default 30, unset here); the oldest file is from 2026-09-25. Volume here: 1.06 GB, 88 main + 462 subagent + 221 workflow-agent files, largest live file 143 MB, full parse about 15 s, so C4 reads from saved byte offsets, ignores a partial last line and never rescans. | Read-only scan of every JSONL by the survey agent (counts and key names only), statusline.py parser, source mirror `utils/cleanup.ts` |
| Statusline payload (stdin JSON to `statusline.py`) | `model {id, display_name}, context_window {context_window_size, used_percentage, current_usage{...}}, effort, thinking, vim, agent, pr, rate_limits {five_hour, seven_day: used_percentage, resets_at}, cost {total_cost_usd, lines}, transcript_path, session_id`. The statusLine command is re-read live (no restart). | statusline.py source; a memory note |
| Hooks (`~/.claude/settings.json`) | Events wired today: SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PreCompact, Stop, Notification (types seen: permission_prompt, idle_prompt, elicitation_dialog). Stdout capped at 10,000 chars; commands run through bash even on Windows. The 2.1.299 docs say settings-file hook edits are picked up by a file watcher and that a `ConfigChange` hook can block "the new settings" from being "applied to the running session"; the memory note "hooks bind at session start only" (6dd7c792) predates this and must be re-tested in Milestone 1 before C4 claims live hook install. | settings.json, agents/docs/hooks.md, state stamps, hooks reference (code.claude.com/docs/en/hooks, fetched 2026-10-25) |
| Hook payloads C4 consumes (docs for 2.1.299) | Common: `session_id, transcript_path, cwd, permission_mode, effort.level, hook_event_name, agent_id, agent_type`. `SessionStart.source` startup/resume/clear/compact/fork plus `model, session_title, context_tokens`. `Stop`: `last_assistant_message, background_tasks[{id, type shell/subagent/monitor/workflow/teammate/cloud session/MCP task, status, description, command, agent_type, server, tool, name}], session_crons[]` (both present when the task registry is reachable, so "turn ended" and "session done" are distinguishable). `StopFailure.error`: rate_limit, overloaded, authentication_failed, oauth_org_not_allowed, account_on_hold, billing_error, invalid_request, model_not_found, server_error, max_output_tokens, cloud_credential_error, unknown. `SessionEnd.reason`: clear, resume, logout, prompt_input_exit, other (1.5 s default budget). `SubagentStart/Stop` with `agent_id, agent_type, agent_transcript_path`. `TaskCreated/TaskCompleted` with `task_id, task_subject`. `PermissionRequest` (`tool_name, tool_input, mcp_server`), `PermissionDenied`, `Notification` (`notification_type` permission_prompt, idle_prompt, elicitation_dialog, agent_needs_input, agent_completed, quota_auto_resume_*). `PreCompact.trigger` manual/auto, `PostCompact.compact_summary`. `CwdChanged old_cwd/new_cwd`, `ConfigChange source user_settings/project_settings/local_settings/policy_settings/skills`, `PreModelSwitch/PostModelSwitch from_model/to_model/source`, `MessageDisplay` (10 s timeout). `"async": true` runs a command hook in the background with no decision power and output delivered on the next turn; `-p` sessions kill async hooks at teardown. | Hooks reference, parts 1 to 3 |
| Statusline payload (docs for 2.1.299, beyond what statusline.py reads today) | `session_name` (custom `--name` or `/rename`, else the AI title; absent for derived names), `prompt_id`, `workspace.repo {host, owner, name}`, `workspace.git_worktree`, `worktree.*`, `context_window.total_input_tokens/total_output_tokens/remaining_percentage`, `prompt_cache {warm, caching_observed, ttl, expires_at, requests, misses, hit_ratio, cache_write_tokens, last_miss_cause, recache_tokens_if_cold}`, `rate_limits.spend_limit`, `fast_mode`, `pr.review_state`. Triggers: new assistant message, compact, permission mode change, vim toggle, statusLine command change (immediate, skips the 300 ms debounce), `refreshInterval` timer, `resets_at` or `expires_at` reached; an in-flight script is canceled by the next trigger; `COLUMNS`/`LINES` are set for the script. `subagentStatusLine` receives `columns` and `tasks[{id, name, type local_agent, agentType, status running/completed/failed/killed, description, label, startTime, model, effort, contextWindowSize, tokenCount, tokenSamples[16], cwd}]` per refresh tick. | Statusline reference (code.claude.com/docs/en/statusline, fetched 2026-10-25) |
| OSC 7501 Program Status Protocol | Claude Code 2.1.295+ reports `state=working|blocked|done|error|idle`, `kind=permission|question|auth` (with blocked), `app`, `title` and `msg` (base64) through `OSC 7501 ; pairs ST` to any terminal that answers the query `OSC 7501 ; ? ST` (the program sends `CSI c` right after; if the DA reply comes first, unsupported). Nothing to enable in Claude Code; off with `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`; inside tmux the check goes to tmux; background sessions do not report. Terminal must drop `working` and `blocked` on process exit or a new prompt (OSC 133 A); `done` and `error` survive. iTerm2 3.7.6 and Ghostty implement it; iTerm2 also takes `OSC 21337 ; status=..;indicator=#rrggbb;status-color=..;detail=..`. | code.claude.com/docs/en/terminal-config, superlogical.com/rex/docs/build/program-status |
| `claude agents --json` documented fields | always `cwd, kind interactive|background, startedAt (ms)`; background `id, state working|blocked|done|failed|stopped`; while the process lives `pid, status busy|waiting|idle`, `waitingFor` in `permission prompt`, `input needed`, `sandbox request`, `worker request`, `dialog open`; `sessionId`, `name` when set. Interactive sessions are listed (verified live here with two pids). Idle unattached background sessions are stopped after about an hour (working, blocked and pinned exempt); on macOS and Linux (2.1.295+) sessions stop about a minute after no supervisor runs unless `claude agents` runs again, so C4's poll can have the side effect of reviving the supervisor. Two-writer guard (2.1.248): attaching to a conversation running in another terminal is refused. `agent_needs_input` and `agent_completed` Notification types exist since 2.1.198. `~/.claude/sessions/` is documented as "one small file per running session, used to detect concurrent sessions and crashes" (format undocumented). | code.claude.com/docs/en/agent-view, claude-directory |
| Settings and live reload | "Claude Code watches your settings files and reloads them when they change ... including edits to `permissions`, `hooks`, and credential helpers"; `ConfigChange` fires per file; `model` and `effortLevel` read at start only. Precedence managed > command line (`--settings <file|json>`, 2 MiB) > project local > project > user; one `statusLine` wins by precedence; `disableAllHooks` also disables the custom status line; `allowManagedHooksOnly`. Hook handler fields: `type command|http|mcp_tool|prompt|agent`, `if`, `statusMessage`, `once`, `args`, `async`, `asyncRewake`, `shell bash|powershell`, `onFailure` (2.1.295+); `http` adds `url`, `headers`, `allowedEnvVars`. Hook env: `CLAUDE_CODE_SESSION_ID` (updated on `/clear`), `CLAUDE_PROJECT_DIR`, `CLAUDE_ENV_FILE`. `terminalSequence` output is restricted to OSC 0/1/2/9/99/777 and BEL. `CLAUDE_CONFIG_DIR` moves the whole root, so C4 resolves it before `~/.claude`. `cleanupPeriodDays` also deletes `plans/`, `debug/`, `session-env/`, `tasks/`, `shell-snapshots/`; `jobs/` and `daemon/` are kept. Live pickup of a newly added MCP server is undocumented (`/mcp reconnect` exists); SKILL.md text and `~/.claude/agents/` edits are picked up live (a new top-level folder needs `/reload-skills` or a restart); plugin changes need `/reload-plugins`. | code.claude.com/docs/en/settings, settings-reference, hooks, env-vars, mcp, skills, sub-agents |
| iTerm2 3.7.x (docs only; no Mac in reach) | Session Status: tab subtitle, dot color, detail line; states working, waiting, idle plus done and error (3.7.5); sources are OSC 21337, `it2 session set-status`, triggers, and the Claude Code integration, which installs a "cc-status" hook into `~/.claude/settings.json` (3.7.6 stopped it from replacing the file). Session Status tool: editable priority (default waiting > working > idle), snooze below a divider, one-shot bell, Notify on Status Change. Cockpit: floating cross-window panel with Status, Window and Workgroup views, a status filter bar and an `@`-addressed composer that sends one command to many sessions (Shift-Return). Workgroups: fixed-membership clusters (Peers, Splits, Tabs) with Regular, Diff and Code Review modes and a Coding Agent + Diff preset. External control: `iterm2:reveal?sessionid=<ITERM_SESSION_ID>` focuses; Python API `async_send_text` (API must be enabled, cookie through AppleScript, permission prompt for scripts launched outside iTerm2); AppleScript `write text ... newline NO` (deprecated); session variables `tty`, `jobPid`, `jobName`, `termid`. Splits Cmd-D / Cmd-Shift-D, navigate Cmd-Opt-Arrow, maximize Cmd-Shift-Enter. | iterm2.com documentation pages, fetched 2026-10-25 |
| Agent SDK terms (not used by C4, but relevant to naming) | Third parties may not offer claude.ai login without approval; products built on the SDK may not use "Claude Code" in their names and must not appear to be an Anthropic product. C4 runs the user's own `claude` CLI and reads the user's own files, so the SDK rules do not bind it, but the README should describe C4 as "a command center for Claude Code sessions" rather than present "Claude Code Command Center" as a product name. | code.claude.com/docs/en/agent-sdk/overview |
| Raw page dumps for implementation (local, read-only) | hooks part 3 `<page-dump>.txt`, statusline `<page-dump>.txt`, hooks part 2 `<page-dump>.txt`, statusline (second copy) `<page-dump>.txt`, CLI reference `<page-dump>.txt`, Remote Control `<page-dump>.txt`, MCP `<page-dump>.txt`, env vars `<page-dump>.txt`, all under `~/.claude/projects/<encoded-cwd>/<session-id>/tool-results/` | this session |
| `~/.claude/history.jsonl` | `{display, pastedContents {id, type, content | contentHash}, timestamp (ms), project, sessionId}`, one row per submitted prompt across all projects, 12,268 rows back to 2026-02 (outlives the 30-day transcript retention, so many sessionIds point at deleted files). `display` is private prompt text: C4 reads it only for the per-session "last prompt" label, never exports it. | Tail and survey |
| `~/.claude/ledger.jsonl` (turn_end.py, one row per Stop) | `{ts, t0, secs, sess (8-char prefix), cwd, project, model, main {up, cache_read, down}, agents {n, up, down, by_model, jobs[]}, files, tools, prompt (160 chars, private)}`; no row when `stop_hook_active`, no usage or no turn start. `~/.claude/cmdlog.jsonl` (audit.py, guardrails.py): `{ts, sess, cwd, tool, cmd (500 chars), ok, dur_ms, err, bg}` and guardrail rows `{event "guardrail", rule, decision}`; appended without a lock (2 corrupt rows of 25,812). Both key on an 8-character session prefix that can collide. | Survey, hook sources |
| Notification types | Emitted by the source mirror (version unknown, older than 2.1.299): `permission_prompt`, `idle_prompt`, `elicitation_dialog`, `elicitation_complete`, `elicitation_response`, `auth_success`, `worker_permission_prompt`, `computer_use_enter`, `computer_use_exit`; the docs for 2.1.299 add `agent_needs_input`, `agent_completed`, `quota_auto_resume_*`, `plugin_notification`. `turn_end.py` reacts to `permission_prompt`, `idle_prompt`, `agent_needs_input`, `elicitation_dialog` with a 60 s cooldown per type (`state/notify-<type>.stamp`). Notification hooks leave no trace in transcripts. | Survey, turn_end.py, notify.json, hooks reference |
| Hook runtimes here (p50 / p95) | caveman-activate 210 / 1,117 ms; caveman-mode-tracker 106 / 489 ms; quota_advisory 131 / 322 ms; wtl_contact_nudge 160 / 356 ms; compact_guard 356 / 4,061 ms; wtl_boot about 0.9 / 2.3 s (network, 4 s timeout); turn_end 257 / 1,093 ms. Every Python hook catches all exceptions and exits 0. A C4 hook forwarder must stay well under these and never block (async). | `stop_hook_summary` records |
| `~/.claude.json` | 100 top-level keys, 203 `projects` entries keyed by path (case-sensitive duplicates), per project `lastSessionId, lastCost, lastModelUsage, lastTotal*Tokens, lastDuration, lastSessionMetrics` (written at session end), `mcpServers` per project and global (playwright stdio, watchthelight http with an Authorization header, systeminformer stdio, rea stdio; one project-level server embeds an API key in its URL, so C4 redacts URLs as well as env and headers). | Survey, earlier read with `-AsHashtable` |
| Other `~/.claude` state | `session-env/<sid>/` folders are empty on Windows ("Session environment not yet supported on Windows"); `ide/` empty (lock files would carry `pid, ideName, transport, authToken`); `plans/<slug>.md` named by the transcript `slug`; `compact-state/<sid>.md`; `file-history/<sid>/`; `stats-cache.json` daily aggregates; `.last-cleanup`. Nothing reads `usage_snapshot.json` on this machine (the analysis UI named in the statusline comment is absent), but the file stays untouched. | Survey |
| statusline.py outputs | `%TEMP%/clstatus-<sessionId>.json` (SIDECAR_V 5: per-file token accumulators, open tool calls, turn timing, context attribution, compactions), `%TEMP%/clstatus-fable.json` (5h/weekly/fable pct + resets + 6 h of samples, fetched by a detached `--refresh-usage` worker with the OAuth token), `clstatus-peers.json`, `~/.claude/usage_snapshot.json` (consumed by the analysis UI), `~/.claude/api_spend.json`; `turn_end.py` imports `parse_usage` and writes `ledger.jsonl`; `audit.py` writes `cmdlog.jsonl`. | Source read in full |

### Process topology on this machine (Windows 11, Windows Terminal 1.24)

`WindowsTerminal.exe (15752) -> pwsh.exe -> claude.exe`, one `OpenConsole.exe` (ConPTY host) per tab also under WindowsTerminal.exe. Children of `claude.exe`: `cmd.exe -> node.exe` (MCP servers), `bash.exe -> bash.exe -> conhost.exe` (Bash tool and hooks; hook `python.exe` sits under that bash), `cmd.exe -> pwsh.exe` (PowerShell tool). `claude.exe` inherits `WT_SESSION=<guid>`, `WT_PROFILE_ID`, and carries `CLAUDECODE=1`, `CLAUDE_CODE_ENTRYPOINT=cli`; `TERM_PROGRAM` is empty under Windows Terminal. The npm install path is `%APPDATA%\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe` (Anthropic-signed).

### Terminal host control surfaces

| Host | Enumerate | Focus | Send text | Verdict |
|---|---|---|---|---|
| Windows Terminal | no public tab API; `wt -w <id>` targets windows only | window via HWND of ancestor `WindowsTerminal.exe`; `focus-tab -t <index>` needs an index C4 cannot derive from `WT_SESSION` | none | Observed (window focus best effort) |
| WezTerm (20240203-110809 here, verified live 2026-10-25) | `wezterm cli list --format json` -> `window_id, tab_id, pane_id, workspace, size {rows, cols, pixel_width, pixel_height, dpi}, title, cwd (file:// URL), cursor_x/y, cursor_shape, cursor_visibility, tab_title, window_title, is_active, is_zoomed, tty_name (null on Windows)` | `activate-pane --pane-id` | `send-text --pane-id N --no-paste` | Interactive Attached (pane id matched through the `WEZTERM_PANE` env of the claude process, since `tty_name` is null on Windows) |
| tmux | `list-panes -a -F` with `pane_pid`, `TMUX_PANE` env | `select-pane` | `send-keys -t <pane> -l` | Interactive Attached (not installed here; unverified on this box) |
| iTerm2 | Python API / AppleScript, `ITERM_SESSION_ID` env | yes | `write text` | Interactive Attached (unverified here, no Mac in reach) |
| Terminal.app | AppleScript tabs, `TERM_SESSION_ID` | yes | `do script in tab` | Interactive Attached (unverified) |
| kitty | `kitty @ ls`, `KITTY_WINDOW_ID` | `kitty @ focus-window` | `kitty @ send-text --match id:` (remote control must be enabled) | Interactive Attached when enabled (unverified) |
| VS Code terminal, GNOME Terminal, Alacritty, conhost/cmd | env or parent chain only | window focus only | none | Observed |
| `claude --bg` worker | roster/jobs files, `claude agents --json` | `claude attach` in a C4 pane | through the attached PTY | Managed (daemon-backed) |
| C4 PTY | own | own | own | Managed |

### Design sources (authentic identity)

- `~/src/labs-site/src/style.css` and `starfield.js` (built "in the old uprooted.sh style"): bg `#08080c`, panel `#0d0e12`, panel-2 `#111217`, text `#c8ccd0`, head `#e1e5e8`, dim `#8a8e96`, mute `#5a5e66`, faint `#2a2d33`, line `#1c1e23`, sage `#6a9a6e`, sage-hi `#7bc67e`, sage-deep `#4d7a51`, sage-wash `rgba(106,154,110,.08)`, amber `#b8a44a`, red `#c45454`; mono stack with Space Mono for display; 14px/1.65; square tiles with 1px faint border and an 8x8 corner square; eyebrows 12px uppercase .18em sage; state chips 11px uppercase bordered; square-pixel starfield with three parallax layers, 10% sage stars, rare shooting star, static under reduced motion.
- uprooted.sh OG image (`watchthelight/assets/art/logos/uprooted-og.png`): lowercase mono wordmark, short sage rule, dim mono tagline, thin sage bars top and bottom, faint pixel stars.
- Quiet Machine terminal palette (`~/.wezterm.lua`, `starship.toml`): fg `#C9D1C4`, bg `#0D120B`, cursor `#ECB442`, selection `#586555`, 16 ANSI colors listed in the design agent brief; JetBrainsMono Nerd Font Medium 12.
- Trestle Claude Code theme (`~/src/agents/claude/themes/trestle.json`): bg `rgb(16,19,13)`, accent `rgb(191,159,100)`, success `rgb(138,160,122)`, error `rgb(184,71,42)`.
- Gap: the original uprooted.sh website source is not on this machine (the archived DiscordDMShare app used an Inter/indigo rounded style that the user has since rejected). DESIGN-SYSTEM.md states that C4's tokens derive from the labs rebuild, the OG image and the Quiet Machine palette.

### Environment facts that shape the plan

- Toolchain present: node 25.5, npm 11.21, pnpm, python 3.11.9, git 2.52, gh 2.93 (authenticated), cmake, VS Build Tools 2022 (VC x64), WebView2 155, Windows Terminal 1.24, WezTerm. Missing: rustup/cargo (winget `Rustlang.Rustup` 1.29.1 available), tmux, 7z, signtool, WSL distros.
- `github.com/watchthelight/C4` does not exist (no collision). Global git email is a noreply address; the C4 clone must set `user.email admin@watchthelight.org` locally.
- MCP user scope: playwright, watchthelight (http), systeminformer, rea. `~/.claude.json` has project keys differing only by case; parse case-sensitively.
- REA toolkit at `<rea-root>` (`_toolkit\bin\rea.cmd mcp` registers its MCP server; project settings set `MCP_TIMEOUT` and a smaller autocompact window; skills `new-rea-project`, `rea-resume`, `rea-readme`, `rea-close`).
- Humanize skill is `~/.claude/skills/humanize/SKILL.md` only (no lint script exists); C4 ships its own `scripts/slop_lint.py`.
- The hooks and statusline source of truth is the public repo `github.com/watchthelight-labs/agents` (local clone `~/src/agents`, head `a80aa596` of 2026-10-24, ISC). Test F and test M check it out at that pinned commit in CI; locally they default to `~/src/agents`.

## Decisions

1. Stack: Tauri 2.12.5 shell, Rust core on stable 1.99.1, xterm.js 6 with the WebGL addon and DOM fallback, vanilla TypeScript with preact signals, SQLite through rusqlite. One `c4` binary plus the `c4-shim` sidecar.
2. Managed mode = a detached `c4 host` process per session so a dashboard crash never takes a session down; daemon-backed `claude --bg` sessions are a first-class option and are attached with `claude attach`.
3. Attachment matrix: Windows Terminal is Observed (window focus only); WezTerm is Interactive Attached (verified); tmux, iTerm2 and kitty are Interactive Attached but labeled unverified until run on hardware; Terminal.app, VS Code, GNOME Terminal, Alacritty and conhost are Observed. Observed sessions still get the documented peer-message channel, labeled as a message, never as keyboard input.
4. Telemetry consumes statusline.py (shim tee, sidecars, fable cache, `c4_usage.py` bridge) and reimplements nothing but the workflow graph; all of statusline.py's outputs and consumers stay untouched; no OAuth call from C4.
5. Hooks: async command hooks through the shim for the 17 events listed in Architecture, never decision hooks; the `http` hook type is an experiment, not a dependency.
6. State machine rules (a) to (d) in Architecture: no Failed from silence, no Completed with in-flight work, observed beats heuristic, coarse mode without hooks.
7. Config changes go through Suggest, Prepare, Apply with backups, inverse ops and `restart_required` by default; C4 never assumes live reload beyond what the docs state per kind (MCP servers: new sessions only; the per-kind table is in Architecture, Capability library).
8. License MIT with a generated THIRD-PARTY-NOTICES.md; every runtime dependency reviewed (section Repository).
9. Repo `~/src/c4` pushed to the new public `github.com/watchthelight/C4`, branch `main`, small lowercase commits pushed after every green pre-commit run, no trailers of any kind (Bash's a standing rule overrides the harness attribution reminder), `<wtl>` headers, plain ASCII, humanize rules enforced by `scripts/slop_lint.py`.
10. Identity: the labs-site tokens, the OG image and the Quiet Machine palette; the missing uprooted.sh source and the unknown wordmark typeface are documented gaps, not inventions.
11. Remote hosts arrive in Milestone 7 behind a flag with pairing and encryption; local discovery never needs the network.
12. Project memory: register `project:c4` in watchthelight on the first `remember` after approval and record every decision above as notes.
13. C4's terminal panes implement the OSC 7501 query reply and parse 7501 and 21337 reports, so managed and tmux-attached sessions report state first-party; hooks and the registry cover sessions in other terminals.
14. Naming: the repo and binary stay `C4`; the README and docs call it "C4, a command center for Claude Code sessions" and never present "Claude Code Command Center" as a product name, because Anthropic's terms reserve "Claude Code" for its own products. Flagged for Bash in Open questions.
15. `~/.claude/sessions/*.json`, the daemon files and the transcript format are treated as unstable: parsers ignore unknown fields, default missing ones and are gated on the `version` field, with fixtures tagged per Claude Code version.

## Architecture

### Stack

| Layer | Choice (versions verified on crates.io / npm, 2026-10-25) | Why |
|---|---|---|
| Shell | Tauri 2.12.5 (wry 0.57, tao 0.37); not the 3.0 alpha | Full CSS and canvas for the uprooted.sh identity; WebView2 present here, WKWebView on macOS, WebKitGTK 4.1 on Linux |
| Terminal renderer | @xterm/xterm 6.0.0 with addon-webgl 0.19.0 (WebGL2; `onContextLoss` disposes and falls back) and the DOM renderer as the fallback (6.0 removed the canvas renderer); a user toggle for the renderer because WebKitGTK can put WebGL on a slow path on Linux; addons fit, search, web-links, unicode11, clipboard, serialize | The only VT renderer proven on all three OSes (VS Code's) |
| Frontend | TypeScript, Vite, pnpm, @preact/signals-core, no virtual DOM | Hot paths are xterm, canvas and one virtual list |
| Core | Rust stable 1.99.1 pinned (tauri MSRV 1.90), tokio 1.54, serde, tracing | |
| PTY | portable-pty 0.9.0 (ConPTY on Windows, as WezTerm uses it) and vt100 0.18 (MIT) as the host-side screen model for snapshots; alacritty_terminal 0.26 (Apache-2.0) is the fallback if vt100 lacks a feature | |
| Discovery | sysinfo 0.39.6 (targeted refresh), notify 8.2.1, windows crate for `GetProcessTimes` | |
| Store | SQLite through rusqlite 0.41 (`bundled`, WAL, FTS5) | Range queries, aggregation, searchable history, inspectable with any SQLite tool; redb lost on FTS |
| Rejected | egui 0.37, iced 0.14, Slint 1.18 (no mature terminal renderer, design fidelity costly), GPUI 0.2 (API churn, re-evaluate at 1.0) | |

### Processes

| Process | Role |
|---|---|
| `c4` (Tauri app) | GUI by default; subcommands `host`, `sessions --json`, `telemetry`, `perf`, `install`, `uninstall`, `doctor` branch before `tauri::Builder` so no webview starts |
| `c4-shim` (sidecar, std-only Rust, bundled as `externalBin`, copied to `~/.c4/bin/` by `c4 install`) | `c4-shim statusline -- <original command>`: read stdin (1 MiB cap), send one JSON line `{kind, recv_at, payload}` to the C4 socket with a 20 ms connect timeout, on failure append to `~/.c4/spool/<date>.jsonl` (temp and rename, 20 MiB cap), then spawn the original command with the same env and stdin bytes, pipe its stdout and stderr through, propagate its exit code. `c4-shim hook`: same tee, empty stdout, exit 0 always. Startup cost about 2 ms, so the 300 ms statusline debounce and the hook budget are unaffected |
| `c4 host --session <id>` | One detached process per managed session (`DETACHED_PROCESS` and `CREATE_NEW_PROCESS_GROUP` on Windows, `setsid` elsewhere, never inside a kill-on-close job): owns the PTY, a 2 MiB raw ring buffer, the screen model, the controller lease and a local socket; registers in `~/.c4/hosts/<sid>.json`. A UI crash or exit leaves it running; "stop sessions on quit" is an explicit option |
| `python -I hooks/c4_usage.py <transcript>` | On-demand bridge that imports the user's `~/.claude/statusline.py` by path and prints `parse_usage()` as JSON, for sessions whose statusline has not run in the last 5 s; throttled per session |

### Crates

| Crate | Responsibility |
|---|---|
| `c4-core` | Typed schemas: `SessionId`, `ProcessRef {pid, proc_start}`, `Evidence {source, Observed or Heuristic, at}`, state enum, versioned event envelope, the `reduce` state machine |
| `c4-discovery` | Readers for `~/.claude/sessions`, `claude agents --json`, daemon roster and jobs (structs that omit `rvAuth`/`ptyAuth`), process scan and parent chains, transcript index (metadata only), identity reconciliation, liveness and PID-reuse checks, host identification from the claude process env (`WT_SESSION`, `WEZTERM_PANE`, `TMUX_PANE`, `ITERM_SESSION_ID`, `KITTY_WINDOW_ID`, `TERM_PROGRAM`, `VSCODE_PID`) |
| `c4-transcript` | Incremental JSONL tailer from stored byte offsets (ignores a partial last line, tolerates 0-byte files and files that vanish after 30 days), tolerant record parser gated on `version`, subagent and workflow directory scanner, workflow graph and timeline model |
| `c4-telemetry` | Statusline payload ingest, sidecar v5 and `clstatus-fable.json` readers, `cost-state` and ledger readers, quota model with explicit unknowns, the `c4_usage.py` bridge |
| `c4-ingest` | Local socket server (tokio named pipe on Windows with a current-user DACL, Unix socket mode 0600), spool replay, bounded queues (drop-oldest with a counter, never block a producer) |
| `c4-adapters` | `HostAdapter` trait: `enumerate() -> Vec<PaneInfo>`, `verify(&ProcessRef) -> Option<VerifiedPaneHandle>`, `focus(&VerifiedPaneHandle)`, `send_text(&VerifiedPaneHandle, &str)`; implementations windows_terminal (focus only), wezterm, tmux, iterm2, terminal_app, kitty, vscode (focus and `code -r`), generic |
| `c4-host` | The session host binary logic: portable-pty, ring buffer, screen model, lease, socket framing |
| `c4-controls` | Command router, target validation, leases, audit log |
| `c4-capabilities` | Library model, toolkits (REA first), config transactions with backups and inverse ops, redaction |
| `c4-store` | SQLite schema: `sessions`, `identities` (pid and proc_start history), `events` (append-only, per-session seq), `cursors` (transcript path, byte offset, size, last uuid), `usage_samples`, `agents`, `tasks`, `config_tx`, `audit`, `hosts`, `layouts`, `scrollback` (FTS5); retention by age and rows with a vacuum job |
| `c4-remote` | Pairing and encrypted transport (Milestone 7), off by default |
| `c4-shim` | The sidecar |
| `c4-cli` | Subcommand implementations shared by the `c4` binary |
| `c4-testkit` | Fixture builders, scratch claude home, fake_claude driver |
| `c4-desktop` (`apps/desktop/src-tauri`) | Tauri commands, channels, windows, tray |

### Data flow

```
~/.claude/sessions/*.json ----notify----+
claude agents --json (15 s, off path) --+      +-- c4-store (SQLite, append)
daemon/roster, jobs (enrichment) -------+--> c4-discovery --+
process scan (sysinfo, 5 s) ------------+                   |
transcripts (notify + byte offsets) -------> c4-transcript --+--> c4-core reduce --> event bus --+-- Tauri Channel patches (<= 60 Hz) --> UI store (1 commit per rAF)
c4-shim statusline / hook (socket, spool) -> c4-ingest ------+                                   +-- notifications, audit
c4 host PTY bytes --socket--> Channel<InvokeResponseBody::Raw> --> ArrayBuffer --> xterm.write  (bypasses the store)
```

PTY path: the host reads the PTY on a dedicated thread, coalesces into frames every 8 to 16 ms of at most 64 KiB, and allows at most 2 frames in flight per terminal (credit returned by the `term.write` callback). The host never stops draining the child; a slow or reconnecting UI gets a screen snapshot plus the tail from the ring buffer. The `Channel<InvokeResponseBody>` type parameter is mandatory: `Channel<Vec<u8>>` would serialize bytes as a JSON array.

### Discovery and identity

Config root: `CLAUDE_CONFIG_DIR` when set, else `~/.claude`, with a settings list for extra roots. `SessionId` is the durable key. Match order: registry (sessionId, pid, procStart, cwd) then `claude agents --json` then hook and statusline payloads (session_id, transcript_path, cwd: the authoritative link) then roster `linkScanPath` then the transcript path derived from cwd. `procStart` on Windows equals the process creation FILETIME (verified: `(Get-Process 4432).StartTime.ToFileTimeUtc()` matches the registry value); the macOS and Linux encodings are unverified. A claude process with no registry entry (`--bare`, older version) is linked to a transcript modified within 60 s under its cwd's project dir as a heuristic only.

Stale and reused: registry entry whose pid is dead becomes Disconnected (identity kept; the file is Claude Code's and is never deleted). Pid alive but creation time or exe name mismatch means a reused pid, also Disconnected. Two live pids on one sessionId (a resume in two terminals) shows a warning badge; C4 never resolves it by killing. A process's sessionId changes on `/clear` or `/resume` (process 50112 moved from 2b9d0422 to 421e24d2 today); `SessionEnd reason clear|resume` plus the next `SessionStart` re-identify it.

Attachment decision, in order: (1) C4 host registry entry -> Managed; (2) daemon background session -> Interactive Attached through `claude attach <id>` in a C4 host PTY (documented, keeps the session running); (3) adapter returns a `VerifiedPaneHandle` (constructible only from a pid-ancestry or tty match) and supports send -> Interactive Attached; (4) process alive otherwise -> Observed, plus the peer-message channel when `messagingSocketPath` exists; (5) dead or unknown -> Disconnected with a "Resume as managed" offer. Invariant: `send_text` takes a `VerifiedPaneHandle`; no API accepts a raw pid or HWND for input; no `WriteConsoleInput`, `SendInput`, ptrace or injection anywhere. WezTerm calls always pass `--no-auto-start` (without it `wezterm cli` spawns a mux server, observed today). Non-destructive continuation for a live foreign session is instruction only: "press Left on an empty prompt or type `/background`, then C4 attaches" or "`/fork` copies it".

### Session state machine

| Observed input | Transition |
|---|---|
| `UserPromptSubmit` | Working |
| `PreToolUse {tool_name, tool_use_id}` / `PostToolUse`, `PostToolUseFailure` | RunningTool(tool) / Working |
| `Notification` permission_prompt / idle_prompt / elicitation_dialog / agent_needs_input | WaitingPermission / WaitingUserInput / WaitingUserInput(elicitation) / WaitingUserInput(agent) |
| `Stop {background_tasks[], session_crons[]}` | non-empty `background_tasks` -> BackgroundAgentsActive(tasks); else Idle |
| `SubagentStart` / `SubagentStop {agent_id, agent_type, agent_transcript_path}` | agent set updated; an unmatched Start keeps BackgroundAgentsActive |
| `StopFailure {error}` | Failed(error) while the process lives |
| `SessionEnd {reason}` | `clear` and `resume`: same process, new session id, re-identify; `logout`, `prompt_input_exit`, `other`: Completed or Exited once no in-flight work is known |
| `PreCompact` / `PostCompact` | Working(compacting) |
| Process dead (pid and procStart) | Disconnected for interactive sessions; a dead background worker with job state `done` or `blocked` is Idle (parked), because the supervisor stops idle workers |
| `claude agents --json` state | working -> Working, blocked -> Waiting, done -> Idle, failed -> Failed, stopped -> Exited, always overlaid with liveness |
| Transcript records (observed, may lag) | user prompt -> Working; assistant `stop_reason tool_use` -> RunningTool; `tool_result` -> Working; `turn_duration` -> turn ended, `pendingBackgroundAgentCount > 0` -> BackgroundAgentsActive; Agent `toolUseResult.status async_launched` -> agent started |
| Statusline payload arriving | process is rendering (presence only) |
| Registry or `claude agents --json` `status` and `waitingFor` (documented: `busy`, `waiting`, `idle`; `permission prompt`, `input needed`, `sandbox request`, `worker request`, `dialog open`) | Working / WaitingPermission or WaitingUserInput(reason) / Idle, first-party and hook-free; the registry file is watched (immediate), `claude agents --json` is polled every 15 s off the critical path |
| OSC 7501 from the process inside a C4 pane or an attached tmux pane (`state`, `kind`, `msg`) and iTerm2-style OSC 21337 | working -> Working; blocked + permission -> WaitingPermission; blocked + question or auth -> WaitingUserInput; done -> Idle; error -> Failed; dropped on process exit or a new prompt. C4's xterm panes answer the `OSC 7501 ; ?` query, so every managed session reports live state with nothing installed |

Rules enforced in `c4-core::reduce`: (a) Failed never derives from silence; only `StopFailure`, job state `failed`, a non-zero exit of a managed child or an exhausted api_retry produce it; silence over 10 min while Working adds a "stalled?" heuristic badge only. (b) Completed requires `SessionEnd` or process exit and no known in-flight `background_tasks`, subagents or daemon `fan[]`. (c) A heuristic never overrides an observed fact younger than 30 s. (d) Sessions without hook telemetry run on registry `status`, transcript and sidecar, flagged "coarse". Every transition carries its `Evidence`, and the UI shows observed vs heuristic.

### Telemetry

What C4 reads: the shim payload (`context_window`, `rate_limits`, `cost`, `model`, `prompt_cache`, `session_name`, `effort`); sidecar v5 `main`, `turns`, `agents` (tokens, cache split, turn timing, context attribution, open tools, compactions, `ctx_hist`); `clstatus-fable.json` (`data.five_hour/seven_day/fable`, `samples`; older than 5 min means "quota unknown"); transcript `cost-state.modelUsage` for per-model cost and tokens (first-party, per session); `ledger.jsonl` tail for per-turn receipts. What C4 computes: the workflow graph and timeline from incremental tailing (tool_use and tool_result pairs, Agent results to subagent files, durations, `is_error`), with the same max-merge-by-`message.id` dedupe as `_take_usage` for per-node token labels; the burn-rate slope with the same rule as `burn()` on the same samples; the autocompact threshold ported from `autocompact_threshold` with the env resolved per project (REA sets 285000, the user default is 312500), labeled heuristic. Totals always come from the sidecar; a mismatch over 1 % shows a badge instead of a silent re-sum. Local token totals are session tokens, never subscription usage. C4 never calls `api/oauth/usage` and never opens `.credentials.json`. Workflow agents under `subagents/workflows/` are counted by C4 and labeled "not in statusline totals" because statusline.py and turn_end.py read only the top level. `usage_snapshot.json` is last-writer-wins across sessions, so C4 keys everything by `session_id` from the payload instead.

Hook forwarder: separate hook groups for SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, Notification, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, PreCompact, PostCompact, CwdChanged, ConfigChange, SessionEnd, all `"async": true` except SessionEnd (sync, 1 s timeout inside the 1.5 s budget), all empty stdout and exit 0. Never PermissionRequest, Elicitation, PreModelSwitch or any decision hook. On Windows the forwarder is registered with `"shell": "powershell"` if that avoids the Git Bash spawn (verified in Milestone 2; bash form otherwise). The `http` hook type (`url`, `headers`, `allowedEnvVars`, with `onFailure` from 2.1.295) would avoid a process spawn per event; its behavior when C4 is not running is unverified, so it stays an experiment in Milestone 4. Managed sessions get the same hook groups and the statusLine wrapper through `--settings` on the command line (precedence above user settings, dedupe of identical handlers), so a managed session is fully observed even when the global install was declined. The installer dedupes by command string, so it coexists with iTerm2's cc-status hook and the agents repo hooks, and it honors `disableAllHooks`, `allowManagedHooksOnly` and `--setting-sources` by showing "hook telemetry unavailable" instead of fighting them.

Installer (`c4 install`, reversible): back up `~/.claude/settings.json` to `settings.json.bak-c4-<ts>`, write temp and rename, record before and after hashes and the exact keys touched in `~/.c4/install-manifest.json`; `c4 uninstall` removes only those keys. New statusLine command keeps the bash form: `PYTHONIOENCODING=utf-8 ~/.c4/bin/c4-shim statusline -- python ~/.claude/statusline.py`. Coexistence with `~/src/agents/install/install.py`: its `merge_settings` keeps hook groups whose commands differ (ours survive) but overwrites `statusLine` unconditionally, so `c4 doctor` and a watch on settings.json detect the un-shimmed state and offer a re-apply; the one-line template change in the agents repo is Bash's call. The statusline command change applies to running sessions immediately (documented); hook pickup by running sessions is tested in Milestone 2, and until the first event with a session's id arrives the sidebar shows "hook telemetry pending", which is correct either way.

### Managed sessions

| | C4 host per session (default) | Daemon-backed (`claude --bg`, `claude attach` in a C4 PTY) |
|---|---|---|
| Survives UI crash or exit | yes, detached host | yes, supervisor-owned |
| Failure domain | one host per session | one supervisor for all; a dead supervisor left a stale roster here |
| Input and interrupt | direct PTY | through attach (double Ctrl+C or Ctrl+Z detaches instead of exiting) |
| Visible to `claude agents` and Remote Control | no | yes |
| Schema dependence | none | `agents --json` is stable, jobs files are not |

"Start as background session" is offered next to "Start managed", and every discovered daemon session attaches through `claude attach`. Semantics: interrupt = write ESC to the PTY; graceful terminate = `/exit` and Enter, or `claude stop <id>` for daemon sessions; hard terminate = `TerminateProcess` or SIGTERM on the claude process only after a typed confirmation; both only for sessions C4 started or the user explicitly chose. One `ControllerLease {holder, expires}` per session (5 s, renewable) in the host; only the holder may write; one attach client per daemon session. Every command carries `SessionId + ProcessRef + handle`; the router re-checks liveness and the handle before dispatch and refuses on mismatch.

### Controls router

| Command | Validation | Managed | Attached | Observed | Disconnected |
|---|---|---|---|---|---|
| focus | handle verified | pane | pane (`activate-pane`, `select-pane`, AppleScript) | window only (WT HWND, foreground-lock caveat) | none |
| open terminal here | cwd exists | spawn wt, wezterm or the default | same | same | same |
| send message | lease, sid and pid match, text under 1 MB | PTY write | host CLI text then Enter | peer message through `messagingSocketPath` (documented inbox; auth line on Windows; delivered as a message from another process, cannot answer permission prompts, may be held by `crossSessionInbound`), labeled as such, never as keyboard input | queued for resume |
| start / resume | `--session-id` UUID, `--name`, `-r`, `--fork-session` | yes | none | none | `claude --resume <sid>` managed, or `--resume --bg` |
| open dir / inspect git / open in editor | path inside cwd | yes | yes | yes | yes |
| assign toolkit / suggest skill | capability transaction | prepare | prepare | prepare | prepare |
| inspect agents / tokens / export | none | yes | yes | yes | yes |
| interrupt | lease, C4-started | ESC | ESC through attach | refused | none |
| terminate | lease, typed confirm, C4-started | yes | `claude stop` | refused | none |

Audit (`audit` table and `~/.c4/audit.jsonl`): ts, actor, command, target sid, pid, proc_start, mode, decision, reason; message bodies hashed, not stored.

### Capability library and config transactions

Sources: `~/.claude.json` `mcpServers` and `projects[<cwd>].mcpServers` (case-sensitive keys, order and unknown keys preserved), project `.mcp.json`, `~/.claude/settings.json` (hooks, `enabledPlugins`, env), `~/.claude/skills/*/SKILL.md`, `~/.claude/agents/*.md`, plugins, toolkits as bundles. REA first: `<rea-root>\_toolkit\bin\rea.cmd mcp` server entry, skills `new-rea-project`, `rea-resume`, `rea-readme`, `rea-close`, env `MCP_TIMEOUT` and `CLAUDE_CODE_AUTO_COMPACT_WINDOW` from `<rea-root>\.claude\settings.json`. Model: `Capability {id, kind, scope, source_file, json_pointer, state active|pending|restart_required, secret_keys (names only)}`, `Toolkit {id, capabilities[], claude_md_fragment}`. Suggest (read-only diff) -> Prepare (staged in `~/.c4/staged/<tx>.json`, exact edits shown) -> Apply (confirmation, backup to `~/.c4/backups/<ts>/`, temp and rename, parse-verify, inverse ops recorded in `config_tx`, one-click rollback). After Apply every running session in scope is `restart_required` until a SessionStart with a new pid is observed; `pending` = written, nothing restarted; `active` = session started after the write. Reload rules per kind, from the docs and shown as labels in the UI: MCP servers "applies to new sessions; in a running session run `/mcp`" (restart_required); hooks "live, watched"; skills "live for SKILL.md text; a new top-level folder needs `/reload-skills`"; agents "live within seconds; a new folder needs a restart"; plugins "`/reload-plugins`"; model and effort "next start". The labels are advice for the user only: nothing moves to `active` on a live-reload claim, and `active` always needs an observed SessionStart with a new pid, so C4 never assumes a running session reloaded anything. Redaction covers env values, headers and URLs (one local server embeds an API key in its URL); secrets are typed into a paste field that writes the file directly and never touch the store or logs.

### Performance budgets and instrumentation

| Stage | Budget |
|---|---|
| Cold start to first session list | 2 s: registry read 5 ms, process scan 150 ms, transcript stat index 300 ms per 500 files; `claude agents --json` (up to 1.5 s) off the critical path |
| Hook event to UI | 150 ms p95: shim connect and write 5 ms, parse and reduce 2 ms, channel 16 ms, DOM apply 16 ms |
| Transcript append to graph update | 500 ms |
| PTY | 8 to 16 ms frames, 2 in flight |
| Memory | 8 MiB core per session (ring and screen model) plus xterm scrollback; under 400 MiB RSS at 30 sessions |
| Idle CPU | under 1 %: no poll faster than 5 s, rAF paused when hidden |

`tracing` spans with a redaction layer, JSON logs in `~/.c4/logs`, `c4 perf` printing p50 and p95 per stage, the webview reporting rAF deltas back; criterion benches on synthetic JSONL; a soak test with 40 synthetic sessions pushing 3k events per minute through the real shim path; an xterm throughput test with a 50 MB file.

### Security rules enforced in code

Never read: `~/.claude/.credentials.json`, `sessions/*.key` except at peer-message send time (zeroized buffer, never persisted or logged), `daemon/*` auth fields, `ide/*.lock` beyond existence, `state/wtl/token`, `notify.json`, `channels/*/.env`, env values named like `*TOKEN*`, `*KEY*`, `*SECRET*`, `Authorization`. Deserialize-deny list dropped before any struct exists: `rvAuth`, `ptyAuth`, `authToken`, MCP `env` and `headers` values, `CLAUDE_CODE_MESSAGING_TOKEN`. Logs and exports pass a redaction layer (Bearer, `sk-ant`, OAuth shapes, home paths rewritten to `~`); transcripts and prompts export only on explicit user action; tests use `fixtures/` only. Sockets: Windows named pipe with a current-user DACL and remote clients rejected; Unix sockets 0600 in a private dir. Tauri CSP locked, no remote URLs in the webview, asset protocol scoped to the bundle. C4 never sets `hasTrustDialogAccepted` in `~/.claude.json`, never answers a workspace-trust or permission prompt on the user's behalf (claude-squad types a carriage return into the trust prompt; C4 does not), never offers its own claude.ai login, and never copies code from AGPL or GPL dashboards (claude-squad, opcode, cmux, Zed's terminal crates). Remote hosts off by default; pairing by short code to X25519 with pinned keys; the remote agent exposes the read-only discovery surface and controls need the same lease and validation.

## Visual specification

Identity sources: labs-site CSS and starfield (chrome), the uprooted OG image and favicon (mark, wordmark layout), Quiet Machine (terminal colors), Trestle (what Claude Code paints inside the PTY; not a C4 token). DESIGN-SYSTEM.md records the gap: the original uprooted.sh site source was not found locally, the OG wordmark typeface is not identifiable from the PNG (Space Mono 700 is used, as in the labs `.mark`), and the DiscordDMShare look (Inter, indigo, rounded, shadows) is rejected.

Deviations from the labs CSS, each listed in DESIGN-SYSTEM.md: no glow on the mark; tile hover changes border color only (no translateY, no blurred shadow); no backdrop blur; no gradients; 11px labels use `--dim` not `--mute` (`--mute` on `--bg` is 3.1:1); unicode arrows and chevrons become 8px pixel glyphs.

### Window layout

| Region | Size | Content |
|---|---|---|
| Top bar | 32px | mark + "c4", layout name, attention count, shared 5h / weekly / fable meters |
| Sidebar | 320px default, 220 min, collapses to a 36px rail of state squares, detachable into a second window | session rows, group headers, filter |
| Workspace | rest | tab strip 28px, split tree of panes, surface tabs (Library, MCP/Skills Center, Telemetry, Settings) pinned at the right end, never closable, never unmount terminal panes |
| Inspector | 360px default, collapsible | tabs Overview, Workflow, Tokens, Agents, Git, Capabilities, Log; follows pane focus by default |
| Status strip | 22px | attention count, quota meters, peer count, layout name, key hints |

Frameless with a `data-tauri-drag-region` title bar on all platforms; Windows Snap-layout behavior needs a spike, fallback is native decorations on Windows. Dialogs (Assign, Confirm, Send message, Save layout, Settings) are in-window modals over `--scrim`; native dialogs only for file pickers.

### Frontend strategy

Vanilla TypeScript plus `@preact/signals-core`, components as factory functions returning `{el, dispose}`; no virtual DOM over terminal containers. One event queue from the core, one store commit per `requestAnimationFrame`; terminal bytes bypass the store and go straight to `xterm.write`. Sidebar is a virtual list with fixed row heights (Compact 22, Standard 34, Expanded 56, header 24), visible rows plus 8 overscan, pooled nodes, `transform: translateY`. Inactive tabs are `display:none` and refit once on show; the WebGL addon lives only on panes in the active tab (hidden panes dispose it, Chromium caps contexts near 16; DOM renderer when WebGL creation fails or the context is lost). Workflow and charts: 2D canvas, time-sorted data, binary-searched visible range, cached static layer, at most one draw per frame per visible canvas, nothing while hidden.

### Sidebar

Row anatomy by density: Compact = 8px state square, name, activity glyph, ctx % (3ch), elapsed (5ch), attention dot. Standard adds a second line (activity text with an "observed" or "~" heuristic marker, active tool) and a 24px ctx mini-bar with the autocompact tick, project/branch, subagent chip. Expanded adds mode badge, model, up/down tokens, host, 3px ctx bar. 2px left attention stripe for the two waiting states and Failed.

| State | Color | 8x8 glyph | Sort weight |
|---|---|---|---|
| Waiting for permission | `--amber` | filled square with a dark inner dot, 1 Hz two-frame pulse | 100 |
| Waiting for user input | `--amber` | outlined square | 90 |
| Failed | `--red` | filled square | 80 |
| Disconnected | `--red` outline | outlined square with a 1px gap | 70 |
| Running a tool | `--sage-hi` | filled square, 2px bar advancing beneath | 40 |
| Background agents active | `--sage` | two squares offset 2px | 35 |
| Working | `--sage` | filled square | 30 |
| Idle | `--mute` | outlined square | 10 |
| Completed or exited | `--mute` | filled square | 0 |

The text label is always present (Standard and Expanded rows, aria-label and tooltip in Compact); color is never the only carrier. Attention sort key = (pinned, weight desc, waiting-since asc, last activity desc). Glyph and color change immediately; row reordering is deferred while the pointer is inside the list or a row has keyboard focus, applied on leave or after 2 s idle. Group headers use the labs `.group-head` anatomy (index in `--sage`, Space Mono title, rule, counts in `--dim`), collapsed state persisted. Filter (`/`): plain text on name, project, branch; tokens `state:`, `project:`, `tool:`, `mode:`, `host:`. Hover card after 350 ms: mode badge and reason, activity, model, ctx bar, tokens, cache %, agents, last three tool calls, cwd, branch, elapsed. Keys in the list: Up/Down, Home/End, Enter opens the terminal, Space expands subagents, `i` Inspector, `p` pin, `.` menu, `f` focus the real window; no destructive single keys. Persisted: pins, group-by, collapsed groups, density, width, collapsed state, detached bounds, notification toggles (OS notifications coalesced within 2 s). Borrowed from iTerm2's Session Status tool and Cockpit: the state priority order is editable in Settings (default as in the table); a row can be snoozed (dims and drops below a divider; any state change un-snoozes it); a one-shot bell per row fires once on the next state change; "Notify on state change" is a global toggle; the status filter bar is the `state:` token. The composer at the bottom of the detached sidebar and of the Cockpit view takes `@session`, `@tab` or `@group` targets and sends one message to every chosen Managed or Attached session with Shift+Enter, each target validated by the router; Observed targets are listed as "focus only". A "Workgroup" layout preset = the session's terminal, a pane running `git diff` in its cwd, and the Inspector Git tab, with fixed membership.

### Terminal workspace

Tab strip: square tabs, active = `--head` text with a 2px `--sage` bottom rule, state square of the primary session, pane count, close on hover, drag reorder, middle-click close, `Ctrl+1..9`. Amber is reserved for "needs you" (WezTerm's amber active tab is not carried over).

Split tree `{id, kind: leaf|split, dir: row|col, ratio, children}`, leaf = `{paneId, sessionRef, kind: pty|attached|observed|empty}`; operations split right/down, close, swap, zoom, resize, move; 1px `--line` separators; minimum pane 20 cols x 5 rows. Pane header 18px (always / hover / never): mode badge, session name, cwd tail, current tool, hover buttons. Focus ring 1px inset `--sage-hi`.

| Pane kind | Badge | Behavior |
|---|---|---|
| Managed | `MANAGED` sage chip | real PTY, full control, interrupt and terminate behind a confirm |
| Interactive Attached | `ATTACHED <host> <pane id>` sage-deep border | xterm bound to the host pane, detach and reattach |
| Observed | `OBSERVED <host>, window focus only` amber border | read-only virtualized transcript view, no prompt line, no cursor, one button "Focus window", text "No input here. Type in the real terminal." |
| Disconnected | `DISCONNECTED <reason>` red border | frozen readable buffer, actions Resume, Reconnect, Close |

Drag and drop with pointer events and a custom drag layer (HTML5 DnD differs between WebView2 and WKWebView): sources are pane headers and sidebar rows; targets are the five pane zones and the tab strip; preview is a 1px `--sage` rectangle filled `--sage-wash`. Layouts `{name, tabs[{title, tree}], sidebar, inspector}` match panes by session id first, then by slot (project, cwd, profile); presets Single, Two by two, Main + stack, Observer; a missing session restores as an empty slot with Start and Resume buttons, never a fake prompt; autosave debounced 500 ms. Fullscreen: pane zoom, focus mode (hide sidebar and inspector), window fullscreen. Search: in-pane bar with the SearchAddon (regex, case, "3/41"), "History" over the core's persisted scrollback, `Ctrl+Shift+F` across all sessions. Scrollback 10k lines default, 50k cap, full output persisted for managed PTYs, "Load earlier" at the top. Pane switching never re-mounts xterm.

### Workflow inspector

Swimlanes by actor (main, subagents in start order with nested agents indented, Tasks lane last); tool bars on the caller's lane, concurrent bars stack into sub-rows; 1px orthogonal connectors from a parent Agent bar to the child lane and back on completion. X axis elapsed since session start (wall clock in a muted row); Ctrl+wheel zoom at the cursor, drag or Shift+wheel pan, `+`/`-`/Home/End, 16px minimap, "compress idle" collapses gaps over 2 min into a hatched break; live mode follows now (1px `--sage-hi` line) until the user pans. Bars 14px Compact, 20px Standard; fill sage completed, sage-deep with an advancing edge running, amber waiting, red failed with an 8px "x"; label always present (tool name + summary using the same rules as `_tool_summary` in statusline.py, plus duration; agents and turns add up/down tokens). Heuristic values carry a "~" prefix and a dashed border. Selecting a bar opens a detail table (actor, tool, started, duration, tokens, model, status, redacted input summary, result size, parent). At most 5k bars drawn per frame.

### Telemetry views

Numbers and formulas are those of statusline.py (see Architecture: the core reads its sidecars or reproduces the same formulas); the UI must never show a number the status line would contradict.

| Chart | Form | Marks |
|---|---|---|
| Context over turns | line with 3px square points, every point labeled | threshold line "autocompact 147K" from `autocompact_threshold`, amber band from thr-20K to thr, dashed compaction markers "compact 1: 160K to 42K", dashed projection from the median of the last 8 per-turn growths, "~3 turns to compact" |
| 5h and weekly meters | 20px bars plus a 6h sample sparkline | percent, "resets 2h14m", burn "2.1%/h" (1h lookback, 5 min minimum span) and "0.8%/d", "empty in 40m" in red when it beats the reset, reset markers, peer count, fable meter when present |
| Tokens per turn | paired bars | up = fresh over cache read, down = `--sage`, last-turn delta |
| Cache ratio | line, percent | bands >= 80 sage, 50 to 80 amber, < 50 dim; warm/cold from the TTL |
| Model attribution | stacked horizontal bars | per model and per actor; subagent share = agent up / total up |
| Mix | one stacked bar | system, you, claude, summary, other, tool names; top three plus the hog |
| Cross-session comparison | sortable table plus small multiples on one y scale | session, model, ctx %, up, down, cache %, agents, work time, est. 5h share, spend |
| API spend | bar | "$12.40 / $200", red under $20 left, amber under $60; account (ledger) vs session (`total_cost_usd`) |

Labels 10px mono `--dim`; on collision keep first, last, extrema and every Nth; unknowns render the word "unknown" in `--mute` with a reason tooltip, never 0. Status line bands map to sage (< 75), amber (>= 75), red (>= 90). One redraw per rAF per visible chart, 10 fps while any visible pane streams, quota meters on the core's 60 s cache.

### Capability library and MCP/skills center

Category rail (Skills, MCP servers, Agent definitions, CLI tools, Toolkits, Workflow templates); tile grid with the labs `.tile` anatomy (1px `--faint`, 8x8 corner square, Space Mono title, `--dim` description, square chips for scope, source path, version, "in N sessions"); detail panel with README, redacted config excerpt, requirements, sessions that have it. REA is the first toolkit tile. Assign dialog steps "01 Suggest 02 Prepare 03 Apply": Suggest lists candidates with a reason line each; Prepare shows exact diffs per file, backup paths, schema validation, a per-edit restart-required flag; Apply shows "3 files, 2 backups, 1 restart required" and result chips active (sage), pending (amber outline), restart-required (amber filled). Restart-required banner (28px, 1px amber) with Restart now (managed only, confirmed), Later, Revert. MCP/Skills Center tables: name, transport, redacted command, scope, status running/stopped/error/unknown, tool count; skills: name, path, scope, trigger; every change routes through Prepare/Apply; History tab lists backups with Revert after a diff. Redaction: keys matching token, secret, key, password, authorization, cookie, bearer, or values with known prefixes (`sk-`, `ghp_`, `xox`, `AKIA`), render as "redacted (12 chars)"; copy, export and logs always use the redacted form.

### Palette, shortcuts, focus, accessibility

Palette `Ctrl+Shift+P` (`Cmd+Shift+P`), switcher `Ctrl+K`; prefixes `>` actions, `@` sessions, `#` layouts, `:` tab or pane; recents first. App chords use Ctrl+Shift on Windows and Linux so shell keys pass through; the user's WezTerm habits `Ctrl+T` (new tab) and `Ctrl+W` (close pane) stay plain and rebindable. Split right/down `Ctrl+Shift+D` / `Ctrl+Shift+E`; focus pane `Ctrl+Shift+Arrow`; tabs `Ctrl+Tab`, `Ctrl+1..9`; zoom `Ctrl+Shift+Enter`; focus mode `Ctrl+Shift+F`; find `Ctrl+F`; regions Sidebar/Inspector/Library/Center/Telemetry `Ctrl+Shift+B/I/L/M/Y`; cycle regions F6, Esc back to the terminal; save layout `Ctrl+Shift+S`; density `Ctrl+Shift+=`; interrupt and terminate only through the palette or pane menu with a confirm. macOS uses Cmd for the same chords. One focus owner in the store; xterm's textarea is blurred while any overlay is open and refocused on close. `prefers-reduced-motion` stops drift, transitions and the pulse. Contrast on `--bg`: head 15.8:1, text 12.4:1, dim 6.1:1, sage-hi 9.7:1, sage 6.2:1, amber 8.0:1, red 4.5:1, mute decorative only. Sidebar is `role=listbox` with per-row labels; a polite live region announces attention changes (one per 2 s); each chart has an adjacent hidden table from the same data; xterm screen reader mode is a setting.

### Motion and starfield

Starfield on one canvas behind the sidebar, status strip and surface tabs only; the pane host is opaque (#0D120B) so stars never sit behind terminal text. Parameters as sourced from starfield.js: three layers (parallax .25/.5/1, drift .04/.07/.12, size 1/1/2), star rgb(200,210,220), 10% sage rgb(122,170,126), twinkle alpha .15 to .65, count = area/7000 capped at 400, dpr capped at 2, square `fillRect` stars; shooting star every 7 to 16 s and cursor parallax on surface tabs only. 30 fps, 1 ms budget; frozen when the window is hidden, when any visible pane received bytes in the last 250 ms, during splitter drags, and under reduced motion. Transitions `--t-fast` 100 ms, `--t-base` 150 ms, `--t-slow` 200 ms; animate only opacity, transform, border-color and small background-color; never width or height; the only repeating motion is the permission pulse and the tool-activity bar.

### DESIGN-SYSTEM.md token set

Chrome colors: `--bg` #08080c, `--panel` #0d0e12, `--panel-2` #111217, `--head` #e1e5e8, `--text` #c8ccd0, `--dim` #8a8e96, `--mute` #5a5e66, `--faint` #2a2d33, `--line` #1c1e23, `--sage` #6a9a6e, `--sage-hi` #7bc67e, `--sage-deep` #4d7a51, `--sage-wash` rgba(106,154,110,.08), `--amber` #b8a44a, `--red` #c45454, `--star` rgb(200,210,220), `--star-sage` rgb(122,170,126), `--scrim` rgba(8,8,12,.72). Terminal (Quiet Machine): bg #0D120B, fg #C9D1C4, cursor #ECB442, selection #586555, ANSI #161C13 #B5563F #8A9A5B #ECB442 #6E8FA0 #A07089 #7FB0A8 #C9D1C4, bright #586555 #E06A4F #A8C46A #F6C95A #8FB8C9 #C68AA0 #9AD0C6 #ECF0E6; never used in chrome. Chart categorical `--cat-1..6`: #7bc67e #7FB0A8 #6E8FA0 #A07089 #b8a44a #c8ccd0.

Type: `--display` Space Mono 400/700 (eyebrows, titles, wordmark, stat values); `--mono` JetBrains Mono bundled with fallbacks Cascadia Mono, Consolas, Liberation Mono; terminal font JetBrainsMono Nerd Font Medium via `local()` when installed, else bundled JetBrains Mono, 12px, line height 1.05, ligatures on. Sizes 10 to 28; letter spacing eyebrow .18em, chips .08em; eyebrows and chips uppercase. Spacing `--s-1..8` = 2, 4, 6, 8, 12, 16, 24, 32; density rows 22/34/56, controls 20/24/28. `--radius: 0` everywhere; 1px `--faint` on panels, chips, inputs, tiles; elevation by 1px borders and `0 0 0 1px` rings only; focus `outline: 1px solid var(--sage-hi); outline-offset: 3px`. Icons are square pixel glyphs on 8x8 or 12x12 grids as SVG rects in `currentColor`; the mark is a 10x10 `--sage` square beside "c4"; the app icon reproduces the favicon (centered sage square, three star pixels). Fonts ship under `src/assets/fonts/` with each OFL.txt.

Do not: rounded corners, gradients, blur, glow, drop shadows, hover lift, unicode arrows or em dashes in UI text, "at a glance", color as the only carrier of state, fake prompts or cursors in non-interactive panes, numbers that disagree with the status line, starfield behind terminal text, amber for anything other than "needs you".

### Synthetic screenshots

`fixtures/sessions/` holds generated transcripts (seeded PRNG) for invented projects with fake paths, tool calls, usage, one compaction, one failed tool, one permission wait, two subagents, plus a fixture usage cache and usage snapshot. `C4_FIXTURE=1` makes the core serve fixture discovery; managed panes replay an ANSI transcript through a scripted fake shell. Playwright shots at 1440x900 and 1920x1080, dpr 2, fixed clock, seeded starfield, shooting stars off: sidebar in three densities, 2x2 workspace, Observed pane, workflow, telemetry, library, assign dialog, palette; written to `docs/screenshots/` with a line stating no real transcript is used.

## Repository, CI, tests, release

### Layout (`~\src\c4`, remote `github.com/watchthelight/C4`, branch `main`)

```
C4/
  .github/workflows/pr.yml           every PR: lint + clippy + cargo test --lib on ubuntu (~5 min)
  .github/workflows/verify.yml       push to main, weekly, dispatch: lint, 4-OS test, smoke, bench-smoke
  .github/workflows/release.yml      tags v*: build matrix, SHA256SUMS, attestation, undraft
  .github/workflows/post-release.yml release published: download, sha256sum -c, gh attestation verify
  .github/dependabot.yml  ISSUE_TEMPLATE/  PULL_REQUEST_TEMPLATE.md  commit-template.txt
  .githooks/pre-commit, commit-msg   thin wrappers over scripts/precommit.py
  Cargo.toml Cargo.lock rust-toolchain.toml deny.toml about.toml about.hbs
  package.json pnpm-workspace.yaml pnpm-lock.yaml .npmrc
  crates/c4-core          session, agent, host, event types; state machine (reduce); event bus
  crates/c4-discovery     registry scanner, agents-json poller, daemon enrichment, process probe (sysinfo), identity reconciliation
  crates/c4-transcript    incremental JSONL tailer (notify, byte offsets), subagent and workflow scanner, workflow graph model
  crates/c4-telemetry     statusline payload + sidecar readers, quota windows, usage helper bridge
  crates/c4-ingest        local socket server, spool replay, bounded queues
  crates/c4-adapters      HostAdapter trait: wezterm, windows-terminal, tmux, iterm2, terminal-app, kitty, vscode, observed
  crates/c4-host          `c4 host`: portable-pty, ring buffer, screen model (vt100), controller lease, socket framing
  crates/c4-controls      command router, target validation, leases, audit log
  crates/c4-capabilities  skills, MCP, agents, toolkits library; settings merge with backup and revert
  crates/c4-store         SQLite persistence: sessions, events, cursors, usage, layouts, pins, groups, scrollback (FTS5)
  crates/c4-remote        pairing and encrypted transport (Milestone 7, off by default)
  crates/c4-shim          the std-only sidecar: statusline and hook tee with spool
  crates/c4-cli           headless `c4`: sessions --json, telemetry, perf, install, uninstall, doctor
  crates/c4-testkit       fixture builders, scratch claude home, fake-claude driver (dev only)
  apps/desktop/src-tauri  c4-desktop crate, tauri.conf.json, capabilities/, icons/, externalBin c4-shim
  apps/desktop/src        TS + Vite + xterm.js; src/bridge/ (commands, channels), src/bridge/mock.ts
  apps/desktop/tests      vitest;  apps/desktop/e2e  Playwright (mock bridge) and wdio/ (built app)
  hooks/c4_usage.py       imports the user's statusline.py by path, calls parse_usage, prints JSON (stdlib only)
  hooks/tests/            python -m unittest
  fixtures/registry/ projects/ sidecars/ agents-json/ settings/ statusline-payload/   all synthetic
  scripts/precommit.py scan.py slop_lint.py check_headers.py gen_notices.py checksums.py
          changelog_section.py bump_version.py fake_claude.py loadgen.py screenshots.py verify_release.py
  assets/fonts/SpaceMono (+OFL.txt)  assets/fonts/JetBrainsMono (+OFL.txt)  assets/icons/
  docs/  README.md  CHANGELOG.md  CONTRIBUTING.md  LICENSE  THIRD-PARTY-NOTICES.md  CLAUDE.md
```

`docs/` holds the required files plus `adapters/<host>.md`, `config/` (example `c4.toml`, example hook group, statusLine before and after), `screenshots/` (synthetic, from `scripts/screenshots.py`), `prompts/` (the directive and the three plan sections, home paths and session ids redacted), `specs/` (data model, IPC command table), `VERIFICATION.md` (tests A to M and the manual checklist with a result log). Every text file carries a `<wtl>` header in its comment style; `scripts/check_headers.py` enforces it.

Fixtures copy observed key names only: registry entry (`pid, sessionId, cwd, startedAt, procStart, version, peerProtocol, peerFeatures, kind, entrypoint, pidDomain, messagingSocketPath, name, nameSource, nameSince, updatedAt, status, statusUpdatedAt`), `claude agents --json` elements, sidecar (`_v, main, turns, agents`), transcripts with the several-records-per-message-id pattern, dummy `<pid>.<hash>.key` files that the scanner must ignore. All values generated (uuid4, `/home/example/proj`, `C:/Users/example/proj`).

`.gitignore`: `target/ node_modules/ dist/ apps/desktop/src-tauri/gen/ coverage/ playwright-report/ test-results/ *.log .env* *.pem *.pfx *.p12 *.key !fixtures/registry/*.key *.bak* .claude/settings.local.json perf/results/`. `scripts/scan.py` mirrors `~/src/agents/tools/scan.py` (same patterns, `.scanignore`, `scan:allow`) and adds `claudeAiOauth`, `accessToken`, real `messagingSocketPath` values, `users.noreply.github.com`.

### Licensing

C4 is MIT (`Copyright (c) 2026 watchthelight`). Runtime dependencies verified 2026-10-25: tauri 2.12.5 and wry (Apache-2.0 OR MIT), tao 0.37 (Apache-2.0 only: ship its license text), tauri plugins shell, window-state, single-instance, store, dialog, fs, notification, opener (Apache-2.0 OR MIT), portable-pty 0.9 (MIT), vt100 0.18 (MIT), sysinfo 0.39 (MIT), notify 8.2 (CC0-1.0), tokio, serde, tracing (MIT or dual), redb 4.4 (MIT OR Apache-2.0) or rusqlite 0.41 (MIT, bundled SQLite public domain), @xterm/xterm 6.0 and addons webgl, fit, search, web-links, unicode11, clipboard (MIT), JetBrains Mono and Space Mono (OFL-1.1, texts shipped, never renamed). No icon library: glyphs are in-house pixel SVGs. Dev only: Playwright (Apache-2.0), vitest (MIT), @wdio/tauri-service (MIT), criterion, cargo-about, cargo-deny. `THIRD-PARTY-NOTICES.md` is generated by `scripts/gen_notices.py` (`cargo about generate` plus `pnpm licenses list --json` plus the two OFL texts) and CI fails when it is stale; `deny.toml` allowlist MIT, Apache-2.0, ISC, CC0-1.0, OFL-1.1, Unicode-3.0, BSD-2/3, Zlib, MPL-2.0 only on need, plus advisories. `tauri.conf.json` `bundle.licenseFile` points at LICENSE; the notices file ships as a resource shown in the About pane.

### Tests A to M

Harness: `scripts/fake_claude.py` (`--home <scratch> --cwd <fake project> --session <uuid> --timeline <json>`) writes a registry entry with its own pid, appends transcript records (duplicated message ids, tool_use blocks, timestamps), creates `subagents/agent-*.jsonl`, flips `status` on schedule, exits or waits for a signal. The core reads `C4_CLAUDE_HOME` and `C4_AGENTS_CMD` (a script printing fixture arrays) so no test touches `~/.claude`. Frontend: `src/bridge/mock.ts` implements the same command and event names from fixtures; vitest uses `mockIPC` from `@tauri-apps/api/mocks`; Playwright runs the Vite dev server with `VITE_C4_BRIDGE=mock`. Built-app smoke: WebdriverIO with `@wdio/tauri-service` (Windows: tauri-driver plus msedgedriver; Linux: `webkit2gtk-driver` under `xvfb-run`; macOS: embedded provider, debug builds only, `continue-on-error` until the first green run).

| Test | Automation | CI OS | Status |
|---|---|---|---|
| A existing sessions appear | fake_claude starts 3 sessions before `c4 sessions --json` and the smoke test | cli on all four, smoke on win and linux | real external terminals manual |
| B new session appears | start fake_claude after discovery runs; event within 2 s | all four | verified by test |
| C live status | timeline flips status; assert event order and sidebar badge | all four, Playwright on ubuntu | verified |
| D agents and subagents | subagent files appear mid-run; agents-json fixture returns background agents; per-agent totals | all four | verified; real `claude agents` manual |
| E sidebar interactions | vitest component tests plus Playwright (click, double-click, search, filter, group, pin) | ubuntu Playwright, vitest everywhere | verified |
| F token reconciliation | `hooks/tests/test_reconcile.py` runs `parse_usage` from the agents repo statusline.py (CI checkout of watchthelight-labs/agents at a pinned commit under `vendor/agents`, local default `~/src/agents`, override `C4_STATUSLINE_PY`) with `TEMP` set to scratch, compares with `c4 telemetry --transcript --json`; zero tolerance on input, cache_creation, cache_read, output, per agent; also asserts the shim passes the payload and the sidecar through unchanged | all four | verified; skipped with a visible notice if the agents repo is unreachable |
| G REA toolkit assignment | apply the suggestion to a scratch settings.json; assert `settings.json.bak-<time>`, exact JSON diff, byte-identical revert | all four | verified |
| H managed PTY | spawn a real shell (`cmd.exe`, `pwsh`, `sh`), write, read echo, send 0x03 to a long command, terminate, assert exit | all four | ConPTY on hosted Windows runners unverified until the first green run |
| I observed fallback | registry entry whose host cannot be matched becomes Observed; no input affordance in the UI | all four, ubuntu Playwright | verified |
| J recovery | store round trip; smoke: pin, group, split, relaunch, assert persisted and rediscovered | store everywhere, smoke on win and linux | verified where smoke runs |
| K performance | criterion benches (parse 50k JSONL lines, scan 200 registry entries); `scripts/loadgen.py` spawns N fake sessions; `c4 perf` reports discovery ms, event-to-UI p95 (frontend echoes receipt time), FPS, idle CPU, RSS | bench `--test` on ubuntu; full numbers local and weekly | thresholds enforced locally |
| L publishing | `scripts/verify_release.py <tag>`: visibility, verify.yml conclusions, asset list, SHA256SUMS, `gh attestation verify`, docs present | ubuntu, post-release | verified after v0.1.0 |
| M existing setup keeps working | scratch home seeded with `fixtures/settings/full.json`; C4 install; then the agents repo `install.py --home scratch` twice; assert hooks, skills, MCP keys untouched and the statusline still runs | all four | verified; see risk 1 |

Manual checklist in `docs/VERIFICATION.md`: A, B, C with Windows Terminal and WezTerm on this machine; M against the real `~/.claude` with a dry run first; H interactive feel; iTerm2, Terminal.app, tmux, kitty rows stay "unverified, no hardware" until someone runs the checklist and files the result.

### CI

`rust-toolchain.toml` pins stable 1.99.1 (tauri 2.12.5 MSRV is 1.90) with rustfmt and clippy; `dtolnay/rust-toolchain` reads the file; `Swatinem/rust-cache@v2`; `pnpm/action-setup@v4` from `packageManager`; `actions/setup-node@v6` with pnpm cache; `actions/checkout@v7`; `actions/setup-python@v5` with 3.11; lockfiles committed; Dependabot weekly, grouped. Linux apt: `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils`, plus `webkit2gtk-driver xvfb` for smoke. Every job has `timeout-minutes`, `permissions: contents: read` by default, and `concurrency` with cancel-in-progress.

| Job (verify.yml) | Runner | Steps |
|---|---|---|
| lint | ubuntu-latest | check_headers, slop_lint, scan, `cargo fmt --check`, `cargo deny check`, notices freshness, `pnpm lint`, `pnpm typecheck`, `python -m compileall hooks scripts`, `python -m unittest discover hooks/tests` |
| test | windows-latest, ubuntu-latest, macos-latest (arm64), macos-15-intel (x64), fail-fast off | `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm test`, Playwright on ubuntu, `cargo tauri build --debug --no-bundle` |
| smoke | windows-latest, ubuntu-latest, macos-latest (continue-on-error) | tauri-driver, wdio |
| bench-smoke | ubuntu-latest | `cargo bench --workspace -- --test` |

`scripts/slop_lint.py` on `*.md`, `docs/`, frontend string tables and commit messages: fail on U+2014 and U+2013, "at a glance", arrow code points, curly quotes, any non-ASCII outside fenced code and Mermaid blocks, `Co-Authored-By`, `Generated with`, `Generated-By`, "it's worth noting", "here's the kicker", "the truth is simple", "think of it as"; warn on "tapestry", "landscape", "delve", "not just", a tricolon closing a paragraph, "The X? A Y." headings. Same HARD list as `lint_public.py` on the Hackintosh box.

### Release

`release.yml` on `v*` tags: build matrix windows-latest (nsis + msi), macos-latest twice with `--target aarch64-apple-darwin` and `--target x86_64-apple-darwin` (dmg + app), ubuntu-22.04 (AppImage + deb, low glibc floor); `tauri-apps/tauri-action@v1` with `releaseDraft: true`, `uploadUpdaterJson: false`, release body from `scripts/changelog_section.py`. `finalize` downloads the draft assets, writes `SHA256SUMS`, runs `actions/attest-build-provenance@v4` (`subject-checksums`, fallback `subject-path`; SLSA Build L2), then undrafts, so a half-built release is never visible. `post-release.yml` re-downloads everything and runs `sha256sum -c` and `gh attestation verify <file> --repo watchthelight/C4`. Signing: Windows unsigned (SmartScreen note in the README; `bundle.windows.signCommand` left as the hook for a certificate later), macOS ad-hoc (`signingIdentity "-"`, quarantine note, notarization documented as absent), Linux unsigned; checksums plus attestations are the integrity story everywhere. SemVer 0.y.z; one version in `Cargo.toml`, `tauri.conf.json`, `package.json` kept in step by `scripts/bump_version.py`; CI fails on drift. Asset names (Tauri defaults, confirmed on the first draft): `C4_0.1.0_x64-setup.exe`, `C4_0.1.0_x64_en-US.msi`, `C4_0.1.0_aarch64.dmg`, `C4_0.1.0_x64.dmg`, `C4_0.1.0_amd64.AppImage`, `C4_0.1.0_amd64.deb`, `SHA256SUMS`. Release notes carry a platform table: Windows x64 built, tested, manually verified, unsigned; macOS arm64 and x64 built and CI-tested, not run on hardware, unverified; Linux x64 built and CI-tested, not run on a desktop, unverified. No updater at first; the About pane never calls the network.

### Commit and push discipline

Once per clone: `git config --local user.name watchthelight`, `user.email admin@watchthelight.org`, `core.hooksPath .githooks`, `commit.template .github/commit-template.txt`. Push after every green commit. Never force-push, never `--no-verify`, never amend a pushed commit, never rewrite history. No `Co-Authored-By`, no "Generated with", no AI attribution of any kind in commits, PRs or files (Bash's standing a standing rule overrides the harness attribution reminder). `scripts/precommit.py`: identity check; staged-path denylist (`.claude/`, `*.jsonl` outside `fixtures/`, key and certificate files outside `fixtures/registry/`, `.env*`, `settings.local.json`, `*.bak*`, `credentials*`); scan regexes over added lines; header and slop lint on staged files; `cargo fmt --check` and `cargo test --workspace --lib` (full workspace with `C4_PRECOMMIT_FULL=1`); `pnpm test` when `apps/` is staged; `python -m unittest` when `hooks/` is staged. `.githooks/commit-msg` rejects attribution trailers, `Signed-off-by`, em dashes, non-ASCII, subjects over 72 characters or starting with a capital; it does not reject the words Claude or Anthropic, since the product is about Claude Code. `CLAUDE.md` in the repo restates these rules for future sessions.

### Documentation

| File | Content | Diagrams (Mermaid) |
|---|---|---|
| README.md | purpose paragraph, synthetic screenshot, install per OS with SmartScreen and quarantine notes, checksum and attestation verification, quick start, how C4 finds sessions, adapter table with a verified column, `c4.toml`, docs index, license | Claude Code files to discovery to UI |
| docs/HOW-IT-WORKS.md | data sources with key tables, discovery loop and timing, status rules, what C4 never reads | session state diagram, discovery sequence |
| docs/ARCHITECTURE.md | crate graph, event flow, tasks and threads, IPC command table, store schema, HostAdapter trait, errors, performance budget | crate graph, file-event-to-UI sequence, adapter class diagram |
| docs/SESSION-INTEGRATION.md | what install changes (hook group, statusLine wrapper) with before and after JSON, compatibility with install.py's dedupe-by-command merge, backup and revert, attachment modes, per-host requirements | install and revert flow, hook event sequence |
| docs/DESIGN-SYSTEM.md | tokens, type, states, components, motion, keyboard, accessibility, deviations, gaps, screenshot gallery | none |
| docs/SECURITY.md | local-only threat model, read-only transcript access, paths never read, hook input validation, store contents, no network by default, reporting via GitHub security advisories | trust boundary data flow |
| docs/BUILDING.md | prerequisites per OS, `pnpm tauri dev`, tests, benches, notices regeneration, local release build, troubleshooting | CI pipeline |
| CHANGELOG.md | Keep a Changelog 1.1.0 with compare links | none |
| CONTRIBUTING.md | setup and hooks, style (plain ASCII, no em dashes, `->`, straight quotes, headers, no trailers, lowercase subjects), tests per crate, PR checklist, the humanize anti-patterns in brief | none |

### Risks

| Risk | Handling |
|---|---|
| `install.py` line 54 overwrites `statusLine` unconditionally, so re-running the agents installer removes C4's shim (hooks survive because the merge dedupes by command string) | Test M covers it; `c4 doctor` detects the unwrapped state and offers to re-wrap; C4 keeps working from sidecars and the hook alone. Open question for Bash: add the shim to the agents repo template instead |
| ConPTY on hosted Windows runners | test H marked until the first green run |
| macOS WebDriver smoke unproven | `continue-on-error` until green |
| Intel macOS runners may be retired | pin `macos-15-intel`; if removed, x64 becomes unverified and the table says so |
| registry and `claude agents --json` schemas may change | fixtures tagged with 2.1.299; parsers ignore unknown fields and default missing ones; weekly verify run |
| action inputs and asset name placeholders unconfirmed | confirm on `v0.1.0-rc.1` |
| agents repo visibility for test F | check `gh repo view watchthelight-labs/agents --json visibility` before wiring the CI checkout |
| `.key` registry files | never read or copied; dummy fixtures prove the scanner ignores them |

## Prerequisites that need approval (done first, in this order)

1. Install the Rust toolchain: `winget install Rustlang.Rustup` then `rustup default stable`, `cargo install tauri-cli cargo-deny cargo-about --locked` (tauri-driver later for smoke tests). Node 25, pnpm, Python 3.11, VS Build Tools and WebView2 are already present.
2. Create `~/src/c4`, `git init -b main`, set `user.name watchthelight` and `user.email admin@watchthelight.org` locally, `gh repo create watchthelight/C4 --public --source . --push` after the first commit (the name was free on 2026-10-25; a collision is reported, never worked around).
3. In Milestone 2, `c4 install` edits `~/.claude/settings.json` (backup first, manifest-based uninstall) to add the async hook groups and wrap the statusLine command. Nothing else in `~/.claude` is modified, ever.
4. Tests that need a live Claude Code process start throwaway sessions (`claude -p`, `claude --bg`) in scratch directories; existing sessions are never restarted or killed.
5. Register `project:c4` in watchthelight memory with the decisions above.
6. Housekeeping: the research probe left a `wezterm-mux-server.exe` (PID 42064, 7.5 MB) running; kill it or leave it, your call.
7. Separate, your call: a one-line change in `~/src/agents/claude/settings.template.json` so the agents installer writes the shim form of `statusLine` instead of un-shimming C4 on every rerun.

## Milestones and commit plan

Each milestone ends green on `verify.yml`, with its tests in `docs/VERIFICATION.md`, a CHANGELOG entry, and a tag from Milestone 2 on. Commits are small and lowercase (`area: change`), pushed after each green pre-commit run.

| Milestone | Deliverables | Tests | Tag |
|---|---|---|---|
| 1 Foundation | workspace, toolchain pin, scripts (precommit, scan, slop_lint, check_headers, gen_notices), `pr.yml` and `verify.yml`, docs skeleton with the redacted directive, `c4-core` types and reducer, synthetic fixtures, `c4-discovery` (registry, agents-json, daemon enrichment, process probe, transcript index, reconciliation), `c4-testkit` with fake_claude, `c4 sessions --json`, Tauri shell that lists sessions through the core, hook-pickup experiment (does a running session see a new settings hook group?) | A, B headless; M harness | none |
| 2 Sidebar | tokens.css, starfield, top bar and status strip, virtual-list sidebar with the nine states, grouping, filter, pins, density, hover card, detached window, persistence; `c4-shim`, `c4-ingest`, `c4 install|uninstall|doctor`; live states from hooks and the statusline payload; notifications | C, D (states), E, I, M | v0.1.0 |
| 3 Terminal workspace | `c4 host`, PTY byte path, xterm panes answering OSC 7501 queries and parsing 7501 and 21337 reports, tabs, split tree, drag and drop, layouts and presets, zoom and focus mode, search and persisted scrollback, managed start and resume, `claude attach` for daemon sessions, WezTerm adapter, Windows Terminal focus, command palette and shortcuts | H, J | v0.2.0 |
| 4 Telemetry | sidecar and fable readers, payload ingest, `c4_usage.py` bridge, context and quota charts, tokens per turn, cache, model attribution from `cost-state`, mix, cross-session table, API spend; `http` hook experiment measured | F | v0.3.0 |
| 5 Agents and workflows | incremental tailer, subagent and workflow scanner, workflow canvas and timeline, Agents tab, background tasks from `Stop` payloads and `turn_duration` | D full | v0.4.0 |
| 6 Capability management | library surfaces, REA toolkit, Suggest, Prepare, Apply, MCP/Skills Center with backups and revert, restart-required banner, redaction | G | v0.5.0 |
| 7 Cross-platform | tmux, iTerm2, kitty, Terminal.app adapters with unverified labels, macOS and Linux CI smoke, `c4-remote` pairing behind a flag, perf soak and `c4 perf` numbers in VERIFICATION.md | K | v0.6.0 |
| 8 Production release | all required docs final, synthetic screenshots, VERIFICATION.md results, `v1.0.0-rc.1` through the release pipeline, checksums and attestations verified, `v1.0.0` only if Windows is manually verified and all four CI test jobs are green; otherwise `v0.7.0` with the platform table saying so | L | v1.0.0 |

Milestone 1 commit sequence: `init: cargo workspace, toolchain pin, license and readme stub`; `scripts: precommit, secret scan, slop lint and header check`; `ci: pr and verify workflows with lint job`; `docs: skeleton for required docs and redacted directive`; `core: session, agent, host and event types with serde tests`; `fixtures: synthetic registry, transcript, sidecar and agents json`; `discovery: registry scanner over a claude home, ignores key files`; `transcript: incremental tailer with subagent directory watch`; `discovery: process liveness probe and stale entry rule`; `testkit: fake claude stand-in and scratch home harness`; `cli: c4 sessions --json and c4 telemetry`; `desktop: tauri shell listing sessions through the core`; `ci: three os test matrix, caching, vitest and playwright`; `docs: how-it-works and architecture with diagrams`; `changelog: unreleased section for milestone 1`. Later milestones follow the same grain.

## Verification

Automated: `cargo test --workspace`, `pnpm test`, Playwright on the mock bridge, wdio smoke on the built app, `python -m unittest discover hooks/tests`, `cargo bench -- --test`, all wired in `verify.yml`; the A to M mapping and status columns live in the Repository section and in `docs/VERIFICATION.md`.

Manual on this machine, recorded in `docs/VERIFICATION.md`:

1. Start `c4` while this session (<session-a>, pid 50112) and <session-b> (pid 4432) run: both rows appear within 2 s with name, cwd, host "Windows Terminal", mode Observed, state from the registry; the stale background job 3a2c7cea shows Disconnected, not blocked (test A).
2. Open a new `claude` in WezTerm: it appears within 2 s, mode Interactive Attached with pane id; "send message" delivers to a throwaway session only (tests B, I).
3. After `c4 install`: the sidebar context percentage equals the status line's meter in the same terminal; `~/.claude/usage_snapshot.json`, `clstatus-*.json`, `ledger.jsonl` keep updating; `c4 uninstall` restores settings.json byte for byte except for the removed keys; `python ~/src/agents/install/install.py --dry-run` lists no change beyond statusLine (tests F, M).
4. Managed terminal: start a managed `claude` in a scratch dir, type, interrupt with ESC, close the C4 window, reopen, the session is still there with its scrollback (tests H, J).
5. Performance: `scripts/loadgen.py --sessions 40 --events-per-min 3000` with `c4 perf`; record discovery ms, hook-to-UI p95, FPS, idle CPU, RSS (test K).
6. Publishing: `scripts/verify_release.py <tag>` after each tag; `sha256sum -c SHA256SUMS` and `gh attestation verify` on a downloaded asset (test L).
7. REA: in a scratch project, Suggest shows REA with its reason, Prepare shows the exact diffs and backup paths, Apply writes them, the session shows restart-required, Revert restores (test G).

## Open questions and unverified items

Resolved by tests during implementation (no input needed):

1. `procStart` encoding on macOS and Linux (compare with `ps -o lstart` and `/proc/<pid>/stat`).
2. Registry `status` values: the docs give `busy`, `waiting`, `idle` plus `waitingFor` for `claude agents --json`; confirm the registry file carries the same (source mirror pending; observe live while idle and waiting).
3. Whether a running session picks up a new hook group from settings.json (docs say yes, the memory note says no): tested in Milestone 1 with a harmless async hook.
4. Peer-message wire format after the auth line and whether `sessions/<pid>.<hash>.key` holds the token: tested against a throwaway `claude -p` session with `crossSessionInbound: accept`.
5. Killing a `claude attach` client leaves the session running: tested with taskkill.
6. sysinfo `environ()` and `cwd()` for claude.exe on Windows; fallback is the parent chain plus the registry cwd.
7. xterm 6 DOM-renderer fallback throughput; WebKitGTK WebGL performance; simultaneous visible WebGL panes (cap 12 until measured).
8. portable-pty ConPTY feature flags and a detached host's ConPTY surviving UI exit.
9. Any `wt` action targeting a pane by `WT_SESSION` (none known in 1.24).
10. Whether `ConfigChange` fires for `~/.claude.json` MCP edits; until then MCP edits are always restart-required.
11. `claude agents --json --all` staleness after supervisor death.
12. tmux `send-keys -l`, iTerm2 `ITERM_SESSION_ID` to `unique id`, Terminal.app `do script in tab`, kitty `send-text` options: verified only on hardware; rows stay labeled.
13. The `http` hook type's behavior when C4 is not running, and what `onFailure` offers; whether `"shell": "powershell"` removes the Git Bash spawn on Windows.
13a. Whether polling `claude agents --json` revives a stopped supervisor on macOS and Linux (documented side effect): measured in Milestone 7, with a setting to poll less often.
14. Windows custom title bar with Snap layouts; fallback native decorations.
15. Whether 2.1.299 emits `agent_needs_input` (configured in notify.json, absent from the older source mirror).
16. Asset name placeholders, `attest-build-provenance@v4` inputs, `actions/setup-python` major, `ubuntu-22.04` availability: confirmed on the first rc tag.

Documented gaps (no action possible): the original uprooted.sh site source; the OG wordmark typeface; per-session share of the 5-hour window (account-level endpoint only, shown as an estimate or "unknown").

Bash's calls, listed in Prerequisites: the agents template statusLine change; the stray mux server; keeping the olive terminal field (#0D120B) beside the cool chrome (#08080c), which the design keeps on purpose; the product wording ("C4, a command center for Claude Code sessions" in the README instead of "Claude Code Command Center" as a name, see Decision 14).
