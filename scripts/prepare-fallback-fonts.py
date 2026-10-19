#!/usr/bin/env python3
"""Reproduce static CJK/Hangul assets from hash-pinned, locally supplied Noto sources.

Requires fontTools 4.61.1. No download or system font discovery is performed.
Source hashes, useful Unicode coverage and original license files are recorded in
lucent/crates/lucent-ui/assets/fallback-fonts.json. Existing output files are
replaced only after source-hash validation and successful preparation of both.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sc-source', type=Path, required=True)
    parser.add_argument('--kr-source', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parents[1] / 'lucent/crates/lucent-ui/assets')
    args = parser.parse_args()
    from fontTools import __version__
    from fontTools.ttLib import TTFont
    from fontTools.varLib.instancer import instantiateVariableFont
    from fontTools import subset
    if __version__ != '4.61.1':
        parser.error('fontTools 4.61.1 is required for reproducible output')
    manifest_path = args.output / 'fallback-fonts.json'
    manifest = json.loads(manifest_path.read_bytes())
    sources = {'NotoSansCJK.ttf': args.sc_source, 'NotoSansHangul.ttf': args.kr_source}
    for name, source in sources.items():
        if hashlib.sha256(source.read_bytes()).hexdigest() != manifest['fonts'][name]['source_sha256']:
            parser.error('supplied Noto source hash differs from the asset manifest')
    with tempfile.TemporaryDirectory(prefix='lucent-fonts-', dir=args.output) as scratch:
        outputs = []
        for name, source in sources.items():
            descriptor = manifest['fonts'][name]
            font = TTFont(source, recalcTimestamp=False)
            instantiateVariableFont(font, {'wght': manifest['weight']}, inplace=True, optimize=True)
            options = subset.Options()
            options.layout_features = ['*']
            options.name_IDs = ['*']
            options.name_legacy = True
            options.name_languages = ['*']
            options.recalc_timestamp = False
            codepoints = []
            for span in descriptor['unicode_ranges']:
                low, high = (int(value, 16) for value in span.removeprefix('U+').split('-'))
                codepoints.extend(range(low, high + 1))
            worker = subset.Subsetter(options=options)
            worker.populate(unicodes=codepoints)
            worker.subset(font)
            family = descriptor['renamed_family']
            for entry in font['name'].names:
                value = family if entry.nameID in (1, 16) else ('Regular' if entry.nameID in (2, 17) else (family.replace(' ', '') + '-Regular' if entry.nameID in (3, 4, 6) else None))
                if value is not None:
                    entry.string = value.encode('utf-16-be' if entry.isUnicode() else 'ascii')
            output = Path(scratch) / name
            font.save(output, reorderTables=True)
            if hashlib.sha256(output.read_bytes()).hexdigest() != descriptor['asset_sha256']:
                parser.error('prepared asset differs from the recorded reproducible hash')
            outputs.append((output, args.output / name))
        for source, destination in outputs:
            source.replace(destination)
    print('Both static fallback assets match their recorded hashes.')


if __name__ == '__main__':
    main()
