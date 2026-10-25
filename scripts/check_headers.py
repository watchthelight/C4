#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/check_headers.py | author: watchthelight </wtl>
"""Every text file carries a <wtl> header naming the project, its own path and the author. Exit 1 on a miss.

  python scripts/check_headers.py [PATH ...]      (default: all tracked files)

The header is a comment in the file's own syntax, anywhere in the first 2 KB:
  <wtl> project: c4 | file: scripts/check_headers.py | author: watchthelight </wtl>
Exempt: LICENSE, lock files, JSON and JSONL (no comment syntax), fonts, images, third-party license
texts under assets/fonts, and everything under fixtures/.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HEADER = re.compile(r"<wtl> project: c4 \| file: (?P<file>[^|]+?) \| author: watchthelight </wtl>")
EXEMPT_NAMES = {"LICENSE", "Cargo.lock", "pnpm-lock.yaml", "package-lock.json"}
EXEMPT_SUFFIXES = {".json", ".jsonl", ".lock", ".svg", ".png", ".ico", ".icns", ".ttf", ".otf", ".woff",
                   ".woff2", ".gif", ".jpg", ".jpeg", ".webp", ".pdf", ".zip", ".bin"}
EXEMPT_DIRS = {"fixtures", "node_modules", "target", "dist", ".git", "vendor"}
EXEMPT_PREFIXES = ("assets/fonts/",)


def tracked():
    out = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard"], cwd=ROOT,
                         capture_output=True, text=True, check=False)
    return [ROOT / n for n in out.stdout.splitlines()]


def exempt(rel: str) -> bool:
    p = Path(rel)
    if p.name in EXEMPT_NAMES or p.suffix.lower() in EXEMPT_SUFFIXES:
        return True
    if set(p.parts) & EXEMPT_DIRS:
        return True
    return rel.startswith(EXEMPT_PREFIXES)


def check(path: Path):
    rel = path.relative_to(ROOT).as_posix()
    if exempt(rel):
        return None
    try:
        head = path.read_bytes()[:2048].decode("utf-8", errors="replace")
    except OSError:
        return None
    m = HEADER.search(head)
    if not m:
        return f"{rel}: missing <wtl> header"
    if m.group("file").strip() != rel:
        return f"{rel}: header names '{m.group('file').strip()}' instead of its own path"
    return None


def main(argv):
    paths = [Path(a).resolve() for a in argv] if argv else tracked()
    problems = [r for r in (check(p) for p in paths if p.is_file()) if r]
    for p in problems:
        print(p)
    print(f"{len(problems)} header problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
