<!-- <wtl> project: c4 | file: .github/release-notes.md | author: watchthelight </wtl> -->
<!-- The second half of every release body: scripts/changelog_section.py prints the version's
     changelog section, then this file with its comments dropped. Keep it true for the release at
     hand; the platform lines follow docs/VERIFICATION.md. -->

## Installing

| Platform | Asset | What to expect |
|---|---|---|
| Windows x64 | `c4-<version>-win-x64.exe` (installs for the current user, no administrator rights), or the `.msi` | not signed, so SmartScreen says "Windows protected your PC": More info, then Run anyway |
| macOS, Apple silicon | `c4-<version>-mac-arm64.dmg` | not signed or notarized, so macOS refuses the first open; allow it under System Settings, Privacy & Security (Open Anyway), or run `xattr -d com.apple.quarantine /Applications/c4.app`. Not checked on a Mac: no hardware in reach |
| macOS, Intel | `c4-<version>-mac-x64.dmg` | the same |
| Linux x64, Debian or Ubuntu | `c4-<version>-linux-amd64.deb` | installs `/opt/C4/c4` with `/usr/bin/c4`; where the kernel restricts user namespaces (Ubuntu 24.04 and later) its post-install script also loads the AppArmor profile Chromium's sandbox needs |
| Linux x64, any distribution | `c4-<version>-linux-x86_64.AppImage` | no profile comes with it, so on Ubuntu 24.04 and later it needs `--no-sandbox` on the command line (the page then runs without its renderer sandbox) or `sysctl kernel.apparmor_restrict_unprivileged_userns=0`; docs/BUILDING.md, Packaging |

Nothing is signed: there is no Windows certificate, no Apple identity and no Linux key. What stands
in for a signature is `SHA256SUMS` and the build provenance GitHub recorded for every asset when the
release workflow built it:

    sha256sum --ignore-missing -c SHA256SUMS
    gh attestation verify c4-<version>-win-x64.exe --repo watchthelight/C4

`c4` opens the window; `c4 --help` lists the headless commands; `c4 install` wires the shim into
Claude Code's settings file after a backup, and `c4 uninstall` takes it back.

## Platform status

Windows x64: built, tested in CI and run by hand. macOS arm64 and x64: built and tested in CI, where
the app starts; not run on a Mac. Linux x64: built and tested in CI; not run on a desktop.
docs/VERIFICATION.md has the detail and the runs.
