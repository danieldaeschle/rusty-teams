"""Keep a CBDT Noto Color Emoji face loadable by GPUI.

GPUI drops a requested family when the face has no glyph for "m", and it
only paints color bitmaps for the PostScript name NotoColorEmoji. Cosmic Text
on Linux falls back to the family name Noto Color Emoji. This script leaves
both names alone and maps U+006D onto an existing glyph so the face survives
that check. The system COLRv1 face of the same name is hidden at runtime.
"""

import sys
from fontTools.ttLib import TTFont

SOURCE = sys.argv[1]
DEST = sys.argv[2]


def add_latin_m(font: TTFont) -> None:
    cmap = font.getBestCmap()
    if not cmap:
        raise SystemExit("emoji font has no unicode cmap")
    glyph = next(iter(cmap.values()))
    for table in font["cmap"].tables:
        if table.isUnicode():
            table.cmap[ord("m")] = glyph
    if font.getGlyphID(glyph) == 0:
        raise SystemExit("mapped m onto .notdef")


def main() -> None:
    font = TTFont(SOURCE)
    if "CBDT" not in font or "CBLC" not in font:
        raise SystemExit("source is not a CBDT/CBLC color font")
    if "COLR" in font:
        raise SystemExit("source also has a COLR table")
    add_latin_m(font)
    font.save(DEST)
    saved = TTFont(DEST)
    names = {record.nameID: record.toUnicode() for record in saved["name"].names if record.nameID in (1, 6)}
    if names.get(1) != "Noto Color Emoji" or names.get(6) != "NotoColorEmoji":
        raise SystemExit(f"unexpected names: {names}")
    if ord("m") not in (saved.getBestCmap() or {}):
        raise SystemExit("cmap is missing m")
    print("wrote", DEST)


if __name__ == "__main__":
    main()
