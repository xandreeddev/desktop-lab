"""Bounded pure/owned-file checks; no guest, installed app, bus or UI access."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import third_party_tray_guest as guest
import third_party_tray_policy as policy
spec = importlib.util.spec_from_file_location('third_party_tray_driver', ROOT / 'vm/test-third-party-tray.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


def menu():
    return {'id': 0, 'properties': {}, 'children': [
        {'id': 1, 'properties': {'label': 'Enable _Networking', 'toggle-type': 'checkmark',
                                 'toggle-state': 1}, 'children': []},
        {'id': 4, 'properties': {'label': 'Connection _Information', 'enabled': True}, 'children': []},
        {'id': 7, 'properties': {'label': 'Edit Connections…'}, 'children': []}]}


def descriptor():
    return {'owner': ':1.14', 'pid': 100, 'item_path': '/StatusNotifierItem', 'menu_path': '/Menu',
            'revision': 4, 'layout_sha256': policy.digest(menu()), 'information_id': 4}


class MenuPolicy(unittest.TestCase):
    def test_only_exact_root_information_leaf_is_safe(self):
        self.assertEqual(policy.information_entry(menu()), 4)
        self.assertEqual(policy.label('Foo__bar _Baz'), 'Foo_bar Baz')

    def test_no_fuzzy_localized_editor_or_toggle_substitution(self):
        for name in ('Connection Information…', 'connection information', 'Edit Connections…',
                     'Enable Networking', 'Información de conexión'):
            value = menu()
            value['children'][1]['properties']['label'] = name
            with self.subTest(name=name), self.assertRaises(policy.Blocked):
                policy.information_entry(value)

    def test_disabled_hidden_toggle_and_submenu_are_rejected(self):
        for field, value in (('enabled', False), ('visible', False), ('type', 'separator'),
                             ('toggle-type', 'checkmark'), ('toggle-state', 0), ('children-display', 'submenu')):
            tree = menu()
            tree['children'][1]['properties'][field] = value
            with self.subTest(field=field), self.assertRaises(policy.Blocked):
                policy.information_entry(tree)

    def test_duplicate_ids_duplicate_labels_and_nested_action_are_rejected(self):
        for kind in ('id', 'label', 'nested'):
            tree = menu()
            if kind == 'id':
                tree['children'][2]['id'] = 4
            elif kind == 'label':
                tree['children'].append(copy.deepcopy(tree['children'][1]))
                tree['children'][-1]['id'] = 44
            else:
                tree['children'][2]['children'].append(tree['children'].pop(1))
            with self.subTest(kind=kind), self.assertRaises(policy.Blocked):
                policy.information_entry(tree)

    def test_count_and_depth_limits(self):
        tree = menu()
        tree['children'] += [{'id': n, 'properties': {}, 'children': []} for n in range(10, 266)]
        with self.assertRaises(policy.Blocked):
            policy.information_entry(tree)
        tree = menu()
        tail = tree['children'][2]
        for n in range(10, 20):
            child = {'id': n, 'properties': {}, 'children': []}
            tail['children'].append(child)
            tail = child
        with self.assertRaises(policy.Blocked):
            policy.information_entry(tree)

    def test_every_captured_menu_and_owner_identity_must_match_before_release(self):
        baseline = descriptor()
        policy.current_menu(baseline, copy.deepcopy(baseline))
        for field in baseline:
            changed = copy.deepcopy(baseline)
            changed[field] = 'changed'
            with self.subTest(field=field), self.assertRaises(policy.Blocked):
                policy.current_menu(baseline, changed)


class NativeAdmission(unittest.TestCase):
    def test_plain_left_right_escape_and_catchall_bindings_block(self):
        for binding in ({'key': 'mouse:272'}, {'key': 'mouse:right'}, {'keycode': 281},
                        {'key': 'Escape'}, {'catch_all': True}):
            with self.subTest(binding=binding), self.assertRaises(policy.Blocked):
                policy.unbound_input([dict(modmask=0, **binding)])
        policy.unbound_input([{'key': 'mouse:272', 'modmask': 64}])
        with self.assertRaises(policy.Blocked):
            policy.unbound_input([{'key': 'mouse:273', 'modmask': 2}])

    def test_typed_qmp_buttons_only(self):
        self.assertEqual(policy.button_event('right', True),
                         {'type': 'btn', 'data': {'button': 'right', 'down': True}})
        for button, down in (('wheel-up', True), ('left', 1), ('right', 'false')):
            with self.subTest(button=button, down=down), self.assertRaises(policy.Blocked):
                policy.button_event(button, down)

    def test_rounded_menu_target_must_be_fully_visible(self):
        hit = {'id': 'tray-menu-4', 'x': 16, 'y': 20, 'width': 200, 'height': 40,
               'clip': {'x': 0, 'y': 0, 'width': 250, 'height': 200}}
        surface = {'width': 250, 'height': 200, 'hits': [hit]}
        self.assertEqual(policy.visible_point(surface, {'x': 1200, 'y': 60}, 'tray-menu-4'), (1316, 100))
        hit['clip']['height'] = 45
        with self.assertRaises(policy.Blocked):
            policy.visible_point(surface, {'x': 0, 'y': 0}, 'tray-menu-4')

    def test_unknown_cursor_movement_is_sticky_and_never_adopted(self):
        cursor = policy.CursorOwnership({'x': 50, 'y': 60})
        cursor.begin({'x': 50, 'y': 60}, (150, 160))
        cursor.finish({'x': 150, 'y': 160})
        with self.assertRaises(policy.Blocked):
            cursor.observe({'x': 151, 'y': 160})
        with self.assertRaises(policy.Blocked):
            cursor.observe({'x': 150, 'y': 160})

    def test_recycled_pid_wrong_argv_and_missing_private_marker_are_not_owned(self):
        intent = {'pid': 100, 'start': '42', 'cgroup': '/owned', 'applet_sha256': 'abc', 'token': 'a' * 24}
        current = {'pid': 100, 'start': '42', 'cgroup': '/owned', 'executable_sha256': 'abc',
                   'argv': ['/usr/bin/nm-applet', '--indicator', '--no-agent'], 'token': 'a' * 24}
        policy.owned_process(intent, current)
        for field, value in (('start', '43'), ('cgroup', '/foreign'), ('executable_sha256', 'other'),
                             ('argv', ['/usr/bin/nm-applet', '--indicator']), ('token', None)):
            with self.subTest(field=field), self.assertRaises(policy.Blocked):
                policy.owned_process(intent, dict(current, **{field: value}))

    def test_exact_cgroup_rejects_descendant_or_other_user(self):
        unit = 'lucent-third-party-tray-' + 'a' * 24 + '.service'
        group = f'/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}'
        self.assertEqual(policy.cgroup_path(group, 1000, unit), group)
        for value in (group + '/child', group.replace('1000', '1001'), group.replace('/app.slice/', '/../')):
            with self.subTest(value=value), self.assertRaises(policy.Blocked):
                policy.cgroup_path(value, 1000, unit)


class NonsecretNetworkSnapshot(unittest.TestCase):
    def fake_bus(self, wifi=False, owner_change=False, loopback=False):
        bus = object.__new__(guest.Bus)
        calls = []
        bus.plain = lambda value: value
        owners = iter((':1.7', ':1.8' if owner_change else ':1.7'))
        bus.owner = lambda *_args: next(owners)
        root = {'Devices': ['/device'], 'ActiveConnections': ['/active'], 'NetworkingEnabled': True,
                'WirelessEnabled': True, 'WwanEnabled': False}
        if loopback:
            root['ActiveConnections'].append('/loopback-active')
        def properties(_bus, _owner, _path, interface):
            if interface == guest.NM:
                return root
            if interface.endswith('.Device'):
                return {'DeviceType': 2 if wifi else 1, 'Interface': 'fixture0', 'State': 100}
            if interface.endswith('.Connection.Active'):
                if _path == '/loopback-active':
                    return {'Type': 'loopback', 'State': 2, 'Uuid': 'loopback-test-uuid'}
                return {'Type': '802-3-ethernet', 'State': 2, 'Uuid': 'owned-test-uuid'}
            return {'Unsaved': False, 'Flags': 0}
        def call(_bus, _owner, _path, _interface, method, *_args):
            calls.append(method)
            if method == 'ListConnections':
                return ['/profile']
            self.assertEqual(method, 'GetSettings')
            return {'connection': {'uuid': 'owned-test-uuid', 'id': 'Private fixture name'}}
        bus.properties, bus.call, bus.system = properties, call, object()
        return bus, calls

    def test_hashes_nonsecret_settings_and_never_requests_secrets(self):
        bus, calls = self.fake_bus()
        value = bus.profiles()
        self.assertEqual(calls, ['ListConnections', 'GetSettings'])
        self.assertNotIn('Private fixture name', json.dumps(value))
        self.assertEqual(len(value['profiles'][0][1]), 64)

    def test_wifi_is_rejected_before_menu_or_profile_calls(self):
        bus, calls = self.fake_bus(wifi=True)
        with self.assertRaisesRegex(policy.Blocked, 'wired_only'):
            bus.profiles()
        self.assertEqual(calls, [])

    def test_existing_active_loopback_is_preserved_alongside_one_ethernet(self):
        bus, _ = self.fake_bus(loopback=True)
        observed = bus.profiles()
        self.assertEqual([item[2] for item in observed['active']], ['802-3-ethernet', 'loopback'])

    def test_daemon_owner_replacement_invalidates_observation(self):
        bus, _ = self.fake_bus(owner_change=True)
        with self.assertRaisesRegex(policy.Blocked, 'owner_changed'):
            bus.profiles()


class OwnedKernelFilesAndCleanup(unittest.TestCase):
    def test_history_and_new_config_directory_changes_invalidate_private_checkpoint(self):
        with tempfile.TemporaryDirectory(prefix='lucent-tray-home-') as scratch:
            home = Path(scratch)
            state = home / '.local/state/lucent'
            state.mkdir(parents=True)
            (state / 'desktop.json').write_text('{"live_wallpapers":{}}')
            (state / 'wallpaper').write_bytes(b'owned test image')
            marker = home / '.local/state/omarchy/indicators/stay-awake'
            marker.parent.mkdir(parents=True)
            marker.write_bytes(b'')
            before = guest.files(home, {})
            history = state / 'notifications.json'
            self.assertIsNone(before['files'][str(history)])
            history.write_text('{"version":1}')
            self.assertNotEqual(guest.files(home, {}), before)
            history.unlink()
            (home / '.config/nm-applet').mkdir(parents=True)
            self.assertNotEqual(guest.files(home, {}), before)
            with self.assertRaisesRegex(policy.Blocked, 'default_state_and_config_roots'):
                guest.files(home, {'XDG_STATE_HOME': '/different'})

    def test_real_file_and_symlink_target_changes_are_detected_without_restoration(self):
        with tempfile.TemporaryDirectory(prefix='lucent-tray-file-') as scratch:
            path = Path(scratch) / 'profile'
            path.write_text('original')
            link = Path(scratch) / 'reference'
            link.symlink_to(path)
            original = guest.file_record(link)
            path.write_text('changed!')
            self.assertNotEqual(guest.file_record(link), original)
            self.assertEqual(path.read_text(), 'changed!')

    def test_private_receipt_modes_and_symlinks_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix='lucent-tray-file-') as scratch:
            directory = Path(scratch)
            guest.private(directory / 'receipt', {'owned': True})
            self.assertEqual(guest.read(directory, 'receipt'), {'owned': True})
            (directory / 'receipt').chmod(0o644)
            with self.assertRaises(policy.Blocked):
                guest.read(directory, 'receipt')
            (directory / 'link').symlink_to(directory / 'receipt')
            with self.assertRaises(policy.Blocked):
                guest.read(directory, 'link')

    def fixture(self, scratch, populated='0'):
        uid = os.getuid()
        unit = 'lucent-third-party-tray-' + 'b' * 24 + '.service'
        group_name = f'/user.slice/user-{uid}.slice/user@{uid}.service/app.slice/{unit}'
        group = Path(scratch) / group_name.lstrip('/')
        group.mkdir(parents=True)
        for name, text in (('cgroup.procs', ''), ('cgroup.threads', ''), ('cgroup.events', f'populated {populated}\n')):
            (group / name).write_text(text)
        saved = {'unit': unit}
        intent = {'unit': unit, 'cgroup': group_name, 'invocation': 'aa',
                  'group_identity': guest.identity(group.stat())[:2]}
        properties = {'MainPID': '0', 'InvocationID': 'aa', 'ActiveState': 'inactive'}
        real_path = Path
        def paths(value):
            return real_path(scratch) if str(value) == '/sys/fs/cgroup' else real_path(value)
        return saved, intent, properties, paths

    def test_actual_private_tree_quiescence_and_descendant_population(self):
        for populated in ('0', '1'):
            with self.subTest(populated=populated), tempfile.TemporaryDirectory(prefix='lucent-tray-cgroup-') as scratch:
                saved, intent, properties, paths = self.fixture(scratch, populated)
                with patch.object(guest, 'Path', side_effect=paths), \
                     patch.object(guest, 'read', side_effect=lambda _d, name: saved if name == 'checkpoint.json' else intent), \
                     patch.object(guest, 'unit_properties', return_value=properties):
                    if populated == '0':
                        self.assertTrue(guest.stop(Path(scratch))['exact_owned_unit_quiescent'])
                    else:
                        with self.assertRaisesRegex(policy.Blocked, 'descendants_not_quiescent'):
                            guest.stop(Path(scratch))

    def test_active_reused_process_never_gets_stop_signal(self):
        with tempfile.TemporaryDirectory(prefix='lucent-tray-cgroup-') as scratch:
            saved, intent, properties, paths = self.fixture(scratch)
            properties['MainPID'] = '999'
            with patch.object(guest, 'Path', side_effect=paths), \
                 patch.object(guest, 'read', side_effect=lambda _d, name: saved if name == 'checkpoint.json' else intent), \
                 patch.object(guest, 'unit_properties', return_value=properties), \
                 patch.object(guest, 'owner', side_effect=policy.Blocked('pid_recycled')), \
                 patch.object(guest, 'run') as command:
                with self.assertRaisesRegex(policy.Blocked, 'pid_recycled'):
                    guest.stop(Path(scratch))
                command.assert_not_called()


class DriverOrdering(unittest.TestCase):
    def test_fresh_native_guard_can_block_after_slow_menu_observation_before_down(self):
        vm = Mock()
        run = driver.Run(vm, '/private', Path('/private'))
        run.cursor = policy.CursorOwnership({'x': 1, 'y': 2})
        target = {'point': (1, 2)}
        run.point = Mock(return_value=target)
        run.move = Mock()
        run.probe = Mock(return_value=descriptor())
        run.guard = Mock(side_effect=[None, policy.Blocked('fresh_foreign_layer')])
        with self.assertRaisesRegex(policy.Blocked, 'fresh_foreign_layer'):
            run.click('left', identifier='tray-menu-4', menu=descriptor())
        vm.qmp.assert_not_called()

    def test_menu_revision_change_after_down_cancels_before_balanced_up(self):
        vm = Mock()
        vm.inspect.return_value = {'client': {'tray': {'open': False}}}
        run = driver.Run(vm, '/private', Path('/private'))
        run.cursor = Mock()
        run.point, run.move, run.guard = Mock(return_value={'point': (1, 2)}), Mock(), Mock()
        run.probe = Mock(side_effect=[descriptor(), dict(descriptor(), revision=5)])
        with self.assertRaisesRegex(policy.Blocked, 'retired_before_release'):
            run.click('left', identifier='tray-menu-4', menu=descriptor())
        self.assertEqual(vm.qmp.call_args_list[0].args[0], [policy.button_event('left', True)])
        self.assertEqual(vm.qmp.call_args_list[-1].args[0], [policy.button_event('left', False)])
        vm.virsh.assert_called_once_with('send-key', vm.name, '--codeset', 'linux', '--holdtime', '30', '1')

    def test_cursor_loss_still_stops_only_owned_unit_and_never_restores_cursor(self):
        vm = Mock()
        vm.hypr.return_value = {'x': 9, 'y': 9}
        run = driver.Run(vm, '/private', Path('/private'))
        run.started = True
        run.cursor = policy.CursorOwnership({'x': 1, 'y': 2})
        run.probe = Mock(return_value={'preserved': True})
        result = run.cleanup()
        self.assertTrue(result['owned_applet_quiescent'])
        self.assertTrue(result['state_preserved'])
        self.assertFalse(result['cursor_restored'])
        self.assertIn('cursor_authority_lost_no_input_or_restore', result['failures'])
        self.assertEqual(run.probe.call_args_list[0].args, ('stop',))
        vm.qmp.assert_not_called()

    def test_lost_button_release_reply_does_not_prevent_owned_process_cleanup(self):
        vm = Mock()
        vm.qmp.side_effect = policy.Blocked('transport_lost')
        run = driver.Run(vm, '/private', Path('/private'))
        run.started, run.held = True, 'left'
        run.cursor = policy.CursorOwnership({'x': 1, 'y': 2})
        run.probe = Mock(return_value={'preserved': True})
        result = run.cleanup()
        self.assertTrue(result['owned_applet_quiescent'])
        self.assertFalse(result['physical_button_released'])
        self.assertEqual(run.probe.call_args_list[0].args, ('stop',))
        self.assertEqual(vm.qmp.call_count, 1)


if __name__ == '__main__':
    unittest.main()
