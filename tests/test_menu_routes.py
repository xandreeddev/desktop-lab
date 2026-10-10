"""Pure menu definition compatibility; tests never dispatch real system actions."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import tempfile
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'scripts'))
spec = importlib.util.spec_from_file_location('routes', Path(__file__).resolve().parents[1]/'scripts/lucent-menu-routes.py')
routes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(routes)


class MenuRoutes(unittest.TestCase):
    def model(self):
        return routes.Model({
            'system': {'label': 'System', 'aliases': ['power-menu']},
            'system.lock': {'label': 'Lock', 'action': 'lock-fixture'},
            'system.sleep': {'label': 'Suspend', 'when': 'hidden', 'action': 'sleep-fixture'},
            'system.installed': {'label': 'Installed', 'disabled': 'yes', 'action': 'must-not-run'},
            'shortcut': {'target': 'system'},
        }, {'system.lock': {'label': 'Custom lock'}})

    def test_jsonc_preserves_urls_quoted_commas_and_comments(self):
        source = '''{ // heading
          "action": {"label":"A /* literal */ // name", "action":"open https://example.test/a,b",}, /* end */
        }'''
        self.assertEqual(routes.jsonc(source)['action']['label'], 'A /* literal */ // name')
        self.assertEqual(routes.jsonc(source)['action']['action'], 'open https://example.test/a,b')

    def test_extensions_links_aliases_and_conditions(self):
        model = self.model()
        self.assertEqual(model.resolve('power_menu'), 'system')
        self.assertEqual(model.resolve('shortcut'), 'system')
        self.assertEqual(model.items['system.lock']['action'], 'lock-fixture')
        rows = model.rows('system', lambda command, default: {'hidden': False, 'yes': True}.get(command, default))
        self.assertEqual([r['label'] for r in rows], ['Custom lock', 'Installed'])
        self.assertTrue(rows[1]['disabled'])
        with self.assertRaises(ValueError): model.resolve('missing')
        with self.assertRaises(ValueError): routes.Model({'a': {'target': 'b'}, 'b': {'target': 'a'}}, {}).resolve('a')

    def test_back_navigation_then_exact_configured_action(self):
        seen = []
        responses = iter([(0, 'system\n'), (3, ''), (0, 'system\n'), (0, 'system.lock\n')])
        def pick(payload, capture=False):
            seen.append((payload['prompt'], payload['back']))
            code, value = next(responses)
            return SimpleNamespace(returncode=code, stdout=value)
        with tempfile.TemporaryDirectory() as temp, patch.dict(routes.os.environ, {'XDG_RUNTIME_DIR': temp}), \
                patch.object(routes, 'load', side_effect=self.model), patch.object(routes, 'picker_command', return_value=None), \
                patch.object(routes, 'condition', side_effect=lambda c, d: {'hidden': False, 'yes': True}.get(c, d)), \
                patch.object(routes, 'dispatch') as dispatch:
            self.assertEqual(routes.run([], pick), 0)
            dispatch.assert_called_once_with('lock-fixture')
        self.assertEqual(seen, [('Menu', False), ('System', True), ('Menu', False), ('System', True)])

    def test_disabled_or_unoffered_value_cannot_execute(self):
        for value in ['system.installed', 'system.sleep', 'injected;command']:
            with self.subTest(value=value), tempfile.TemporaryDirectory() as temp, \
                    patch.dict(routes.os.environ, {'XDG_RUNTIME_DIR': temp}), \
                    patch.object(routes, 'load', side_effect=self.model), patch.object(routes, 'picker_command', return_value=None), \
                    patch.object(routes, 'condition', side_effect=lambda c, d: {'hidden': False, 'yes': True}.get(c, d)), \
                    patch.object(routes, 'dispatch') as dispatch:
                with self.assertRaises(ValueError):
                    routes.run(['summon', 'system'], lambda *a, **k: SimpleNamespace(returncode=0, stdout=value+'\n'))
                dispatch.assert_not_called()

    def test_provider_values_remain_literal_argv(self):
        with patch.object(routes, 'output', side_effect=['Safe Font\n$(not-a-command)', 'Safe Font']):
            rows, actions = routes.provider_rows('fonts')
        self.assertEqual(rows[0]['detail'], 'Current')
        self.assertEqual(actions['@provider:1'], ['omarchy-font-set', '$(not-a-command)'])

    def test_native_routes_are_explicit_and_user_actions_take_precedence(self):
        defaults = routes.native_defaults({
            'style': {'label': 'Style'},
            'style.theme': {'action': 'any-upstream-theme-command'},
            'style.background': {'action': 'any-upstream-wallpaper-command'},
            'custom': {'action': 'omarchy-theme-bg-switcher'},
        })
        model = routes.Model(defaults, {'style.background': {'action': 'my-wallpaper-tool'}})
        self.assertIsNone(model.items['style.theme']['provider'])
        self.assertEqual(model.items['style.theme']['action'], ['lucent-cli','themes','open'])
        self.assertEqual(model.items['style.background']['action'], 'my-wallpaper-tool')
        self.assertEqual(model.items['custom']['action'], 'omarchy-theme-bg-switcher')
        self.assertEqual(defaults['style.background']['action'], ['lucent-cli', 'wallpapers', 'open'])

    def test_failed_availability_check_stops_navigation(self):
        with patch.object(routes.subprocess, 'run', side_effect=routes.subprocess.TimeoutExpired('fixture', 2)):
            with self.assertRaisesRegex(ValueError, 'timed out'):
                routes.condition('slow-fixture', False)
