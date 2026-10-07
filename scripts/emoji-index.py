#!/usr/bin/env python3
"""Build crates/app/assets/emoji/emoji.tsv from emojibase-data (MIT).

Usage: scripts/emoji-index.py [emojibase-data version, default 17.0.0]
Line: emoji <TAB> English gemoji codes (space) <TAB> German aliases (|), sorted by emojibase order.
"""
import json
import pathlib
import subprocess
import sys
import tarfile
import tempfile

version = sys.argv[1] if len(sys.argv) > 1 else "17.0.0"
repo_root = pathlib.Path(__file__).resolve().parent.parent
output_path = repo_root / "crates/app/assets/emoji/emoji.tsv"

with tempfile.TemporaryDirectory() as work_directory:
    subprocess.run(
        ["npm", "pack", f"emojibase-data@{version}", "--silent"],
        cwd=work_directory, check=True, stdout=subprocess.DEVNULL,
    )
    archive = next(pathlib.Path(work_directory).glob("emojibase-data-*.tgz"))
    with tarfile.open(archive) as tar:
        tar.extractall(work_directory, filter="data")
    package = pathlib.Path(work_directory) / "package"
    load = lambda relative: json.loads((package / relative).read_text(encoding="utf-8"))
    english = load("en/compact.json")
    german = {entry["hexcode"]: entry for entry in load("de/compact.json")}
    english_codes = load("en/shortcodes/github.json")
    german_codes = load("de/shortcodes/cldr.json")

as_list = lambda value: value if isinstance(value, list) else [value]
lines = []
for entry in sorted(english, key=lambda entry: entry.get("order", 1 << 30)):
    hexcode = entry["hexcode"]
    codes = as_list(english_codes.get(hexcode, []))
    if not codes:
        continue
    german_entry = german.get(hexcode, {})
    aliases = []
    candidates = [german_entry.get("label", "")]
    candidates += [code.replace("_", " ") for code in as_list(german_codes.get(hexcode, []))]
    candidates += german_entry.get("tags", [])
    for alias in candidates:
        alias = alias.lower().strip()
        if alias and alias not in aliases and "|" not in alias and "\t" not in alias:
            aliases.append(alias)
    emoji = entry["unicode"]
    if emoji.endswith("\ufe0f") and len(emoji) == 2 and ord(emoji[0]) >= 0x1F300:
        emoji = emoji[0]
    lines.append(f"{emoji}\t{' '.join(codes)}\t{'|'.join(aliases)}")

output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"{len(lines)} emoji -> {output_path.relative_to(repo_root)}")
