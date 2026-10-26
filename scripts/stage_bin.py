#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/stage_bin.py | author: watchthelight </wtl>
"""Copy c4-cli and c4-shim into apps/desktop/resources/bin/, where electron-builder picks them up.

  python scripts/stage_bin.py                     from target/release/ (cargo build --release -p c4-cli -p c4-shim)
  python scripts/stage_bin.py --profile debug     from target/debug/
  python scripts/stage_bin.py --target <triple>   from target/<triple>/<profile>/, a cross build

The copies are what the packaged app carries under its resources: `c4 <command>` relays to that c4-cli, and
`c4 install` takes the shim beside it. resources/ is not tracked; the two names are replaced on every run so a
stale copy never ships, and a missing source is an error that names the build to run.
"""
import argparse
import os
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "apps" / "desktop" / "resources" / "bin"
NAMES = ("c4-cli", "c4-shim")


def main(argv) -> int:
    parser = argparse.ArgumentParser(description="stage c4-cli and c4-shim for the packaged app")
    parser.add_argument("--profile", default="release", help="the cargo profile directory to copy from (default: release)")
    parser.add_argument("--target", default=None, help="a target triple, for a build under target/<triple>/")
    args = parser.parse_args(argv)

    source = ROOT / "target"
    if args.target:
        source = source / args.target
    source = source / args.profile
    suffix = ".exe" if os.name == "nt" else ""
    files = [source / f"{name}{suffix}" for name in NAMES]
    missing = [f for f in files if not f.is_file()]
    if missing:
        names = ", ".join(str(f.relative_to(ROOT)) for f in missing)
        flag = " --release" if args.profile == "release" else ""
        target = f" --target {args.target}" if args.target else ""
        print(f"stage_bin: missing {names}; run cargo build{flag}{target} -p c4-cli -p c4-shim", file=sys.stderr)
        return 1

    DEST.mkdir(parents=True, exist_ok=True)
    for f in files:
        out = DEST / f.name
        if out.exists():
            out.unlink()
        shutil.copy2(f, out)
        print(f"staged {out.relative_to(ROOT).as_posix()} ({out.stat().st_size} bytes) from {f.relative_to(ROOT).as_posix()}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
