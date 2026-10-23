"""Pure policy and owned local temporary files; no VM/compositor/network/build."""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import struct
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import runtime_output_policy as p


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec); spec.loader.exec_module(value)
    return value


guard = module('test_resource_output_guard', ROOT / 'vm/runtime_output_guard.py')
controller = module('test_resource_output_controller', ROOT / 'vm/test-runtime-output.py')


def monitor():
    return {**p.BASELINE, 'id': 1, 'description': 'QEMU Virtual-1', 'disabled': False,
            'dpmsStatus': True, 'availableModes': ['1920x1080@60.00Hz', '3840x2160@60.00Hz']}


def rendered():
    return {'frame_profiling': False, 'presentation_feedback': {'enabled': False},
        'surfaces': [{'id': 'background [Virtual-1]', 'role': 'background', 'instance': 'monitor-1',
            'output': 'Virtual-1', 'width': 2560, 'height': 1440, 'scale': 2, 'density': 1.5,
            'buffer_width': 3840, 'buffer_height': 2160, 'frames': 4, 'native_frames': 4}],
        'client': {'monitor_hosts': [{'id': 'Virtual-1', 'instance': 'monitor-1', 'generation': 7}],
            'live_wallpaper': {'players': 1, 'running': 1, 'awaiting_first_frame': False,
                'producers': [{'revision': 4, 'target': [3840, 2160], 'frame_size': [3840, 2160],
                    'source_aspect': [3840, 2160], 'source_frames': None}]}}}


class FakeBackend:
    def __init__(self, config=None):
        self.config = config; self.calls = []; self.records = []
        self.current = {'monitor': monitor(), 'off': True, 'compositor': ['pid-10', 'birth-5', 'socket-20'],
            'service': 'invocation-a', 'supervisor': ['pid-11', 'birth-6'],
            'process': ['pid-12', 'birth-7', p.DESKTOP_SHA], 'global': 9,
            'host': {'id': 'Virtual-1', 'instance': 'monitor-1', 'generation': 7},
            'source_proof': {'settings': 'original', 'security': 'original'},
            'cursor': {'x': 300, 'y': 100}, 'workspace': {'active': {'id': 1}, 'special': {}}}
        self.scene = rendered(); self.mutate_then_fail = False; self.fail_before_mutation = False
        self.clock = 0

    def now(self):
        return self.clock

    def observe(self, *, full, idle):
        value = copy.deepcopy(self.current)
        if self.config:
            meta = self.config.stat()
            value['source_proof']['config'] = [meta.st_dev, meta.st_ino, stat.S_IMODE(meta.st_mode),
                                             hashlib.sha256(self.config.read_bytes()).hexdigest()]
        return value

    def record(self, phase, value):
        self.records.append((phase, copy.deepcopy(value)))

    def configure(self, value, *, expected):
        p.same_output(self.current['monitor'], monitor(), expected)
        self.calls.append(p.mode_code(value))
        if self.fail_before_mutation:
            raise TimeoutError('lost before command')
        self.current['monitor'] = {**value, 'availableModes': monitor()['availableModes']}
        if self.mutate_then_fail:
            self.mutate_then_fail = False
            raise TimeoutError('lost after native commit')

    def settle(self, value, deadline):
        p.same_output(self.current['monitor'], value, value)

    def geometry(self):
        return p.ready_geometry(self.scene)


class OutputOwnership(unittest.TestCase):
    def test_baseline_requires_exact_advertised_mode_and_one_vm_output(self):
        self.assertEqual(p.profile(p.original([monitor()])), p.BASELINE)
        for change in ('name', 'scale', 'mode', 'count'):
            values = [monitor()]
            if change == 'name': values[0]['name'] = 'DP-1'
            if change == 'scale': values[0]['scale'] = 1.5
            if change == 'mode': values[0]['availableModes'] = ['3840x2160@59.94Hz']
            if change == 'count': values.append(monitor())
            with self.subTest(change=change), self.assertRaises(p.Blocked): p.original(values)

    def test_normal_apply_and_cleanup_preserve_config_bytes_inode_and_mode(self):
        with tempfile.TemporaryDirectory() as raw:
            config = Path(raw) / 'monitors.lua'; config.write_bytes(b'caller Lua\n'); config.chmod(0o640)
            before = config.stat()
            backend = FakeBackend(config); lease = p.OutputLease(backend, 10)
            lease.apply(11); facts = lease.facts(12); result = lease.cleanup()
            self.assertTrue(facts['read_only']); self.assertFalse(facts['measured'])
            self.assertEqual(facts['expected_monitor'], p.PROFILE)
            self.assertTrue(result['output_restored']); self.assertTrue(result['cursor_unchanged'])
            self.assertEqual(backend.calls, [p.mode_code(lease.target), p.mode_code(lease.original)])
            after = config.stat()
            self.assertEqual((before.st_dev, before.st_ino, before.st_mode), (after.st_dev, after.st_ino, after.st_mode))
            self.assertEqual(config.read_bytes(), b'caller Lua\n')

    def test_apply_is_never_repeated_after_lost_native_reply(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0); backend.mutate_then_fail = True
        with self.assertRaises(TimeoutError): lease.apply(1)
        with self.assertRaises(p.Blocked): lease.apply(2)
        self.assertTrue(lease.cleanup()['output_restored'])
        self.assertEqual(len(backend.calls), 2)

    def test_lost_reply_before_native_commit_does_not_issue_restore(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0); backend.fail_before_mutation = True
        with self.assertRaises(TimeoutError): lease.apply(1)
        self.assertTrue(lease.cleanup()['output_restored'])
        self.assertEqual(len(backend.calls), 1)

    def test_lost_restore_reply_is_reconciled_readonly_without_second_command(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0); lease.apply(1)
        backend.mutate_then_fail = True
        with self.assertRaises(TimeoutError): lease.cleanup()
        self.assertTrue(lease.cleanup()['output_restored'])
        self.assertEqual(len(backend.calls), 2)

    def test_expiry_reserves_cleanup_and_does_not_extend_after_requests(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 10); lease.apply(11)
        deadline = lease.deadline
        for now in (12, 120, deadline - p.CLEANUP_RESERVE - .001):
            lease.facts(now); self.assertEqual(lease.deadline, deadline)
        with self.assertRaises(p.Blocked): lease.facts(deadline - p.CLEANUP_RESERVE)
        self.assertTrue(lease.cleanup()['output_restored'])
        other = p.OutputLease(FakeBackend(), 10)
        with self.assertRaises(p.Blocked): other.apply(other.deadline - p.CLEANUP_RESERVE)
        self.assertEqual(other.backend.calls, [])

    def test_foreign_output_layout_or_lifetime_is_preserved_without_native_command(self):
        for key, changed in [('id', 2), ('name', 'DP-1'), ('description', 'replacement'),
                             ('scale', 2), ('width', 2560), ('refreshRate', 59.94), ('x', 20)]:
            backend = FakeBackend(); lease = p.OutputLease(backend, 0); lease.apply(1)
            backend.current['monitor'][key] = changed
            before = copy.deepcopy(backend.current)
            with self.subTest(key=key), self.assertRaises(p.Blocked): lease.cleanup()
            self.assertEqual(backend.current, before); self.assertEqual(len(backend.calls), 1)

    def test_changed_source_security_config_and_runtime_are_not_adopted(self):
        for key in (*p.INVARIANTS, 'source_proof', 'off'):
            backend = FakeBackend(); lease = p.OutputLease(backend, 0); lease.apply(1)
            backend.current[key] = False if key == 'off' else {'foreign': 'replacement'}
            with self.subTest(key=key), self.assertRaises(p.Blocked): lease.cleanup()
            self.assertEqual(len(backend.calls), 1)

    def test_equal_bytes_in_new_config_inode_are_foreign(self):
        with tempfile.TemporaryDirectory() as raw:
            config = Path(raw) / 'monitors.lua'; config.write_bytes(b'caller')
            backend = FakeBackend(config); lease = p.OutputLease(backend, 0); lease.apply(1)
            other = Path(raw) / 'replacement'; other.write_bytes(config.read_bytes()); os.replace(other, config)
            captured = config.stat()
            with self.assertRaises(p.Blocked): lease.cleanup()
            self.assertEqual(config.stat().st_ino, captured.st_ino); self.assertEqual(len(backend.calls), 1)

    def test_cursor_or_workspace_change_is_reported_and_never_warped_or_replayed(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0); lease.apply(1)
        backend.current['cursor'] = {'x': 500, 'y': 400}
        backend.current['workspace'] = {'active': {'id': 2}, 'special': {}}
        result = lease.cleanup()
        self.assertFalse(result['cursor_unchanged']); self.assertFalse(result['workspace_unchanged'])
        self.assertEqual(backend.current['cursor'], {'x': 500, 'y': 400})
        self.assertFalse(result['input_synthesized']); self.assertFalse(result['windows_closed'])
        self.assertFalse(result['runtime_restarted'])

    def test_same_target_present_before_our_attempt_is_not_owned(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0)
        backend.current['monitor'] = copy.deepcopy(lease.target)
        with self.assertRaises(p.Blocked): lease.cleanup()
        self.assertEqual(backend.calls, [])

    def test_authority_changed_during_geometry_never_becomes_an_active_proof(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0)
        def changed_geometry():
            backend.current['global'] = 42
            return p.ready_geometry(rendered())
        backend.geometry = changed_geometry
        with self.assertRaises(p.Blocked): lease.apply(1)
        self.assertNotEqual(lease.state, 'active')
        with self.assertRaises(p.Blocked): lease.cleanup()
        self.assertEqual(len(backend.calls), 1)

    def test_facts_revalidate_authority_after_geometry_before_returning(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0); lease.apply(1)
        def changed_geometry():
            backend.current['monitor']['width'] = 2560
            return p.ready_geometry(rendered())
        backend.geometry = changed_geometry
        with self.assertRaises(p.Blocked): lease.facts(2)

    def test_slow_source_proof_cannot_consume_cleanup_reserve_then_mutate(self):
        backend = FakeBackend(); lease = p.OutputLease(backend, 0)
        original_observe = backend.observe
        def slow(**kwargs):
            backend.clock = lease.deadline - p.CLEANUP_RESERVE
            return original_observe(**kwargs)
        backend.observe = slow
        with self.assertRaises(p.Blocked): lease.apply(1)
        self.assertFalse(lease.attempted); self.assertEqual(backend.calls, [])


class PhysicalGeometry(unittest.TestCase):
    def test_fractional_density_is_independent_of_integer_surface_scale(self):
        scene = rendered(); self.assertEqual(scene['surfaces'][0]['scale'], 2)
        value = p.ready_geometry(scene)
        self.assertEqual(value['background']['density'], 1.5)

    def test_four_k_source_alone_is_not_four_k_output_or_frame_acceptance(self):
        for key, value in [('width', 1920), ('height', 1080), ('density', 1),
                           ('buffer_width', 2560), ('buffer_height', 1440), ('native_frames', 0)]:
            scene = rendered(); scene['surfaces'][0][key] = value
            with self.subTest(key=key), self.assertRaises(p.Blocked): p.ready_geometry(scene)
        for key in ('target', 'frame_size', 'source_aspect'):
            scene = rendered(); scene['client']['live_wallpaper']['producers'][0][key] = [1920, 1080]
            with self.subTest(key=key), self.assertRaises(p.Blocked): p.ready_geometry(scene)

    def test_foreign_instance_duplicate_background_and_tracing_are_rejected(self):
        for changed in ('instance', 'duplicate', 'presentation', 'frame_profile', 'source_pts'):
            scene = rendered()
            if changed == 'instance': scene['surfaces'][0]['instance'] = 'replacement'
            if changed == 'duplicate': scene['surfaces'].append(copy.deepcopy(scene['surfaces'][0]))
            if changed == 'presentation': scene['presentation_feedback']['enabled'] = True
            if changed == 'frame_profile': scene['frame_profiling'] = True
            if changed == 'source_pts': scene['client']['live_wallpaper']['producers'][0]['source_frames'] = {}
            with self.subTest(changed=changed), self.assertRaises(p.Blocked): p.ready_geometry(scene)

    def test_only_two_literal_non_destructive_mode_commands_exist(self):
        for value in (monitor(), {**monitor(), **p.PROFILE}):
            code = p.mode_code(value)
            self.assertTrue(code.startswith('hl.monitor({output="Virtual-1"'))
            self.assertNotIn('dispatch', code); self.assertNotIn('remove', code)
        for key, value in [('name', 'DP-1'), ('scale', 2), ('x', 10), ('transform', 1)]:
            bad = monitor(); bad[key] = value
            with self.subTest(key=key), self.assertRaises(p.Blocked): p.mode_code(bad)


class PrivateAuthority(unittest.TestCase):
    def test_import_and_default_are_inert_and_presentation_refuses_before_io(self):
        with (patch.object(controller, 'qualify', side_effect=AssertionError('no qualification')),
              patch.object(controller, 'common_root', side_effect=AssertionError('no filesystem'))):
            for argv in (['test-runtime-output.py'], ['test-runtime-output.py', '--mode', 'presentation', '--begin']):
                with patch.object(sys, 'argv', argv), patch('sys.stdout', new_callable=io.StringIO) as output:
                    if '--begin' in argv:
                        with self.assertRaisesRegex(ValueError, 'presentation_requires'): controller.main()
                    else:
                        self.assertEqual(controller.main(), 0)
                        self.assertFalse(json.loads(output.getvalue())['guest_actions'])

    def test_bundle_source_change_and_symlink_are_rejected_without_execution(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw) / ('lucent-output-4k.' + 'a' * 24); directory.mkdir(mode=0o700)
            source = directory / 'runtime_output_guard.py'; source.write_bytes(b'owned'); source.chmod(0o600)
            bundle = {'tag': 'a' * 24, 'sources': {source.name: hashlib.sha256(b'owned').hexdigest()}}
            packet = directory / 'bundle.json'; guard.private(packet, bundle)
            digest = hashlib.sha256(packet.read_bytes()).hexdigest()
            self.assertEqual(guard.context(directory, digest)[1], bundle)
            source.write_bytes(b'foreign')
            with self.assertRaisesRegex(ValueError, 'source_changed'): guard.context(directory, digest)
            source.unlink(); source.symlink_to(packet)
            with self.assertRaises(OSError): guard.context(directory, digest)

    def test_receipt_replacement_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / 'receipt.json'
            with path.open('w+') as stream:
                value = os.fstat(stream.fileno()); expected = (value.st_dev, value.st_ino, value.st_uid)
                replacement = Path(raw) / 'foreign'; replacement.write_text('foreign'); os.replace(replacement, path)
                with self.assertRaises(ValueError): controller.update(stream, path, expected, {'changed': True})
                self.assertEqual(path.read_text(), 'foreign')

    def test_native_mode_adapter_only_calls_the_literal_monitor_api(self):
        backend = object.__new__(guard.Backend)
        backend.p = p; backend.verify_sources = Mock(); backend.reference_metadata = {}
        backend.reference = {'critical': {}}; backend.initial_compositor = 'compositor'; backend.initial_supervisor = 'supervisor'
        backend.first_observation = FakeBackend().current
        scene = rendered(); scene['outputs'] = [{'name': 'Virtual-1', 'global': 9}]
        backend.base = types.SimpleNamespace(preserved_files=Mock(return_value={}),
            compositor=Mock(return_value='compositor'), supervisor=Mock(return_value='supervisor'), inspect=Mock(return_value=scene),
            hypr=Mock(side_effect=lambda *argv: json.dumps([monitor()]) if argv == ('-j', 'monitors') else 'ok'))
        backend.configure({**monitor(), **p.PROFILE}, expected=p.monitor_signature(monitor()))
        self.assertEqual(backend.base.hypr.call_args_list,
            [unittest.mock.call('-j', 'monitors'), unittest.mock.call('eval', p.mode_code({**monitor(), **p.PROFILE}))])
        backend.base.hypr.reset_mock(); backend.base.compositor.return_value = 'foreign'
        with self.assertRaises(ValueError): backend.configure(monitor(), expected=p.monitor_signature(monitor()))
        backend.base.hypr.assert_not_called()

    def test_large_preservation_hash_has_its_explicit_45s_phase_bound(self):
        backend = object.__new__(guard.Backend)
        backend.reference = {'service': {'id': 'original'}, 'process': {'pid': 10}, 'supervisor': {'pid': 11},
                             'critical': {}, 'preservation': {}, 'mount_authority': {}}
        backend.units = types.SimpleNamespace(unit_snapshot=lambda *_: {'id': 'original'},
                                             process_snapshot=lambda pid, *_: {'pid': pid})
        stamps = {'asset': {'sha256': 'video'}, 'awake': {'sha256': 'marker'}}
        backend.asset = Path('asset'); backend.asset_stamp = stamps['asset']
        backend.awake_path = Path('awake'); backend.awake_stamp = stamps['awake']
        checked = Mock(return_value=b'{}')
        backend.worker = types.SimpleNamespace(checked=checked, mount_authority=lambda: {},
            file_stamp=lambda path, *_: stamps[str(path)])
        def fingerprint(command):
            command('/usr/bin/python3', '-c', 'read_only_fixture')
            return {}
        backend.preservation = types.SimpleNamespace(fingerprint=fingerprint)
        self.assertEqual(backend.source_proof()['preservation'], {})
        checked.assert_called_once_with(['/usr/bin/python3', '-c', 'read_only_fixture'], 45, limit=4 * 1024 * 1024)

    def test_status_during_cleanup_never_waits_on_the_non_accepting_server(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw); digest = 'f' * 64
            guard.private(directory / 'ready.json', {'bundle_sha256': digest, 'guardian': {'owned': True},
                                                     'pid': os.getpid(), 'deadline': 900})
            guard.private(directory / 'journal-0003.json', {'bundle_sha256': digest, 'phase': 'restore-intent'})
            units = types.SimpleNamespace(unit_snapshot=Mock(return_value={}), assert_unit_origin=Mock())
            with (patch.object(guard, 'context', return_value=(directory, {'tag': 'a' * 24})),
                  patch.object(guard, 'module', return_value=units),
                  patch.object(guard.socket, 'socket', side_effect=AssertionError('no socket request during cleanup'))):
                value = guard.request(directory, digest, 'status')
            self.assertEqual(value['status']['phase'], 'restore-intent')
            self.assertTrue(value['status']['read_only']); units.assert_unit_origin.assert_called_once()

    def test_existing_process_timer_is_not_adopted_or_replaced(self):
        with (patch.object(guard.signal, 'getitimer', return_value=(1.0, 0.0)),
              patch.object(guard, 'Backend', side_effect=AssertionError('no guest admission')),
              patch.object(guard.signal, 'signal') as handler, patch.object(guard.signal, 'setitimer') as timer):
            with self.assertRaisesRegex(ValueError, 'fresh_guard_process_without_timer'):
                guard.serve(Path('/unused'), 'f' * 64)
            handler.assert_not_called(); timer.assert_not_called()


class IndependentGuard(unittest.TestCase):
    def exercise(self, mode, timer_fault=None):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw); clock = [0.0]
            backend = FakeBackend(); backend.directory = directory; backend.p = p
            backend.bundle = {'tag': 'a' * 24}
            backend.owner_units = types.SimpleNamespace(capture_unit_origin=Mock(
                return_value={'process': {'pid': os.getpid()}}))
            handlers = {}; timer_calls = []
            def handler(number, action):
                if timer_fault == 'initial_handler' and number == guard.signal.SIGALRM and number not in handlers:
                    raise RuntimeError('private timer installation failure')
                handlers[number] = action
            def timer(kind, seconds):
                timer_calls.append(seconds)
                if timer_fault == 'initial_arm' and len(timer_calls) == 1:
                    raise OSError('private initial arming failure')
                if timer_fault == 'cleanup_arm' and len(timer_calls) == 2:
                    raise TimeoutError('pending expiry or arming failure')
                if timer_fault == 'cleanup_arm_foreign' and len(timer_calls) == 2:
                    backend.current['monitor']['width'] = 2560
                    raise TimeoutError('arming failure with foreign postimage')
                if timer_fault == 'pending_old_expiry' and len(timer_calls) == 2:
                    handlers[guard.signal.SIGALRM](guard.signal.SIGALRM, None)
                if timer_fault == 'pending_at_cancel' and seconds == 0:
                    clock[0] = 1100
                    handlers[guard.signal.SIGALRM](guard.signal.SIGALRM, None)
                if timer_fault == 'final_cancel' and seconds == 0:
                    raise OSError('private timer cancellation failure')
            observed = backend.observe
            def observe(**kwargs):
                if timer_fault == 'cleanup_expires' and len(timer_calls) >= 2 and len(backend.calls) == 1:
                    clock[0] = 1020
                    handlers[guard.signal.SIGALRM](guard.signal.SIGALRM, None)
                return observed(**kwargs)
            backend.observe = observe
            class Peer:
                def __enter__(self): return self
                def __exit__(self, *_): return False
                def getsockopt(self, *_): return struct.pack('3i', os.getpid(), os.getuid(), os.getgid())
                def settimeout(self, *_): pass
                def recv(self, *_):
                    if mode == 'slow':
                        clock[0] = 900
                        return b' '
                    return json.dumps({'operation': 'apply', 'bundle_sha256': 'f' * 64}).encode() + b'\n'
                def sendall(self, *_):
                    if mode == 'lost_reply': raise BrokenPipeError()
                    clock[0] = 900
            class Server:
                def __enter__(self): return self
                def __exit__(self, *_): return False
                def bind(self, path): Path(path).touch(mode=0o600)
                def listen(self, *_): pass
                def settimeout(self, *_): pass
                def accept(self): return Peer(), None
            with (patch.object(guard, 'Backend', return_value=backend),
                  patch.object(guard.time, 'monotonic', side_effect=lambda: clock[0]),
                  patch.object(guard.socket, 'socket', return_value=Server()),
                  patch.object(guard.signal, 'signal', side_effect=handler),
                  patch.object(guard.signal, 'setitimer', side_effect=timer),
                  patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(directory)})):
                guard.serve(directory, 'f' * 64)
            result = json.loads((directory / 'finished.json').read_text())
            return result, backend.calls

    def test_absolute_expiry_runs_guest_cleanup_after_host_stops_sending_requests(self):
        result, calls = self.exercise('expiry')
        self.assertTrue(result['expired']); self.assertTrue(result['cleanup_success'])
        self.assertEqual(len(calls), 2)
        self.assertIn('3840x2160@60', calls[0]); self.assertIn('1920x1080@60', calls[1])

    def test_lost_apply_reply_ends_lease_and_restores_without_a_second_apply(self):
        result, calls = self.exercise('lost_reply')
        self.assertEqual(result['primary_failure_class'], 'output_reply_lost')
        self.assertTrue(result['cleanup_success']); self.assertEqual(len(calls), 2)

    def test_slow_incomplete_request_cannot_extend_deadline_or_trigger_mutation(self):
        result, calls = self.exercise('slow')
        self.assertTrue(result['expired']); self.assertTrue(result['cleanup_success'])
        self.assertEqual(calls, [])

    def test_cleanup_arming_failure_still_attempts_exactly_one_restore_and_writes_receipt(self):
        result, calls = self.exercise('expiry', 'cleanup_arm')
        self.assertFalse(result['cleanup_success']); self.assertTrue(result['restoration']['output_restored'])
        self.assertEqual(len(calls), 2); self.assertEqual(result['timer_errors'][0]['phase'], 'cleanup_arm')

    def test_actual_pending_old_handler_at_cleanup_arm_does_not_abort_new_budget(self):
        result, calls = self.exercise('expiry', 'pending_old_expiry')
        self.assertTrue(result['cleanup_success']); self.assertEqual(len(calls), 2)

    def test_final_timer_cancel_failure_keeps_verified_restore_and_failure_receipt(self):
        result, calls = self.exercise('expiry', 'final_cancel')
        self.assertFalse(result['cleanup_success']); self.assertTrue(result['restoration']['output_restored'])
        self.assertEqual(len(calls), 2); self.assertEqual(result['timer_errors'][0]['phase'], 'final_cancel')

    def test_initial_handler_failure_has_no_native_mutation_but_records_failed_lifecycle(self):
        result, calls = self.exercise('expiry', 'initial_handler')
        self.assertFalse(result['cleanup_success']); self.assertTrue(result['restoration']['output_restored'])
        self.assertEqual(calls, []); self.assertEqual(result['timer_errors'][0]['phase'], 'initial_handler_alrm')

    def test_initial_arm_failure_has_no_native_mutation_and_keeps_a_failure_receipt(self):
        result, calls = self.exercise('expiry', 'initial_arm')
        self.assertFalse(result['cleanup_success']); self.assertTrue(result['restoration']['output_restored'])
        self.assertEqual(calls, []); self.assertEqual(result['timer_errors'][0]['phase'], 'initial_arm')

    def test_current_cleanup_deadline_still_aborts_uncompleted_source_proof(self):
        result, calls = self.exercise('expiry', 'cleanup_expires')
        self.assertFalse(result['cleanup_success']); self.assertNotIn('restoration', result)
        self.assertEqual(result['failure_class'], 'TimeoutError'); self.assertEqual(len(calls), 1)

    def test_pending_alarm_during_final_cancellation_cannot_skip_the_receipt(self):
        result, calls = self.exercise('expiry', 'pending_at_cancel')
        self.assertTrue(result['cleanup_success']); self.assertEqual(len(calls), 2)

    def test_arming_failure_never_grants_a_foreign_native_postimage_restore_authority(self):
        result, calls = self.exercise('expiry', 'cleanup_arm_foreign')
        self.assertFalse(result['cleanup_success']); self.assertNotIn('restoration', result)
        self.assertEqual(result['failure_class'], 'Blocked'); self.assertEqual(len(calls), 1)


if __name__ == '__main__':
    unittest.main()
