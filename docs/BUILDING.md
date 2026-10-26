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
cargo test --workspace                 # core, discovery, store, cli tests
pnpm install                           # frontend dependencies (apps/desktop)
pnpm -r test                           # vitest
pnpm -C apps/desktop tauri dev         # the app with hot reload
pnpm -C apps/desktop tauri build       # a release bundle for this platform
cargo bench --workspace                # criterion benches
python scripts/precommit.py            # what the git hook runs
python scripts/gen_notices.py          # regenerate THIRD-PARTY-NOTICES.md
```

## Headless commands

Until the desktop app lands, `cargo run -p c4-cli --` builds and runs the `c4` executable:

```sh
cargo run -p c4-cli -- sessions                        # every session discovery can see, as a table
cargo run -p c4-cli -- sessions --json                 # the same as JSON; --redact for pasting
cargo run -p c4-cli -- sessions --watch                # keep refreshing, print state changes
cargo run -p c4-cli -- doctor                          # where the Claude home is, whether claude runs
cargo run -p c4-cli -- telemetry --transcript <file>   # tokens, cost and agents of one transcript
```

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
