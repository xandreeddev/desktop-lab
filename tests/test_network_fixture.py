"""Ownership checks for the root-only guest fixture; no real network changes."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('network_fixture', Path(__file__).resolve().parents[1] / 'vm/network_fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class NetworkFixtureOwnershipTests(unittest.TestCase):
    def test_existing_wifi_is_rejected_before_any_mutation(self):
        with patch.object(fixture.os, 'geteuid', return_value=0), \
             patch.object(fixture.Path, 'exists', return_value=True), \
             patch.object(fixture, 'run', return_value='qemu') as run, \
             patch.object(fixture, 'interfaces', return_value=['existing-radio']), \
             patch.object(fixture.os, 'chown') as chown:
            with self.assertRaisesRegex(SystemExit, 'existing Wi-Fi'):
                fixture.start(Path('/tmp/lucent-network-unused'))
            run.assert_called_once_with('systemd-detect-virt', '--vm')
            chown.assert_not_called()

    def test_recycled_pid_does_not_kill_an_unrelated_process(self):
        proc = Mock()
        proc.exists.return_value = True
        proc.read_text.return_value = ' '.join(['0'] * 21 + ['new-start'])
        with patch.object(fixture, 'Path', return_value=proc), patch.object(fixture.os, 'killpg') as kill:
            fixture.terminate({'pid': 12345, 'start': 'old-start'})
            kill.assert_not_called()
            fixture.terminate({'pid': 12345, 'start': 'new-start'})
            kill.assert_called_once_with(12345, fixture.signal.SIGTERM)

    def test_cleanup_removes_failed_owned_profiles_preserves_others_and_restores_radio(self):
        with tempfile.TemporaryDirectory(prefix='lucent-network-unit-') as temporary:
            directory = Path(temporary)
            registered = '00000000-0000-0000-0000-000000000001'
            original = '00000000-0000-0000-0000-000000000000'
            unregistered = '00000000-0000-0000-0000-000000000002'
            unrelated = '00000000-0000-0000-0000-000000000003'
            (directory / 'state.json').write_text(json.dumps({'profile_keys': [registered], 'original_profiles': [original], 'ssid': 'owned-fixture', 'children': [], 'module': False, 'wifi_enabled': False}))
            (directory / 'hostapd.conf').write_text('private fixture credential')
            (directory / 'hostapd.log').write_text('retained diagnosis')
            exists = Path.exists
            def response(*args):
                if args == ('nmcli', '-g', 'UUID', 'connection', 'show'):
                    return '\n'.join([original, registered, unregistered, unrelated])
                if args[:3] == ('nmcli', '-g', '802-11-wireless.ssid'):
                    return 'owned-fixture' if args[-1] == unregistered else 'unrelated-network'
                return ''
            with patch.object(fixture, 'run', side_effect=response) as run, \
                 patch.object(fixture.subprocess, 'run') as process, \
                 patch.object(fixture, 'interfaces', return_value=[]), \
                 patch.object(fixture.time, 'sleep'), \
                 patch.object(Path, 'exists', lambda p: False if str(p) == '/sys/module/mac80211_hwsim' else exists(p)):
                fixture.stop(directory)
                self.assertEqual(run.call_args.args, ('nmcli', 'radio', 'wifi', 'off'))
                self.assertEqual({call.args[0][-1] for call in process.call_args_list}, {registered, unregistered})
                self.assertEqual(process.call_count, 2)
            self.assertFalse((directory / 'state.json').exists())
            self.assertFalse((directory / 'hostapd.conf').exists())
            self.assertEqual((directory / 'hostapd.log').read_text(), 'retained diagnosis')


if __name__ == '__main__':
    unittest.main()
