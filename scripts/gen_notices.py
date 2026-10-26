#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/gen_notices.py | author: watchthelight </wtl>
"""Write THIRD-PARTY-NOTICES.md from the dependency graphs. With --check, exit 1 when the file is stale.

  python scripts/gen_notices.py            rewrite THIRD-PARTY-NOTICES.md
  python scripts/gen_notices.py --check    regenerate in memory and compare (what CI runs)

Three sources: `cargo about generate --format json --workspace` over about.toml (the crates that link into
the release targets; build and dev dependencies are excluded there), `pnpm -r licenses list --prod --json`
plus the license file inside each package (the npm packages bundled into the window), and the OFL.txt
beside every font under assets/fonts/. Identical license texts are printed once, with every package that
carries them named above the text. No local path reaches the output, so the file is the same on every
machine; sorting makes it the same on every run.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "THIRD-PARTY-NOTICES.md"
ABOUT = ROOT / "about.toml"
FONTS = ROOT / "assets" / "fonts"
LICENSE_FILE = re.compile(r"(?i)^(licen[cs]e|copying|unlicense)\b")
HEADER = "<!-- <wtl> project: c4 | file: THIRD-PARTY-NOTICES.md | author: watchthelight </wtl> -->"


def env_for_cargo():
    """cc-rs skips its own MSVC setup when CC is set, so a stray CC breaks nothing here (cargo-about
    compiles nothing) but is dropped anyway to match scripts/precommit.py."""
    env = dict(os.environ)
    if os.name == "nt" and env.get("CC", "").lower().endswith("cl.exe") and not env.get("INCLUDE"):
        env.pop("CC", None)
    return env


def find(name: str, *fallbacks: Path) -> str:
    names = [name, name + ".cmd", name + ".exe"] if os.name == "nt" else [name]
    for n in names:
        p = shutil.which(n)
        if p:
            return p
    for f in fallbacks:
        if f.exists():
            return str(f)
    sys.exit(f"{name} not found on PATH")


def run(cmd, **kw) -> str:
    print("+ " + " ".join(str(c) for c in cmd), file=sys.stderr, flush=True)
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", **kw)
    if r.returncode != 0:
        sys.stderr.write(r.stderr)
        sys.exit(f"{cmd[0]} {cmd[1]} failed with exit code {r.returncode}")
    return r.stdout


def run_json(cmd, **kw):
    return json.loads(run(cmd, **kw))


def norm(text: str) -> str:
    return "\n".join(line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")).strip("\n") + "\n"


def cargo_entries():
    """(packages, texts): one package row per crate, one text row per distinct license text."""
    cargo = find("cargo", Path.home() / ".cargo" / "bin" / "cargo", Path.home() / ".cargo" / "bin" / "cargo.exe")
    with tempfile.TemporaryDirectory() as tmp:
        # cargo-about refuses to write JSON to a redirected stdout under PowerShell, so it writes a file.
        out = Path(tmp) / "about.json"
        run([cargo, "about", "generate", "--format", "json", "--workspace", "--locked", "-c", str(ABOUT), "-o", str(out)],
            env=env_for_cargo())
        data = json.loads(out.read_text(encoding="utf-8"))
    packages = {}
    texts = []
    for entry in data["licenses"]:
        # A text with no source path was synthesized from the SPDX id (the crate ships no license file).
        from_license_file = LICENSE_FILE.match(os.path.basename(entry.get("source_path") or "LICENSE")) is not None
        users = []
        for u in entry["used_by"]:
            crate = u["crate"]
            if crate.get("source") is None:
                continue  # a workspace member: C4's own code, covered by LICENSE
            key = (crate["name"], crate["version"])
            packages.setdefault(key, {"kind": "crate", "name": crate["name"], "version": crate["version"],
                                      "declared": crate.get("license") or "?",
                                      "source": crate.get("repository") or "", "used": []})
            users.append(key)
        if users:
            texts.append({"id": entry["id"], "name": entry["name"], "text": norm(entry["text"]),
                          "users": sorted(set(users)), "from_license_file": from_license_file})
    # cargo-about also picks up source files that open with a license header (a copied .rs or .h file).
    # When the crate ships a real license file for the same license, that source file is noise.
    real = {(key, t["id"]) for t in texts if t["from_license_file"] for key in t["users"]}
    for t in texts:
        if not t["from_license_file"]:
            t["users"] = [k for k in t["users"] if (k, t["id"]) not in real]
    texts = [t for t in texts if t["users"]]
    for t in texts:
        for key in t["users"]:
            if t["id"] not in packages[key]["used"]:
                packages[key]["used"].append(t["id"])
    packages = {k: v for k, v in packages.items() if v["used"]}
    return packages, texts


def accepted_order():
    return tomllib.loads(ABOUT.read_text(encoding="utf-8"))["accepted"]


def pick_license(declared: str, order):
    """The license C4 takes a dual-licensed package under: the first accepted one of its OR choices."""
    choices = [c.strip().strip("()") for c in re.split(r"\s+OR\s+", declared)]
    for want in order:
        if want in choices:
            return want
    return choices[0]


def license_file_for(pkg_dir: Path, chosen: str):
    files = sorted(p for p in pkg_dir.iterdir() if p.is_file() and LICENSE_FILE.match(p.name))
    token = chosen.split("-")[0].upper()  # MIT, APACHE, BSD, ...
    specific = [p for p in files if token in p.name.upper()]
    generic = [p for p in files if not re.search(r"(?i)(mit|apache|bsd|isc|mpl|zlib)", p.name)]
    for group in (specific, generic, files):
        if group:
            return group[0]
    return None


def npm_entries():
    pnpm = find("pnpm")
    data = run_json([pnpm, "-r", "licenses", "list", "--prod", "--json"])
    order = accepted_order()
    packages, texts = {}, []
    for _, items in data.items():
        for item in items:
            declared = item.get("license") or "?"
            chosen = pick_license(declared, order)
            for version, path in zip(item["versions"], item.get("paths") or []):
                key = (item["name"], version)
                packages[key] = {"kind": "npm", "name": item["name"], "version": version, "declared": declared,
                                 "source": item.get("homepage") or "", "used": [chosen]}
                f = license_file_for(Path(path), chosen)
                if f is None:
                    sys.exit(f"{item['name']}@{version}: no license file in the package")
                texts.append({"id": chosen, "name": chosen, "text": norm(f.read_text(encoding="utf-8")), "users": [key]})
    return packages, texts


def font_entries():
    packages, texts = {}, []
    if not FONTS.is_dir():
        return packages, texts
    for d in sorted(p for p in FONTS.iterdir() if p.is_dir()):
        ofl = next((p for p in d.iterdir() if p.name.upper() in ("OFL.TXT", "OFL")), None)
        if ofl is None:
            sys.exit(f"{d.relative_to(ROOT).as_posix()}: no OFL.txt beside the font")
        key = (d.name, "")
        packages[key] = {"kind": "font", "name": d.name, "version": "", "declared": "OFL-1.1",
                         "source": "", "used": ["OFL-1.1"]}
        texts.append({"id": "OFL-1.1", "name": "SIL Open Font License 1.1", "text": norm(ofl.read_text(encoding="utf-8")),
                      "users": [key]})
    return packages, texts


def merge_texts(groups):
    """Identical texts from different sources collapse into one entry with the union of their users."""
    by_text = {}
    for t in groups:
        e = by_text.setdefault((t["id"], t["text"]), {"id": t["id"], "name": t["name"], "text": t["text"], "users": set()})
        e["users"].update(t["users"])
    out = [dict(e, users=sorted(e["users"])) for e in by_text.values()]
    out.sort(key=lambda e: (e["id"], e["users"], e["text"]))
    return out


def label(key):
    name, version = key
    return f"{name} {version}".strip()


def table(rows, kind_name, head):
    lines = [f"## {kind_name} ({len(rows)})", "", head, "|---|---|---|---|---|"]
    for r in rows:
        lines.append(f"| {r['name']} | {r['version'] or '-'} | {', '.join(sorted(r['used']))} | {r['declared']} | {r['source'] or '-'} |")
    return lines + [""]


def render(crates, npm, fonts, texts):
    lines = [HEADER,
             "<!-- Generated by scripts/gen_notices.py from about.toml and the pnpm workspace. Do not edit by hand;",
             "     run the script and commit the result. -->",
             "",
             "# Third-party notices",
             "",
             "C4 is MIT licensed (see LICENSE). The executable and the window it opens also contain the packages",
             "listed here, each under its own license. Build tools, test tools and development dependencies are not",
             "part of the shipped program and are not listed. \"Used under\" is the license C4 takes a package under",
             "when its author offers a choice; \"Declared\" is the package's own license expression.",
             ""]
    lines += table(sorted(crates.values(), key=lambda r: (r["name"], r["version"])), "Rust crates",
                   "| Crate | Version | Used under | Declared | Source |")
    lines += table(sorted(npm.values(), key=lambda r: (r["name"], r["version"])), "npm packages",
                   "| Package | Version | Used under | Declared | Source |")
    if fonts:
        lines += table(sorted(fonts.values(), key=lambda r: r["name"]), "Fonts",
                       "| Font | Version | Used under | Declared | Source |")
    lines += ["## License texts", "",
              f"{len(texts)} distinct texts. Identical texts are shown once; the packages that carry each one are",
              "named above it.", ""]
    counts = {}
    for t in texts:
        counts[t["id"]] = counts.get(t["id"], 0) + 1
    seen = {}
    for t in texts:
        seen[t["id"]] = seen.get(t["id"], 0) + 1
        lines.append(f"### {t['id']} ({seen[t['id']]} of {counts[t['id']]})")
        lines.append("")
        lines.append("Used by: " + ", ".join(label(u) for u in t["users"]))
        lines.append("")
        lines.append("````text")
        lines.append(t["text"].rstrip("\n"))
        lines.append("````")
        lines.append("")
    return "\n".join(lines).rstrip("\n") + "\n"


def main(argv):
    check = "--check" in argv
    crates, crate_texts = cargo_entries()
    npm, npm_texts = npm_entries()
    fonts, font_texts = font_entries()
    text = render(crates, npm, fonts, merge_texts(crate_texts + npm_texts + font_texts))
    if check:
        current = OUT.read_text(encoding="utf-8").replace("\r\n", "\n") if OUT.exists() else ""
        if current != text:
            a, b = current.splitlines(), text.splitlines()
            at = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
            print(f"THIRD-PARTY-NOTICES.md is stale (first difference at line {at + 1}); run scripts/gen_notices.py")
            return 1
        print("THIRD-PARTY-NOTICES.md is current")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.name}: {len(crates)} crates, {len(npm)} npm packages, {len(fonts)} fonts, {len(text)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
