#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/verify_release.py | author: watchthelight </wtl>
"""Check a published C4 release: test L of docs/VERIFICATION.md.

  python scripts/verify_release.py v0.1.0                 download the assets to a temporary directory and check them
  python scripts/verify_release.py v0.1.0 --assets DIR    check the assets already in DIR (what a post-release job downloaded)
  python scripts/verify_release.py v0.1.0 --repo OWNER/NAME

Everything goes through the gh command line, signed in with read access to the repository and its
workflow runs. The checks: the release exists and is published, not a draft; the asset set is complete
(the Windows installer and msi, a dmg per macOS architecture, the Linux AppImage and deb, SHA256SUMS);
every asset matches SHA256SUMS and every asset is listed there; every asset has a build provenance
attestation (gh attestation verify); the verify workflow passed on the tagged commit and the release
workflow passed on the tag; the documents the directive requires exist at the tag. One line per check,
and exit 1 when any failed.
"""
import argparse
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

DEFAULT_REPO = "watchthelight/C4"
REQUIRED_DOCS = (
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "LICENSE",
    "THIRD-PARTY-NOTICES.md",
    "docs/HOW-IT-WORKS.md",
    "docs/ARCHITECTURE.md",
    "docs/SESSION-INTEGRATION.md",
    "docs/DESIGN-SYSTEM.md",
    "docs/SECURITY.md",
    "docs/BUILDING.md",
    "docs/VERIFICATION.md",
)
CHECKSUM_LINE = re.compile(r"^(?P<digest>[0-9a-f]{64}) [ *](?P<name>\S.*)$")
WAIT_MINUTES = 10


def expected_assets(version: str) -> dict[str, re.Pattern]:
    """One pattern per asset a release carries; the Linux names spell the architecture as each format does."""
    v = re.escape(version)
    return {
        "windows installer": re.compile(rf"^c4-{v}-win-x64\.exe$"),
        "windows msi": re.compile(rf"^c4-{v}-win-x64\.msi$"),
        "macos arm64 dmg": re.compile(rf"^c4-{v}-mac-arm64\.dmg$"),
        "macos x64 dmg": re.compile(rf"^c4-{v}-mac-x64\.dmg$"),
        "linux appimage": re.compile(rf"^c4-{v}-linux-[^.]+\.AppImage$"),
        "linux deb": re.compile(rf"^c4-{v}-linux-[^.]+\.deb$"),
        "checksums": re.compile(r"^SHA256SUMS$"),
    }


class Report:
    """One line per check, a count at the end."""

    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0

    def ok(self, text: str) -> None:
        self.passed += 1
        print(f"ok    {text}")

    def fail(self, text: str) -> None:
        self.failed += 1
        print(f"FAIL  {text}")

    def check(self, condition: bool, ok_text: str, fail_text: str) -> None:
        if condition:
            self.ok(ok_text)
        else:
            self.fail(fail_text)

    def finish(self) -> int:
        print(f"{self.passed} passed, {self.failed} failed")
        return 1 if self.failed else 0


def gh(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8")
    if check and result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)}: exit {result.returncode}: {result.stderr.strip()}")
    return result


def gh_json(*args: str):
    return json.loads(gh(*args).stdout)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tag_commit(repo: str, tag: str) -> str:
    """The commit a tag names, through an annotated tag object when there is one."""
    obj = gh_json("api", f"repos/{repo}/git/ref/tags/{tag}")["object"]
    if obj["type"] == "tag":
        obj = gh_json("api", f"repos/{repo}/git/tags/{obj['sha']}")["object"]
    return obj["sha"]


def run_conclusion(repo: str, run: dict) -> str:
    """A run's conclusion, waiting a while for one still going (the release run is, when the
    post-release workflow starts from its last step)."""
    deadline = time.monotonic() + WAIT_MINUTES * 60
    while run["status"] != "completed":
        if time.monotonic() > deadline:
            return f"still {run['status']} after {WAIT_MINUTES} min"
        time.sleep(15)
        run = gh_json("run", "view", str(run["databaseId"]), "--repo", repo, "--json", "status,conclusion,databaseId")
    return run["conclusion"]


def check_runs(report: Report, repo: str, tag: str) -> None:
    try:
        sha = tag_commit(repo, tag)
    except RuntimeError as err:
        report.fail(f"the tag's commit: {err}")
        return
    fields = "databaseId,status,conclusion,url"
    runs = gh_json("run", "list", "--repo", repo, "--commit", sha, "--workflow", "verify.yml", "--event", "push",
                   "--json", fields, "--limit", "3")
    if not runs:
        report.fail(f"verify: no push run for commit {sha[:10]}")
    else:
        conclusion = run_conclusion(repo, runs[0])
        report.check(conclusion == "success", f"verify run {runs[0]['url']}: {conclusion} on commit {sha[:10]}",
                     f"verify run {runs[0]['url']}: {conclusion} on commit {sha[:10]}")
    runs = gh_json("run", "list", "--repo", repo, "--workflow", "release.yml", "--branch", tag, "--json", fields,
                   "--limit", "3")
    if not runs:
        report.fail(f"release: no run for tag {tag}")
    else:
        conclusion = run_conclusion(repo, runs[0])
        report.check(conclusion == "success", f"release run {runs[0]['url']}: {conclusion}",
                     f"release run {runs[0]['url']}: {conclusion}")


def check_docs(report: Report, repo: str, tag: str) -> None:
    missing = []
    for path in REQUIRED_DOCS:
        result = gh("api", f"repos/{repo}/contents/{path}?ref={tag}", "--jq", ".size", check=False)
        if result.returncode != 0:
            missing.append(path)
    report.check(not missing, f"the {len(REQUIRED_DOCS)} required documents exist at {tag}",
                 f"documents missing at {tag}: {', '.join(missing)}")


def main(argv) -> int:
    parser = argparse.ArgumentParser(description="check a published C4 release (test L)")
    parser.add_argument("tag", help="the release tag, such as v0.1.0")
    parser.add_argument("--repo", default=DEFAULT_REPO, help=f"OWNER/NAME (default {DEFAULT_REPO})")
    parser.add_argument("--assets", type=Path, default=None,
                        help="a directory that already holds the assets; otherwise they are downloaded")
    args = parser.parse_args(argv)
    tag = args.tag
    version = tag[1:] if tag.startswith("v") else tag
    report = Report()

    try:
        release = gh_json("release", "view", tag, "--repo", args.repo, "--json",
                          "tagName,isDraft,isPrerelease,publishedAt,assets,url")
    except RuntimeError as err:
        report.fail(f"release {tag}: {err}")
        return report.finish()
    kind = "pre-release" if release.get("isPrerelease") else "release"
    report.check(not release["isDraft"], f"{kind} {tag} published {release.get('publishedAt')}: {release['url']}",
                 f"{kind} {tag} is still a draft: {release['url']}")

    names = [asset["name"] for asset in release["assets"]]
    for label, pattern in expected_assets(version).items():
        hits = [name for name in names if pattern.match(name)]
        report.check(len(hits) == 1, f"{label}: {hits[0] if hits else ''}",
                     f"{label}: {'none' if not hits else 'several, ' + ', '.join(hits)} among {', '.join(names) or 'no assets'}")

    if args.assets is None:
        assets = Path(tempfile.mkdtemp(prefix="c4-release-"))
        gh("release", "download", tag, "--repo", args.repo, "--dir", str(assets), "--clobber")
        print(f"downloaded to {assets}")
    else:
        assets = args.assets
    files = sorted(path for path in assets.iterdir() if path.is_file() and path.name != "SHA256SUMS")
    sums = assets / "SHA256SUMS"
    if not sums.is_file():
        report.fail("SHA256SUMS: not among the files")
    else:
        listed: dict[str, str] = {}
        for line in sums.read_text(encoding="utf-8").splitlines():
            match = CHECKSUM_LINE.match(line.strip())
            if match:
                listed[match.group("name")] = match.group("digest")
            elif line.strip():
                report.fail(f"SHA256SUMS: a line that is not a checksum: {line.strip()!r}")
        for path in files:
            digest = sha256(path)
            if path.name not in listed:
                report.fail(f"{path.name}: not listed in SHA256SUMS")
            else:
                report.check(listed[path.name] == digest, f"{path.name}: sha256 {digest[:16]}... matches",
                             f"{path.name}: sha256 {digest} differs from the listed {listed[path.name]}")
        for name in listed:
            if not (assets / name).is_file():
                report.fail(f"SHA256SUMS lists {name}, which is not among the files")

    for path in files:
        result = gh("attestation", "verify", str(path), "--repo", args.repo, check=False)
        report.check(result.returncode == 0, f"{path.name}: attestation verified",
                     f"{path.name}: gh attestation verify exit {result.returncode}: {result.stderr.strip()[-400:]}")

    check_runs(report, args.repo, tag)
    check_docs(report, args.repo, tag)
    return report.finish()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
