#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/precommit.py | author: watchthelight </wtl>
"""Pre-commit gate. Exit 1 stops the commit.

  python scripts/precommit.py            (run by .githooks/pre-commit)
  C4_PRECOMMIT_FULL=1                    run the whole Rust workspace test suite instead of --lib only
  C4_PRECOMMIT_SKIP_TESTS=1              skip the test suites (lint and scans still run; for WIP commits only)

Steps: git identity must be watchthelight <admin@watchthelight.org>; staged paths are checked against a
denylist (nothing from .claude/, no transcripts, keys, env files, backups or credentials); the secret scan,
header check and slop lint run on the staged files; then the test suites the staged files touch: cargo fmt
and cargo test for Rust, vitest for apps/, unittest for hooks/, compileall for Python.
"""
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PY = sys.executable
IDENTITY = ("watchthelight", "admin@watchthelight.org")
DENY_DIRS = {".claude"}
DENY_NAMES = {"settings.local.json"}


ENV = dict(os.environ)


def scrub_env():
    """cc-rs skips its own MSVC environment setup when CC is set, so outside a developer prompt every C
    dependency fails to compile. Drop CC for the child processes of this run and say so."""
    cc = ENV.get("CC", "")
    if os.name == "nt" and cc.lower().endswith("cl.exe") and not ENV.get("INCLUDE"):
        print("note: CC points at cl.exe without a developer-prompt environment; ignoring CC for this run")
        ENV.pop("CC", None)


def run(cmd, **kw):
    print("+ " + " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run(cmd, cwd=ROOT, env=ENV, **kw).returncode


def git(*args) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=False).stdout


def staged_paths():
    out = subprocess.run(["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"], cwd=ROOT,
                         capture_output=True, text=True, check=False).stdout
    return [p for p in out.split("\0") if p]


def denied(rel: str):
    p = Path(rel)
    parts = set(p.parts)
    name = p.name.lower()
    if parts & DENY_DIRS:
        return "paths under .claude/ are never committed"
    if name in DENY_NAMES or name.startswith("credentials"):
        return "local settings and credential files are never committed"
    if name.startswith(".env"):
        return "env files are never committed"
    if ".bak" in name:
        return "backup files are never committed"
    if p.suffix == ".jsonl" and p.parts[:1] != ("fixtures",):
        return "JSONL outside fixtures/ looks like a transcript or ledger"
    if p.suffix in {".pem", ".pfx", ".p12", ".crt", ".key"} and not rel.startswith("fixtures/registry/"):
        return "key and certificate files are never committed"
    return None


def find_cargo():
    c = shutil.which("cargo")
    if c:
        return c
    home = Path.home() / ".cargo" / "bin" / ("cargo.exe" if os.name == "nt" else "cargo")
    return str(home) if home.exists() else None


def find_pnpm():
    if os.name == "nt":
        for name in ("pnpm.cmd", "pnpm.exe"):
            p = shutil.which(name)
            if p:
                return p
    return shutil.which("pnpm")


def main():
    scrub_env()
    name = git("config", "user.name").strip()
    email = git("config", "user.email").strip()
    if (name, email) != IDENTITY:
        print(f"git identity is {name} <{email}>; set user.name/user.email to {IDENTITY[0]} <{IDENTITY[1]}> locally")
        return 1
    staged = staged_paths()
    if not staged:
        print("nothing staged")
        return 0
    bad = [(p, denied(p)) for p in staged if denied(p)]
    for p, why in bad:
        print(f"refused: {p}: {why}")
    if bad:
        return 1
    existing = [p for p in staged if (ROOT / p).is_file()]
    for script in ("scan.py", "check_headers.py", "slop_lint.py"):
        if run([PY, "-I", str(ROOT / "scripts" / script), *existing]) != 0:
            return 1
    skip_tests = os.environ.get("C4_PRECOMMIT_SKIP_TESTS") == "1"
    rust = [p for p in staged if p.endswith((".rs", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"))]
    py = [p for p in staged if p.endswith(".py")]
    hooks = [p for p in staged if p.startswith("hooks/")]
    front = [p for p in staged if p.startswith("apps/")]
    if py and run([PY, "-m", "compileall", "-q", *py]) != 0:
        return 1
    if rust:
        cargo = find_cargo()
        if not cargo:
            print("cargo not found; install the Rust toolchain before committing Rust changes")
            return 1
        if run([cargo, "fmt", "--all", "--check"]) != 0:
            return 1
        if not skip_tests:
            scope = ["--workspace"] if os.environ.get("C4_PRECOMMIT_FULL") == "1" else ["--workspace", "--lib"]
            if run([cargo, "test", *scope]) != 0:
                return 1
    if hooks and not skip_tests and (ROOT / "hooks" / "tests").is_dir():
        if run([PY, "-m", "unittest", "discover", "-s", "hooks/tests", "-t", "."]) != 0:
            return 1
    if front and not skip_tests and (ROOT / "apps" / "desktop" / "package.json").is_file():
        pnpm = find_pnpm()
        if not pnpm:
            print("pnpm not found; install it before committing frontend changes")
            return 1
        if not (ROOT / "apps" / "desktop" / "node_modules").is_dir():
            print("apps/desktop/node_modules missing; run pnpm install first")
            return 1
        if run([pnpm, "-C", "apps/desktop", "test"]) != 0:
            return 1
    print("pre-commit: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
