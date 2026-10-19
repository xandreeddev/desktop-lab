"""Original-unit cleanup authority; temporary files and mocked private D-Bus only."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import owned_application_units as units
spec = importlib.util.spec_from_file_location('owned_intent', ROOT / 'vm/fixtures/application-intent-fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


def authority(name='lucent-application-intent-abcdef.service'):
    argv = ['/usr/bin/python3', '/owned/fixture.py', 'start-example', '/owned', 'abcdef']
    unit = {'load': 'loaded', 'active': 'active', 'transient': True,
            'executable': argv[0], 'argv': argv, 'main_pid': 202,
            'invocation': '1' * 32, 'group': '/app.slice/' + name}
    process = {'pid': 202, 'start': '42', 'group': unit['group'],
               'executable': [1, 2, 3], 'sha256': 'a' * 64}
    return name, argv, {'unit': unit, 'process': process}


class OriginalUnitAuthority(unittest.TestCase):
    def test_original_wrapper_may_exec_but_not_change_invocation_birth_or_group(self):
        name, argv, origin = authority()
        current = dict(origin['process'], executable=[4, 5, 6], sha256='b' * 64)
        with patch.object(units, 'process_snapshot', return_value=current):
            self.assertEqual(units.assert_unit_origin(origin['unit'], name, argv, origin), current)
            for changed in ({'start': '43'}, {'group': '/foreign'}, {'pid': 203}):
                with self.subTest(changed=changed), patch.object(units, 'process_snapshot',
                                                              return_value=dict(current, **changed)):
                    with self.assertRaises(AssertionError):
                        units.assert_unit_origin(origin['unit'], name, argv, origin)
            with self.assertRaises(AssertionError):
                units.assert_unit_origin(dict(origin['unit'], invocation='2' * 32), name, argv, origin)

    def test_capture_requires_original_main_pid_and_nonzero_invocation(self):
        name, argv, origin = authority()
        origin['unit']['main_pid'] = os.getpid()
        origin['process']['pid'] = os.getpid()
        with patch.object(units, 'unit_snapshot', return_value=origin['unit']), \
             patch.object(units, 'process_snapshot', return_value=origin['process']):
            self.assertEqual(units.capture_unit_origin(name, argv), origin)
            for changed in ({'main_pid': os.getpid() + 1}, {'invocation': '0' * 32}, {'argv': ['other']}):
                with self.subTest(changed=changed), patch.object(units, 'unit_snapshot',
                                                               return_value=dict(origin['unit'], **changed)):
                    with self.assertRaises(AssertionError):
                        units.capture_unit_origin(name, argv)

    def test_successor_is_refused_even_if_it_appears_inactive_after_original_stop(self):
        name, argv, original = authority()
        successor = dict(original['unit'], active='inactive', main_pid=0, group='', invocation='2' * 32)
        manager = Mock()
        bus = Mock()
        dbus = types.SimpleNamespace(SessionBus=Mock(return_value=bus), Interface=Mock(return_value=manager))
        with patch.dict(sys.modules, {'dbus': dbus}), \
             patch.object(units, 'unit_snapshot', side_effect=[original['unit'], successor]), \
             patch.object(units, 'process_snapshot', return_value=original['process']):
            with self.assertRaises(AssertionError):
                units.retire_unit(name, argv, captured=original)
        manager.StopUnit.assert_called_once_with(name, 'replace', timeout=5)

    def test_original_exec_receipt_is_frozen_again_before_stop(self):
        name, argv, origin = authority()
        current = dict(origin['process'], executable=[4, 5, 6], sha256='b' * 64)
        with patch.object(units, 'unit_snapshot', return_value=origin['unit']), \
             patch.object(units, 'process_snapshot', return_value=current), \
             patch.object(units, 'retire_unit') as retire:
            units.retire_original_unit(name, argv, origin, max_executable_bytes=512 * 1024 * 1024)
        self.assertEqual(retire.call_args.args[3], {'unit': origin['unit'], 'process': current})
        self.assertEqual(retire.call_args.args[4], 512 * 1024 * 1024)

    def test_invalid_executable_limits_are_refused_before_proc_access(self):
        for limit in (0, -1, True, 512 * 1024 * 1024 + 1):
            with self.subTest(limit=limit), self.assertRaises(AssertionError):
                units.process_snapshot(os.getpid(), max_executable_bytes=limit)


class OwnedFileAuthority(unittest.TestCase):
    def make_manifest(self, directory):
        path = directory / 'manifest.json'
        path.write_text('{"run_id":"abcdef"}')
        path.chmod(0o600)
        return path

    def test_cold_manifest_read_allows_only_atime_to_change(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            path = self.make_manifest(directory)
            os.utime(path, ns=(1, path.stat().st_mtime_ns))
            before = path.stat()
            self.assertEqual(units.owned_manifest(str(directory / 'fixture.py'), 'abcdef')[1],
                             {'run_id': 'abcdef'})
            after = path.stat()
            self.assertGreater(after.st_atime_ns, before.st_atime_ns)
            self.assertEqual(after.st_mtime_ns, before.st_mtime_ns)
            self.assertEqual(after.st_ctime_ns, before.st_ctime_ns)

    def test_actual_midread_mutation_with_restored_mtime_is_refused(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            path = self.make_manifest(directory)
            before = path.stat()
            real_fdopen = os.fdopen
            class MutatingRead:
                def __init__(self, stream): self.stream = stream
                def __enter__(self): self.stream.__enter__(); return self
                def __exit__(self, *args): return self.stream.__exit__(*args)
                def fileno(self): return self.stream.fileno()
                def read(self, count):
                    data = self.stream.read(count)
                    path.write_text('{"run_id":"ghijkl"}')
                    os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))
                    return data
            with patch.object(units.os, 'fdopen', side_effect=lambda *args: MutatingRead(real_fdopen(*args))):
                with self.assertRaises(AssertionError):
                    units.owned_manifest(str(directory / 'fixture.py'), 'abcdef')

    def test_path_replacement_after_read_is_refused(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            path = self.make_manifest(directory)
            real_fdopen = os.fdopen
            class ReplacingRead:
                def __init__(self, stream): self.stream = stream
                def __enter__(self): self.stream.__enter__(); return self
                def __exit__(self, *args): return self.stream.__exit__(*args)
                def fileno(self): return self.stream.fileno()
                def read(self, count):
                    data = self.stream.read(count)
                    path.rename(directory / 'original')
                    path.write_bytes(data)
                    return data
            with patch.object(units.os, 'fdopen', side_effect=lambda *args: ReplacingRead(real_fdopen(*args))):
                with self.assertRaises(AssertionError):
                    units.owned_manifest(str(directory / 'fixture.py'), 'abcdef')


class FixtureRetirement(unittest.TestCase):
    def test_actual_successor_refusal_survives_host_fallback_and_watchdog(self):
        _, argv, origin = authority()
        admitted = dict(origin, socket=[1, 2, 3, os.getuid()])
        for changed_unit, changed_process in ((dict(origin['unit'], invocation='2' * 32), origin['process']),
                                               (origin['unit'], dict(origin['process'], start='43'))):
            with self.subTest(changed_unit=changed_unit, changed_process=changed_process), \
                 tempfile.TemporaryDirectory() as name:
                directory = Path(name)
                fixture.private(directory / 'events.lock', b'')
                fixture.private(directory / 'events.json', b'[]')
                fixture.original_receipt(directory, 'example-origin.json', origin)
                fixture.original_receipt(directory, 'example-admission.json', admitted)
                dbus = types.SimpleNamespace(SessionBus=Mock())
                with patch.dict(sys.modules, {'dbus': dbus}), \
                     patch.object(fixture, 'context', return_value=(directory, {})), \
                     patch.object(fixture, 'owned_argv', return_value=argv), \
                     patch.object(fixture, 'unit_snapshot', return_value=changed_unit), \
                     patch.object(units, 'unit_snapshot', return_value=changed_unit), \
                     patch.object(units, 'process_snapshot', return_value=changed_process), \
                     patch.object(fixture, 'capture_unit_origin', return_value=origin), \
                     patch.object(fixture.signal, 'alarm'), patch.object(fixture.time, 'sleep'):
                    with self.assertRaises(AssertionError):
                        fixture.retire_example(directory, 'abcdef', admitted)
                    with self.assertRaises(ValueError):
                        fixture.retire(directory, 'abcdef')
                    with self.assertRaises(ValueError):
                        fixture.watchdog(directory, 'abcdef')
                dbus.SessionBus.assert_not_called()

    def test_admitted_fallback_and_expiry_reuse_identical_socket_process_receipt(self):
        name, argv, origin = authority()
        admitted = copy.deepcopy(origin)
        admitted['process']['sha256'] = 'b' * 64  # The original process exec'd the Rust release.
        admitted['socket'] = [1, 2, 3, os.getuid()]
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            fixture.private(directory / 'events.lock', b'')
            fixture.private(directory / 'events.json', b'[]')
            fixture.original_receipt(directory, 'example-origin.json', origin)
            fixture.original_receipt(directory, 'example-admission.json', admitted)
            with patch.object(fixture, 'context', return_value=(directory, {})), \
                 patch.object(fixture, 'owned_argv', return_value=argv), \
                 patch.object(fixture, 'unit_snapshot', return_value=origin['unit']), \
                 patch.object(fixture, 'assert_unit_origin'), \
                 patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(directory)}), \
                 patch.object(fixture, 'retire_unit', side_effect=AssertionError('successor')) as stop:
                with self.assertRaises(AssertionError):
                    fixture.retire_example(directory, 'abcdef', admitted)
                with self.assertRaises(ValueError):
                    fixture.retire(directory, 'abcdef')
                # Independent expiry repeats exactly the same captured refusal.
                with patch.object(fixture, 'capture_unit_origin', return_value=origin), \
                     patch.object(fixture.signal, 'alarm'), patch.object(fixture.time, 'sleep'):
                    with self.assertRaises(ValueError):
                        fixture.watchdog(directory, 'abcdef')
                self.assertEqual(stop.call_count, 8)
                for call in stop.call_args_list:
                    self.assertEqual(call.kwargs['captured'], admitted)

    def test_lost_start_reply_uses_durable_origin_and_never_adopts_later_unit(self):
        _, argv, origin = authority()
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            fixture.original_receipt(directory, 'example-origin.json', origin)
            with patch.object(fixture, 'context', return_value=(directory, {})), \
                 patch.object(fixture, 'owned_argv', return_value=argv), \
                 patch.object(fixture, 'unit_snapshot', return_value=origin['unit']), \
                 patch.object(fixture, 'retire_original_unit') as stop:
                fixture.retire_example(directory, 'abcdef')
                self.assertEqual(stop.call_args.args[2], origin)
                with self.assertRaises(ValueError):
                    fixture.original_receipt(directory, 'example-origin.json',
                                             dict(origin, unit=dict(origin['unit'], invocation='2' * 32)))
                self.assertEqual(fixture.original_receipt(directory, 'example-origin.json'), origin)

    def test_loaded_unit_without_original_receipt_cannot_be_adopted_for_cleanup(self):
        _, argv, origin = authority()
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            with patch.object(fixture, 'context', return_value=(directory, {})), \
                 patch.object(fixture, 'owned_argv', return_value=argv), \
                 patch.object(fixture, 'unit_snapshot', return_value=origin['unit']), \
                 patch.object(fixture, 'retire_original_unit') as stop:
                with self.assertRaises(FileNotFoundError):
                    fixture.retire_example(directory, 'abcdef')
                stop.assert_not_called()

    def test_browser_retirement_carries_original_pre_exec_invocation(self):
        name, argv, origin = authority('run-owned.service')
        event = {'cgroup': origin['unit']['group'], 'operation': 'open', 'uris': [], 'origin': origin}
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            fixture.private(directory / 'events.lock', b'')
            with patch.object(fixture, 'context', return_value=(directory, {})), \
                 patch.object(fixture, 'events', return_value=[event]), \
                 patch.object(fixture, 'retire_example'), \
                 patch.object(fixture, 'retire_original_unit') as stop:
                fixture.retire(directory, 'abcdef')
                self.assertEqual(stop.call_args.args[2], origin)
                self.assertEqual(stop.call_args.kwargs['max_executable_bytes'], 512 * 1024 * 1024)

    def test_initial_presence_refuses_start_without_stopping_foreign_unit(self):
        with patch.object(fixture, 'context'), \
             patch.object(fixture, 'unit_snapshot', return_value={'load': 'loaded'}), \
             patch.object(fixture, 'retire_unit') as stop:
            with self.assertRaises(ValueError):
                fixture.initial_absence('/owned', 'abcdef')
        stop.assert_not_called()

    def test_original_receipt_is_durable_before_example_exec_and_not_replaced(self):
        _, argv, origin = authority()
        manifest = {'example_sha256': fixture.digest(b'owned binary'), 'app_id': 'owned.desktop',
                    'files': [{'name': 'one'}, {'name': 'two'}]}
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            fixture.private(directory / 'events.lock', b'')
            fixture.private(directory / 'application-intent', b'owned binary')
            def exec_observation(executable, arguments):
                self.assertEqual(fixture.original_receipt(directory, 'example-origin.json'), origin)
                self.assertEqual(arguments, [str(directory / 'application-intent'), '--app', 'owned.desktop',
                    '--action', 'Probe', '--target', str(directory / 'one'), '--target', str(directory / 'two')])
                raise RuntimeError('exec reached')
            with patch.object(fixture, 'context', return_value=(directory, manifest)), \
                 patch.object(fixture, 'owned_argv', return_value=argv), \
                 patch.object(fixture, 'capture_unit_origin', return_value=origin), \
                 patch.object(fixture.signal, 'alarm'), \
                 patch.object(fixture.os, 'execv', side_effect=exec_observation) as execute:
                with self.assertRaisesRegex(RuntimeError, 'exec reached'):
                    fixture.start_example(directory, 'abcdef')
                changed = dict(origin, unit=dict(origin['unit'], invocation='2' * 32))
                with patch.object(fixture, 'capture_unit_origin', return_value=changed):
                    with self.assertRaises(ValueError):
                        fixture.start_example(directory, 'abcdef')
                self.assertEqual(execute.call_count, 1)

    def test_legacy_name_only_entrypoint_cannot_bypass_intent_receipts(self):
        body = units.RETIRE.removeprefix(units.UNIT_IDENTITY)
        for prefix in ('lucent-application-intent-', 'lucent-application-intent-watchdog-'):
            stop = Mock()
            with patch.object(sys, 'argv', ['-c', prefix, prefix + 'abcdef.service', '/owned/application-intent', 'abcdef']):
                with self.subTest(prefix=prefix), self.assertRaises(AssertionError):
                    exec(body, dict(vars(units), retire_unit=stop))
            stop.assert_not_called()


if __name__ == '__main__':
    unittest.main()
