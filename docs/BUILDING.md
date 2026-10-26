<!-- <wtl> project: c4 | file: docs/BUILDING.md | author: watchthelight </wtl> -->

# Building C4

## Prerequisites

| | Windows | macOS | Linux (Debian or Ubuntu) |
|---|---|---|---|
| Rust | rustup from rustup.rs or `winget install Rustlang.Rustup`; the toolchain in `rust-toolchain.toml` installs itself on first use | rustup | rustup |
| C toolchain | Visual Studio Build Tools 2022 with the "Desktop development with C++" workload (MSVC and a Windows SDK) | Xcode command line tools | `build-essential` |
| WebView | WebView2 (preinstalled on Windows 11) | WKWebView (part of macOS) | `libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils` |
| Node and pnpm | Node 22 or later, pnpm 10 | same | same |
| Python | 3.11 or later, for the scripts and the hook bridge | same | same |

Nothing in the build calls the network beyond the package registries.

## Commands

```sh
cargo test --workspace                                 # every crate, including the desktop crate
cargo clippy --workspace --all-targets -- -D warnings  # what CI runs
pnpm install                                           # frontend dependencies (apps/desktop)
pnpm -r lint && pnpm -r typecheck && pnpm -r test      # eslint, tsc, vitest
pnpm -C apps/desktop tauri dev                         # the app with hot reload
pnpm -C apps/desktop tauri build --debug --no-bundle   # the debug app without installers (what CI builds)
pnpm -C apps/desktop tauri build                       # a release bundle for this platform
cargo bench --workspace                                # criterion benches
python scripts/precommit.py                            # what the git hook runs
python scripts/gen_notices.py                          # regenerate THIRD-PARTY-NOTICES.md
```

## The desktop app

`apps/desktop` holds the window. `src-tauri` is the `c4-desktop` crate that builds the `c4`
executable, `src` is the window itself (TypeScript and Vite, no framework), `tests` is the vitest
suite. The crate links `c4-cli`, `c4-core` and `c4-discovery` directly; there is no server and no
second process.

`pnpm -C apps/desktop tauri dev` starts Vite on port 1420 and opens the app against it. `tauri build`
runs `pnpm build` first and embeds `dist/` in the executable. A plain `cargo build` or `cargo test` of
the workspace needs neither Node nor `dist/`: without the Tauri CLI the crate is built in development
mode and its window would load `http://localhost:1420`, so the executable to run is the one
`tauri build` produces, `target/debug/c4` or `target/release/c4`.

The window picks its data source at startup. Inside the app it calls the Rust commands. Opened in a
browser (`pnpm -C apps/desktop dev`, then http://localhost:1420) it serves eight synthetic sessions
from `src/bridge/mock.ts`, so the layout can be worked on without Claude Code; `VITE_C4_BRIDGE=mock`
or `=tauri` forces either.

The app refreshes from the Claude home's files every 2 s (`C4_REFRESH_MS`, floor 250) and runs the
session list command on every eighth pass. Hook and status line telemetry arrive in Milestone 2.

The icons under `src-tauri/icons/` come from `assets/icons/c4-1024.png`, which `scripts/make_icon.py`
draws. To regenerate them run `pnpm -C apps/desktop exec tauri icon ../../assets/icons/c4-1024.png -o <dir>`
and copy the files from `<dir>` (the command also writes Android and iOS sets, which C4 does not ship).
The 256 px file is named `256x256.png` here; the generator names it with a `2x` suffix after an `@`,
which reads as an e-mail address to the secret scan. The pixel size is what the Linux bundler reads.

## Headless commands

Any argument on the command line makes `c4` run a headless command and exit without opening a window:

```sh
c4 sessions                        # every session discovery can see, as a table
c4 sessions --json                 # the same as JSON; --redact for pasting
c4 sessions --watch                # keep refreshing, print state changes
c4 doctor                          # where the Claude home is, whether claude runs
c4 telemetry --transcript <file>   # tokens, cost and agents of one transcript
```

During development `cargo run -p c4-cli -- <command>` runs the same commands as their own executable,
`c4-cli`, which builds in seconds and needs neither Node nor a webview. A release `c4` on Windows is a
windowed executable: it attaches to the console of the shell that started it, so the output appears
there, but the shell does not wait for a windowed program and can print its prompt first. Pipe the
output, or use `c4-cli`, when that matters. Debug builds keep a console of their own.

`sessions` and `doctor` read the Claude home named by `C4_CLAUDE_HOME`, else `CLAUDE_CONFIG_DIR`, else
`~/.claude`, or the one given with `--home`. `C4_AGENTS_CMD` replaces `claude agents --json` with
another command, which is how the tests feed fixtures in; `--files-only` runs no command at all.
`scripts/fake_claude.py` registers a stand-in session in a scratch home for manual checks:

```sh
python scripts/fake_claude.py --home /tmp/scratch --cwd /tmp/proj --status busy --name demo &
C4_CLAUDE_HOME=/tmp/scratch cargo run -p c4-cli -- sessions --files-only
```

## Troubleshooting

- **`cl.exe` exits with code 2 while compiling a C dependency on Windows.** A `CC` environment
  variable that points at `cl.exe` makes cc-rs skip its own MSVC environment setup, so headers are not
  found outside a developer prompt. Unset it for the build (`$env:CC = $null` in PowerShell) or build
  from a "x64 Native Tools" prompt. `scripts/precommit.py` drops it for its own child processes.
- **Slow terminal rendering on Linux.** WebKitGTK can put WebGL on a software path. Settings has a
  renderer toggle; the DOM renderer is the fallback and is always available.
- **The app window is blank on Linux.** Check that the WebKitGTK 4.1 development package was present
  at build time and the runtime package is installed.
- **`pnpm tauri build` fails on `macos-15-intel`.** The x64 build is a CI-only target; see the release
  notes for its verification status.
