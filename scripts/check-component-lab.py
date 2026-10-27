#!/usr/bin/env python3
"""Verify the native component catalog, its rendering inputs and exported PNGs."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import struct

ROOT = Path(__file__).resolve().parents[1]


def need(value, message):
    if not value:
        raise ValueError(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def local(root, relative):
    need(isinstance(relative, str), 'Catalog paths must be strings')
    path = PurePosixPath(relative)
    need(relative == path.as_posix() and not path.is_absolute()
         and '..' not in path.parts and path.parts, f'Invalid catalog path: {relative}')
    target = root / relative
    need(target.resolve() == target and target.is_file(), f'Missing regular catalog file: {relative}')
    return target


def token_names(tree, prefix=''):
    result = set()
    for key, value in tree.items():
        if key.startswith('$'):
            continue
        name = prefix + key
        if '$value' in value:
            result.add(name)
        else:
            result.update(token_names(value, name + '.'))
    return result


def validate(root=ROOT, catalog=None):
    root = Path(root).resolve()
    output = Path(catalog).absolute() if catalog else root / 'site/public/component-lab'
    need(output.resolve() == output and output.is_dir(), 'Catalog must be a canonical existing directory')
    manifest_file = local(output, 'manifest.json')
    need(manifest_file.stat().st_size <= 4 * 1024 * 1024, 'Catalog manifest exceeds its bound')
    data = json.loads(manifest_file.read_text())
    need(data['schema_version'] == 1, 'Unsupported component catalog schema')
    renderer = data['renderer']
    need(renderer['engine'] == 'lucent-render' and renderer['scope'] == 'offscreen-native',
         'Catalog must identify its native offscreen renderer')
    need(re.fullmatch('[0-9a-f]{40}', renderer['source_commit']), 'Catalog needs a full source commit')
    tokens_file = local(root, 'design/tokens.json')
    need(digest(tokens_file) == renderer['token_sha256'], 'Stale component token captures; regenerate the catalog')
    inputs = renderer['inputs']
    need(isinstance(inputs, dict) and 8 <= len(inputs) <= 1024, 'Missing bounded rendering input fingerprints')
    required = {'design/tokens.json', 'configs/lucent/palettes.json', 'lucent/Cargo.lock',
                'lucent/crates/lucent-design/src/lib.rs', 'lucent/crates/lucent-design/src/tokens.rs',
                'lucent/crates/lucent-api/src/lib.rs', 'lucent/crates/lucent-ui/src/lib.rs',
                'lucent/crates/lucent-render/src/lib.rs', 'lucent/apps/lucent-menu/src/lib.rs',
                'lucent/apps/lucent-desktop/assets/LucentSans.ttf',
                'lucent/apps/lucent-desktop/assets/LucentDisplay.ttf'}
    producer_directories = ('lucent/apps/lucent-lab', 'lucent/apps/lucent-menu',
                            'lucent/crates/lucent-api', 'lucent/crates/lucent-design',
                            'lucent/crates/lucent-ui', 'lucent/crates/lucent-render',
                            'lucent/crates/lucent-domain', 'lucent/crates/lucent-usecases',
                            'lucent/crates/lucent-video-frame')
    required.add('lucent/Cargo.toml')
    required.update(directory + '/Cargo.toml' for directory in producer_directories)
    for directory in ('lucent/apps/lucent-lab/src', 'lucent/apps/lucent-menu/src',
                      'lucent/crates/lucent-api/src', 'lucent/crates/lucent-design/src',
                      'lucent/crates/lucent-ui/src', 'lucent/crates/lucent-render/src',
                      'lucent/crates/lucent-domain/src', 'lucent/crates/lucent-usecases/src',
                      'lucent/crates/lucent-video-frame/src'):
        required.update(p.relative_to(root).as_posix() for p in (root / directory).rglob('*')
                        if p.is_file() and p.suffix in ('.rs', '.wgsl'))
    required.update(p.relative_to(root).as_posix()
                    for p in (root / 'lucent/crates/lucent-ui/assets').rglob('*')
                    if p.is_file() and p.suffix in ('.ttf', '.otf', '.json'))
    need(required <= inputs.keys() and any(p.startswith('lucent/apps/lucent-lab/src/') for p in inputs),
         'Catalog omits a production rendering input')
    for path, expected in inputs.items():
        need(re.fullmatch('[0-9a-f]{64}', expected), f'Invalid input hash: {path}')
        need(digest(local(root, path)) == expected, f'Stale native capture input: {path}')
    palettes = json.loads(local(root, 'configs/lucent/palettes.json').read_text())
    expected_palettes = {p['id']: p['name'] for p in palettes}
    need(len(data['palettes']) == len(expected_palettes)
         and {p['id']: p['name'] for p in data['palettes']} == expected_palettes,
         'Catalog palettes must match the production palette catalog')
    tokens = token_names(json.loads(tokens_file.read_text()))
    stories, images, count = set(), set(), 0
    need(1 <= len(data['stories']) <= 100, 'Invalid story count')
    for story in data['stories']:
        name = story['id']
        need(re.fullmatch('[a-z][a-z0-9-]*', name) and name not in stories, 'Duplicate or invalid story identity')
        stories.add(name)
        need(all(isinstance(story[k], str) and story[k].strip()
                 for k in ('family', 'title', 'description', 'recipe')), f'Incomplete story: {name}')
        local(root, story['source']['path'])
        need(story['source']['symbol'], f'Missing source symbol: {name}')
        need(story['tokens'] and set(story['tokens']) <= tokens, f'Unknown token reference: {name}')
        combinations, states, ids = set(), set(), set()
        for variant in story['variants']:
            need(variant['id'] not in ids, f'Duplicate variant identity: {name}')
            ids.add(variant['id'])
            palette, state = variant['palette'], variant['state']
            need(palette in expected_palettes and re.fullmatch('[a-z][a-z0-9-]*', state), f'Invalid preset: {name}')
            states.add(state)
            density, viewport, image = variant['density'], variant['viewport'], variant['image']
            need(type(density['numerator']) is int and type(density['denominator']) is int
                 and density['denominator'] == 1 and density['numerator'] in (1, 2), f'Invalid density: {name}')
            need(viewport['id'] in ('regular', 'narrow')
                 and all(type(viewport[k]) is int and 1 <= viewport[k] <= 2048 for k in ('width', 'height')),
                 f'Invalid logical viewport: {name}')
            key = (palette, state, density['numerator'], viewport['id'])
            need(key not in combinations, f'Duplicate capture combination: {name}')
            combinations.add(key)
            need(isinstance(variant['time_ms'], (int, float)) and 0 <= variant['time_ms'] <= 60_000,
                 f'Invalid sample time: {name}')
            need(image['path'].startswith('images/') and image['path'].endswith('.png')
                 and image['path'] not in images, f'Duplicate or invalid image: {name}')
            images.add(image['path'])
            file = local(output, image['path'])
            need(file.stat().st_size <= 16 * 1024 * 1024, f'Oversized image: {name}')
            with file.open('rb') as stream:
                header = stream.read(24)
            need(len(header) == 24 and header[:8] == b'\x89PNG\r\n\x1a\n'
                 and header[12:16] == b'IHDR', f'Invalid PNG: {name}')
            width, height = struct.unpack('>II', header[16:24])
            scale = density['numerator']
            need(width == image['width'] == viewport['width'] * scale
                 and height == image['height'] == viewport['height'] * scale, f'Image density mismatch: {name}')
            need(digest(file) == image['sha256'], f'Changed capture pixels: {name}')
            count += 1
            need(count <= 1000, 'Capture count exceeds its bound')
        expected = {(p, s, d, v) for p in expected_palettes for s in states
                    for d in (1, 2) for v in ('regular', 'narrow')}
        need(combinations and combinations == expected, f'Incomplete state/palette/density/viewport matrix: {name}')
    actual_images = {p.relative_to(output).as_posix() for p in (output / 'images').rglob('*') if p.is_file()}
    need(actual_images == images, 'Unlisted or missing catalog images')
    return {'stories': len(stories), 'captures': count, 'palettes': len(expected_palettes),
            'source_commit': renderer['source_commit'], 'rendering_inputs': len(inputs)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--catalog', type=Path, help='Check a fresh export before promoting it to the website')
    args = parser.parse_args()
    try:
        print(json.dumps(validate(args.root, args.catalog)))
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(1, f'Component Lab validation failed: {error}\n')


if __name__ == '__main__':
    main()
