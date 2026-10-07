import sys
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
SOURCE = sys.argv[1]
STYLES = [("Regular", 400, 0, False), ("SemiBold", 600, 0, False), ("Bold", 700, 0, False), ("Italic", 400, -10, True)]
for style, weight, slant, italic in STYLES:
    font = TTFont(SOURCE)
    pinned = instancer.instantiateVariableFont(font, {"wght": weight, "wdth": 100, "opsz": 14, "GRAD": 0, "ROND": 100, "slnt": slant})
    name = pinned["name"]
    family = "Google Sans Flex"
    if weight == 600:
        family_legacy, sub_legacy = "Google Sans Flex SemiBold", "Regular"
    elif weight == 700:
        family_legacy, sub_legacy = family, "Bold"
    elif italic:
        family_legacy, sub_legacy = family, "Italic"
    else:
        family_legacy, sub_legacy = family, "Regular"
    full = f"{family} {style}" if style != "Regular" else family
    for record in list(name.names):
        if record.nameID in (1, 2, 3, 4, 6, 16, 17, 25):
            name.removeNames(nameID=record.nameID)
    for platform, encoding, language in ((3, 1, 0x409), (1, 0, 0)):
        name.setName(family_legacy, 1, platform, encoding, language)
        name.setName(sub_legacy, 2, platform, encoding, language)
        name.setName(f"{family}-{style}", 3, platform, encoding, language)
        name.setName(full, 4, platform, encoding, language)
        name.setName(f"GoogleSansFlex-{style}", 6, platform, encoding, language)
        name.setName(family, 16, platform, encoding, language)
        name.setName(style, 17, platform, encoding, language)
    pinned["OS/2"].usWeightClass = weight
    pinned["OS/2"].fsSelection = (pinned["OS/2"].fsSelection & ~0b1100001) | (1 if italic else 0) << 0 | (0x20 if weight == 700 else 0x40 if not italic and weight == 400 else 0)
    pinned["head"].macStyle = (1 if weight == 700 else 0) | (2 if italic else 0)
    for table in ("STAT", "MVAR", "HVAR", "avar", "fvar", "gvar"):
        if table in pinned:
            del pinned[table]
    pinned.save(f"{sys.argv[2]}/GoogleSansFlex-{style}.ttf")
    print(style, "ok")
