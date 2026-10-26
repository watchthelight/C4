#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/stage_shim.py | author: watchthelight </wtl>
"""Stage the built c4-shim where the Tauri bundler expects a sidecar.

  python scripts/stage_shim.py                       the release shim, for the host target
  python scripts/stage_shim.py --profile debug       the debug shim (what CI stages)
  python scripts/stage_shim.py --target <triple>     a shim built with cargo --target, under target/<triple>/

Tauri names a sidecar `<name>-<target triple>` (plus `.exe` on Windows) under src-tauri/binaries/ and
strips the triple when it places the file beside the app. This copies target/[<triple>/]<profile>/
c4-shim[.exe] to apps/desktop/src-tauri/binaries/c4-shim-<triple>[.exe] and prints what it did. It
builds nothing: run `cargo build -p c4-shim --release` first. The sidecar is declared in
apps/desktop/src-tauri/tauri.sidecar.conf.json, which `tauri build --config` merges in, so a plain
cargo build or test of the workspace never needs a staged file.
"""
import argparse
import hashlib
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BINARIES = ROOT / "apps" / "desktop" / "src-tauri" / "binaries"


def host_triple() -> str:
    rustc = shutil.which("rustc") or shutil.which("rustc.exe")
    if not rustc:
        sys.exit("rustc not found on PATH; pass --target <triple>")
    out = subprocess.run([rustc, "-vV"], capture_output=True, text=True, check=True).stdout
    for line in out.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    sys.exit("rustc -vV printed no host line")


def shown(path: Path) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def main() -> int:
    ap = argparse.ArgumentParser(description="Stage the built c4-shim as the app's Tauri sidecar.")
    ap.add_argument("--profile", default="release", help="the cargo profile directory to take it from (default: release)")
    ap.add_argument("--target", help="the target triple (default: the host, from rustc -vV)")
    ap.add_argument("--target-dir", default=str(ROOT / "target"), help="the cargo target directory (default: target/)")
    args = ap.parse_args()

    triple = args.target or host_triple()
    exe = ".exe" if "windows" in triple else ""
    target_dir = Path(args.target_dir)
    # A build with --target lands under target/<triple>/<profile>/; a host build under target/<profile>/.
    candidates = [target_dir / triple / args.profile, target_dir / args.profile]
    if not args.target:
        candidates.reverse()
    src = next((d / f"c4-shim{exe}" for d in candidates if (d / f"c4-shim{exe}").is_file()), None)
    if src is None:
        flag = " --release" if args.profile == "release" else ""
        looked = " or ".join(shown(d / f"c4-shim{exe}") for d in candidates)
        sys.exit(f"no built shim at {looked}; run `cargo build -p c4-shim{flag}` first")

    dest = BINARIES / f"c4-shim-{triple}{exe}"
    BINARIES.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dest)
    digest = hashlib.sha256(dest.read_bytes()).hexdigest()
    print(f"staged {shown(dest)} from {shown(src)}: {dest.stat().st_size} bytes, sha256 {digest[:16]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
