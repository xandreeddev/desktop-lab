"""Fixture process ownership checks; never load modules or touch a real radio."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location(
    'bluetooth_fixture', Path(__file__).resolve().parents[1] / 'vm/bluetooth_fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class BluetoothFixtureOwnershipTests(unittest.TestCase):
    def test_existing_controller_is_rejected_before_any_mutation(self):
        with patch.object(fixture, 'require_guest'), \
             patch.object(fixture, 'controllers', return_value=['hci0']), \
             patch.object(fixture.os, 'chown') as chown, \
             patch.object(fixture.subprocess, 'Popen') as popen:
            with self.assertRaisesRegex(RuntimeError, 'not ours'):
                fixture.start(Path('/tmp/lucent-bluetooth-vhci-unit'), '0' * 64)
            chown.assert_not_called()
            popen.assert_not_called()

    def test_process_stat_handles_spaces_parentheses_and_disappearance(self):
        proc = Mock()
        proc.read_text.return_value = '123 (owned emulator (worker)) ' + ' '.join(['S'] + ['0'] * 18 + ['456'])
        with patch.object(fixture, 'Path', return_value=proc):
            self.assertEqual(fixture.process_stat(123), ('S', '456'))
            proc.read_text.side_effect = FileNotFoundError
            self.assertIsNone(fixture.process_stat(123))

    def test_recycled_missing_or_zombie_process_is_never_signalled(self):
        child = {'pid': 123, 'start': 'owned'}
        with patch.object(fixture.os, 'killpg') as kill:
            for observed in [('S', 'other'), ('Z', 'owned'), None]:
                with patch.object(fixture, 'process_stat', return_value=observed):
                    fixture.terminate(child)
            with self.assertRaisesRegex(RuntimeError, 'birth identity'):
                fixture.terminate({'pid': 123, 'start': None})
            kill.assert_not_called()

    def test_exit_between_identity_check_and_signal_is_safe(self):
        with patch.object(fixture, 'process_stat', return_value=('S', 'owned')), \
             patch.object(fixture.os, 'killpg', side_effect=ProcessLookupError):
            fixture.terminate({'pid': 123, 'start': 'owned'})

    def test_exit_after_signal_does_not_read_removed_proc_twice(self):
        with patch.object(fixture, 'process_stat', side_effect=[('S', 'owned'), None]), \
             patch.object(fixture.os, 'killpg') as kill:
            fixture.terminate({'pid': 123, 'start': 'owned'})
            kill.assert_called_once_with(123, fixture.signal.SIGTERM)
