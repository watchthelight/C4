#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/scan.py | author: watchthelight </wtl>
"""Secret and identity scan for this public repo. Exit 1 on any hit. CI runs it on every push.

  python scripts/scan.py [PATH ...]      (default: the whole repo, tracked and untracked files)

Looks for tokens and keys, private addresses (including Tailscale's carrier-grade NAT range), personal e-mail
addresses (only admin@watchthelight.org and example addresses are allowed), Discord webhook URLs and long
numeric ids, home-folder paths with a real user name, SSH private keys, Claude Code OAuth fields, daemon
auth literals and real messaging socket names. Lines can opt out with "scan:allow" when the value is a
deliberate fake (fixtures), and .scanignore lists globs whose contents are fixtures or vendored files.
"""
import fnmatch
import ipaddress
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKIP_DIRS = {".git", "node_modules", "__pycache__", "dist", "target", "vendor"}
SKIP_EXT = {".png", ".jpg", ".jpeg", ".gif", ".ico", ".icns", ".ttf", ".otf", ".woff", ".woff2", ".lock", ".pdf"}
ALLOWED_EMAILS = {"admin@watchthelight.org", "owner@example.org", "you@example.com", "git@github.com"}
IGNORE_FILE = ROOT / ".scanignore"

PATTERNS = [
    (re.compile(r"\bgh[pousr]_[A-Za-z0-9]{20,}"), "GitHub token"),
    (re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}"), "GitHub token"),
    (re.compile(r"\bsk-(?:ant-)?[A-Za-z0-9_-]{20,}"), "API key"),
    (re.compile(r"\bxox[bpa]-[A-Za-z0-9-]{10,}"), "Slack token"),
    (re.compile(r"\bAKIA[0-9A-Z]{16}\b"), "AWS key"),
    (re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"), "private key"),
    (re.compile(r"discord(?:app)?\.com/api/webhooks/\d+/[\w-]+"), "Discord webhook"),
    (re.compile(r"\b[MN][A-Za-z\d]{23,25}\.[\w-]{6}\.[\w-]{27,38}\b"), "Discord token"),
    (re.compile(r"(?i)\bbearer\s+[A-Za-z0-9._~+/-]{24,}"), "bearer token"),
    (re.compile(r"(?i)\b(?:password|passwd|secret|api[_-]?key|token)\s*[:=]\s*['\"][^'\"\s]{12,}['\"]"), "credential literal"),
    (re.compile(r"(?i)\"(?:rvAuth|ptyAuth|authToken|accessToken|refreshToken)\"\s*:\s*\"[^\"]{8,}\""), "auth literal"),
    (re.compile(r"\bclaudeAiOauth\b"), "OAuth block"),
    (re.compile(r"\bcc-msg-[0-9a-f]{16}\b"), "messaging socket name"),
    (re.compile(r"\bcc-socks-\d+\b"), "messaging socket name"),
    (re.compile(r"(?i)ntfy\.sh/[A-Za-z0-9_-]{6,}"), "ntfy topic"),
    (re.compile(r"(?i)[A-Za-z]:\\\\?Users\\\\?(?!you\b|USER\b|me\b|example\b|runner\b|Shared\b|Public\b|Default\b|<|\$|%|\{)[A-Za-z][\w.-]+"), "home path with a user name"),
    # A lookbehind keeps repo paths such as fixtures/home/sessions out; a path that starts at a quote,
    # a space, an equals sign or a URL's third slash still counts.
    (re.compile(r"(?<![\w.-])(?:/Users/|/home/)(?!you\b|USER\b|me\b|example\b|runner\b|Shared\b|ubuntu\b|<|\$|\{)[A-Za-z][\w.-]+"), "home path with a user name"),
    (re.compile(r"\b\d{17,20}\b"), "long numeric id (Discord/snowflake?)"),
]
IPV4 = re.compile(r"(?<![\d.])(\d{1,3}(?:\.\d{1,3}){3})(?![\d.])")
EMAIL = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")


def files(paths):
    if paths:
        for p in paths:
            p = Path(p).resolve()  # relative to absolute, so .scanignore applies to command-line paths too
            if p.is_dir():
                yield from (q for q in p.rglob("*") if q.is_file())
            elif p.is_file():
                yield p
        return
    out = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard"], cwd=ROOT,
                         capture_output=True, text=True)
    names = out.stdout.splitlines() if out.returncode == 0 else [str(p.relative_to(ROOT)) for p in ROOT.rglob("*")]
    for n in names:
        p = ROOT / n
        if p.is_file() and not (set(Path(n).parts) & SKIP_DIRS):
            yield p


def ip_bad(s: str) -> bool:
    try:
        ip = ipaddress.ip_address(s)
    except ValueError:
        return False
    if ip.is_loopback or ip.is_unspecified or str(ip) in ("255.255.255.255", "255.255.255.0"):
        return False
    doc = [ipaddress.ip_network(n) for n in ("192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24")]
    if any(ip in n for n in doc):
        return False
    return True  # private, Tailscale (100.64/10) and public addresses all point at a real machine


def ignored(rel: str, globs) -> bool:
    rel = rel.replace("\\", "/")
    return any(fnmatch.fnmatch(rel, g) for g in globs)


def main():
    globs = [l.strip() for l in IGNORE_FILE.read_text(encoding="utf-8").splitlines()
             if l.strip() and not l.startswith("#")] if IGNORE_FILE.exists() else []
    hits = []
    for p in files(sys.argv[1:]):
        if p.suffix.lower() in SKIP_EXT or (ROOT in p.parents and ignored(str(p.relative_to(ROOT)), globs)):
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        rel = p.relative_to(ROOT) if ROOT in p.parents else p
        for n, line in enumerate(text.splitlines(), 1):
            if "scan:allow" in line:
                continue
            for rx, what in PATTERNS:
                if rx.search(line):
                    hits.append(f"{rel}:{n}: {what}")
            for m in IPV4.finditer(line):
                if ip_bad(m.group(1)) and not re.search(r"(?i)version|\bv\d", line[max(0, m.start() - 12):m.start()]):
                    hits.append(f"{rel}:{n}: IP address")
            for m in EMAIL.finditer(line):
                e = m.group(0).lower()
                if e not in ALLOWED_EMAILS and not e.endswith(("@example.org", "@example.com")):
                    hits.append(f"{rel}:{n}: e-mail address")
    for h in hits[:80]:
        print(h)
    print(f"{len(hits)} hit(s)")
    sys.exit(1 if hits else 0)


if __name__ == "__main__":
    main()
