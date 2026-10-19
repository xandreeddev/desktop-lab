"""Native-input ownership boundaries only; no guest, D-Bus or input actions."""
import copy
import importlib.util
import os
from pathlib import Path
import stat
import struct
import sys
import types
import unittest
from unittest.mock import MagicMock, Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import application_intent_policy as policy
import owned_application_units as units
spec = importlib.util.spec_from_file_location('intent_fixture', ROOT / 'vm/fixtures/application-intent-fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


def geometry():
    hit = {'id': 'intent-open', 'x': 16, 'y': 20, 'width': 200, 'height': 40,
           'clip': {'x': 0, 'y': 0, 'width': 320, 'height': 240}}
    surface = {'id': 'intent', 'instance': 1, 'output': 'Virtual-1', 'width': 320, 'height': 240,
               'frames': 1, 'native_frames': 1, 'pointer_input': 'all', 'hits': [hit]}
    layer = {'namespace': 'application-intent-intent', 'pid': 202, 'address': '0x123',
             'x': 800, 'y': 200, 'w': 320, 'h': 240}
    return surface, layer


class InputPolicy(unittest.TestCase):
    def test_compositor_destination_keeps_exact_logical_pixels_and_rejects_code(self):
        self.assertEqual(policy.compositor_move((960, 138)), 'hl.dsp.cursor.move({x=960,y=138})')
        self.assertEqual(policy.compositor_move((1919, 1079)), 'hl.dsp.cursor.move({x=1919,y=1079})')
        for point in ((1920, 0), (0, 1080), (-1, 0), (True, 2), (1.0, 2),
                      ('1}); os.execute("foreign")', 2), (1,), (1, 2, 3)):
            with self.subTest(point=point), self.assertRaises(ValueError):
                policy.compositor_move(point)

    def test_all_layers_need_exact_desktop_or_example_pid_and_namespace(self):
        _, layer = geometry()
        baseline = {'Virtual-1': {'layers': [[{'namespace': 'lucent-bar', 'pid': 101}, layer]]}}
        self.assertEqual(policy.owned_layers(baseline, 101, 202), layer)
        for replacement in ({'namespace': 'application-intent-intent', 'pid': 999},
                            {'namespace': 'lucent-bar', 'pid': 999},
                            {'namespace': 'other-overlay', 'pid': 101},
                            {'namespace': 'lucent-authorization-1', 'pid': 101}):
            value = copy.deepcopy(baseline)
            value['Virtual-1']['layers'][0].append(replacement)
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                policy.owned_layers(value, 101, 202)
        with self.assertRaises(ValueError):
            policy.owned_layers({'layers': [layer, layer]}, 101, 202)
        with self.assertRaises(ValueError):
            policy.owned_layers({'layers': [layer]}, 101)

    def test_recreated_layer_changed_hit_or_source_instance_retires_press(self):
        surface, layer = geometry()
        before = policy.target(surface, layer, 'intent-open')
        policy.same_target(before, policy.target(copy.deepcopy(surface), dict(layer), 'intent-open'))
        changes = ((surface, dict(layer, address='0x456')),
                   (dict(surface, instance=2), layer))
        changed = copy.deepcopy(surface)
        changed['hits'][0]['x'] += 8
        for source, native in (*changes, (changed, layer)):
            with self.subTest(source=source, native=native), self.assertRaises(ValueError):
                policy.same_target(before, policy.target(source, native, 'intent-open'))

    def test_unsubmitted_clipped_or_nonfinite_geometry_is_not_a_target(self):
        surface, layer = geometry()
        for source in (dict(surface, native_frames=0), dict(surface, pointer_input='none'),
                       dict(surface, width=float('nan'))):
            with self.assertRaises(ValueError):
                policy.target(source, layer, 'intent-open')
        surface['hits'][0]['clip']['height'] = 30
        with self.assertRaises(ValueError):
            policy.target(surface, layer, 'intent-open')

    def test_fresh_binding_policy_rejects_catchall_and_lock_variants(self):
        policy.unbound_pointer([{'modmask': 64, 'key': 'mouse:272'}])
        for binding in ({'modmask': 0, 'catch_all': True}, {'modmask': 2, 'key': 'mouse:left'},
                        {'modmask': 16, 'keycode': 280}, {'modmask': 0, 'key': 'Any'}, {'key': ''}):
            with self.subTest(binding=binding), self.assertRaises(ValueError):
                policy.unbound_pointer([binding])

    def test_exact_socket_peer_pid_and_user_required(self):
        policy.socket_peer((202, 1000, 1000), 202, 1000)
        for peer in ((999, 1000, 1000), (202, 1001, 1001), (202, True, 1000)):
            with self.subTest(peer=peer), self.assertRaises(ValueError):
                policy.socket_peer(peer, 202, 1000)

    def test_cursor_loss_is_sticky_and_does_not_adopt_a_nearby_user_move(self):
        cursor = policy.CursorOwnership({'x': 20, 'y': 30})
        cursor.begin({'x': 20, 'y': 30}, (100, 200))
        cursor.finish({'x': 100, 'y': 200})
        with self.assertRaises(ValueError):
            cursor.begin({'x': 101, 'y': 200}, (200, 300))
        with self.assertRaises(ValueError):
            cursor.begin({'x': 100, 'y': 200}, cursor.initial)

    def test_pointer_endpoints_and_pending_warp_must_match_exactly(self):
        self.assertEqual([event['data']['value'] for event in policy.absolute_move((1919, 1079))], [32767, 32767])
        for point in ((1920, 1080), (-1, 0), (1.0, 2)):
            with self.assertRaises(ValueError):
                policy.absolute_move(point)
        cursor = policy.CursorOwnership({'x': 20, 'y': 30})
        cursor.begin({'x': 20, 'y': 30}, (100, 200))
        with self.assertRaises(ValueError):
            cursor.finish({'x': 100, 'y': 201})


class NativeBoundaryOrdering(unittest.TestCase):
    def test_foreign_socket_peer_gets_no_command_even_with_matching_owned_path(self):
        expected = {'unit': {'main_pid': 202}, 'process': {'pid': 202},
                    'socket': [1, 2, stat.S_IFSOCK | 0o600, os.getuid()]}
        info = types.SimpleNamespace(st_dev=1, st_ino=2, st_mode=stat.S_IFSOCK | 0o600, st_uid=os.getuid())
        path = MagicMock()
        path.__truediv__.return_value = path
        path.lstat.return_value = info
        connection = MagicMock()
        connection.__enter__.return_value = connection
        connection.getsockopt.return_value = struct.pack('3i', 999, os.getuid(), os.getgid())
        with patch.object(fixture, 'context'), patch.object(fixture, 'unit_snapshot', return_value=expected['unit']), \
             patch.object(fixture, 'process_snapshot', return_value=expected['process']), \
             patch.object(fixture, 'Path', return_value=path), \
             patch.dict(os.environ, {'XDG_RUNTIME_DIR': '/owned-runtime'}), \
             patch.object(fixture.socket, 'socket', return_value=connection):
            with self.assertRaisesRegex(ValueError, 'socket_peer'):
                fixture.example_command('/owned', 'abcdef', 'open', expected)
        connection.sendall.assert_not_called()

    def test_replaced_socket_path_gets_no_command_despite_the_same_peer_pid(self):
        expected = {'unit': {'main_pid': 202}, 'process': {'pid': 202},
                    'socket': [1, 2, stat.S_IFSOCK | 0o600, os.getuid()]}
        original = types.SimpleNamespace(st_dev=1, st_ino=2, st_mode=stat.S_IFSOCK | 0o600, st_uid=os.getuid())
        changed = types.SimpleNamespace(st_dev=1, st_ino=3, st_mode=stat.S_IFSOCK | 0o600, st_uid=os.getuid())
        path = MagicMock()
        path.__truediv__.return_value = path
        path.lstat.side_effect = [original, changed]
        connection = MagicMock()
        connection.__enter__.return_value = connection
        connection.getsockopt.return_value = struct.pack('3i', 202, os.getuid(), os.getgid())
        with patch.object(fixture, 'context'), patch.object(fixture, 'unit_snapshot', return_value=expected['unit']), \
             patch.object(fixture, 'process_snapshot', return_value=expected['process']), \
             patch.object(fixture, 'Path', return_value=path), \
             patch.dict(os.environ, {'XDG_RUNTIME_DIR': '/owned-runtime'}), \
             patch.object(fixture.socket, 'socket', return_value=connection):
            with self.assertRaises(ValueError):
                fixture.example_command('/owned', 'abcdef', 'open', expected)
        connection.sendall.assert_not_called()

    def test_reused_unit_invocation_or_process_birth_gets_no_stop_request(self):
        unit = 'lucent-application-intent-abcdef.service'
        value = {'load': 'loaded', 'transient': True, 'active': 'active', 'main_pid': 202,
                 'group': '/app.slice/' + unit, 'executable': '/owned/application-intent',
                 'argv': ['/owned/application-intent'], 'invocation': 'original'}
        captured = {'unit': value, 'process': {'pid': 202, 'start': '42'}}
        for changed_unit, changed_process in ((dict(value, invocation='new'), captured['process']),
                                               (value, {'pid': 202, 'start': '43'})):
            fake_dbus = types.SimpleNamespace(SessionBus=Mock())
            with patch.dict(sys.modules, {'dbus': fake_dbus}), \
                 patch.object(units, 'unit_snapshot', return_value=changed_unit), \
                 patch.object(units, 'process_snapshot', return_value=changed_process):
                with self.assertRaises(AssertionError):
                    units.retire_unit(unit, value['argv'], captured=captured)
            fake_dbus.SessionBus.assert_not_called()

    def test_owned_current_process_fingerprint_reads_real_proc_without_environment(self):
        value = units.process_snapshot(os.getpid(), hashed=True)
        self.assertEqual(value['pid'], os.getpid())
        self.assertEqual(len(value['sha256']), 64)
        self.assertEqual(value, units.process_snapshot(os.getpid(), hashed=True))
        self.assertNotIn('environment', value)


if __name__ == '__main__':
    unittest.main()
