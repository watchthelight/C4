<!-- <wtl> project: c4 | file: docs/prompts/c4-directive.md | author: watchthelight </wtl> -->

This is the directive that started the project, kept verbatim apart from redacted home paths and session ids. The implementation plan that answered it is docs/prompts/c4-plan.md.

---

# C4: Claude Code Command Center
## Plan Mode + Ultrathink | Full Implementation Directive

Enter **Plan Mode** immediately and use your highest available reasoning effort (Ultrathink).

You are the principal architect and implementation engineer for **C4 (Claude Code Command Center)**.

Your mission is to build a production-quality, low-latency, cross-platform desktop application that discovers, observes, organizes, and controls Claude Code sessions through a single native command center.

**C4 must be a real application, distributed as a self-contained executable and developed in a public GitHub repository.**

The intended experience combines:

- The session orchestration capabilities of tmux
- The terminal workspace capabilities of iTerm2
- iTerm2's Session Status sidebar, Cockpit, and Workgroups concepts
- The interface design language of my uprooted.sh applications
- The observability of my existing `statusline.py`
- A live workflow and subagent inspector
- A reusable toolkit, MCP, and skills control center

C4 is not a conventional chat application, static dashboard, or reskinned terminal.

It is a command center for the entire Claude Code development environment.

---

# 1. Identity and public repository

**Application name:** C4  
**Full name:** Claude Code Command Center  
**Executable name:** `c4`  
**Preferred repository:** `https://github.com/watchthelight/C4`  
**Repository visibility:** Public  
**Primary branch:** `main`

Use the preferred repository name if available and accessible. Report a collision or permission problem rather than silently publishing under an unrelated account.

Create the actual GitHub repository during the implementation phase.

Do not merely document a hypothetical repository.

## Git identity and publishing rules

Follow my established watchthelight conventions:

- Git author: `watchthelight`
- Git email: `admin@watchthelight.org`
- Small, frequent, focused commits
- Lowercase, natural commit messages
- Push verified commits to `origin/main` regularly
- Never force-push or rewrite published history
- Never bypass existing Git hooks
- No `Co-Authored-By` trailers
- No generated-by or AI attribution trailers
- No AI disclaimers in README files or project pages
- Commit project prompts and relevant design specifications
- Apply the `/humanize` skill to documentation and visible application text
- Never use em dashes in prose, documentation, or commit messages
- Use the existing `<wtl>` author-header conventions where appropriate

Before committing, validate Git identity, inspect the staged diff, run the applicable tests, and check for sensitive information.

Do not publish local credentials, private session transcripts, personal filesystem information, OAuth tokens, secrets, or third-party proprietary assets.

Create these project files:

- `README.md`
- `docs/HOW-IT-WORKS.md`
- `docs/ARCHITECTURE.md`
- `docs/SESSION-INTEGRATION.md`
- `docs/DESIGN-SYSTEM.md`
- `docs/SECURITY.md`
- `docs/BUILDING.md`
- `CHANGELOG.md`
- `CONTRIBUTING.md`
- `.github/workflows/` with cross-platform build and verification workflows

Choose and document a suitable public license after reviewing third-party component licenses.

Configure CI to build and test Windows, macOS, and Linux artifacts.

Publish versioned releases with checksums. Include signing where credentials and infrastructure permit.

The repository should be useful to other developers, not just tailored to my filesystem.

Design machine-specific integrations through configurable adapters.

---

# 2. Mandatory research

Before choosing the application stack or implementing the interface, research my environment.

## My existing infrastructure

Use watchthelight MCP to investigate:

- uprooted.sh applications and their original interface designs
- `~/.claude/statusline.py`
- Existing Claude Code hooks
- `~/.claude/settings.json`
- `~/.claude/projects/`
- Existing token and usage snapshots
- Existing Claude analytics infrastructure
- `watchthelight-labs/agents`
- Global and project-specific skills
- Available MCP server configurations
- REA reverse engineering toolkit
- Relevant Windows, macOS, and Linux development workflows

Inspect the actual files where accessible.

Historical MCP records are background evidence, not proof of the current machine state.

Preserve existing functionality and configuration.

## Current Claude Code capabilities

Research the installed CLI and official documentation for:

- Session lifecycle events
- Hook APIs and event payloads
- Transcript formats
- Process and session identifiers
- Subagent lifecycle and task relationships
- Background agent behavior
- Tool invocation events
- Statusline integration
- Context and token accounting
- MCP runtime management
- Skill discovery and invocation
- Session resume and recovery
- Agent SDK capabilities
- Supported remote-control mechanisms
- Terminal and PTY integration

Identify which operations can be performed on existing sessions and which require sessions to be launched under C4 control.

Document unsupported capabilities explicitly.

Never fabricate APIs.

## iTerm2 research

Research the current iTerm2 implementations of:

- Session Status
- Cockpit
- Toolbelt
- Claude Code Integration
- Workgroups
- Session prioritization
- Terminal split views

Official references:

https://iterm2.com/documentation-session-status.html

https://iterm2.com/documentation-one-page.html

Use the actual behavior as a functional and interaction-design reference.

Do not simply copy screenshots.

---

# 3. C4 visual identity

There are two complementary design requirements.

**Primary visual identity:** uprooted.sh applications.

**Primary workspace interaction inspiration:** iTerm2.

Do not confuse them.

C4 should look like an uprooted.sh product while behaving like a sophisticated multi-session terminal workstation.

Investigate original uprooted.sh interfaces using source repositories, accessible assets, and watchthelight MCP evidence.

Extract authentic design tokens, typography, surfaces, spacing, iconography, and interactive behavior.

If the original assets cannot be located, document the gap rather than inventing their exact appearance.

The final interface must be:

- Dark-first and polished
- Information-dense without feeling cluttered
- Highly responsive
- Visually hierarchical
- Keyboard friendly
- Suitable for continuously monitoring many concurrent sessions
- Consistent across Windows, macOS, and Linux

Avoid generic SaaS-dashboard design.

Favor sharp geometry, carefully controlled color accents, readable typography, compact controls, and deliberate motion.

Do not let cosmetic effects interfere with terminal performance or event updates.

---

# 4. Main interface: workstation sidebar

The **Workstation Sidebar** is C4's central interaction element.

It must be visible in the primary workspace and should feel inspired by iTerm2's Session Status tool and Cockpit.

Instead of merely listing terminal tabs, it provides a live overview of every accessible Claude Code session.

## Session rows

Each session entry should show:

- Session name
- Current project
- Git repository and branch
- Current work status
- Short description of its current activity
- Active tool, when available
- Active subagent count
- Session activity indicator
- Context usage
- Elapsed execution time
- Attention indicator when user interaction is needed

Example layout:

```text
C4
CLAUDE CODE COMMAND CENTER

WORKSTATIONS                         08
────────────────────────────────────────

NEEDS ATTENTION                       02

● Project Legion
  Waiting for permission
  Bash · NvGfx / xe
  Context 62%                   3 agents

● Shatterfish
  Awaiting your response
  Testing: search improvements
  Context 78%                   0 agents

WORKING                               04

◉ REA
  Decompiling target binary
  Ghidra · function analysis
  Context 43%                   2 agents

◉ XeGfx
  Building driver changes
  Compiling 14 of 32 targets
  Context 71%                   4 agents

◉ Research
  Gathering upstream references
  Context 19%                   5 agents

IDLE                                  02

○ Documentation
  Completed: architecture review
  Context 24%                   0 agents
```

Treat this as an information hierarchy example, not an exact visual design specification.

## Required session states

Implement reliable state detection:

- Working
- Waiting for permission
- Waiting for user input
- Idle
- Background agents active
- Running a tool
- Failed
- Disconnected
- Completed or exited

Use actual Claude lifecycle events, transcript events, and process information.

A session with background tasks still running must not be incorrectly classified as completed.

A stalled task must not be declared failed solely because no new transcript data has appeared.

## Sidebar behavior

Provide:

1. Click to focus the relevant session.
2. Double-click or keyboard shortcut to open the session workspace.
3. Hover to reveal expanded current activity.
4. Expand session rows to see subagents and tasks.
5. Group by project, repository, host, or workstation.
6. Pin important sessions.
7. Search session names, paths, branches, and tasks.
8. Filter by active state.
9. Sort attention-required sessions to the top.
10. Show status transitions immediately.
11. Persist custom sidebar organization.
12. Offer optional notifications on completion or required input.

Users must be able to understand what all sessions are doing without opening each terminal.

## Sidebar density

Support Compact, Standard, and Expanded modes.

Compact shows one-line session entries with indicators.

Standard displays state and current work.

Expanded provides subagents, active tools, progress, and resource information.

Allow the sidebar to be resized, collapsed, detached, or moved when the window layout supports it.

---

# 5. Main workspace

Use a modular native desktop layout:

```text
┌────────────────────────────────────────────────────┐
│ C4   Workspaces   Sessions   Tools   Usage          │
├────────────┬──────────────────────────┬──────────────┤
│ WORKSTATION│                          │   INSPECTOR  │
│ SIDEBAR    │     ACTIVE TERMINAL      │              │
│            │                          │   TOKENS     │
│ ● Legion   │  Claude Code session     │   AGENTS     │
│ ● REA      │                          │   MCP        │
│ ○ Docs     ├──────────────────────────┤   SKILLS     │
│            │   SECOND SESSION         │   EVENTS     │
│            │                          │              │
├────────────┴──────────────────────────┴──────────────┤
│ Context 64% | 5h quota 48% | Agents 8 | MCP 12       │
└────────────────────────────────────────────────────┘
```

Support native-feeling:

- Terminal tabs
- Horizontal and vertical splits
- Independent panes
- Drag-and-drop session arrangement
- Saved workspace layouts
- Workspace presets
- Fullscreen focused terminal
- Persistent scrollback
- Searchable terminal history
- Terminal color and ANSI support
- Resizing and keyboard navigation
- Session switching without losing terminal state

Every controllable terminal pane must be connected to a real process or terminal session.

Do not simulate execution through a fake chat UI.

Implement a global command palette.

Support shortcuts for jumping to sessions, focusing the terminal, opening the inspector, finding a tool, assigning a toolkit, and searching events.

---

# 6. Automatic session discovery

C4 must discover existing Claude Code sessions on startup, including sessions opened outside C4.

Target:

- iTerm2
- macOS Terminal
- Windows Terminal
- PowerShell
- Command Prompt
- VS Code terminals
- tmux
- WSL terminals
- Linux terminal emulators
- SSH environments when explicitly paired
- C4-managed terminals

Use platform-specific process enumeration, transcript storage, verified terminal mappings, and Claude lifecycle events.

Correlate sessions using durable IDs and validated process metadata.

Do not equate historical transcript files with live processes.

Implement the following attachment modes.

### Managed

Sessions launched through C4 using a controlled PTY.

Full supported terminal input/output, resizing, lifecycle actions, and monitoring.

### Interactive Attached

Independently launched sessions that expose a verified, safe bidirectional control mechanism.

Support the strongest control allowed by the terminal and OS integration.

### Observed

Sessions that can be monitored but not safely written to.

C4 must continue providing real-time telemetry without disrupting them.

### Disconnected

Sessions that exited or whose control/monitoring connection was lost.

Never claim universal input control if the underlying host does not support it.

Offer an explicit, non-destructive migration or managed continuation path where available.

Do not hijack PTYs or inject into arbitrary processes to achieve the appearance of universal control.

## Remote workstations

Implement optional paired-host support.

A remote companion can report local Claude sessions and accept authenticated supported control requests.

Use secure pairing and encrypted communications.

Local session discovery must work without a remote account or cloud service.

---

# 7. Live activity and agent workflows

The dashboard must reveal what Claude Code is doing right now.

A session should expose its observable activity in real time.

Examples:

- Reading source files
- Editing code
- Running tests
- Compiling
- Searching documentation
- Invoking MCP tools
- Waiting for user approval
- Running subagents
- Performing background tasks
- Reviewing changes
- Completing a response

Derive descriptions from real activity.

Clearly distinguish observed operations from heuristic summaries.

## Workflow visualization

Include a dynamic graph showing:

- Main session
- Subagents
- Tools
- Tasks
- Dependencies where known
- Concurrent execution
- Completion states
- Failures
- Durations
- Token consumption

Provide both graph and timeline views.

Selecting an agent node opens its details and relevant transcript or event history.

Do not pretend to expose private internal reasoning that Claude does not publish.

---

# 8. Token monitoring

Integrate my existing `statusline.py` instead of duplicating its logic.

Preserve compatibility with its existing data consumers.

Monitor:

- Input tokens
- Output tokens
- Context utilization
- Subagent usage
- Cache reads and writes
- Model attribution
- Token burn rate
- Recent consumption
- Compaction thresholds
- Estimated turns before compaction
- Five-hour quota
- Weekly quota
- Reset windows
- API expenses where applicable
- Account-wide versus session-specific measurements

Create smooth, readable graphs.

Support comparisons across sessions and workstations.

Metrics should update incrementally.

Prevent duplicate accounting for cached or repeated transcript events.

Use explicit unknown states when information is unavailable.

Do not infer real subscription usage exclusively from local token totals.

---

# 9. Capability Library and toolkit assignment

C4 needs a reusable capability management system.

Discover:

- Claude Code skills
- MCP servers
- Agent definitions
- CLI tools
- Toolkits
- Workflow templates

Present them in a searchable library with descriptions, availability, dependencies, and compatibility.

## REA integration

Use my REA reverse engineering toolkit as the initial example.

Inspect its actual configuration.

Identify its skill, CLI, MCP server, providers, and installed analysis tools.

Respect existing safety and installation rules.

## Assigning a toolkit

From any session:

1. Select the session.
2. Open the Capability Library.
3. Select REA or another toolkit.
4. Inspect the proposed capabilities.
5. Select Suggest, Prepare, or Apply.
6. Confirm the target session and changes.
7. View the result and any reload requirements.

Suggest sends a contextual recommendation to a writable session.

Prepare validates dependencies and optionally prepares workspace context.

Apply performs supported configuration changes with user approval.

Never silently elevate permissions or assume that a running Claude session dynamically reloads MCP servers and skills.

Use clear active, pending, and restart-required states.

---

# 10. MCP and skills control center

Build a dedicated management interface for MCP servers and skills.

Display:

- Name
- Description
- Scope
- Configuration source
- Connection availability
- Health when verifiable
- Exposed capabilities
- Related sessions
- Compatibility
- Errors

Provide backed-up, reversible configuration changes.

Redact credentials.

Do not expose raw MCP environment variables containing secrets.

Changes to running sessions must respect actual Claude Code runtime limitations.

---

# 11. Session control

Provide a capability-aware command router.

Supported actions should include:

- Focus session
- Open terminal
- Send a message
- Start session
- Resume session
- Open working directory
- Inspect Git changes
- Open in editor
- Assign toolkit
- Suggest skill
- Inspect agents
- Inspect tokens
- Export activity
- Interrupt managed execution
- Gracefully terminate a managed session

Always validate the target session.

Do not send input to whichever terminal happens to be active.

Never restart or kill pre-existing sessions without an explicit instruction.

Prevent concurrent writes to the same session from different controllers.

---

# 12. Engineering architecture

Choose an appropriate native application stack after comparing the real tradeoffs.

A Rust-based core is preferred for:

- Process discovery
- Event ingestion
- Cross-platform IPC
- Concurrency
- Memory safety
- Resource monitoring
- Performance

Evaluate Tauri, egui, Slint, and other relevant UI options.

The chosen stack must support uprooted.sh design fidelity and a real, high-performance terminal renderer.

Design independent modules for:

- Discovery
- Session identity
- Terminal adapters
- Event ingestion
- Transcript parsing
- Usage calculation
- Agent tracking
- Workflow modeling
- Toolkit management
- MCP management
- Session controls
- Local persistence
- Native UI
- Optional remote connections

Use typed event schemas, incremental processing, bounded queues, and resilient lifecycle handling.

C4 must not interfere with Claude Code's normal execution.

A dashboard crash must not terminate independently running sessions.

---

# 13. Performance targets

Target:

- Existing local session discovery in approximately 2 seconds
- Local event-to-UI latency below 150 ms at p95
- Responsive 60 FPS interface interactions on capable hardware
- Dozens of concurrent sessions
- Thousands of events per minute
- Minimal idle CPU utilization
- Bounded memory usage
- No repeated full-transcript scans
- Efficient incremental file watching
- Virtualized event lists
- Throttled graph rendering

Treat these as measurable engineering targets, not guaranteed results.

Build performance instrumentation and benchmarks.

---

# 14. Public distribution

C4 should be installable by other Claude Code developers without requiring my personal infrastructure.

Build:

- Windows x64 executable
- macOS universal or architecture-specific application
- Linux x64 application
- Additional architectures where feasible

Bundle dependencies where practical.

Document unavoidable system dependencies accurately.

Use GitHub Actions to verify builds and publish release artifacts.

Maintain a changelog and version tags.

The public repository should include:

- The application
- Cross-platform adapters
- Installation instructions
- Test suites
- Architecture diagrams
- Public configuration examples
- Plugin and toolkit interface documentation
- Screenshots or recordings using synthetic, non-sensitive session data
- Clear explanations of supported and unsupported capabilities

Never publish my private session history as sample data.

Use synthetic fixtures for tests.

---

# 15. Required tests

Before declaring C4 functional, demonstrate:

**A. Existing sessions:** Three Claude Code sessions launched outside C4 appear automatically.

**B. New sessions:** Starting a new Claude session causes it to appear without manually refreshing.

**C. Live status:** A session transitions accurately between working, waiting, and idle.

**D. Background agents:** Session activity remains accurate while background agents run.

**E. Sidebar navigation:** Selecting a session focuses or opens its actual terminal where supported.

**F. Token accounting:** Existing statusline measurements and dashboard results reconcile.

**G. Toolkit suggestion:** REA can be suggested to a selected writable session and the action is verified.

**H. Managed terminal:** C4 can start and fully operate a Claude session through its owned PTY.

**I. Observation fallback:** A non-controllable session remains visible with accurate capability labeling.

**J. Recovery:** Closing and reopening C4 does not terminate or corrupt independent sessions.

**K. Performance:** Measure CPU, memory, event latency, and dropped events under load.

**L. Publishing:** Verify the public repository, commit history, CI, documentation, and release artifact integrity.

**M. Existing setup:** Verify my original Claude hooks, skills, MCP servers, and statusline remain functional.

Cross-platform claims must be backed by actual platform tests or marked unverified.

---

# 16. Implementation sequence

After completing Plan Mode and receiving approval, implement in working milestones.

### Milestone 1: Foundation

Create the C4 repository, development structure, typed event models, session identity system, and initial native UI.

### Milestone 2: Workstation Sidebar

Implement automatic local discovery, accurate session states, grouping, attention sorting, and click-to-focus integration.

This should be the first major user-visible milestone.

### Milestone 3: Terminal Workspace

Add managed PTYs, split terminals, tabs, saved layouts, and safe attachment adapters.

### Milestone 4: Telemetry

Integrate `statusline.py`, transcripts, token analytics, and event timelines.

### Milestone 5: Agents and Workflows

Implement live subagent tracking, execution graphs, task history, and activity descriptions.

### Milestone 6: Capability Management

Implement the REA integration, toolkit assignment, skill library, and MCP management.

### Milestone 7: Cross-platform Completion

Develop and verify Windows, macOS, and Linux integrations, optional remote pairing, and recoverability.

### Milestone 8: Production Release

Complete design refinement, performance optimization, testing, documentation, CI, packaging, and public release.

Keep commits small, tested, and pushed throughout development.

Do not defer all GitHub publishing until the project is finished.

---

# 17. Definition of done

I should be able to launch **C4** and immediately see every accessible Claude Code session I have running.

The workstation sidebar should tell me what each session is working on, whether it needs my attention, which agents are active, and how heavily it is using context.

I should be able to open the relevant terminal, switch between projects, inspect workflows, monitor tokens, and suggest or assign toolkits such as REA.

The interface should have the visual identity of my uprooted.sh applications and the workspace efficiency of iTerm2.

The application should operate as a genuine session command center rather than a passive log viewer.

The source should live in a public, properly maintained GitHub repository under my established identity.

**Begin in Plan Mode. Ultrathink. Research my existing systems, iTerm2's actual behavior, and the applicable Claude Code APIs. Produce the implementation architecture and visual specification before building. Following plan approval, construct and release the real C4 application.**
