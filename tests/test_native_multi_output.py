"""Tooling policy and owned-process lease tests; no compositor or VM is used."""
import copy
import importlib.util
import json
import multiprocessing
import os
from pathlib import Path
import socket
import re
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import multi_output_guard as guard
import multi_output_policy as policy

spec = importlib.util.spec_from_file_location('native_multi_output_driver', ROOT / 'vm/test-native-multi-output.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


NAME = 'LUCENT-TEST-' + '1' * 24


def monitor(name='DP-1', identifier=0, x=0, scale=1):
    return dict(id=identifier, name=name, description='owned test fixture',
                width=1920, height=1080, x=x, y=0, scale=scale, transform=0,
                refreshRate=60.0, disabled=False, dpmsStatus=True)


class Backend:
    """Owned fixture with independent native/global identities and a mutation log."""
    def __init__(self):
        self.baseline = [monitor()]
        self.current = None
        self.extra = []
        self.global_value = 100
        self.mutations = []
        self.changed = False
        self.fail_configure = False
        self.fail_remove = False
        self.files, self.supervisor, self.compositor = {}, {}, {}
        self.hyprland_version = {'commit': 'private-fixture-not-a-native-proof', 'dirty': False}
        self.pointer, self.workspace, self.pid = {'x': 10, 'y': 80}, 1, 123
        self.restored = False

    def preserve(self, full=False):
        policy.require(not self.changed, 'fixture_concurrent_settings_change')

    def input_preflight(self):
        self.preserve()

    def monitors(self):
        return copy.deepcopy(self.baseline + self.extra + ([self.current] if self.current else []))

    def create(self, name):
        self.mutations.append(('create', name))
        self.global_value += 1
        self.current = monitor(name, 1, 1920)

    def wait_monitor(self, name):
        policy.require(self.current and self.current['name'] == name, 'fixture_output_missing')
        return copy.deepcopy(self.current)

    def global_id(self, name):
        policy.require(self.current and self.current['name'] == name, 'fixture_global_missing')
        return self.global_value

    wait_global = global_id

    def configure(self, case):
        self.mutations.append(('configure', case['name']))
        self.current.update({key: value for key, value in case.items() if key != 'refresh'})
        self.current['refreshRate'] = case['refresh']
        if self.fail_configure:
            raise policy.Blocked('fixture_lost_configuration_reply')

    def wait_case(self, name, case):
        policy.require(policy.matches_case(self.current, case), 'fixture_case_not_applied')
        return copy.deepcopy(self.current)

    def remove(self, name):
        policy.require(name == NAME and self.current['name'] == NAME, 'fixture_foreign_remove')
        self.mutations.append(('remove', name))
        self.current = None
        if self.fail_remove:
            raise policy.Blocked('fixture_lost_remove_reply')

    def wait_absent(self, name):
        policy.require(self.current is None, 'fixture_output_not_removed')

    def restore_focus(self):
        self.restored = True


class NativeGeometry(unittest.TestCase):
    def test_fractional_density_keeps_layer_and_hit_logical_then_adds_global_origin(self):
        native = monitor(NAME, 1, 1920, 1.25)
        native.update(width=1280, height=960, y=-500)
        source = {'pointer_input': 'content', 'width': 1024, 'height': 48,
                  'hits': [{'id': 'audio', 'x': 40, 'y': 8, 'width': 24, 'height': 24,
                            'clip': {'x': 0, 'y': 0, 'width': 1024, 'height': 48}}]}
        point = policy.hit_center(native, source, {'x': 12, 'y': 8}, 'audio')
        self.assertEqual(point, (1984, -472))
        self.assertTrue(policy.cursor_matches({'x': 1984, 'y': -472}, point))
        self.assertFalse(policy.cursor_matches({'x': 64, 'y': 28}, point))
        source['hits'][0]['clip'].update(x=50, width=30)
        self.assertEqual(policy.hit_center(native, source, {'x': 12, 'y': 8}, 'audio'), (1989, -472))

    def test_invisible_nonfinite_or_ambiguous_hits_never_gain_input_authority(self):
        source = {'pointer_input': 'content', 'width': 100, 'height': 48,
                  'hits': [{'id': 'audio', 'x': 10, 'y': 8, 'width': 20, 'height': 24,
                            'clip': {'x': 0, 'y': 0, 'width': 100, 'height': 48}}]}
        for mutation in ('clipped', 'nonfinite', 'duplicate', 'no-input'):
            current = copy.deepcopy(source)
            if mutation == 'clipped':
                current['hits'][0]['clip']['width'] = 11
            elif mutation == 'nonfinite':
                current['hits'][0]['x'] = float('nan')
            elif mutation == 'duplicate':
                current['hits'].append(copy.deepcopy(current['hits'][0]))
            else:
                current['pointer_input'] = 'none'
            with self.subTest(mutation=mutation), self.assertRaises(policy.Blocked):
                policy.hit_center(monitor(), current, {'x': 0, 'y': 0}, 'audio')

    def test_native_namespace_is_selected_on_exact_monitor_and_pid(self):
        source = {'id': 'bar [private-instance]', 'width': 640, 'height': 48}
        layer = {'namespace': 'lucent-' + source['id'], 'pid': 123, 'address': '0x456',
                 'x': 0, 'y': 0, 'w': 640, 'h': 48}
        layers = {NAME: {'levels': {'2': [layer]}}, 'DP-1': {'levels': {}}}
        self.assertEqual(policy.layer_for(layers, NAME, source, 123), layer)
        for name, pid in (('DP-1', 123), (NAME, 999)):
            with self.assertRaises(policy.Blocked):
                policy.layer_for(layers, name, source, pid)

    def test_background_bar_and_widget_selector_never_follows_active_host(self):
        state = {'client': {'named_monitor_hosts': True, 'active_monitor': 'DP-1',
                           'monitor_hosts': [{'id': 'DP-1', 'instance': 'one'},
                                             {'id': NAME, 'instance': 'two'}]},
                 'surfaces': [{'id': 'bar-one', 'role': 'bar', 'instance': 'one', 'output': 'DP-1'},
                              {'id': 'bar-two', 'role': 'bar', 'instance': 'two', 'output': NAME}]}
        self.assertEqual(policy.surface(state, NAME, 'bar')['id'], 'bar-two')
        state['surfaces'][1]['output'] = 'DP-1'
        with self.assertRaises(policy.Blocked):
            policy.surface(state, NAME, 'bar')

    def test_qmp_button_payload_has_real_typed_left_button_and_boolean_state(self):
        self.assertEqual(policy.button_event(True),
                         {'type': 'btn', 'data': {'button': 'left', 'down': True}})
        self.assertIs(policy.button_event(False)['data']['down'], False)
        for value in (1, 0, 'left', 'true', None):
            with self.assertRaises(policy.Blocked):
                policy.button_event(value)

    def test_foreign_same_prefix_layer_client_cannot_claim_desktop_input_authority(self):
        layers = {'DP-1': {'levels': {'2': [{'namespace': 'lucent-bar', 'pid': 123}]}}}
        policy.require_owned_layers(layers, 123)
        for pid in (999, None):
            layers['DP-1']['levels']['2'].append({'namespace': 'lucent-overlay', 'pid': pid})
            with self.assertRaisesRegex(policy.Blocked, 'foreign_native_layer_owner'):
                policy.require_owned_layers(layers, 123)
            layers['DP-1']['levels']['2'].pop()

    def test_plain_escape_pointer_and_catchall_bindings_are_rechecked_without_rejecting_super_drag(self):
        policy.require_unbound_input([{'modmask': 64, 'key': 'mouse:272'}, {'modmask': 0, 'key': 'F1'}])
        for binding in ({'modmask': 0, 'key': 'Escape'}, {'modmask': 2, 'key': 'mouse:272'},
                        {'modmask': 16, 'keycode': 1}, {'modmask': 0, 'key': 'catchall'},
                        {'modmask': 0, 'key': '', 'catch_all': True}):
            with self.subTest(binding=binding), self.assertRaises(policy.Blocked):
                policy.require_unbound_input([binding])

    def test_new_display_settings_panel_is_part_of_idle_and_closed_foreground_checks(self):
        client = {'settings_writable': True, 'settings_save_status': 'saved',
                  'background_ready': True, 'named_monitor_hosts': True}
        guard.idle(client)
        client['displays'] = {'open': True}
        with self.assertRaisesRegex(policy.Blocked, 'all_panels_closed'):
            guard.idle(client)
        client.update(launcher=False, notification_history_open=False)
        vm = type('VM', (), {'inspect': lambda _: {'client': client}})()
        with self.assertRaisesRegex(policy.Blocked, 'foreground_panel'):
            driver.Run(vm, Path('/unused'), '/unused', 123).panels_stay_closed(.01)

    def test_removal_postimage_requires_the_exact_reviewed_hyprland_source_commit(self):
        guard.require_reviewed_hyprland({'commit': guard.REVIEWED_HYPRLAND_COMMIT, 'dirty': False})
        for value in ({'commit': 'unknown-source', 'dirty': False},
                      {'commit': guard.REVIEWED_HYPRLAND_COMMIT, 'dirty': True}, {}):
            with self.assertRaisesRegex(policy.Blocked, 'matching_removal_source_review'):
                guard.require_reviewed_hyprland(value)


class CursorAuthority(unittest.TestCase):
    def test_only_exact_declared_guardian_warp_or_known_removal_postimage_is_admitted(self):
        cursor = policy.CursorOwnership((10, 80))
        cursor.begin({'x': 10, 'y': 80}, (2012, 20))
        cursor.confirm({'x': 2012, 'y': 20})
        cursor.begin({'x': 2012, 'y': 20}, (960, 540))
        # A lost reply may reconcile only the exact already-declared postimage.
        cursor.check({'x': 960, 'y': 540})
        self.assertEqual(cursor.expected, (960, 540))
        self.assertIsNone(cursor.pending)
        cursor.begin({'x': 960, 'y': 540}, (10, 80))
        cursor.confirm({'x': 10, 'y': 80})

    def test_user_movement_or_unknown_backend_warp_permanently_blocks_our_restoration(self):
        for unknown in ({'x': 11, 'y': 80}, {'x': 0, 'y': 0}, None, {'x': True, 'y': False}):
            cursor = policy.CursorOwnership((10, 80))
            with self.assertRaisesRegex(policy.Blocked, 'foreign_or_uncertain'):
                cursor.check(unknown)
            with self.assertRaises(policy.Blocked):
                cursor.begin({'x': 10, 'y': 80}, (100, 100))
        cursor = policy.CursorOwnership((10, 80))
        cursor.begin({'x': 10, 'y': 80}, (960, 540))
        with self.assertRaisesRegex(policy.Blocked, 'postimage_differs'):
            cursor.confirm({'x': 959, 'y': 540})
        self.assertTrue(cursor.lost)

    def test_native_cleanup_still_removes_owned_output_but_never_restores_over_user_movement(self):
        native = object.__new__(guard.Native)
        native.baseline = [monitor()]
        native.cursor = policy.CursorOwnership((2012, 20))
        native.preserve = lambda: None
        calls = []
        def hypr(*args):
            calls.append(args)
            if args == ('-j', 'cursorpos'):
                return json.dumps({'x': 30, 'y': 80})
            if args == ('-j', 'binds'):
                return '[]'
            self.assertEqual(args, ('output', 'remove', NAME))
            return 'ok'
        with patch.object(guard, 'hypr', side_effect=hypr):
            native.remove(NAME)
            with self.assertRaisesRegex(policy.Blocked, 'foreign_or_uncertain'):
                native.restore_focus()
        self.assertIn(('output', 'remove', NAME), calls)
        self.assertFalse(any(args[0] == 'dispatch' for args in calls))
        self.assertTrue(native.cursor.lost)

    def test_native_remove_reconciles_only_the_declared_backup_center_without_adopting_other_warps(self):
        for point, lost in (({'x': 960, 'y': 540}, False), ({'x': 959, 'y': 540}, True)):
            native = object.__new__(guard.Native)
            native.baseline = [monitor()]
            native.cursor = policy.CursorOwnership((2012, 20))
            replies = iter([json.dumps({'x': 2012, 'y': 20}), 'ok', json.dumps(point)])
            with patch.object(guard, 'hypr', side_effect=lambda *_: next(replies)):
                native.remove(NAME)
            self.assertEqual(native.cursor.lost, lost)
            self.assertEqual(native.cursor.expected, (2012, 20) if lost else (960, 540))


class Ownership(unittest.TestCase):
    def setUp(self):
        self.backend = Backend()
        self.lease = guard.Lease(self.backend, NAME, copy.deepcopy(self.backend.baseline))

    def test_only_finite_cases_modify_owned_output_and_recreation_has_new_global(self):
        self.lease.create()
        original_global = self.lease.global_id
        for scale in (1, 2, 1.25, 1):
            self.lease.configure(scale)
            self.assertEqual(self.lease.expected['scale'], scale)
            self.assertEqual(self.lease.expected['x'], 1952)
            self.assertEqual(self.lease.expected['width'], 1280)
        with self.assertRaises(policy.Blocked):
            self.lease.configure(1.5)
        self.lease.remove()
        self.lease.create()
        self.assertNotEqual(self.lease.global_id, original_global)
        self.lease.remove()
        self.assertEqual(self.backend.monitors(), self.backend.baseline)
        self.assertTrue(all(name == NAME for _, name in self.backend.mutations))

    def test_cleanup_refuses_foreign_layout_output_replacement_and_settings_changes(self):
        for mutation in ('original-layout', 'new-monitor', 'owned-id', 'owned-scale',
                         'wayland-global', 'settings'):
            with self.subTest(mutation=mutation):
                backend = Backend()
                lease = guard.Lease(backend, NAME, copy.deepcopy(backend.baseline))
                lease.create()
                before = len(backend.mutations)
                if mutation == 'original-layout':
                    backend.baseline[0]['x'] = 32
                elif mutation == 'new-monitor':
                    backend.extra.append(monitor('DP-2', 3, -1920))
                elif mutation == 'owned-id':
                    backend.current['id'] += 1
                elif mutation == 'owned-scale':
                    backend.current['scale'] = 1.5
                elif mutation == 'wayland-global':
                    backend.global_value += 1
                else:
                    backend.changed = True
                with self.assertRaises(policy.Blocked):
                    lease.remove()
                self.assertEqual(len(backend.mutations), before)
                self.assertIsNotNone(backend.current)

    def test_existing_output_is_never_adopted_or_removed(self):
        self.backend.current = monitor(NAME, 1, 1920)
        with self.assertRaises(policy.Blocked):
            self.lease.create()
        with self.assertRaises(policy.Blocked):
            self.lease.remove()
        self.assertEqual(self.backend.mutations, [])

    def test_lost_configuration_reply_can_only_admit_exact_authorized_postimage_for_cleanup(self):
        self.lease.create()
        self.backend.fail_configure = True
        with self.assertRaisesRegex(policy.Blocked, 'lost_configuration'):
            self.lease.configure(1.25)
        self.lease.remove()
        self.assertIsNone(self.backend.current)

    def test_lost_removal_reply_recognizes_absence_without_a_second_remove(self):
        self.lease.create()
        self.backend.fail_remove = True
        with self.assertRaisesRegex(policy.Blocked, 'lost_remove'):
            self.lease.remove()
        self.lease.remove()
        self.assertEqual(sum(action == 'remove' for action, _ in self.backend.mutations), 1)


class InputVM:
    def __init__(self):
        self.position = {'x': 0, 'y': 0}
        self.events = []
        self.warp_on_button = False

    def hypr(self, *args):
        if args == ('-j', 'cursorpos'):
            return json.dumps(self.position)
        assert args[0] == 'dispatch' and args[1].startswith('hl.dsp.cursor.move(')
        x, y = re.search(r'x=(-?\d+),y=(-?\d+)', args[1]).groups()
        self.position = {'x': int(x), 'y': int(y)}
        return 'ok'

    def button(self, down):
        self.events.append(policy.button_event(down))
        if down and self.warp_on_button:
            self.position = {'x': 0, 'y': 0}


class RealInputAdmission(unittest.TestCase):
    def run_fixture(self):
        vm = InputVM()
        run = driver.Run(vm, Path('/unused-private-fixture'), '/unused-private-fixture', 123)
        bar = {'id': 'bar-owned', 'role': 'bar', 'instance': 'owned', 'output': NAME,
               'width': 1024, 'height': 48, 'pointer_input': 'content',
               'hits': [{'id': 'bar-audio', 'x': 80, 'y': 8, 'width': 24, 'height': 24,
                         'clip': {'x': 0, 'y': 0, 'width': 1024, 'height': 48}}]}
        value = {'monitor': monitor(NAME, 1, 1920),
                 'host': {'id': NAME, 'instance': 'owned', 'generation': 1},
                 'output': {'name': NAME, 'global': 101}, 'bar': bar,
                 'layers': {'bar-owned': {'x': 0, 'y': 0}}}
        run.settled = lambda _name: copy.deepcopy(value)
        vm.inspect = lambda: {'client': {'named_monitor_hosts': True, 'monitor_hosts': [value['host']]},
                              'outputs': [value['output']], 'surfaces': [value['bar']]}
        def lease(operation, **_kwargs):
            if operation == 'warp':
                point = policy.hit_center(value['monitor'], value['bar'], value['layers']['bar-owned'], 'bar-audio')
                vm.position = dict(zip(('x', 'y'), point))
                return {'point': point, 'surface': value['bar']['id']}
            return {}
        run.lease = lease
        return run, vm, value

    def test_independently_verified_warp_is_followed_by_actual_typed_qmp_press_and_release(self):
        run, vm, _ = self.run_fixture()
        run.press_audio(NAME)
        run.release()
        self.assertEqual(vm.position, {'x': 2012, 'y': 20})
        self.assertEqual(vm.events, [policy.button_event(True), policy.button_event(False)])

    def test_button_that_warps_back_is_blocked_and_balanced_without_accepting_cli_action(self):
        run, vm, _ = self.run_fixture()
        vm.warp_on_button = True
        with self.assertRaisesRegex(policy.Blocked, 'warped_cursor'):
            run.press_audio(NAME)
        run.release()
        self.assertEqual(vm.events, [policy.button_event(True), policy.button_event(False)])

    def test_host_retirement_between_warp_and_press_never_sends_a_button(self):
        run, vm, value = self.run_fixture()
        calls = 0
        def settled(_name):
            nonlocal calls
            calls += 1
            current = copy.deepcopy(value)
            current['host']['generation'] = calls
            return current
        run.settled = settled
        with self.assertRaisesRegex(policy.Blocked, 'retired_before_native_press'):
            run.press_audio(NAME)
        self.assertEqual(vm.events, [])

    def test_hotplug_requires_fresh_pointer_focus_from_the_down_event_before_removal(self):
        run, vm, value = self.run_fixture()
        def inspect():
            if vm.events:
                self.assertEqual(vm.events, [policy.button_event(True)])
                value['bar'].update(focus='bar-audio', focus_visible=False)
            return {'client': {'named_monitor_hosts': True, 'monitor_hosts': [value['host']]},
                    'outputs': [value['output']], 'surfaces': [value['bar']]}
        vm.inspect = inspect
        run.press_action(NAME, 'bar-audio', fresh_focus=True)
        run.release()
        value['bar']['focus'] = 'bar-audio'
        vm.events.clear()
        with self.assertRaisesRegex(policy.Blocked, 'fresh_native_press_focus_required'):
            run.press_action(NAME, 'bar-audio', fresh_focus=True)
        self.assertEqual(vm.events, [])

    def test_late_guard_rejection_after_warp_and_settling_prevents_any_physical_down(self):
        run, vm, _ = self.run_fixture()
        original = run.lease
        status = 0
        def lease(operation, **kwargs):
            nonlocal status
            if operation == 'status':
                status += 1
                if status == 2:
                    raise policy.Blocked('fixture_foreign_layer_arrived_after_geometry')
            return original(operation, **kwargs)
        run.lease = lease
        with self.assertRaisesRegex(policy.Blocked, 'foreign_layer_arrived'):
            run.press_audio(NAME)
        self.assertEqual(status, 2)
        self.assertEqual(vm.events, [])

    def test_host_retirement_after_final_guard_check_also_prevents_physical_down(self):
        run, vm, value = self.run_fixture()
        def inspect():
            current = copy.deepcopy(value['host'])
            current['generation'] += 1
            return {'client': {'named_monitor_hosts': True, 'monitor_hosts': [current]},
                    'outputs': [value['output']], 'surfaces': [value['bar']]}
        vm.inspect = inspect
        with self.assertRaisesRegex(policy.Blocked, 'final_input_admission'):
            run.press_audio(NAME)
        self.assertEqual(vm.events, [])


def lease_child(directory):
    """Only this isolated child sees a private runtime directory and fake backend."""
    os.environ['XDG_RUNTIME_DIR'] = str(directory)
    guard.Native = lambda _sha: Backend()
    guard.LEASE_SECONDS = .8
    guard.serve(directory, NAME, '0' * 64)


class IndependentCleanup(unittest.TestCase):
    def test_owned_socket_owner_expires_after_client_disappears_without_heartbeat_extension(self):
        with tempfile.TemporaryDirectory(prefix='lucent-native-outputs-') as temporary:
            directory = Path(temporary)
            process = multiprocessing.get_context('fork').Process(target=lease_child, args=(directory,))
            process.start()
            try:
                deadline = time.monotonic() + 3
                while not (directory / 'ready.json').exists() and time.monotonic() < deadline:
                    time.sleep(.01)
                self.assertTrue((directory / 'ready.json').exists())

                def request(operation):
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                        client.settimeout(2)
                        client.connect(str(directory / 'control.sock'))
                        client.sendall((json.dumps({'operation': operation}) + '\n').encode())
                        return json.loads(client.recv(65536))

                self.assertTrue(request('create')['ok'])
                for _ in range(3):
                    time.sleep(.1)
                    self.assertTrue(request('status')['ok'])
                # No finish request, no host-side finally, no remaining client.
                process.join(3)
                self.assertFalse(process.is_alive())
                self.assertEqual(process.exitcode, 0)
                result = json.loads((directory / 'finished.json').read_text())
                self.assertTrue(result['expired'])
                self.assertTrue(result['removed_only_owned_output'])
                self.assertTrue(result['preserved'])
                self.assertIsNone(result['last_owned_identity']['owned'])
                self.assertLess(time.monotonic(), deadline + 1)
            finally:
                if process.is_alive():
                    process.terminate()
                process.join(2)

    def test_partial_slow_request_cannot_extend_owner_deadline_or_skip_owned_cleanup(self):
        with tempfile.TemporaryDirectory(prefix='lucent-native-outputs-') as temporary:
            directory = Path(temporary)
            process = multiprocessing.get_context('fork').Process(target=lease_child, args=(directory,))
            process.start()
            try:
                deadline = time.monotonic() + 3
                while not (directory / 'ready.json').exists() and time.monotonic() < deadline:
                    time.sleep(.01)
                self.assertTrue((directory / 'ready.json').exists())
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                    client.settimeout(2)
                    client.connect(str(directory / 'control.sock'))
                    client.sendall(b'{"operation":"create"}\n')
                    self.assertTrue(json.loads(client.recv(65536))['ok'])
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as slow:
                    slow.settimeout(2)
                    slow.connect(str(directory / 'control.sock'))
                    slow.sendall(b'{')
                    for _ in range(4):
                        time.sleep(.1)
                        slow.sendall(b' ')
                    process.join(2)
                    self.assertFalse(process.is_alive())
                    self.assertEqual(slow.recv(1), b'')
                self.assertEqual(process.exitcode, 0)
                result = json.loads((directory / 'finished.json').read_text())
                self.assertTrue(result['expired'])
                self.assertTrue(result['removed_only_owned_output'])
                self.assertTrue(result['preserved'])
                self.assertIsNone(result['last_owned_identity']['owned'])
            finally:
                if process.is_alive():
                    process.terminate()
                process.join(2)


if __name__ == '__main__':
    unittest.main()
