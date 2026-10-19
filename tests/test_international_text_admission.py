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
             patch.object(sys, 'argv', ['fixture', json.dumps(config)]), patch('builtins.print'):
            body = policy.RETIRE.removeprefix(policy.OBSERVATION_FUNCTIONS)
            if replaced:
                with self.assertRaisesRegex(AssertionError, 'only_exact_owned_retired_socket'):
                    exec(body, namespace)
            else:
                exec(body, namespace)
        return events, path

    def test_cleanup_retires_example_first_and_only_then_peer_and_its_exact_socket(self):
        events, path = self.retirement()
        self.assertTrue(events[0].startswith('lucent-international-test-'))
        self.assertTrue(events[1].startswith('lucent-international-peer-'))
        path.unlink.assert_called_once_with()

    def test_replaced_socket_is_preserved_and_cleanup_failure_stays_explicit(self):
        events, path = self.retirement(replaced=True)
        self.assertEqual(len(events), 2)
        path.unlink.assert_not_called()

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
