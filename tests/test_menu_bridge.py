"""Contract with Omarchy's unmodified menu callers and action dispatcher."""
import importlib.util
import io
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('menu_bridge', Path(__file__).resolve().parents[1]/'scripts/lucent-menu.py')
menu = importlib.util.module_from_spec(spec)
spec.loader.exec_module(menu)


class MenuBridge(unittest.TestCase):
    def test_keybinding_display_keeps_original_dispatch_value(self):
        line = 'SUPER CTRL + RETURN                 → Custom terminal'
        self.assertEqual(menu.entry(line, 'Keybindings'), {
            'label': 'Custom terminal', 'detail': 'SUPER CTRL + RETURN', 'value': line})

    def test_icon_and_subtext_contract_preserves_duplicate_values(self):
        payload = menu.request('select', ['Choose'], io.StringIO('glyph\tSame\tfirst\nglyph\tSame\tsecond\nplain\n'))
        self.assertEqual([e['value'] for e in payload['entries']], ['Same\tfirst', 'Same\tsecond', 'plain'])
        self.assertEqual(menu.entry('glyph\tNo subtext', 'Choose')['value'], 'No subtext')

    def test_stdin_and_size_flags(self):
        payload = menu.request('select', ['Keybindings', '--', '--width', '800', '--height', '500'], io.StringIO('a\nb\n'))
        self.assertEqual((payload['width'], payload['max_height']), (800, 500))
        self.assertEqual(len(payload['entries']), 2)
        with self.assertRaises(ValueError):
            menu.request('select', ['Choose', 'a', '--', '--width'], io.StringIO())
        with self.assertRaises(ValueError):
            menu.request('select', ['Choose'], io.StringIO())

    def test_input_and_literal_values_do_not_execute_commands(self):
        self.assertEqual(menu.request('input', ['Reminder', '--width', '400'], io.StringIO())['mode'], 'input')
        value = '$(touch forbidden); quoted " text'
        self.assertEqual(menu.request('select', ['Choose', value], io.StringIO())['entries'][0]['value'], value)
