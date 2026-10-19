"""Compatibility translation only; never connect to a desktop or run system commands."""
import importlib.util
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location(
    'panel_compat', Path(__file__).resolve().parents[1] / 'scripts/lucent-compat.py')
compat = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compat)


class GlobalPanelDismissal(unittest.TestCase):
    def test_global_hide_delegates_the_panel_set_to_the_client_and_closes_the_selector(self):
        selector = SimpleNamespace(picker_command=Mock())
        for args in (['shell', 'hide'], ['-q', 'shell', 'hide']):
            with self.subTest(args=args), patch.object(compat, 'cli', return_value='ok') as cli, \
                    patch.dict(sys.modules, {'menu_routes': selector}), \
                    patch.object(compat.subprocess, 'Popen') as launch:
                selector.picker_command.reset_mock()
                self.assertEqual(compat.shell(args), 'ok')
                cli.assert_called_once_with('shell', 'hide')
                selector.picker_command.assert_called_once_with('close')
                launch.assert_not_called()

    def test_a_missing_desktop_does_not_leave_the_separate_selector_open_or_hide_the_error(self):
        selector = SimpleNamespace(picker_command=Mock())
        error = subprocess.CalledProcessError(1, ['fixture-cli'])
        with patch.object(compat, 'cli', side_effect=error), \
                patch.dict(sys.modules, {'menu_routes': selector}):
            with self.assertRaises(subprocess.CalledProcessError) as result:
                compat.shell(['shell', 'hide'])
        self.assertIs(result.exception, error)
        selector.picker_command.assert_called_once_with('close')
