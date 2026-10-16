#!/usr/bin/env python3
"""Install the bundled live-wallpaper samples without changing desktop preferences."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile


ASSETS = Path(__file__).resolve().parents[1] / 'assets/live-wallpapers'


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def install(destination):
    manifest = json.loads((ASSETS / 'manifest.json').read_text())
    samples = []
    # Validate every source and existing destination before installing any file.
    for sample in manifest['samples']:
        name = sample['filename']
        if Path(name).name != name or not name.endswith('.mp4'):
            raise ValueError('Invalid sample filename in manifest')
        source, target = ASSETS / name, destination / name
        if source.is_symlink() or not source.is_file() or digest(source) != sample['sha256']:
            raise ValueError(f'Bundled sample failed its checksum: {name}')
        if target.is_symlink() or (target.exists() and (
                not target.is_file() or digest(target) != sample['sha256'])):
            raise ValueError(f'Refusing to replace an existing file: {name}')
        samples.append((source, target))

    destination.mkdir(parents=True, exist_ok=True)
    for source, target in samples:
        if target.exists():
            print(f'Already installed: {target.name}')
            continue
        with tempfile.NamedTemporaryFile(dir=destination, prefix='.lucent-sample-',
                                         delete=False) as staged:
            staging = Path(staged.name)
            try:
                with source.open('rb') as original:
                    shutil.copyfileobj(original, staged, length=65536)
                staged.flush()
                os.fsync(staged.fileno())
                staging.chmod(0o644)
                # Linking publishes atomically and refuses a conflicting racing writer.
                os.link(staging, target, follow_symlinks=False)
            finally:
                staging.unlink(missing_ok=True)
        print(f'Installed: {target.name}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destination', type=Path,
                        default=Path.home() / 'Videos/Wallpapers',
                        help='sample directory (default: ~/Videos/Wallpapers)')
    args = parser.parse_args()
    try:
        install(args.destination.expanduser())
    except (OSError, ValueError, KeyError) as error:
        print(f'Could not install wallpaper samples: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
