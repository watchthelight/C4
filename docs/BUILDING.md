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
pnpm -C apps/desktop tauri build --debug --no-bundle   # the debug app without installers
pnpm -C apps/desktop tauri build                       # a release bundle for this platform, without the shim
python -I scripts/stage_shim.py                        # stage the built c4-shim as the app's sidecar (see below)
cargo bench --workspace                                # criterion benches
python scripts/precommit.py                            # what the git hook runs
python scripts/gen_notices.py                          # regenerate THIRD-PARTY-NOTICES.md
```

Code under `#[cfg(unix)]` is not compiled on a Windows machine, so a lint in it shows up on the CI
runners first. Clippy only type-checks, so it can check that code for another target without a
cross linker, once the target's standard library is installed:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo clippy -p c4-ingest -p c4-cli --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
```

The desktop crate is left out of such a run: its build script needs the Linux WebKit packages.

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

### The shim as a sidecar

The app ships `c4-shim` beside `c4`, which is where `c4 install` takes its copy from. Tauri calls
such a file a sidecar: the bundler reads it from `src-tauri/binaries/c4-shim-<target triple>` (with
`.exe` on Windows), places it next to the app without the triple, and `tauri-build` copies it into
`target/<profile>/` as well, so a development build finds it there. The declaration lives in
`src-tauri/tauri.sidecar.conf.json`, not in `tauri.conf.json`: Tauri's build script refuses to build
while a declared sidecar is missing, and a plain `cargo build` or `cargo test` of the workspace should
never need a staged file. A release bundle with the shim inside is three commands:

```sh
cargo build -p c4-shim --release
python -I scripts/stage_shim.py                                              # to src-tauri/binaries/
pnpm -C apps/desktop tauri build --config src-tauri/tauri.sidecar.conf.json
```

`--profile debug` stages the debug shim, which is what CI does on every runner before
`tauri build --debug --no-bundle --config src-tauri/tauri.sidecar.conf.json`; `--target <triple>`
names a shim built with `cargo --target`, under `target/<triple>/`. The config file must not be named
`tauri.<platform>.conf.json`: Tauri merges files named that way on its own, which would make every
build of the desktop crate need the staged shim.

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
c4 doctor                          # the Claude home, claude, and C4's shim, install and endpoint
c4 telemetry --transcript <file>   # tokens, cost and agents of one transcript
c4 install [--dry-run]             # copy the shim, wrap the status line, add the hook groups
c4 uninstall [--dry-run]           # the way back, from the backup or entry by entry
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

`install` copies the `c4-shim` that sits beside the executable. During development that is
`target/debug/c4-shim` (`.exe` on Windows) after `cargo build -p c4-shim`, so
`cargo run -p c4-cli -- install --dry-run` shows what would change in the real home without writing
anything; `--shim FILE` names another copy. Point `--home` and `--c4-home` at scratch directories to
try the real thing. Test M (`crates/c4-cli/tests/install.rs`) runs the agents repo's installer and
needs that repo at `~/src/agents` or wherever `C4_AGENTS_REPO` points; without it the test prints a
notice and passes without running that part. docs/SESSION-INTEGRATION.md describes what install
writes and how uninstall takes it back.

## Licenses

C4 is MIT. Two files describe what may ship inside it: `deny.toml` for `cargo deny check` (advisories,
licenses, duplicate versions and dependency sources, evaluated for the four release targets) and
`about.toml` for `cargo about`, which `scripts/gen_notices.py` turns into `THIRD-PARTY-NOTICES.md`
together with the npm packages of the window and the fonts under `assets/fonts/`. Both tools install
with `cargo install cargo-deny cargo-about --locked`; CI pins cargo-deny 0.20 and cargo-about 0.9.

After a dependency change run `python scripts/gen_notices.py` and commit the result; the lint job runs
it with `--check` and fails when the file is out of date. A new license in the tree fails `cargo deny
check` until it is added to `deny.toml` and `about.toml` on purpose. MPL-2.0 is accepted per crate in
`deny.toml`, so a new copyleft dependency is a deliberate decision, not a side effect.

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
