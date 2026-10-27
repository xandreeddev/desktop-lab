"""Native gallery integrity: stale producer code and mismatched pixels cannot ship."""
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('component_lab_check', ROOT / 'scripts/check-component-lab.py')
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def png(width, height):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress((b'\0' + b'\x40\x80\xa0\xff' * width) * height))
            + chunk(b'IEND', b''))


class CatalogIntegrity(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        paths = ('design/tokens.json', 'configs/lucent/palettes.json', 'lucent/Cargo.lock', 'lucent/Cargo.toml',
                 'lucent/crates/lucent-design/src/lib.rs', 'lucent/crates/lucent-design/src/tokens.rs',
                 'lucent/crates/lucent-api/src/lib.rs', 'lucent/crates/lucent-ui/src/lib.rs',
                 'lucent/crates/lucent-render/src/lib.rs', 'lucent/apps/lucent-menu/src/lib.rs',
                 'lucent/apps/lucent-lab/src/lib.rs', 'lucent/crates/lucent-video-frame/src/lib.rs',
                 'lucent/apps/lucent-desktop/assets/LucentSans.ttf',
                 'lucent/apps/lucent-desktop/assets/LucentDisplay.ttf',
                 *(f'lucent/{directory}/Cargo.toml' for directory in (
                     'apps/lucent-lab', 'apps/lucent-menu', 'crates/lucent-api', 'crates/lucent-design',
                     'crates/lucent-ui', 'crates/lucent-render', 'crates/lucent-domain', 'crates/lucent-usecases',
                     'crates/lucent-video-frame')))
        for path in paths:
            self.put(path, b'fixture source\n')
        self.put('design/tokens.json', json.dumps({'radius': {'control': {'$value': 12}}}).encode())
        palettes = [{'id': 'fixture', 'name': 'Fixture'}]
        self.put('configs/lucent/palettes.json', json.dumps(palettes).encode())
        self.data = {'schema_version': 1, 'renderer': {
            'engine': 'lucent-render', 'scope': 'offscreen-native', 'source_commit': 'a' * 40,
            'token_sha256': CHECK.digest(self.root / 'design/tokens.json'),
            'inputs': {path: CHECK.digest(self.root / path) for path in paths}},
            'palettes': palettes, 'stories': [{'id': 'button', 'family': 'Controls', 'title': 'Button',
                'description': 'Native control', 'recipe': 'theme.button(label, message)',
                'source': {'path': 'lucent/crates/lucent-design/src/lib.rs', 'symbol': 'Theme::button'},
                'tokens': ['radius.control'], 'variants': []}]}
        for state in ('default', 'focus'):
            for viewport, width in (('regular', 4), ('narrow', 2)):
                for scale in (1, 2):
                    name = f'images/{state}-{viewport}-{scale}.png'
                    pixels = png(width * scale, 2 * scale)
                    self.put('site/public/component-lab/' + name, pixels)
                    self.data['stories'][0]['variants'].append({
                        'id': name, 'palette': 'fixture', 'state': state,
                        'density': {'numerator': scale, 'denominator': 1},
                        'viewport': {'id': viewport, 'width': width, 'height': 2},
                        'image': {'path': name, 'width': width * scale, 'height': 2 * scale,
                                  'sha256': hashlib.sha256(pixels).hexdigest()}, 'time_ms': 1000})
        self.save()

    def put(self, path, data):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)

    def save(self):
        self.put('site/public/component-lab/manifest.json', json.dumps(self.data).encode())

    def test_complete_matching_catalog_passes(self):
        result = CHECK.validate(self.root)
        self.assertEqual((result['stories'], result['captures']), (1, 8))

    def test_changed_production_recipe_requires_new_captures(self):
        self.put('lucent/crates/lucent-design/src/lib.rs', b'updated recipe')
        with self.assertRaisesRegex(ValueError, 'Stale native capture input'):
            CHECK.validate(self.root)

    def test_new_nested_producer_module_cannot_be_omitted(self):
        self.put('lucent/crates/lucent-ui/src/new_module.rs', b'new producer')
        with self.assertRaisesRegex(ValueError, 'omits a production'):
            CHECK.validate(self.root)

    def test_neutral_video_sources_and_manifest_cannot_be_omitted(self):
        for path in ('lucent/crates/lucent-video-frame/src/lib.rs',
                     'lucent/crates/lucent-video-frame/Cargo.toml'):
            with self.subTest(path=path):
                original = self.data['renderer']['inputs'].pop(path)
                self.save()
                with self.assertRaisesRegex(ValueError, 'omits a production'):
                    CHECK.validate(self.root)
                self.data['renderer']['inputs'][path] = original
        self.save()

    def test_changed_neutral_video_value_requires_new_captures(self):
        self.put('lucent/crates/lucent-video-frame/src/lib.rs', b'changed plane layout contract')
        with self.assertRaisesRegex(ValueError, 'Stale native capture input'):
            CHECK.validate(self.root)

    def test_pixel_hash_and_density_are_checked(self):
        image = self.data['stories'][0]['variants'][0]['image']
        image['width'] += 1
        self.save()
        with self.assertRaisesRegex(ValueError, 'density mismatch'):
            CHECK.validate(self.root)
        image['width'] -= 1
        self.save()
        self.put('site/public/component-lab/' + image['path'], png(4, 2) + b'changed')
        with self.assertRaisesRegex(ValueError, 'Changed capture pixels'):
            CHECK.validate(self.root)

    def test_missing_combination_does_not_silently_fall_back(self):
        self.data['stories'][0]['variants'].pop()
        self.save()
        with self.assertRaisesRegex(ValueError, 'Incomplete state'):
            CHECK.validate(self.root)

    def test_token_and_source_references_are_checked(self):
        story = self.data['stories'][0]
        story['tokens'] = ['radius.invented']
        self.save()
        with self.assertRaisesRegex(ValueError, 'Unknown token'):
            CHECK.validate(self.root)
        story['tokens'] = ['radius.control']
        story['source']['path'] = '../outside.rs'
        self.save()
        with self.assertRaisesRegex(ValueError, 'Invalid catalog path'):
            CHECK.validate(self.root)

    def test_duplicate_variants_and_unlisted_images_are_rejected(self):
        variants = self.data['stories'][0]['variants']
        variants.append(variants[0])
        self.save()
        with self.assertRaisesRegex(ValueError, 'Duplicate variant'):
            CHECK.validate(self.root)
        variants.pop()
        self.save()
        self.put('site/public/component-lab/images/unlisted.png', png(2, 2))
        with self.assertRaisesRegex(ValueError, 'Unlisted or missing'):
            CHECK.validate(self.root)


if __name__ == '__main__':
    unittest.main()
