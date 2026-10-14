#!/usr/bin/env python3
"""Generate renamed static rich-text faces from the pinned OFL variable font.

Requires fontTools 4.66.1. Pass the source file; this script performs no network IO.
Source provenance and SHA-256 live in the desktop assets README.
"""
import argparse
import hashlib
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", type=Path, default=Path("lucent/apps/lucent-desktop/assets"))
    args = parser.parse_args()
    source = args.source.read_bytes()
    digest=hashlib.sha256(source).hexdigest()
    if digest!="a9ac770798857d5e8ac593ced42b22e40a7c08cec98319f92b0eccc2783a77ba":
        parser.error("Source differs from the pinned Google Sans Flex font")
    print("source sha256:",digest)
    args.output.mkdir(parents=True, exist_ok=True)
    for name, weight, slant in [
        ("LucentBold", 700, 0),
        ("LucentItalic", 500, -10),
        ("LucentBoldItalic", 700, -10),
    ]:
        font = instantiateVariableFont(
            TTFont(args.source),
            {"opsz": 18, "wdth": 100, "wght": weight, "ROND": 0, "slnt": slant},
            updateFontNames=False,
        )
        for record in font["name"].names:
            if record.nameID in (1, 2, 3, 4, 6, 16, 17):
                value = "Regular" if record.nameID in (2, 17) else name
                if record.nameID == 3:
                    value = f"1.0;Lucent;{name}"
                record.string = value.encode(record.getEncoding())
        # Deterministic derivatives retain the pinned source timestamps.
        font.recalcTimestamp = False
        font.save(args.output / f"{name}.ttf")


if __name__ == "__main__":
    main()
