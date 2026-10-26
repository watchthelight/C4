<!-- <wtl> project: c4 | file: docs/DESIGN-SYSTEM.md | author: watchthelight </wtl> -->

# Design system

C4 looks like an uprooted.sh product: near-black chrome, sage accents, square geometry, Space Mono for
display text, a square-pixel starfield. It behaves like an iTerm2 workstation: dense rows, keyboard
first, immediate state changes, no decoration that costs terminal frames.

## Sources and gaps

The tokens derive from three places the author already uses: the labs site rebuild (its stylesheet
and starfield script, written "in the old uprooted.sh style"), the uprooted.sh social image (lowercase
mono wordmark, short sage rule, thin sage bars, faint pixel stars), and the Quiet Machine terminal
palette from the author's WezTerm and starship configuration.

Gaps, stated rather than invented: the original uprooted.sh site source was not available, and the
wordmark typeface in the social image could not be identified from the PNG. The wordmark uses Space
Mono 700, which is what the labs rebuild uses for its mark. An earlier app of the author's used an
Inter, indigo, rounded style that is explicitly rejected here.

## Tokens

Chrome colors:

| Token | Value | Use |
|---|---|---|
| `--bg` | `#08080c` | window background |
| `--panel` | `#0d0e12` | sidebar, inspector, bars |
| `--panel-2` | `#111217` | hovered rows, inputs |
| `--head` | `#e1e5e8` | headings, active tab text |
| `--text` | `#c8ccd0` | body text |
| `--dim` | `#8a8e96` | secondary text, 11px labels |
| `--mute` | `#5a5e66` | decorative only (3.1:1 on `--bg`) |
| `--faint` | `#2a2d33` | borders on panels, chips, inputs, tiles |
| `--line` | `#1c1e23` | separators |
| `--sage` | `#6a9a6e` | accent, Working |
| `--sage-hi` | `#7bc67e` | focus ring, Running a tool |
| `--sage-deep` | `#4d7a51` | attached pane border, running bars |
| `--sage-wash` | `rgba(106,154,110,.08)` | selection fill |
| `--amber` | `#b8a44a` | "needs you" and nothing else |
| `--red` | `#c45454` | Failed, Disconnected |
| `--star` | `rgb(200,210,220)` | starfield |
| `--star-sage` | `rgb(122,170,126)` | one star in ten |
| `--scrim` | `rgba(8,8,12,.72)` | behind modals |

Terminal colors (Quiet Machine), used only inside terminal panes: background `#0D120B`, foreground
`#C9D1C4`, cursor `#ECB442`, selection `#586555`; ANSI `#161C13 #B5563F #8A9A5B #ECB442 #6E8FA0
#A07089 #7FB0A8 #C9D1C4`, bright `#586555 #E06A4F #A8C46A #F6C95A #8FB8C9 #C68AA0 #9AD0C6 #ECF0E6`.
The olive terminal field next to the cool chrome is kept on purpose.

Chart categories `--cat-1` to `--cat-6`: `#7bc67e #7FB0A8 #6E8FA0 #A07089 #b8a44a #c8ccd0`.

Type: `--display` is Space Mono 400 and 700 (eyebrows, titles, the wordmark, stat values); `--mono` is
JetBrains Mono, bundled, with Cascadia Mono, Consolas and Liberation Mono as fallbacks; the terminal
font is JetBrainsMono Nerd Font Medium through `local()` when installed, else the bundled JetBrains
Mono, 12px, line height 1.05, ligatures on. Sizes run from 10 to 28px. Eyebrows are 12px uppercase
with .18em tracking; chips are 11px uppercase with .08em tracking.

Spacing `--s-1` to `--s-8`: 2, 4, 6, 8, 12, 16, 24, 32px. Row heights 22 (Compact), 34 (Standard),
56 (Expanded), group headers 24. Controls 20, 24 and 28px.

Geometry: `--radius: 0` everywhere. 1px `--faint` borders on panels, chips, inputs and tiles; tiles
carry an 8x8 square in one corner. Elevation only by 1px borders and `0 0 0 1px` rings. Focus is
`outline: 1px solid var(--sage-hi); outline-offset: 3px`. Icons are square pixel glyphs on 8x8 or
12x12 grids drawn as SVG rects in `currentColor`; the mark is a 10x10 sage square beside "c4".

## Session states

| State | Color | 8x8 glyph | Sort weight |
|---|---|---|---|
| Waiting for permission | `--amber` | filled square with a dark inner dot, 1 Hz two-frame pulse | 100 |
| Waiting for user input | `--amber` | outlined square | 90 |
| Failed | `--red` | filled square | 80 |
| Disconnected | `--red` outline | outlined square with a 1px gap | 70 |
| Running a tool | `--sage-hi` | filled square with a 2px bar advancing beneath | 40 |
| Background agents active | `--sage` | two squares offset by 2px | 35 |
| Working | `--sage` | filled square | 30 |
| Idle | `--mute` | outlined square | 10 |
| Completed or exited | `--mute` | filled square | 0 |

The text label is always present (in the row at Standard and Expanded density, in the tooltip and
the accessible name at Compact). Color is never the only carrier of state. Observed facts are plain;
guessed ones carry a `~` prefix and a dashed border.

## Sidebar rows

The list is virtual: a spacer holds the full height, the rows near the viewport (plus eight beyond
each edge) sit on pooled nodes placed with a transform, and the density sets one row height for the
whole list. Compact (22px) shows the state square, the name, the context figure and the time in
state, with the state text in the tooltip and the accessible name. Standard (34px) adds a second
line with the state text (`~` in front of a guess, amber for the waiting states, red for Failed) and
the host, attachment, pid and coarse tags, and puts a 24 by 3px context bar in front of the figure,
colored by the status line's bands (sage under 75 percent, amber from 75, red from 90). Expanded
(56px) adds a third line: the attachment chip, the model and effort from the last status line
payload, and the cwd. A 2px amber stripe on the left marks the two waiting states, red marks Failed.
Every piece of secondary text is `--dim` at 10 or 11px; `--mute` paints only the idle and completed
squares. A pinned row carries a 6px sage square after its name.

Group headers are 24px, laid out like the labs site's group heads: the fold glyph (an 8px minus, a
plus once folded) in the state square's column, the group's number in `--sage`, the title in Space
Mono 700, a 1px `--faint` rule filling the line, the member count in `--dim` and, when any member
needs you, that count in `--amber`. A folded header keeps its counts and dims its title. The line
above the list holds the filter box, a 24px input on `--panel-2` with a 1px `--faint` border that
turns `--sage-hi` on focus, and the grouping button beside it, an uppercase 11px chip.

The hover card opens beside a row once the pointer has rested on it for 350 ms, to the right of the
sidebar when there is room, else to the left, never past the window's edge. It is 300px wide on
`--bg` with a 1px `--faint` border: the state square and the name in Space Mono 700 with the
attachment chip, a `--dim` line saying why (the host and pane, or what ended the session), the state
text colored as in the row, its evidence in `--dim`, then a two-column fact list under a `--line`
rule (model, context with the same 24 by 3px bar, tool, agents, tasks, cwd, started, last activity,
status line, telemetry) and up to three agent lines, each with a 4px sage square in front. It takes
no pointer events, animates nothing, follows the pointer from row to row without a second wait, and
closes when the pointer leaves the rows, the list scrolls or changes by hand, or a key goes down,
then stays closed until the pointer moves again. The keyboard never needs it: the inspector holds
the same facts.

## Layout

| Region | Size | Content |
|---|---|---|
| Top bar | 32px | mark and "c4", layout name, attention count, shared quota meters |
| Sidebar | 320px default, 220 minimum, set by the 6px splitter at its right edge (the workspace keeps 400px), collapses to a 36px rail of state squares, detachable; the width, pins, grouping, folded groups and density are kept between runs | session rows, group headers, filter |
| Workspace | the rest | tab strip 28px, split tree of panes, surface tabs (Library, Center, Telemetry, Settings) |
| Inspector | 360px default, collapsible | Overview, Workflow, Tokens, Agents, Git, Capabilities, Log |
| Status strip | 22px | attention count, quota meters, peer count, layout name, key hints |

## Motion

Transitions: 100, 150 and 200 ms. Only opacity, transform, border-color and small background-color
changes animate; never width or height. The only repeating motion is the 1 Hz permission pulse and the
tool-activity bar. The starfield runs at 30 fps within a 1 ms budget, only behind the sidebar, status
strip and surface tabs, never behind terminal text; it freezes when the window is hidden, while any
visible pane received bytes in the last 250 ms, during splitter drags, and under reduced motion.
`prefers-reduced-motion` stops drift, transitions and the pulse.

## Keyboard

App chords use Ctrl+Shift on Windows and Linux and Cmd+Shift on macOS so that plain shell keys pass
through to the terminal. Palette `Ctrl+Shift+P`, switcher `Ctrl+K`, split right and down
`Ctrl+Shift+D` and `Ctrl+Shift+E`, focus pane `Ctrl+Shift+Arrow`, tabs `Ctrl+Tab` and `Ctrl+1..9`,
zoom `Ctrl+Shift+Enter`, focus mode `Ctrl+Shift+F`, find `Ctrl+F`, regions `Ctrl+Shift+B/I/L/M/Y`,
row density `Ctrl+Shift+=`, cycle regions F6, Esc back to the terminal. In the session list the
arrow keys, PageUp, PageDown, Home and End move the selection and scroll to it, `p` pins the
selected session, `g` walks the grouping, Left folds the selected session's group and Right opens
it. `/` anywhere outside a text field focuses the filter box; in the box Escape clears, then leaves,
and Enter or the down arrow leaves for the list with the filter kept. The splitter between the
sidebar and the workspace is a focusable separator: Left and Right move it 10px, Home and End go to
its limits, and a double-click puts the default back; its 1px line shows `--sage-deep` under the
pointer and while dragging and `--sage-hi` with keyboard focus. Interrupt and terminate have no
single-key binding; they go through the palette or the pane menu with a confirmation.

## Accessibility

Contrast on `--bg`: head 15.8:1, text 12.4:1, dim 6.1:1, sage-hi 9.7:1, sage 6.2:1, amber 8.0:1,
red 4.5:1. The sidebar is a listbox with per-row labels; a polite live region announces attention
changes at most once per 2 s; every chart has an adjacent hidden table built from the same data;
xterm's screen reader mode is a setting.

## Deviations from the labs stylesheet

No glow on the mark; tile hover changes border color only; no backdrop blur; no gradients; 11px labels
use `--dim` rather than `--mute`; unicode arrows and chevrons become 8px pixel glyphs.

## Do not

Rounded corners, gradients, blur, glow, drop shadows, hover lift, unicode arrows or em dashes in UI
text, color as the only carrier of state, fake prompts or cursors in panes that cannot take input,
numbers that disagree with the status line, starfield behind terminal text, amber for anything other
than "needs you".
