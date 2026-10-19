"""Pure native IME fixture authority checks; no VM, D-Bus or native input."""
import copy
from contextlib import redirect_stdout
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import MagicMock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'vm'))
import international_text_policy as policy


def driver_module():
    spec = importlib.util.spec_from_file_location('native_text_driver', ROOT/'vm/test-international-text.py')
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    return driver


def scene():
    surface = {'id': 'international', 'role': 'international', 'width': 640, 'height': 600,
               'frames': 1, 'native_frames': 1, 'pointer_input': 'content', 'focus': 'single',
               'hits': [{'id': 'single', 'x': 24, 'y': 80, 'width': 592, 'height': 40,
                         'clip': {'x': 0, 'y': 0, 'width': 640, 'height': 600}}]}
    layer = {'address': '0xabc', 'namespace': policy.NAMESPACE, 'pid': 202,
             'x': 640, 'y': 0, 'w': 640, 'h': 600}
    return {'surfaces': [surface], 'client': {'single': '', 'body': ''}}, layer


class InputPolicy(unittest.TestCase):
    def test_fixture_is_actual_multiline_and_namespace_matches_registered_client(self):
        self.assertEqual(policy.MULTILINE_TEXT.count(chr(10)), 2)
        self.assertNotIn(chr(92), policy.MULTILINE_TEXT)
        source = (ROOT/'lucent/examples/international-text/src/main.rs').read_text()
        self.assertIn('"'+policy.APP_NAME+'"', source)
        self.assertEqual(policy.NAMESPACE, policy.APP_NAME+'-international')

    def test_each_competing_panel_or_credential_request_refuses_admission(self):
        policy.idle_client({'settings_save_status': 'saved'})
        for change in ({'launcher': True}, {'query': 'x'}, {'wallpaper_busy': True},
                       {'notification_count': 1}, {'widget_grid': True}, {'settings_save_status': 'saving'},
                       {'network': {'form': { 'ssid': 'x'}}}, {'bluetooth': {'requests': ['x']}},
                       *({name: {'open': True}} for name in ('audio', 'media', 'network', 'bluetooth',
                         'power', 'tray', 'live_wallpaper', 'displays', 'weather', 'settings', 'application_actions'))):
            with self.subTest(change=change), self.assertRaises(ValueError):
                policy.idle_client(dict({'settings_save_status': 'saved'}, **change))

    def test_every_injected_key_and_pointer_chord_is_checked_including_catchall(self):
        for binding in ({'modmask': 0, 'key': 'Control_L'}, {'modmask': 4, 'key': 'Shift_L'},
                        {'modmask': 0, 'key': 'BackSpace'}, {'modmask': 0, 'key': 'Home'},
                        {'modmask': 0, 'key': 'Right'}, {'modmask': 0, 'key': 'mouse:left'},
                        {'modmask': 4, 'key': 'a'}, {'modmask': 4, 'key': 'z'}, {'modmask': 5, 'key': 'z'},
                        {'modmask': 2, 'catch_all': True}, {'modmask': 20, 'keycode': 52}, {'key': 'x'}):
            with self.subTest(binding=binding), self.assertRaises(ValueError):
                policy.safe_bindings([binding])
        policy.safe_bindings([{'modmask': 64, 'key': 'Return'}, {'modmask': 64, 'key': 'mouse:272'}])

    def test_layer_namespace_requires_exact_desktop_and_example_processes(self):
        _, layer = scene()
        layers = [{'namespace': 'lucent-bar', 'pid': 101}, layer]
        self.assertEqual(policy.owned_layers(layers, 101, 202), layer)
        for extra in ({'namespace': 'lucent-bar', 'pid': 999},
                      {'namespace': policy.NAMESPACE, 'pid': 999},
                      {'namespace': 'lucent-authorization-x', 'pid': 101},
                      {'namespace': 'foreign-panel', 'pid': 101}, layer):
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                policy.owned_layers(layers+[extra], 101, 202)
        with self.assertRaises(ValueError):
            policy.owned_layers(layers, 101)

    def test_native_text_input_and_component_focus_both_authorize_keys(self):
        state, _ = scene()
        status = {'active': True, 'matches': True, 'unavailable': False}
        policy.focused_field(state, 'single', status)
        for observation in (dict(status, active=False), dict(status, matches=False), dict(status, unavailable=True)):
            with self.assertRaises(ValueError):
                policy.focused_field(state, 'single', observation)
        state['surfaces'][0]['focus'] = 'body'
        with self.assertRaises(ValueError):
            policy.focused_field(state, 'single', status)
        with self.assertRaises(ValueError):
            policy.focused_field(state, 'masked', status)

    def test_hit_must_be_current_fully_visible_and_submitted(self):
        state, layer = scene()
        _, _, target = policy.field_geometry(state, layer, 'single')
        self.assertEqual(target['point'], (960, 100))
        for key, value in (('native_frames', 0), ('pointer_input', 'none'), ('width', float('nan'))):
            modified = copy.deepcopy(state);modified['surfaces'][0][key] = value
            with self.assertRaises(ValueError):
                policy.field_geometry(modified, layer, 'single')
        state['surfaces'][0]['hits'][0]['clip']['height'] = 90
        with self.assertRaises(ValueError):
            policy.field_geometry(state, layer, 'single')

    def test_lost_or_refused_start_never_authorizes_cleanup_of_a_lookalike_unit(self):
        config = {'desktop_sha256': 'hash', 'example': {'unit': 'existing', 'argv': ['/exact']},
                  'peer': {'unit': 'ours', 'argv': ['/peer']}, 'expected': {'peer': {'unit': {}, 'process': {}}}}
        cleanup, uncertain = policy.captured_cleanup(config, ['example', 'peer'])
        self.assertIsNone(cleanup['example'])
        self.assertEqual(cleanup['peer'], config['peer'])
        self.assertEqual(uncertain, ['example'])
        self.assertIsNotNone(config['example'])

    def test_output_and_active_workspace_changes_are_visible_to_the_guard(self):
        monitor = {'name': 'Virtual-1', 'activeWorkspace': {'id': 1}, 'width': 1920, 'height': 1080,
                   'scale': 1, 'transform': 0, 'refreshRate': 60, 'dpmsStatus': True}
        before = policy.output_state([monitor])
        self.assertNotEqual(before, policy.output_state([dict(monitor, scale=2)]))
        self.assertNotEqual(before, policy.output_state([dict(monitor, activeWorkspace={'id': 2})]))
        self.assertNotEqual(policy.workspace_state({'id': 1}), policy.workspace_state({'id': 2}))


class CompositorPointerPositioning(unittest.TestCase):
    def move(self, *, point=(960, 138), arrival=None, dispatch_error=None,
             dispatch_reply=b'ok', restoring=False, guard_action=None):
        driver = driver_module()
        cursor = policy.CursorOwnership({'x': 960, 'y': 540})
        diagnostics, calls = [], []
        position = {'x': 960, 'y': 540}

        def session(*argv, **options):
            calls.append((argv, options))
            if argv == ('hyprctl', '-j', 'cursorpos'):
                return json.dumps(position).encode()
            self.assertEqual(argv, ('hyprctl', 'dispatch',
                                   'hl.dsp.cursor.move({x='+str(point[0])+',y='+str(point[1])+'})'))
            if dispatch_error is not None:
                raise dispatch_error
            position.update(arrival or {'x': point[0], 'y': point[1]})
            return dispatch_reply

        def guard(**options):
            calls.append(('guard', options))
            if guard_action is not None:
                guard_action(position)

        return driver, cursor, diagnostics, calls, session, guard

    def test_actual_move_uses_exact_global_lua_coordinates_without_qmp_normalization(self):
        driver, cursor, diagnostics, calls, session, guard = self.move()
        with patch.object(policy, 'absolute_move', side_effect=AssertionError('QMP motion forbidden')):
            driver.move_pointer(session, guard, cursor, (960, 138), diagnostics)
        self.assertEqual([call[0] for call in calls], ['guard',
                         ('hyprctl', '-j', 'cursorpos'),
                         ('hyprctl', 'dispatch', 'hl.dsp.cursor.move({x=960,y=138})'),
                         ('hyprctl', '-j', 'cursorpos')])
        self.assertEqual(cursor.expected, (960, 138))
        self.assertIsNone(cursor.pending)
        self.assertEqual(diagnostics[0]['requested'], [960, 138])
        self.assertEqual(diagnostics[0]['before'], {'x': 960, 'y': 540})
        self.assertEqual(diagnostics[0]['observed'], [{'x': 960, 'y': 138}])
        self.assertTrue(diagnostics[0]['confirmed'])
        self.assertTrue(all(0 < call[1]['timeout'] <= 3 for call in calls if call[0] != 'guard'))

    def test_restoration_uses_same_global_primitive_and_explicit_restoration_guard(self):
        driver, cursor, diagnostics, calls, session, guard = self.move(point=(960, 540), restoring=True)
        driver.move_pointer(session, guard, cursor, (960, 540), diagnostics, restoring=True)
        self.assertEqual(calls[0], ('guard', {'owned': False, 'restoring': True}))
        self.assertEqual(calls[2][0], ('hyprctl', 'dispatch', 'hl.dsp.cursor.move({x=960,y=540})'))
        self.assertTrue(diagnostics[0]['restoring'])

    def test_foreign_movement_before_dispatch_blocks_the_warp(self):
        driver, cursor, diagnostics, calls, session, guard = self.move(
            guard_action=lambda position: position.update(x=959))
        with self.assertRaisesRegex(ValueError, 'cursor_authority_lost'):
            driver.move_pointer(session, guard, cursor, (960, 138), diagnostics)
        self.assertEqual(len(calls), 2)
        self.assertTrue(cursor.lost)
        self.assertFalse(diagnostics[0]['confirmed'])
        self.assertEqual(diagnostics[0]['before'], {'x': 959, 'y': 540})

    def test_wrong_arrival_never_becomes_authority_or_authorizes_a_restore_warp(self):
        driver, cursor, diagnostics, calls, session, guard = self.move(arrival={'x': 959, 'y': 138})
        with self.assertRaisesRegex(ValueError, 'cursor_authority_lost'):
            driver.move_pointer(session, guard, cursor, (960, 138), diagnostics)
        with self.assertRaisesRegex(ValueError, 'cursor_authority_lost'):
            cursor.begin({'x': 960, 'y': 540}, cursor.initial)
        self.assertTrue(diagnostics[0]['authority_lost'])
        self.assertEqual(diagnostics[0]['observed'], [{'x': 959, 'y': 138}])
        self.assertEqual(len([v for v in calls if v[0][0] == 'hyprctl' and v[0][1] == 'dispatch']), 1)

    def test_lost_or_rejected_warp_reply_never_falls_back_to_other_input(self):
        for options in ({'dispatch_error': TimeoutError('lost reply')}, {'dispatch_reply': b'unknown dispatcher'}):
            with self.subTest(options=options):
                driver, cursor, diagnostics, calls, session, guard = self.move(**options)
                with self.assertRaises((TimeoutError, driver.Failure)):
                    driver.move_pointer(session, guard, cursor, (960, 138), diagnostics)
                self.assertEqual(len(calls), 3)
                self.assertFalse(diagnostics[0]['confirmed'])
                self.assertEqual(cursor.expected, (960, 540))

    def test_unchanged_cursor_hits_deadline_without_retrying_mutation(self):
        driver, cursor, diagnostics, calls, session, guard = self.move(arrival={'x': 960, 'y': 540})
        with patch.object(driver.time, 'monotonic', side_effect=[0, 1, 1.1, 1.2, 4]), \
             patch.object(driver.time, 'sleep'), self.assertRaisesRegex(driver.Failure, '^exact_owned_pointer_arrival$'):
            driver.move_pointer(session, guard, cursor, (960, 138), diagnostics)
        self.assertEqual(len(calls), 4)
        self.assertEqual(diagnostics[0]['observed'], [{'x': 960, 'y': 540}])
        self.assertFalse(diagnostics[0]['confirmed'])

    def test_invalid_destination_and_full_trace_refuse_before_native_calls(self):
        for point in ((True, 138), (960.0, 138), (1920, 138), (-1, 0), (0, 1080), ('1;bad', 0)):
            driver, cursor, diagnostics, calls, session, guard = self.move(point=point)
            with self.subTest(point=point), self.assertRaises(ValueError):
                driver.move_pointer(session, guard, cursor, point, diagnostics)
            self.assertEqual(calls, [])
        driver, cursor, diagnostics, calls, session, guard = self.move()
        with self.assertRaisesRegex(driver.Failure, 'bounded_pointer_move_diagnostics'):
            driver.move_pointer(session, guard, cursor, (960, 138), [{}]*16)
        self.assertEqual(calls, [])


class FinalPointerPressAdmission(unittest.TestCase):
    def attempt(self, change=None, position=None):
        driver = driver_module()
        state, layer = scene()
        target = policy.field_geometry(state, layer, 'single')[2]
        cursor = policy.CursorOwnership(dict(zip(('x', 'y'), target['point'])))
        order = []

        def guard():
            order.append('general_guard')
            if change:
                change(state, layer)

        def observe():
            order.append('fresh_owned_observation')
            return {'authority': {'desktop': {'pid': 101}, 'example': {'process': {'pid': 202}}},
                    'example': state}

        def session(*argv):
            order.append(argv[-1])
            return json.dumps([layer] if argv[-1] == 'layers' and layer else
                              [] if argv[-1] == 'layers' else
                              position or dict(zip(('x', 'y'), target['point']))).encode()

        return driver, target, cursor, order, session, guard, observe

    def test_exact_target_is_rebound_after_general_guard_and_cursor_is_checked_last(self):
        driver, target, cursor, order, session, guard, observe = self.attempt()
        driver.admit_field_press(session, guard, observe, cursor, 'single', target)
        self.assertEqual(order, ['general_guard', 'fresh_owned_observation', 'layers', 'cursorpos'])

    def test_moved_recreated_clipped_or_retired_field_after_slow_guard_refuses_down(self):
        changes = (lambda state, layer: state['surfaces'][0]['hits'][0].update(x=25),
                   lambda state, layer: layer.update(address='0xreplacement'),
                   lambda state, layer: state['surfaces'][0]['hits'][0]['clip'].update(height=90),
                   lambda state, layer: state['surfaces'].clear(),
                   lambda state, layer: layer.clear())
        for change in changes:
            driver, target, cursor, order, session, guard, observe = self.attempt(change)
            deliver = MagicMock()
            with self.subTest(change=change), self.assertRaises((driver.Failure, ValueError)):
                driver.admit_field_press(session, guard, observe, cursor, 'single', target)
                deliver()
            deliver.assert_not_called()

    def test_pointer_movement_after_final_hit_observation_refuses_physical_down(self):
        driver, target, cursor, order, session, guard, observe = self.attempt(position={'x': 959, 'y': 100})
        with self.assertRaisesRegex(ValueError, 'cursor_authority_lost'):
            driver.admit_field_press(session, guard, observe, cursor, 'single', target)
        self.assertTrue(cursor.lost)
        self.assertEqual(order[-1], 'cursorpos')


class IndependentFixtureProvenance(unittest.TestCase):
    desktop = '8e6d074bfcb30a0bcc0b371afddcd50c08993af0'
    peer = 'b4d67a4560948b78549a371d3ef00ef06089f4f1'

    def test_single_release_default_and_peer_only_override_keep_desktop_example_pinned(self):
        driver = driver_module()
        self.assertEqual(driver.source_commits(self.desktop),
                         {'desktop': self.desktop, 'example': self.desktop, 'peer': self.desktop})
        self.assertEqual(driver.source_commits(self.desktop, self.peer),
                         {'desktop': self.desktop, 'example': self.desktop, 'peer': self.peer})

    def test_invalid_peer_commit_is_refused_before_any_process_or_native_action(self):
        driver = driver_module()
        for invalid in ('', 'b4d67a4', self.peer.upper(), 'g'*40, self.peer+'0'):
            argv = ['test-international-text.py', '--source-commit', self.desktop,
                    '--peer-source-commit', invalid, '--example-bin', '/unused/example',
                    '--peer-bin', '/unused/peer', '--example-sha256', 'a'*64,
                    '--peer-sha256', 'b'*64, '--desktop-sha256', 'c'*64]
            with self.subTest(peer=invalid), patch.object(sys, 'argv', argv), \
                 patch.object(driver, 'run') as run:
                with self.assertRaisesRegex(driver.Failure, '^full_peer_source_commit_required$'):
                    driver.main()
                run.assert_not_called()


class OwnedGuestBoundary(unittest.TestCase):
    def functions(self):
        namespace = {}
        exec(policy.OBSERVATION_FUNCTIONS, namespace)
        return namespace

    def test_socket_with_foreign_peer_receives_no_inspect_command(self):
        functions = self.functions()
        info = types.SimpleNamespace(st_dev=1, st_ino=2, st_mode=stat.S_IFSOCK|0o600,
                                     st_uid=1000, st_mtime_ns=3, st_ctime_ns=4)
        path = MagicMock();path.__truediv__.return_value = path;path.lstat.return_value = info
        socket = MagicMock();connection = socket.return_value.__enter__.return_value
        connection.getsockopt.return_value = struct.pack('3i', 999, 1000, 1000)
        with patch.dict(functions, {'Path': lambda _: path}), patch.object(functions['os'], 'getuid', return_value=1000), \
             patch.dict(functions['os'].environ, {'XDG_RUNTIME_DIR': '/owned'}), \
             patch.object(functions['socket'], 'socket', socket):
            with self.assertRaisesRegex(AssertionError, 'exact_owned_socket_peer'):
                functions['owned_socket'](policy.APP_NAME, 202)
        connection.sendall.assert_not_called()

    def test_replaced_socket_path_receives_no_command_even_with_matching_peer(self):
        functions = self.functions()
        info = types.SimpleNamespace(st_dev=1, st_ino=2, st_mode=stat.S_IFSOCK|0o600,
                                     st_uid=1000, st_mtime_ns=3, st_ctime_ns=4)
        changed = copy.copy(info);changed.st_ino = 3
        path = MagicMock();path.__truediv__.return_value = path;path.lstat.side_effect = [info, changed]
        socket = MagicMock();connection = socket.return_value.__enter__.return_value
        connection.getsockopt.return_value = struct.pack('3i', 202, 1000, 1000)
        with patch.dict(functions, {'Path': lambda _: path}), patch.object(functions['os'], 'getuid', return_value=1000), \
             patch.dict(functions['os'].environ, {'XDG_RUNTIME_DIR': '/owned'}), \
             patch.object(functions['socket'], 'socket', socket):
            with self.assertRaises(AssertionError):
                functions['owned_socket'](policy.APP_NAME, 202)
        connection.sendall.assert_not_called()

    def test_typed_unit_argv_group_executable_and_invocation_must_all_match(self):
        functions = self.functions()
        unit = 'lucent-international-test-'+'a'*24+'.service'
        spec = {'unit': unit, 'argv': ['/owned/international-text'], 'sha256': 'digest'}
        value = {'load': 'loaded', 'active': 'active', 'transient': True, 'group': '/session/'+unit,
                 'executable': spec['argv'][0], 'argv': spec['argv'], 'main_pid': 202, 'invocation': 'first'}
        process = {'pid': 202, 'start': '77', 'group': value['group'], 'sha256': 'digest', 'executable': [1,2,3]}
        functions['unit_snapshot'] = lambda _: value
        functions['process_snapshot'] = lambda *_, **__: dict(process)
        captured = functions['owned_unit'](spec)
        for replacement in (dict(value, argv=['/owned/international-text-extra']), dict(value, invocation='second'),
                            dict(value, executable='/foreign'), dict(value, group='/foreign/'+unit)):
            functions['unit_snapshot'] = lambda _, replacement=replacement: replacement
            with self.subTest(replacement=replacement), self.assertRaises(AssertionError):
                functions['owned_unit'](spec, captured)
        functions['unit_snapshot'] = lambda _: value
        process['executable'] = [7,8,9]
        with self.assertRaises(AssertionError):
            functions['owned_unit'](spec, captured)

    def retirement(self, replaced=False):
        unit = 'lucent-international-test-'+'a'*24+'.service'
        peer = 'lucent-international-peer-'+'a'*24+'.service'
        info = types.SimpleNamespace(st_dev=1, st_ino=2, st_mode=stat.S_IFSOCK|0o600,
                                     st_uid=1000, st_mtime_ns=3, st_ctime_ns=4)
        identity = [info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_mtime_ns, info.st_ctime_ns]
        expected = {'unit': {'group': '/owned/'+unit}, 'process': {}}
        config = {'example': {'unit': unit, 'argv': ['/owned/international-text']},
                  'peer': {'unit': peer, 'argv': ['/owned/input-method-peer']},
                  'expected': {'example': expected, 'peer': {'unit': {'group': '/owned/'+peer}, 'process': {}}, 'example_socket': identity}}
        path = MagicMock();path.__truediv__.return_value = path
        actual = copy.copy(info)
        if replaced:
            actual.st_ino = 999
        path.lstat.return_value = actual
        events = []
        namespace = self.functions()
        namespace.update({'retire_unit': lambda unit, *_args, **_kwargs: events.append(unit),
                          'Path': lambda _: path})
        with patch.object(namespace['os'].path, 'lexists', return_value=True), \
             patch.dict(namespace['os'].environ, {'XDG_RUNTIME_DIR': '/owned'}), \
             patch.object(sys, 'argv', ['fixture', json.dumps(config)]), patch('builtins.print') as output:
            body = policy.RETIRE.removeprefix(policy.OBSERVATION_FUNCTIONS)
            exec(body, namespace)
        return events, path, json.loads(output.call_args.args[0])

    def test_cleanup_retires_example_first_and_only_then_peer_and_its_exact_socket(self):
        events, path, result = self.retirement()
        self.assertTrue(events[0].startswith('lucent-international-test-'))
        self.assertTrue(events[1].startswith('lucent-international-peer-'))
        path.unlink.assert_called_once_with()
        self.assertTrue(result['owned_units_retired'])

    def test_replaced_socket_is_preserved_and_cleanup_failure_stays_explicit(self):
        events, path, result = self.retirement(replaced=True)
        self.assertEqual(len(events), 2)
        path.unlink.assert_not_called()
        self.assertFalse(result['owned_units_retired'])
        self.assertEqual(result['retirement_failures']['socket'],
                         {'error_class': 'AssertionError', 'phase': 'exact_stale_socket_cleanup'})

    def test_completion_dbus_failure_retains_phase_without_adopting_unit_absence(self):
        namespace = self.functions()
        unit = 'lucent-international-test-'+'a'*24+'.service'
        value = {'load':'loaded','active':'active','transient':True,'group':'/owned/'+unit,
                 'executable':'/owned/example','argv':['/owned/example'],'main_pid':202,'invocation':'1'*32}
        process = {'pid':202,'start':'77','group':value['group'],'executable':[1,2,3],'sha256':'digest'}
        captured = {'unit':value,'process':process}
        config = {'example':{'unit':unit,'argv':value['argv']},'expected':{'example':captured},'peer':None}
        class FakeBusError(Exception):
            def get_dbus_name(self):return 'org.freedesktop.DBus.Error.UnknownObject'
        namespace['unit_snapshot'] = MagicMock(side_effect=[value,FakeBusError('never publish payload'),{'load':'not-found'}])
        namespace['process_snapshot'] = MagicMock(return_value=process)
        manager = MagicMock();bus = MagicMock()
        dbus = types.SimpleNamespace(SessionBus=MagicMock(return_value=bus),Interface=MagicMock(return_value=manager))
        with patch.dict(sys.modules,{'dbus':dbus}),patch.object(sys,'argv',['fixture',json.dumps(config)]),patch('builtins.print') as output:
            exec(policy.RETIRE.removeprefix(policy.OBSERVATION_FUNCTIONS),namespace)
        result = json.loads(output.call_args.args[0])
        self.assertFalse(result['owned_units_retired'])
        self.assertEqual(result['retirement_failures']['example']['dbus_error_name'],
                         'org.freedesktop.DBus.Error.UnknownObject')
        self.assertTrue(any(event['phase']=='completion_unit_observation' and 'dbus_error_name' in event
                            for event in result['phase_snapshots']))
        self.assertEqual(result['phase_snapshots'][-1]['unit'],{'load':'not-found'})
        self.assertNotIn('never publish payload',json.dumps(result))
        manager.StopUnit.assert_called_once_with(unit,'replace',timeout=5)

    def test_metadata_trace_is_bounded_and_omits_actual_argv_and_exception_payload(self):
        namespace = self.functions()
        unit = 'lucent-international-test-'+'a'*24+'.service'
        value = {'load':'loaded','active':'active','transient':True,'group':'/owned/'+unit,
                 'executable':'private executable','argv':['private secret'],'main_pid':202,'invocation':'1'*32}
        captured = {'unit':value,'process':{}}
        config = {'example':{'unit':unit,'argv':['/owned/example']},'expected':{'example':captured}}
        namespace['unit_snapshot'] = lambda _unit:value
        def retirement(*_args,**_kwargs):
            for _ in range(80):namespace['unit_snapshot'](unit)
            raise ValueError('private failure payload')
        namespace['retire_unit'] = retirement
        with patch.object(sys,'argv',['fixture',json.dumps(config)]),patch('builtins.print') as output:
            exec(policy.RETIRE.removeprefix(policy.OBSERVATION_FUNCTIONS),namespace)
        result = json.loads(output.call_args.args[0])
        self.assertFalse(result['owned_units_retired']);self.assertTrue(result['phase_snapshots_truncated'])
        self.assertLessEqual(len(result['phase_snapshots']),48)
        self.assertNotIn('private',json.dumps(result))

    def test_initial_typed_unit_preflight_refuses_existing_even_with_exact_arguments(self):
        namespace = self.functions()
        calls = []
        namespace['unit_snapshot'] = lambda name: calls.append(name) or {'load': 'loaded', 'argv': ['/exact']}
        units = ['lucent-international-test-'+'a'*24+'.service', 'lucent-international-peer-'+'a'*24+'.service']
        with patch.object(sys, 'argv', ['fixture', json.dumps(units)]), patch('builtins.print'):
            with self.assertRaisesRegex(AssertionError, 'initial_owned_unit_name_must_be_absent'):
                exec(policy.ABSENT.removeprefix(policy.UNIT_IDENTITY), namespace)
        self.assertEqual(calls, units[:1])

    def test_helpers_import_without_native_or_process_actions(self):
        spec = importlib.util.spec_from_file_location('native_text_driver', ROOT/'vm/test-international-text.py')
        driver = importlib.util.module_from_spec(spec)
        with patch('subprocess.run', side_effect=AssertionError('import must not execute')), \
             patch('subprocess.Popen', side_effect=AssertionError('import must not start processes')):
            spec.loader.exec_module(driver)
        self.assertTrue(callable(driver.main))


class PinnedFileReads(unittest.TestCase):
    def cold_file(self, path, data):
        path.write_bytes(data)
        # Force the next actual read to qualify for Linux relatime. No hash/read
        # precedes the operation under test.
        os.utime(path, ns=(1, path.stat().st_mtime_ns))
        return path.stat()

    def test_only_access_time_is_excluded_from_all_eight_identity_fields(self):
        fields = ('st_dev', 'st_ino', 'st_mode', 'st_uid', 'st_gid', 'st_size',
                  'st_mtime_ns', 'st_ctime_ns')
        original = types.SimpleNamespace(**dict(zip(fields, range(10, 18))), st_atime_ns=1)
        self.assertEqual(policy.stable_file_identity(original), list(range(10, 18)))
        access = copy.copy(original);access.st_atime_ns = 999
        self.assertEqual(policy.stable_file_identity(original), policy.stable_file_identity(access))
        for name in fields:
            changed = copy.copy(original);setattr(changed, name, getattr(changed, name)+1)
            with self.subTest(field=name):
                self.assertNotEqual(policy.stable_file_identity(original), policy.stable_file_identity(changed))

    def test_actual_binary_probe_accepts_cold_read_access_time_update(self):
        driver = driver_module()
        with tempfile.TemporaryDirectory() as directory:
            argv = ['probe'];expected = {};before = {}
            for name in ('example', 'peer', 'desktop'):
                path = Path(directory)/name
                data = (name+'\x00payload').encode()*4096
                before[path] = self.cold_file(path, data)
                expected[name] = hashlib.sha256(data).hexdigest()
                argv.extend((str(path), expected[name]))
            output = io.StringIO()
            with patch.object(sys, 'argv', argv), redirect_stdout(output):
                exec(driver.PROBE, {})
            self.assertEqual(json.loads(output.getvalue()), expected)
            advanced = False
            for path, original in before.items():
                after = path.stat()
                self.assertEqual(policy.stable_file_identity(original), policy.stable_file_identity(after))
                advanced |= after.st_atime_ns != original.st_atime_ns
            if not advanced:
                self.skipTest('filesystem does not update atime on actual cold reads')

    def test_actual_settings_checkpoint_survives_cold_read_and_keeps_identity(self):
        driver = driver_module()
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory);base = home/'.local/state/lucent';base.mkdir(parents=True)
            settings = base/'desktop.json';data = b'{"ordinary":"unchanged"}'
            original = self.cold_file(settings, data)
            target = base/'image';target.write_bytes(b'owned fixture')
            (base/'wallpaper').symlink_to(target)
            output = io.StringIO()
            with patch.object(Path, 'home', return_value=home), redirect_stdout(output):
                exec(driver.CHECKPOINT, {})
            result = json.loads(output.getvalue())
            self.assertEqual(result['settings_sha256'], hashlib.sha256(data).hexdigest())
            self.assertEqual(result['settings_inode'], policy.stable_file_identity(original))
            self.assertEqual(result['settings_inode'], policy.stable_file_identity(settings.stat()))
            self.assertEqual(result['wallpaper'][:8], policy.stable_file_identity((base/'wallpaper').lstat()))

    def read_with_hook(self, path, hook):
        real_fdopen = os.fdopen

        class HookedRead:
            def __init__(self, stream):
                self.stream = stream
                self.called = False

            def __enter__(self):
                self.stream.__enter__()
                return self

            def __exit__(self, *args):
                return self.stream.__exit__(*args)

            def fileno(self):
                return self.stream.fileno()

            def read(self, amount):
                value = self.stream.read(amount)
                if value and not self.called:
                    self.called = True
                    hook()
                return value

        with patch.object(os, 'fdopen', side_effect=lambda *args, **kwargs: HookedRead(real_fdopen(*args, **kwargs))):
            return policy.read_pinned_file(str(path), 1024)

    def test_same_size_content_mutation_during_pinned_read_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'settings';original = self.cold_file(path, b'original')

            def mutate():
                path.write_bytes(b'modified')
                os.utime(path, ns=(1, original.st_mtime_ns+1))

            with self.assertRaisesRegex(AssertionError, '^file_identity_changed_during_read$'):
                self.read_with_hook(path, mutate)

    def test_path_replacement_with_identical_bytes_during_read_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'settings';replacement = Path(directory)/'new'
            self.cold_file(path, b'unchanged');self.cold_file(replacement, b'unchanged')
            # Unlinking the pinned inode can update its ctime first; either
            # descriptor or pathname identity must refuse the replacement.
            with self.assertRaisesRegex(AssertionError, '^(file|path)_identity_changed_during_read$'):
                self.read_with_hook(path, lambda: os.replace(replacement, path))

    def test_size_limit_and_final_component_symlink_are_still_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'file';path.write_bytes(b'12345')
            with self.assertRaisesRegex(AssertionError, '^file_size_envelope$'):
                policy.read_pinned_file(str(path), 4)
            link = Path(directory)/'link';link.symlink_to(path)
            with self.assertRaises(OSError):
                policy.read_pinned_file(str(link), 1024)


class FixedFailureDiagnostics(unittest.TestCase):
    def test_restoration_preserves_fixed_subprocess_diagnostics_not_exception_payload(self):
        driver=driver_module();report={}
        details={'step':'owned_units_retirement_subprocess_failed','kind':'nonzero_exit','returncode':1,
                 'stderr_bytes':42,'stderr_sha256':'a'*64}
        driver.record_restoration_failure(report,'unit_retirement',driver.Failure('captured_unit_retirement_refused',details))
        self.assertEqual(report['restoration_failures']['unit_retirement']['subprocess'],details)
        driver.record_restoration_failure(report,'state_and_pointer',ValueError('private text or path'))
        self.assertEqual(report['restoration_failures']['state_and_pointer'],{'error_class':'ValueError'})
        self.assertNotIn('private',json.dumps(report))

    def test_probe_failure_has_fixed_step_and_hash_without_process_payload(self):
        driver = driver_module()
        stderr = b'private-path-and-payload\nAssertionError: file_identity_changed_during_read\n'
        result = subprocess.CompletedProcess(['private-argv'], 1, stdout=b'private-output', stderr=stderr)
        with patch.object(driver.subprocess, 'run', return_value=result):
            with self.assertRaises(driver.Failure) as raised:
                driver.run(['private-argv'], label='release_binary_probe_failed')
        self.assertEqual(str(raised.exception), 'release_binary_probe_failed')
        self.assertEqual(raised.exception.diagnostics, {
            'step': 'release_binary_probe_failed', 'kind': 'nonzero_exit', 'returncode': 1,
            'stderr_bytes': len(stderr), 'stderr_sha256': hashlib.sha256(stderr).hexdigest(),
            'fixed_reason': 'file_identity_changed_during_read'})
        self.assertNotIn('private', json.dumps(raised.exception.diagnostics))

    def test_non_allowlisted_assertion_payload_is_never_retained(self):
        driver = driver_module()
        result = subprocess.CompletedProcess([], 1, stdout=b'', stderr=b'AssertionError: secret_field_value\n')
        with patch.object(driver.subprocess, 'run', return_value=result):
            with self.assertRaises(driver.Failure) as raised:
                driver.run([], label='settings_checkpoint_failed')
        self.assertNotIn('fixed_reason', raised.exception.diagnostics)
        self.assertNotIn('secret', json.dumps(raised.exception.diagnostics))


if __name__ == '__main__':
    unittest.main()
