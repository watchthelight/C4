<!-- <wtl> project: c4 | file: docs/BUILDING.md | author: watchthelight </wtl> -->

# Building C4

## Prerequisites

| | Windows | macOS | Linux (Debian or Ubuntu) |
|---|---|---|---|
| Rust | rustup from rustup.rs or `winget install Rustlang.Rustup`; the toolchain in `rust-toolchain.toml` installs itself on first use | rustup | rustup |
| C toolchain | Visual Studio Build Tools 2022 with the "Desktop development with C++" workload (MSVC and a Windows SDK) | Xcode command line tools | `build-essential` |
| Electron | downloaded by `pnpm install` (about 120 MB per platform) | same | same |
| Node and pnpm | Node 22 or later, pnpm 10 | same | same |
| Python | 3.11 or later, for the scripts and the hook bridge | same | same |

Nothing in the build calls the network beyond the package registries, except packaging:
electron-builder fetches the Electron build it packages and the tools it needs (NSIS and WiX on
Windows, the AppImage tool on Linux) from GitHub into its own cache on first use.

## Commands

```sh
cargo test --workspace                                 # every crate
cargo clippy --workspace --all-targets -- -D warnings  # what CI runs
pnpm install                                           # frontend dependencies (apps/desktop)
pnpm -r lint && pnpm -r typecheck && pnpm -r test      # eslint, tsc, vitest
pnpm -C apps/desktop dev                               # the Electron window against Vite, reloading on edit
pnpm -C apps/desktop build                             # dist/ (the page) and dist-electron/ (the main process)
pnpm -C apps/desktop start                             # the Electron window from that build
pnpm -C apps/desktop dev:web                           # Vite alone, for a browser and the synthetic bridge
python scripts/stage_bin.py                            # copy the release c4-cli and c4-shim under apps/desktop/resources/bin
pnpm -C apps/desktop package                           # the installers for this platform, under apps/desktop/release/
pnpm -C apps/desktop package:dir                       # the unpacked app alone
pnpm -C apps/desktop e2e                               # Playwright over that unpacked app
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

## The desktop app

`apps/desktop` holds the window: `src` is the page (TypeScript and Vite, no framework), `electron` is
the main process that opens it (the next section), `tests` is the vitest suite and `icons` the icon
set. The page never links the Rust crates: the main process runs `c4-cli serve` as a child process,
and the page reaches it through `window.c4`.

The window picks its data source at startup. Inside the app it talks to the core. Opened in a
browser (`pnpm -C apps/desktop dev:web`, then http://localhost:1420) it serves eight synthetic
sessions from `src/bridge/mock.ts`, so the layout can be worked on without Claude Code;
`VITE_C4_BRIDGE=mock` or `=electron` forces one.

The core refreshes from the Claude home's files every 2 s (`C4_REFRESH_MS`, floor 250) and runs the
session list command on every eighth pass. Hook and status line payloads arrive through the shim on
the C4 home's endpoint (`C4_HOME`, else `~/.c4`) while the window is open; `c4 install` wires the
shim in. The serve tests (`crates/c4-cli/tests/serve.rs`) send lines through the shim's own delivery
code against a scratch C4 home, so they bind that home's endpoint and never the real one.

### The shim beside the executable

`c4 install` copies the `c4-shim` that sits beside the executable it runs from, which in development
is `target/<profile>/c4-shim` beside `c4-cli` after `cargo build -p c4-shim`. The packaged app
carries `c4-cli` and `c4-shim` together under its resources (`resources/bin/`, see Packaging), so
`c4 install` through the packaged `c4` takes the shim from there.

The icons under `apps/desktop/icons/` come from `assets/icons/c4-1024.png`, which `scripts/make_icon.py`
draws: the PNG sizes, `icon.ico` and `icon.icns` were derived from it. After a change to the source,
derive them again with any tool that writes those formats from a PNG. The 256 px file is named
`256x256.png` here; a generator that names it with a `2x` suffix after an `@` writes a name that
reads as an e-mail address to the secret scan.

## The Electron shell

The window runs in Electron, so Chromium renders it on every platform. `apps/desktop/electron/` is
the main process: `main.ts` decides between the window and the command line, `sidecar.ts` runs
`c4-cli serve` as a child process and speaks its line protocol, `protocol.ts` serves `dist/` under
the app's own `c4://app/` scheme with a locked content security policy, `ipc.ts` carries the
window's four requests and the core's event lines,
`preload.cts` is the one script the sandboxed page gets (it puts `window.c4` there), and `window.ts`
opens the window. `tsc -p electron/tsconfig.json` compiles the directory to `dist-electron/`, which git
ignores; the preload is a `.cts` file because a sandboxed preload must be CommonJS while the main
process is ES modules. The tests under `electron/tests/` run the sidecar against `fake-core.mjs`, a
stand-in that speaks the serve protocol.

`pnpm -C apps/desktop build` writes `dist/` and `dist-electron/`; `pnpm -C apps/desktop start` opens
the window from them. The core is `target/debug/c4-cli` (`.exe` on Windows) after
`cargo build -p c4-cli`, or whatever `C4_CLI` names; without it the window shows an error box and
exits. `pnpm -C apps/desktop dev` compiles the main process, starts Vite on 127.0.0.1:1420 and opens
Electron against it (`C4_DEV_URL`), so the page reloads on edit. `C4_DEVTOOLS=1` opens the developer
tools, `C4_LOG=debug` (or `warn`, `error`) sets the main process's log level on stderr, and the core's
own stderr lines appear there with a `core:` prefix. Chromium's files (cache, shader cache, local
storage) live under `<C4 home>/electron`, so `C4_HOME` moves them with the rest.

A request from the page waits for the core's hello line, so the window may load before the core is
up. A core that exits is started again after a pause that grows from one second to thirty while it
keeps failing, the requests in flight fail with the reason, and the page hears about each change as
a `core` event. A core that speaks another protocol version is fatal: an error box names the path.
On quit the shell closes the core's stdin first, after a short grace for the preferences the page
flushes on its way out, and kills a core that has not left two seconds later.

`pnpm install` downloads the Electron build for this platform (about 120 MB) through the package's
install script; `ELECTRON_SKIP_BINARY_DOWNLOAD=1` skips it, which the lint and test jobs set. If
`apps/desktop/node_modules/electron/dist` is missing after an install (pnpm did not run the script on
one machine here; the cause was not found), `node node_modules/electron/install.js` in `apps/desktop`
fetches it.

With the `electron` binary itself (`pnpm start`, `electron .`) a few arguments never reach C4:
Electron's default app answers `--version`, `--abi`, `--help` and `-i` on its own. Everything else
relays as the packaged `c4` does: `electron . sessions --json`, `electron . doctor`. Switches alone
open the window and reach Chromium, which is how a driver attaches to it:
`electron . --remote-debugging-port=9333`.

## Packaging

electron-builder (`apps/desktop/electron-builder.yml`) turns the build into the app: the asar holds
the page bundle, the main process and the icons; `c4-cli` and `c4-shim` sit beside it under
`resources/bin/`; Electron's own files, `LICENSES.chromium.html` among them, sit around it. The
executable is `c4` (`c4.app` on macOS, with `c4` inside) and the product name is C4.

```sh
cargo build --release -p c4-cli -p c4-shim
python scripts/stage_bin.py          # copies them to apps/desktop/resources/bin/, which git ignores
pnpm -C apps/desktop build
pnpm -C apps/desktop package:dir     # the unpacked app: release/win-unpacked/, mac-arm64/, linux-unpacked/
pnpm -C apps/desktop package         # the installers for this platform under apps/desktop/release/
```

`package` refuses to run until both binaries are staged (`electron/package.mjs` checks), so a package
never ships without its core; `python scripts/stage_bin.py --profile debug` stages the debug build,
which is what CI packages. electron-builder builds for the platform it runs on: NSIS and MSI
installers on Windows (`c4-<version>-win-x64.exe` and `.msi`), a dmg for arm64 and one for x64 on
macOS (`c4-<version>-mac-arm64.dmg`, `c4-<version>-mac-x64.dmg`), an AppImage and a deb on Linux
(`c4-<version>-linux-x64.AppImage` and `.deb`). The version is the one in `apps/desktop/package.json`,
kept equal to the workspace version in `Cargo.toml`; the icons come from `apps/desktop/icons/`
(`icon.ico`, `icon.icns`, `icon.png`). Nothing is signed: there is no certificate for Windows or Linux
and no Apple identity, so Windows shows its unknown-publisher warning for the installer. The unsigned
macOS build starts on both macOS runners, where nothing marks it as downloaded; a dmg from a release
carries the quarantine attribute, and what Gatekeeper then says about an unsigned app is not verified
here (no macOS hardware is in reach). The verify workflow packages the unpacked app with the debug
core on every runner, which proves the configuration on all four platforms; the release workflow that
builds the installers is the next step.

On Linux, Chromium's sandbox needs one of two things from the system: a kernel that lets an
unprivileged process create user namespaces, or the setuid helper `chrome-sandbox` beside the
executable, owned by root with mode 4755. Ubuntu 24.04 and later restrict user namespaces through
AppArmor, so electron-builder's post-install script in the deb installs a profile
(`/etc/apparmor.d/c4`) that allows them for `/opt/C4/c4`, and sets the helper setuid only where user
namespaces do not work at all. The AppImage and the unpacked build get neither, so on such a system
they stop before the main process runs, for the window and for a headless command alike, with
`The SUID sandbox helper binary was found, but is not configured correctly` on stderr and a SIGTRAP
exit. The ways out are a profile like the deb's for the binary's path (what the verify workflow loads
for the unpacked build), `--no-sandbox` on the command line (the page then runs without its renderer
sandbox; SECURITY.md), or the sysctl `kernel.apparmor_restrict_unprivileged_userns=0` for the whole
system. `resources/bin/c4-cli` beside the executable runs the headless commands with no Chromium in
the way.

`pnpm -C apps/desktop e2e` runs Playwright over that unpacked app (`apps/desktop/e2e/`, started
through Playwright's Electron launcher, no browser download): the window with the core from its
resources against the fixture Claude home, the fold chord and the preference it leaves behind, and
the relay of headless commands with their exit codes. A fresh temporary C4 home per test, and the
session list command replaced with one that prints an empty list, keep the real home out of it;
`C4_APP` names another build to test. The app runs with Chromium's sandbox on, as an installed one
does (`chromiumSandbox: true`; Playwright's Electron launcher adds `--no-sandbox` on Linux otherwise),
and a relay that fails reports its signal and stderr. The verify workflow runs it on every runner after
the package step, under xvfb and with the AppArmor profile above on Linux.

## Headless commands

A word on the command line, or `--help` or `--version`, makes `c4` run a headless command and exit
without opening a window; a command line of switches alone opens the window:

```sh
c4 sessions                        # every session discovery can see, as a table
c4 sessions --json                 # the same as JSON; --redact for pasting
c4 sessions --watch                # keep refreshing, print state changes
c4 doctor                          # the Claude home, claude, and C4's shim, install and endpoint
c4 telemetry --transcript <file>   # tokens, cost and agents of one transcript
c4 install [--dry-run]             # copy the shim, wrap the status line, add the hook groups
c4 uninstall [--dry-run]           # the way back, from the backup or entry by entry
```

`c4-cli` is the executable behind these commands: `cargo run -p c4-cli -- <command>` runs them during
development, and the packaged `c4` relays its command line to the copy under its resources and exits
with the core's exit code. One Windows caveat, checked with the packaged build: `c4.exe` is a windowed
executable, so an interactive PowerShell prompt comes back before the command has run (within 25 ms,
with no exit code), and the output lands on the console afterwards. Piping the output
(`c4 sessions --json | Out-String`) makes PowerShell wait, cmd.exe waits either way, and
`resources\bin\c4-cli.exe` beside `c4.exe` is the console program with the same command line, which
is what a script should call. On Linux the relay starts Chromium's browser process before the main
process can run the command, so the sandbox note under Packaging applies to `c4 doctor` as much as to
the window; `resources/bin/c4-cli` has no such need.

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
- **The Electron window says the core is missing.** It looked for `target/debug/c4-cli` (`.exe` on
  Windows) two directories above `apps/desktop`, or the path in `C4_CLI`. `cargo build -p c4-cli`
  puts it there.
- **`package` says c4-cli or c4-shim is missing.** They are copied, not built, by the package step:
  `python scripts/stage_bin.py` after `cargo build --release -p c4-cli -p c4-shim`, or
  `--profile debug` for the debug build.
