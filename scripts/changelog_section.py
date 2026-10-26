#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/changelog_section.py | author: watchthelight </wtl>
"""Print one version's section of CHANGELOG.md, as the body of a GitHub release.

  python scripts/changelog_section.py 0.1.0                     the body of the "## [0.1.0]" section
  python scripts/changelog_section.py 0.1.0 --footer FILE       with FILE after it (the install notes every release carries)
  python scripts/changelog_section.py 0.1.0 --or-unreleased     the "## [Unreleased]" section when 0.1.0 has none, marked as such (a dry run)

The heading itself goes and the body stays, with HTML comments (the <wtl> headers) and the link
definitions at the end of the file dropped, so what comes out is markdown alone. Exit 1 with a message
when the version has no section.
"""
import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HEADING = re.compile(r"^## \[(?P<name>[^\]]+)\]")
COMMENT = re.compile(r"<!--.*?-->", re.S)
LINK_DEFINITION = re.compile(r"^\[[^\]]+\]: \S+$")


def sections(text: str) -> dict[str, str]:
    """The level-two sections of a changelog, name to body."""
    out: dict[str, str] = {}
    name = None
    body: list[str] = []
    for line in text.splitlines():
        match = HEADING.match(line)
        if match:
            if name is not None:
                out[name] = "\n".join(body).strip("\n")
            name = match.group("name")
            body = []
        elif name is not None and not LINK_DEFINITION.match(line):
            body.append(line)
    if name is not None:
        out[name] = "\n".join(body).strip("\n")
    return out


def main(argv) -> int:
    parser = argparse.ArgumentParser(description="print one version's section of CHANGELOG.md")
    parser.add_argument("version", help="the version whose section to print, without the v")
    parser.add_argument("--changelog", type=Path, default=ROOT / "CHANGELOG.md", help="another changelog file")
    parser.add_argument("--footer", type=Path, default=None, help="a markdown file to print after the section")
    parser.add_argument(
        "--or-unreleased", action="store_true", help="print the Unreleased section when the version has none"
    )
    args = parser.parse_args(argv)

    found = sections(COMMENT.sub("", args.changelog.read_text(encoding="utf-8")))
    parts = []
    if args.version in found:
        parts.append(found[args.version])
    elif args.or_unreleased and "Unreleased" in found:
        parts.append(
            f"A dry run: CHANGELOG.md has no section for version {args.version} yet, "
            "so this is its Unreleased section."
        )
        parts.append(found["Unreleased"])
    else:
        print(f"changelog_section: no '## [{args.version}]' section in {args.changelog}", file=sys.stderr)
        return 1
    if args.footer is not None:
        parts.append(COMMENT.sub("", args.footer.read_text(encoding="utf-8")).strip("\n"))
    sys.stdout.write("\n\n".join(part for part in parts if part) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
