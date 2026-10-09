"""Token compilation must reject broken themes rather than shipping divergent clients."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('tokens', ROOT / 'scripts/generate-design-tokens.py')
tokens = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tokens)


class DesignTokens(unittest.TestCase):
    def test_references_are_typed_and_cycles_fail(self):
        for source in [
            {'a': {'$type': 'color', '$value': '{missing}'}},
            {'a': {'$type': 'color', '$value': '{b}'}, 'b': {'$type': 'color', '$value': '{a}'}},
            {'a': {'$type': 'color', '$value': '{b}'}, 'b': {'$type': 'dimension', '$value': 12}},
        ]:
            with self.subTest(source=source), self.assertRaises(ValueError):
                tokens.resolve(source)

    def test_generated_consumers_match_source(self):
        tree = json.loads(tokens.SOURCE.read_text())
        for path, content in tokens.outputs(tree).items():
            self.assertEqual(path.read_text(), content, f'Stale tokens: {path.name}')

    def test_native_views_have_no_literal_visual_styles(self):
        for name in ['views.rs', 'widgets.rs']:
            source = (ROOT / 'lucent/apps/lucent-desktop/src' / name).read_text()
            self.assertNotRegex(source, r'Color::hex|#[0-9a-fA-F]{6}\b')
            self.assertNotRegex(source, r'\.(?:font|padding(?:_xy)?|gap|radius)\(\d')
            self.assertNotRegex(source, r'\.size\(\d+[.]')


if __name__ == '__main__':
    unittest.main()
