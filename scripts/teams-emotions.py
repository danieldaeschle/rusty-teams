#!/usr/bin/env python3
"""Build crates/chatsvc/assets/emotions.tsv from the Teams emoticon catalog.

Usage: scripts/teams-emotions.py <en-us.json>
Source: https://statics.teams.cdn.office.net/evergreen-assets/personal-expressions/v1/metadata/<emoticonAssetVersion>/en-us.json (version in the Teams web config).
Line: glyph without U+FE0F <TAB> Teams keys (space), catalog order, first key primary.
"""
import json
import pathlib
import sys

catalog_path = pathlib.Path(sys.argv[1])
repo_root = pathlib.Path(__file__).resolve().parent.parent
output_path = repo_root / "crates/chatsvc/assets/emotions.tsv"

catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
keys_by_glyph = {}
for category in catalog["categories"]:
    for emoticon in category["emoticons"]:
        glyph = emoticon.get("unicode", "").replace("️", "")
        key = emoticon.get("id", "")
        if not glyph or not key:
            continue
        keys = keys_by_glyph.setdefault(glyph, [])
        if key not in keys:
            keys.append(key)

lines = [f"{glyph}\t{' '.join(keys)}" for glyph, keys in keys_by_glyph.items()]
output_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"{len(lines)} glyphs -> {output_path}")
