"""Pure policy and owned temporary files only; no VM, compositor, bus or browser."""
import contextlib
import ast
import importlib.util
import io
import json
import os
from pathlib import Path
import socket
import stat
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import default_uri_policy as p


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


driver = module('default_uri_fixture_test', ROOT / 'vm/fixtures/default-uri-fixture.py')
controller = module('default_uri_controller_test', ROOT / 'vm/test-default-uri-notification.py')


class ConfigurationLease(unittest.TestCase):
    def fixture(self, root, original=b'original\n'):
        run = root / 'run'
        run.mkdir(mode=0o700)
        config = root / 'mimeapps.list'
        if original is not None:
            config.write_bytes(original)
            config.chmod(0o640)
        value = p.plan(run, 'mime', config, b'owned candidate\n')
        return run, config, value

    def test_existing_file_restores_exact_bytes_mode_inode_and_sibling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run, config, value = self.fixture(root)
            sibling = root / 'untouched'
            sibling.write_bytes(b'caller')
            p.publish(value)
            self.assertEqual(config.read_bytes(), b'owned candidate\n')
            # Recreate the observer after publication: no controller reply or
            # in-memory "published" flag is needed for independent recovery.
            p.restore(p.lease(run, 'mime'))
            self.assertEqual(p.snapshot(config), value['original'])
            self.assertEqual(config.read_bytes(), b'original\n')
            self.assertEqual(stat.S_IMODE(config.stat().st_mode), 0o640)
            self.assertEqual(sibling.read_bytes(), b'caller')
            p.restore(value)

    def test_original_absence_is_restored_without_removing_siblings(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, config, value = self.fixture(root, None)
            sibling = root / 'caller'
            sibling.write_bytes(b'untouched')
            p.publish(value)
            p.restore(value)
            self.assertFalse(os.path.lexists(config))
            self.assertEqual(sibling.read_bytes(), b'untouched')

    def test_never_published_intent_is_inert_for_existing_and_absent_targets(self):
        for original in (None, b'original\n'):
            with tempfile.TemporaryDirectory() as temporary:
                _, config, value = self.fixture(Path(temporary), original)
                before = p.snapshot(config)
                p.restore(value)
                self.assertEqual(p.snapshot(config), before)

    def test_foreign_content_or_identical_new_inode_is_preserved(self):
        for changed in (b'owned candidate\n', b'foreign preferences\n'):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                _, config, value = self.fixture(root)
                p.publish(value)
                replacement = root / 'replacement'
                replacement.write_bytes(changed)
                os.replace(replacement, config)
                before = p.snapshot(config)
                with self.assertRaisesRegex(ValueError, 'independent_configuration_change_preserved'):
                    p.restore(value)
                self.assertEqual(p.snapshot(config), before)

    def test_foreign_file_appearing_at_originally_absent_path_is_not_removed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, config, value = self.fixture(root, None)
            p.publish(value)
            config.unlink()
            config.write_bytes(b'caller created this')
            with self.assertRaises(ValueError):
                p.restore(value)
            self.assertEqual(config.read_bytes(), b'caller created this')

    def test_publish_refuses_changed_preimage_or_original_symlink_and_hardlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, config, value = self.fixture(root)
            config.write_bytes(b'independent')
            with self.assertRaisesRegex(ValueError, 'publication_preimage_changed'):
                p.publish(value)
            config.unlink()
            config.symlink_to(root / 'other')
            with self.assertRaises(OSError):
                p.snapshot(config)
            config.unlink()
            config.write_bytes(b'original')
            os.link(config, root / 'alias')
            with self.assertRaisesRegex(ValueError, 'owned_configuration_file'):
                p.snapshot(config)

    def test_mime_edit_changes_only_http_default_and_restoration_keeps_original_format(self):
        original = b'# caller formatting\n[Default Applications]\nx-scheme-handler/https=caller.desktop;\n[Added Associations]\ntext/plain=editor.desktop;\n'
        candidate = p.mime_content(original, 'org.lucent.DefaultUriabcdef123456.desktop')
        self.assertIn(b'x-scheme-handler/http=org.lucent.DefaultUriabcdef123456.desktop;', candidate)
        self.assertIn(b'x-scheme-handler/https=caller.desktop;', candidate)
        self.assertIn(b'text/plain=editor.desktop;', candidate)
        for invalid in ('org.other.App.desktop', '../foreign.desktop', 'org.lucent.DefaultUriabc.desktop'):
            with self.assertRaises(ValueError):
                p.mime_content(original, invalid)

    def test_writer_racing_the_atomic_publication_is_restored_and_not_adopted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, config, value = self.fixture(root)
            actual_rename = p.rename
            raced = []
            def operation(source, destination, flags):
                if not raced:
                    replacement = root / 'independent'
                    replacement.write_bytes(b'foreign concurrent configuration')
                    os.replace(replacement, config)
                    raced.append(p.snapshot(config))
                return actual_rename(source, destination, flags)
            with patch.object(p, 'rename', side_effect=operation):
                with self.assertRaisesRegex(ValueError, 'raced_configuration_publication_preserved'):
                    p.publish(value)
            self.assertEqual(p.snapshot(config), raced[0])

    def test_writer_racing_restoration_keeps_its_exact_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, config, value = self.fixture(root)
            p.publish(value)
            actual_rename = p.rename
            raced = []
            def operation(source, destination, flags):
                if not raced:
                    replacement = root / 'independent'
                    replacement.write_bytes(b'foreign concurrent configuration')
                    os.replace(replacement, config)
                    raced.append(p.snapshot(config))
                return actual_rename(source, destination, flags)
            with patch.object(p, 'rename', side_effect=operation):
                with self.assertRaisesRegex(ValueError, 'raced_configuration_exchange_preserved'):
                    p.restore(value)
            self.assertEqual(p.snapshot(config), raced[0])

    def test_original_target_with_foreign_slot_never_claims_restoration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, config, value = self.fixture(root)
            p.publish(value)
            # Model an interrupted exchange: the original inode is named again,
            # but the payload slot holds a caller update, not our candidate.
            p.rename(value['slot'], config, 2)
            slot = Path(value['slot'])
            replacement = root / 'caller-update'
            replacement.write_bytes(b'foreign caller update retained')
            os.replace(replacement, slot)
            before_target, before_slot = p.snapshot(config), p.snapshot(slot)
            self.assertEqual(before_target, value['original'])
            with self.assertRaisesRegex(ValueError, 'uncertain_configuration_restore_slot_preserved'):
                p.restore(value)
            self.assertEqual(p.snapshot(config), before_target)
            self.assertEqual(p.snapshot(slot), before_slot)


class FixtureCliDispatch(unittest.TestCase):
    def mapped_operation(self, operation, expected_handler):
        requested = Path('/unused-requested-run')
        canonical = Path('/unused-canonical-run')
        run_id = 'abcdef123456'
        result = {'operation': operation, 'dispatched': True}
        callbacks = {name: Mock(return_value=result) for name in ('prepare', 'publish', 'admitted')}
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(sys, 'argv', ['default-uri.py', operation, str(requested), run_id]))
            context = stack.enter_context(patch.object(driver, 'context', return_value=(canonical, {}, Mock())))
            stack.enter_context(patch.object(driver.signal, 'signal'))
            stack.enter_context(patch.object(subprocess, 'run', side_effect=AssertionError('native operations forbidden')))
            for name, callback in callbacks.items():
                stack.enter_context(patch.object(driver, name, callback))
            output = stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            driver.main()
        context.assert_called_once_with(requested, run_id)
        for name, callback in callbacks.items():
            if name == expected_handler:
                callback.assert_called_once_with(canonical, run_id)
            else:
                callback.assert_not_called()
        self.assertEqual(json.loads(output.getvalue()), result)

    def test_prepare_cli_dispatches_prepare_with_admitted_context(self):
        self.mapped_operation('prepare', 'prepare')

    def test_publish_cli_dispatches_publish_with_admitted_context(self):
        self.mapped_operation('publish', 'publish')

    def test_admission_cli_dispatches_admitted_with_admitted_context(self):
        self.mapped_operation('admission', 'admitted')

    def test_warm_ready_cli_keeps_readonly_dispatch_before_general_context(self):
        requested = Path('/unused-requested-run')
        run_id = 'abcdef123456'
        result = {'ready': False, 'blocked': False, 'phase': 'mapped_client'}
        with patch.object(sys, 'argv', ['default-uri.py', 'warm-ready', str(requested), run_id]), \
             patch.object(driver.signal, 'signal'), \
             patch.object(driver, 'context', side_effect=AssertionError('readonly probe owns its own admission')) as context, \
             patch.object(driver, 'warm_ready', return_value=result) as readonly, \
             patch.object(subprocess, 'run', side_effect=AssertionError('native operations forbidden')), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            driver.main()
        context.assert_not_called()
        readonly.assert_called_once_with(requested, run_id)
        self.assertEqual(json.loads(output.getvalue()), result)


class BridgeDiagnostics(unittest.TestCase):
    def packet(self, operation='admission'):
        return p.bridge_error(ValueError('actual_owned_default_handler_selected'), operation, 'handler')

    def entry_failure(self, error, context_error=False, missing_handler=False):
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(sys, 'argv', ['default-uri.py', 'admission', '/unused-run', 'abcdef123456']))
            stack.enter_context(patch.object(driver.signal, 'signal'))
            stack.enter_context(patch.object(subprocess, 'run', side_effect=AssertionError('native operations forbidden')))
            stack.enter_context(patch.object(driver, 'context', side_effect=error if context_error else None,
                                             return_value=(Path('/unused-canonical-run'), {}, Mock())))
            if missing_handler:
                stack.enter_context(patch.dict(driver.__dict__))
                del driver.__dict__['admitted']
            else:
                stack.enter_context(patch.object(driver, 'admitted', side_effect=error))
            output = stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            errors = stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
            status = driver.main()
        self.assertEqual(status, 1)
        self.assertEqual(output.getvalue(), '')
        return p.bridge_error_packet(json.loads(errors.getvalue()), 'admission')

    def test_actual_entrypoint_failure_is_nonzero_and_contains_only_fixed_labels(self):
        for error in (KeyError('private_lowercase_secret'), ValueError('private_lowercase_secret'),
                      type('private_lowercase_secret', (ValueError,), {})('private argument')):
            with self.subTest(error_class=type(error).__name__):
                packet = self.entry_failure(error)
                self.assertEqual(packet['phase'], 'handler')
                self.assertEqual(packet['fixed_reason'], 'unclassified_fixture_failure')
                self.assertNotIn('private', json.dumps(packet))
        self.assertEqual(self.entry_failure(ValueError('actual_owned_default_handler_selected'))['fixed_reason'],
                         'actual_owned_default_handler_selected')

    def test_actual_entrypoint_reports_context_and_missing_dispatch_implementation(self):
        self.assertEqual(self.entry_failure(ValueError('private_lowercase_secret'), context_error=True)['phase'], 'context')
        packet = self.entry_failure(ValueError('unused'), missing_handler=True)
        self.assertEqual(packet['phase'], 'dispatch')
        self.assertEqual(packet['error_class'], 'NameError')
        self.assertEqual(packet['fixed_reason'], 'unclassified_fixture_failure')

    def test_host_success_uses_the_original_bounded_transport_contract(self):
        result = types.SimpleNamespace(returncode=0, stdout=b'{"published":true}\n', stderr=b'')
        argv = ['owned-private-transport', 'opaque argument']
        with patch.object(controller.subprocess, 'run', return_value=result) as execute:
            self.assertEqual(controller.bridge_command(argv, 'publish'), {'published': True})
        execute.assert_called_once_with(argv, capture_output=True, timeout=30)

    def test_exact_error_packet_is_fatal_even_with_an_incorrect_zero_exit_status(self):
        packet = self.packet()
        for status in (0, 1):
            with self.subTest(status=status):
                raw = json.dumps(packet).encode()
                result = types.SimpleNamespace(returncode=status, stdout=raw if status == 0 else b'',
                                               stderr=raw if status else b'')
                with patch.object(controller.subprocess, 'run', return_value=result), \
                     self.assertRaises(controller.BridgeFailure) as caught:
                    controller.bridge_command(['unused'], 'admission')
                self.assertEqual(caught.exception.details, packet)
                self.assertEqual(str(caught.exception), 'owned_default_uri_bridge_failed')

    def test_noise_malformed_unbounded_or_untrusted_packets_cannot_become_results_or_report_data(self):
        packet = self.packet()
        raw = json.dumps(packet).encode()
        cases = [(1, b'', raw + b' private_lowercase_secret'),
                 (1, b'', raw + b'\n' + raw), (1, b'', b'private_lowercase_secret'),
                 (1, b'private_lowercase_secret', raw),
                 (1, b'', b'x' * 2049), (1, b'', b'x' * 65537),
                 (0, b'x' * (8 * 1024 * 1024 + 1), b''),
                 (0, b'{"published":true}', b'private_lowercase_secret'),
                 (0, b'private_lowercase_secret', b'')]
        for key, value in (('operation', 'security'), ('phase', 'private_lowercase_secret'),
                           ('error_class', 'private_lowercase_secret'), ('fixed_reason', 'private_lowercase_secret'),
                           ('environment', 'private_lowercase_secret')):
            cases.append((1, b'', json.dumps({**packet, key: value}).encode()))
        for index, (status, output, errors) in enumerate(cases):
            with self.subTest(case=index):
                result = types.SimpleNamespace(returncode=status, stdout=output, stderr=errors)
                with patch.object(controller.subprocess, 'run', return_value=result), \
                     self.assertRaises(controller.BridgeFailure) as caught:
                    controller.bridge_command(['unused'], 'admission')
                diagnostic = caught.exception.details
                self.assertEqual(diagnostic['phase'], 'transport')
                self.assertNotIn('private', json.dumps(diagnostic))

    def test_transport_deadlines_and_os_errors_never_relay_command_or_partial_output(self):
        for error in (subprocess.TimeoutExpired(['private_lowercase_secret'], 30, stderr=b'private stderr'),
                      OSError('private_lowercase_secret')):
            with self.subTest(error_class=type(error).__name__):
                with patch.object(controller.subprocess, 'run', side_effect=error), \
                     self.assertRaises(controller.BridgeFailure) as caught:
                    controller.bridge_command(['unused'], 'admission')
                self.assertEqual(caught.exception.details['fixed_reason'], 'bounded_bridge_transport_failed')
                self.assertNotIn('private', json.dumps(caught.exception.details))

    def test_post_warm_phase_marks_failed_action_and_first_failure_survives_cleanup(self):
        diagnostic = {}
        self.assertEqual(controller.post_warm_step(diagnostic, 'publication', lambda: 'published'), 'published')
        packet = p.bridge_error_packet(self.packet(), 'admission')
        failure = controller.BridgeFailure(packet)
        with self.assertRaises(controller.BridgeFailure):
            controller.post_warm_step(diagnostic, 'published_admission', Mock(side_effect=failure))
        self.assertEqual(diagnostic, {'phase': 'published_admission', 'completed': ['publication']})
        report = {}
        controller.record_failure(report, failure, diagnostic['phase'])
        original = json.loads(json.dumps(report))
        controller.record_failure(report, ValueError('private_lowercase_secret'), 'cleanup')
        self.assertEqual(report, original)
        self.assertEqual(report['failure']['phase'], 'published_admission')
        self.assertEqual(report['failure']['bridge'], packet)

    def test_unknown_host_labels_and_secret_looking_exception_reasons_are_not_recorded(self):
        report = {}
        controller.record_failure(report, ValueError('private_lowercase_secret'), 'private_lowercase_secret')
        self.assertEqual(report['failure'], {'error_class': 'ValueError', 'scope': 'native_default_uri_acceptance',
                                            'phase': 'setup'})
        action = Mock()
        with self.assertRaisesRegex(ValueError, 'fixed_post_warm_phase_required'):
            controller.post_warm_step({}, 'private_lowercase_secret', action)
        action.assert_not_called()


class AdmissionAndBridge(unittest.TestCase):
    def test_imports_and_default_controller_do_not_access_transport_or_native_desktop(self):
        with patch.object(subprocess, 'run', side_effect=AssertionError('transport forbidden')), \
             patch.object(subprocess, 'check_output', side_effect=AssertionError('transport forbidden')), \
             patch.object(socket, 'socket', side_effect=AssertionError('desktop forbidden')), \
             patch.object(os, 'open', side_effect=AssertionError('mutation forbidden')), \
             patch.object(Path, 'mkdir', side_effect=AssertionError('mutation forbidden')):
            value = module('inert_default_controller', ROOT / 'vm/test-default-uri-notification.py')
            module('inert_default_guest', ROOT / 'vm/fixtures/default-uri-fixture.py')
            with patch.object(sys, 'argv', ['controller']), contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(value.main(), 0)
        self.assertEqual(json.loads(output.getvalue())['guest_actions'], False)

    def test_malformed_vm_uuid_or_missing_explicit_authority_refuses_before_artifact_read(self):
        for uuid in (None, 'desktop-lab-lucent', 'a' * 36):
            args = types.SimpleNamespace(vm_uuid=uuid, expected_desktop_pid=123,
                                         manifest_sha=controller.MANIFEST_SHA, release_manifest=Path('/tmp/never'))
            with patch.object(Path, 'read_bytes', side_effect=AssertionError('no source IO')):
                with self.assertRaisesRegex(ValueError, 'exact_vm_uuid_required'):
                    controller.qualify(args)
        args.vm_uuid = 'a4691892-fa44-41d9-98cf-8f0dc4131ee3'
        args.expected_desktop_pid = None
        with self.assertRaisesRegex(ValueError, 'fresh_desktop_pid_required'):
            controller.qualify(args)

    def test_bridge_forwards_token_by_name_only_and_clears_legacy_startup(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / 'events.lock').write_bytes(b'')
            (run / 'default-plan.json').write_text(json.dumps({'http': {'targets': [{'uri': 'http://127.0.0.1:1234/owned'}]}}))
            (run / 'default-events.json').write_text('[]')
            base = Mock()
            base.atomic.side_effect = lambda path, data: path.write_bytes(data)
            with patch.object(driver, 'context', return_value=(run, {}, base)), \
                 patch.object(driver, 'admitted'), \
                 patch.dict(os.environ, {'XDG_ACTIVATION_TOKEN': 'opaque-test-authority',
                                         'DESKTOP_STARTUP_ID': 'untrusted-legacy'}), \
                 patch.object(driver.subprocess, 'run', return_value=types.SimpleNamespace(returncode=0)) as execute:
                driver.launch(run, 'abcdef123456', 'http://127.0.0.1:1234/owned')
            argv = execute.call_args.args[0]
            self.assertIn('--setenv=XDG_ACTIVATION_TOKEN', argv)
            self.assertIn('--property=UnsetEnvironment=DESKTOP_STARTUP_ID', argv)
            self.assertFalse(any('opaque-test-authority' in value or 'untrusted-legacy' in value for value in argv))
            self.assertNotIn('DESKTOP_STARTUP_ID', execute.call_args.kwargs['env'])
            self.assertEqual(json.loads((run / 'default-events.json').read_text()),
                             [{'selected_uri_matches': True, 'fresh_token_present': True, 'legacy_startup_present': True}])

    def test_warm_browser_setup_never_receives_stale_activation_or_legacy_environment(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            base = Mock()
            base.events.return_value = []
            with patch.object(driver, 'context', return_value=(run, {}, base)), patch.object(driver, 'admitted'), \
                 patch.dict(os.environ, {'XDG_ACTIVATION_TOKEN': 'stale', 'DESKTOP_STARTUP_ID': 'stale'}), \
                 patch.object(driver.subprocess, 'run', return_value=types.SimpleNamespace(returncode=0)) as execute:
                driver.launch(run, 'abcdef123456')
            argv = execute.call_args.args[0]
            self.assertIn('--property=UnsetEnvironment=XDG_ACTIVATION_TOKEN DESKTOP_STARTUP_ID', argv)
            self.assertNotIn('--setenv=XDG_ACTIVATION_TOKEN', argv)
            self.assertNotIn('XDG_ACTIVATION_TOKEN', execute.call_args.kwargs['env'])

    def test_unselected_uri_is_refused_before_starting_any_service(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / 'default-plan.json').write_text(json.dumps({'http': {'targets': [{'uri': 'http://127.0.0.1:1234/owned'}]}}))
            with patch.object(driver, 'context', return_value=(run, {}, Mock())), patch.object(driver, 'admitted'), \
                 patch.object(driver.subprocess, 'run', side_effect=AssertionError('native operation forbidden')):
                with self.assertRaisesRegex(ValueError, 'selected_owned_default_uri_required'):
                    driver.launch(run, 'abcdef123456', 'https://foreign.example/')

    def test_native_keys_refuse_intercepting_bindings(self):
        controller.keyboard_preflight([])
        for row in ({'modmask': 0, 'key': 'Tab'}, {'modmask': 0, 'keycode': 28},
                    {'modmask': 0, 'key': 'mouse:272'}, {'modmask': 0, 'key': '', 'catch_all': True}):
            with self.assertRaises(ValueError):
                controller.keyboard_preflight([row])

    def test_native_acceptance_cannot_silently_use_instrumented_runtime(self):
        value = {'frame_profiling': False, 'presentation_feedback': {'enabled': False},
                 'client': {'live_wallpaper': {'producers': [{'source_frames': None}]}}}
        controller.normal_off(value)
        for changed in ({'frame_profiling': True}, {'presentation_feedback': {'enabled': True}},
                        {'client': {'live_wallpaper': {'producers': [{'source_frames': []}]}}}):
            with self.assertRaisesRegex(ValueError, 'normal_off_instrumentation_required'):
                controller.normal_off({**value, **changed})

    def test_native_link_target_rejects_wrong_owner_buffer_output_or_notification(self):
        uri = 'http://127.0.0.1:1234/owned'
        hit = {'id': 'notification-body-7-2-0/link/' + uri, 'x': 20, 'y': 30, 'width': 120, 'height': 24,
               'clip': {'x': 0, 'y': 0, 'width': 1920, 'height': 1080}}
        surface = {'id': 'notifications', 'role': 'notifications', 'instance': None, 'output': 'Virtual-1',
                   'keyboard': 'exclusive', 'pointer_input': 'all', 'frames': 3, 'native_frames': 2,
                   'density': 1, 'width': 1920, 'height': 1080, 'buffer_width': 1920, 'buffer_height': 1080,
                   'hits': [hit], 'focus': hit['id']}
        layer = {'namespace': 'lucent-notifications', 'address': '0xabc', 'pid': 123, 'x': 0, 'y': 0,
                 'w': 1920, 'h': 1080}
        layers = {'Virtual-1': {'levels': {'3': [layer]}}}
        self.assertEqual(controller.target({'surfaces': [surface]}, layers, 7, uri, 123)['point'], (80, 42))
        for changed in ({'keyboard': 'none'}, {'output': 'foreign'}, {'density': 2}, {'buffer_width': 960}):
            with self.assertRaises(ValueError):
                controller.target({'surfaces': [{**surface, **changed}]}, layers, 7, uri, 123)
        with self.assertRaises(ValueError):
            controller.target({'surfaces': [surface]}, layers, 8, uri, 123)
        with self.assertRaises(RuntimeError):
            controller.target({'surfaces': [surface]}, layers, 7, uri, 456)


class NotificationExpiry(unittest.TestCase):
    def fixture(self, revision=2, identifier=7, count=1):
        records = [{'id': 7, 'owner': ':1.8', 'desktop_pid': 123,
                    'observed_revision': 2, 'expires_after': 0}]
        hits = [{'id': f'notification-dismiss-{identifier}-{revision}'}] if count else []
        state = {'client': {'notification_count': count},
                 'surfaces': [{'role': 'notifications', 'hits': hits}]}
        return records, state

    def run_wait(self, records, observe, times):
        bus = Mock()
        fake = types.SimpleNamespace(SessionBus=lambda **kwargs: bus, Interface=Mock(), UInt32=Mock())
        try:
            with patch.dict(sys.modules, {'dbus': fake}), patch.object(driver, 'admitted'), \
                 patch.object(driver, 'observed_notes', return_value=records), \
                 patch.object(driver, 'notification_owner', return_value=':1.8'), \
                 patch.object(driver, 'inspection', side_effect=observe), \
                 patch.object(driver.time, 'monotonic', side_effect=times), \
                 patch.object(driver.time, 'sleep'):
                result = driver.wait_expiry(Path('/tmp/unused-owned-fixture'), 'abcdef123456')
        finally:
            # Also prove the replacement/error branches performed no DBus
            # mutation before refusing; their exceptions must not skip this.
            fake.Interface.assert_not_called()
            fake.UInt32.assert_not_called()
            bus.close.assert_called_once()
        return result

    def test_same_daemon_wire_id_replacement_is_preserved_and_fails_acceptance(self):
        records, replaced = self.fixture(revision=3)
        before = json.loads(json.dumps(replaced))
        with self.assertRaisesRegex(ValueError, 'replaced_notification_revision_preserved'):
            self.run_wait(records, [replaced], [0])
        self.assertEqual(replaced, before)

    def test_replacement_after_read_can_never_lead_to_unconditional_wire_id_close(self):
        records, original = self.fixture()
        _, replacement = self.fixture(revision=3)
        with self.assertRaisesRegex(ValueError, 'replaced_notification_revision_preserved'):
            self.run_wait(records, [original, replacement], [0, 1])
        # No notification deletion operation exists to race this observation.
        for path in (ROOT / 'vm/fixtures/default-uri-fixture.py', ROOT / 'vm/test-default-uri-notification.py'):
            tree = ast.parse(path.read_text())
            self.assertFalse(any(isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
                                 and node.func.attr in ('CloseNotification', 'DismissNotification', 'RemoveNotification')
                                 for node in ast.walk(tree)))
            self.assertFalse(any(isinstance(node, ast.Call) and len(node.args) >= 2
                                 and isinstance(node.args[0], ast.Constant) and node.args[0].value == 'notifications'
                                 and isinstance(node.args[1], ast.Constant)
                                 and str(node.args[1].value).startswith(('dismiss', 'clear'))
                                 for node in ast.walk(tree)))
            self.assertNotIn('notifications dismiss', path.read_text())

    def test_foreign_remaining_notice_is_not_removed_or_treated_as_ours(self):
        records, foreign = self.fixture(identifier=9)
        with self.assertRaisesRegex(ValueError, 'foreign_notification_preserved'):
            self.run_wait(records, [foreign], [0])
        self.assertEqual(foreign['client']['notification_count'], 1)

    def test_absence_before_expiry_window_does_not_claim_the_window_elapsed(self):
        records, absent = self.fixture(count=0)
        records[0]['expires_after'] = 25
        self.assertEqual(self.run_wait(records, [absent, absent], [0, 1, 25]),
                         {'absent_after_expiry_window': True})

    def test_unidentified_remaining_notice_times_out_without_removing_it(self):
        records, hidden = self.fixture()
        hidden['surfaces'] = []
        with self.assertRaisesRegex(ValueError, 'remaining_notification_preserved_after_expiry_window'):
            self.run_wait(records, [hidden], [0, 31])
        self.assertEqual(hidden['client']['notification_count'], 1)

    def test_daemon_successor_is_never_considered_the_original_expiry_source(self):
        records, absent = self.fixture(count=0)
        fake = types.SimpleNamespace(SessionBus=lambda **kwargs: Mock())
        with patch.dict(sys.modules, {'dbus': fake}), patch.object(driver, 'admitted'), \
             patch.object(driver, 'observed_notes', return_value=records), \
             patch.object(driver, 'notification_owner', return_value=':1.9'), \
             patch.object(driver, 'inspection', return_value=absent), \
             patch.object(driver.time, 'monotonic', return_value=0):
            with self.assertRaisesRegex(ValueError, 'notification_daemon_changed_during_expiry_wait'):
                driver.wait_expiry(Path('/tmp/unused-owned-fixture'), 'abcdef123456')


class WarmStartupBarrier(unittest.TestCase):
    def fixture(self, run):
        origin = {'unit': {'group': '/owned/run-original.service'},
                  'process': {'pid': 321, 'start': '12345'}}
        declared = {'class': 'LucentIntentabcdef123456'}
        event = {'operation': 'open', 'uris': [], 'fresh_token_present': False,
                 'legacy_startup_present': False, 'origin': origin, 'cgroup': origin['unit']['group']}
        lease = {'process': {'pid': 321, 'start': '12345', 'group': origin['unit']['group']},
                 'scope': None, 'command': {'sha256': 'before-title'}}
        client = {'pid': 321, 'class': declared['class'], 'address': '0xabc',
                  'mapped': True, 'hidden': False}
        endpoint = {'port': 23456, 'browser': '/devtools/browser/owned', 'file': [1, 2]}
        control = {'id': 'owned-blank', 'url': 'about:blank', 'title': 'about:blank'}
        base = types.SimpleNamespace(
            events=Mock(return_value=[event]), context=Mock(return_value=(run, {'browser_startup': declared})),
            chromium=types.SimpleNamespace(original_gone=Mock(return_value=False), observe=Mock(return_value=lease)),
            browser_endpoint=Mock(return_value=endpoint), browser_pages=Mock(return_value=[control]),
            browser_lease=Mock(side_effect=AssertionError('no immutable lease before readiness')),
            browser_authority=Mock(side_effect=AssertionError('no immutable admission before readiness')),
            original_receipt=Mock(side_effect=AssertionError('no immutable writes in read-only barrier')))
        return base, origin, declared, lease, client, endpoint, control

    def probe(self, run, base, clients):
        with patch.object(driver, 'context', return_value=(run, {}, base)), \
             patch.object(driver, 'admitted'), \
             patch.object(driver, 'warm_native_clients', side_effect=clients), \
             patch.object(driver.p, 'private', side_effect=AssertionError('no private writes')) as write:
            value = driver.warm_ready(run, 'abcdef123456')
        write.assert_not_called()
        base.browser_lease.assert_not_called()
        base.browser_authority.assert_not_called()
        base.original_receipt.assert_not_called()
        return value

    def test_full_readonly_ready_proof_is_stable_before_any_immutable_admission(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            base, origin, declared, lease, client, _, _ = self.fixture(run)
            self.assertEqual(self.probe(run, base, [[client], [client]]),
                             {'ready': True, 'blocked': False, 'phase': 'ready'})
            self.assertEqual(base.chromium.observe.call_args_list,
                             [unittest.mock.call(origin, declared), unittest.mock.call(origin, declared, lease)])
            self.assertEqual(base.browser_endpoint.call_count, 2)
            self.assertEqual(list(run.iterdir()), [])

    def test_missing_mapping_and_endpoint_are_not_ready_without_immutable_writes(self):
        for missing in ('mapping', 'endpoint', 'control', 'connection'):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as temporary:
                run = Path(temporary)
                base, _, _, _, client, _, _ = self.fixture(run)
                if missing == 'endpoint':
                    base.browser_endpoint.side_effect = FileNotFoundError()
                if missing == 'control':
                    base.browser_pages.return_value = []
                if missing == 'connection':
                    base.browser_pages.side_effect = driver.URLError('private opaque token must not escape')
                result = self.probe(run, base, [[]] if missing == 'mapping' else [[client]])
                self.assertFalse(result['ready']); self.assertFalse(result['blocked'])
                self.assertEqual(result['phase'], {'mapping': 'mapped_client', 'endpoint': 'cdp_endpoint',
                                                  'control': 'blank_control', 'connection': 'blank_control'}[missing])
                self.assertNotIn('private opaque token', json.dumps(result))
                self.assertEqual(list(run.iterdir()), [])

    def test_unknown_process_birth_is_an_ownership_failure_before_mapping_or_cdp(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            base, _, _, _, _, _, _ = self.fixture(run)
            base.chromium.original_gone.return_value = True
            result = self.probe(run, base, [])
            self.assertEqual(result, {'ready': False, 'blocked': True, 'phase': 'original_birth',
                                      'error_class': 'ValueError'})
            base.chromium.observe.assert_not_called(); base.browser_endpoint.assert_not_called()

    def test_unstable_title_scope_or_original_authority_is_never_adopted(self):
        for change in ('title', 'scope', 'unit'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                run = Path(temporary)
                base, _, _, lease, client, _, _ = self.fixture(run)
                # The canonical observer's expected=first contract rejects a
                # changed title/scope/unit. The barrier must neither refresh
                # that observation nor turn this ownership error into waiting.
                changed = json.loads(json.dumps(lease))
                if change == 'title': changed['command']['sha256'] = 'changed-title'
                if change == 'scope':
                    changed['scope'] = {'invocation': 'new-scope'}
                    changed['process']['group'] = '/owned/app-original.scope'
                if change == 'unit': changed['unit'] = {'invocation': 'successor'}
                def observe(origin, declared, expected=None):
                    if expected is None:
                        return lease
                    self.assertEqual(expected, lease)
                    if changed != expected:
                        raise ValueError('opaque token and argv must stay private')
                    self.fail('changed external state must not match the expected observation')
                base.chromium.observe.side_effect = observe
                result = self.probe(run, base, [[client]])
                self.assertEqual(result, {'ready': False, 'blocked': True, 'phase': 'authority_after',
                                          'error_class': 'ValueError'})
                self.assertEqual(base.chromium.observe.call_count, 2)
                self.assertNotIn('opaque token', json.dumps(result))
                self.assertEqual(list(run.iterdir()), [])

    def test_foreign_or_changed_mapping_and_nonblank_controls_fail_before_write(self):
        for change in ('foreign_pid', 'floating_pid', 'foreign_class', 'unmapped', 'mapping_changed', 'nonblank', 'extra_control',
                       'endpoint_changed'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                run = Path(temporary)
                base, _, _, _, client, endpoint, control = self.fixture(run)
                first = dict(client); second = dict(client)
                if change == 'foreign_pid': first['pid'] = 999
                if change == 'floating_pid': first['pid'] = float(first['pid'])
                if change == 'foreign_class': first['class'] = 'PersonalBrowser'
                if change == 'unmapped': first['mapped'] = False
                if change == 'mapping_changed': second['address'] = '0xdef'
                if change == 'nonblank': base.browser_pages.return_value = [{**control, 'url': 'https://foreign.test'}]
                if change == 'extra_control': base.browser_pages.return_value = [control, {**control, 'id': 'another'}]
                if change == 'endpoint_changed':
                    base.browser_endpoint.side_effect = [endpoint, {**endpoint, 'port': endpoint['port'] + 1}]
                result = self.probe(run, base, [[first], [second]])
                self.assertFalse(result['ready']); self.assertTrue(result['blocked'])
                self.assertEqual(list(run.iterdir()), [])

    def test_existing_failed_or_admitted_immutable_files_are_never_refreshed(self):
        for name in ('chromium-lease.json', 'browser-admission.json'):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                run = Path(temporary)
                file = run / name; file.write_bytes(b'existing immutable evidence')
                base, _, _, _, _, _, _ = self.fixture(run)
                result = self.probe(run, base, [])
                self.assertTrue(result['blocked']); self.assertEqual(result['phase'], 'context')
                self.assertEqual(file.read_bytes(), b'existing immutable evidence')
                base.events.assert_not_called(); base.chromium.observe.assert_not_called()

    def test_controller_commits_exactly_once_only_after_readonly_ready(self):
        history = []
        results = iter([{'ready': False, 'blocked': False, 'phase': 'mapped_client'},
                        {'ready': False, 'blocked': False, 'phase': 'cdp_endpoint'},
                        {'ready': True, 'blocked': False, 'phase': 'ready'}])
        control = {'id': 'blank', 'url': 'about:blank'}
        def call(operation, *args, **kwargs):
            history.append(operation)
            if operation == 'warm-ready': return next(results)
            self.assertEqual(operation, 'browser-snapshot'); self.assertTrue(kwargs['canonical'])
            self.assertEqual(json.loads(args[0]), {'identity': None})
            return {'identity': {'control': control}, 'pages': [control]}
        def wait(predicate, label, timeout):
            self.assertEqual(timeout, 30)
            for _ in range(3):
                if predicate(): return True
            self.fail(label)
        diagnostic = {}
        self.assertEqual(controller.admit_warm_browser(call, wait, diagnostic)['pages'], [control])
        self.assertEqual(history, ['warm-ready', 'warm-ready', 'warm-ready', 'browser-snapshot'])
        self.assertEqual(diagnostic['polls'], 3); self.assertTrue(diagnostic['immutable_admission'])

    def test_controller_does_not_commit_or_blindly_retry_ownership_failure(self):
        history = []
        def call(operation, *args, **kwargs):
            history.append(operation)
            self.assertEqual(operation, 'warm-ready')
            return {'ready': False, 'blocked': True, 'phase': 'authority_after', 'error_class': 'ValueError'}
        def wait(predicate, label, timeout): return predicate()
        diagnostic = {}
        with self.assertRaisesRegex(ValueError, 'warm_readonly_authority_after_refused'):
            controller.admit_warm_browser(call, wait, diagnostic)
        self.assertEqual(history, ['warm-ready']); self.assertFalse(diagnostic['immutable_admission'])
        self.assertEqual(diagnostic['phases'][0]['phase'], 'authority_after')

    def test_wait_return_without_ready_proof_cannot_persist_an_immutable_lease(self):
        history = []
        def call(operation, *args, **kwargs):
            history.append(operation)
            self.assertEqual(operation, 'warm-ready')
            return {'ready': False, 'blocked': False, 'phase': 'mapped_client'}
        with self.assertRaisesRegex(ValueError, 'warm_readonly_startup_not_proven'):
            controller.admit_warm_browser(call, lambda probe, *args, **kwargs: probe(), {})
        self.assertEqual(history, ['warm-ready'])

    def test_diagnostic_packets_cannot_relay_raw_exception_or_environment_values(self):
        for value in ({'ready': False, 'blocked': True, 'phase': 'argv=private', 'error_class': 'ValueError'},
                      {'ready': False, 'blocked': True, 'phase': 'authority_before', 'error_class': 'private token'},
                      {'ready': False, 'blocked': False, 'phase': 'original_birth'},
                      {'ready': True, 'blocked': False, 'phase': 'ready', 'argv': ['private']}):
            diagnostic = {}
            with self.assertRaisesRegex(ValueError, 'bounded_warm_readiness_reply'):
                controller.admit_warm_browser(lambda *_: value, lambda probe, *args, **kwargs: probe(), diagnostic)
            self.assertEqual(diagnostic['phases'], [])
            self.assertNotIn('private', json.dumps(diagnostic))


class GuardianRecovery(unittest.TestCase):
    def test_independent_expiry_restores_both_configurations_after_controller_disappears(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run = root / 'run'
            run.mkdir(mode=0o700)
            runtime = root / 'runtime'
            runtime.mkdir(mode=0o700)
            mime, entry = root / 'mimeapps.list', root / 'owned.desktop'
            mime.write_bytes(b'# exact caller bytes\n')
            mime.chmod(0o640)
            values = [p.plan(run, name, path, content) for name, path, content in
                      (('default-mime', mime, b'owned default\n'), ('default-entry', entry, b'owned entry\n'))]
            for value in values:
                p.publish(value)
            with patch.object(driver, 'context', return_value=(run, {}, Mock())), \
                 patch.object(driver, 'capture_unit_origin', return_value={'origin': 'owned'}), \
                 patch.object(driver, 'wait_expiry'), patch.object(driver, 'close_history'), \
                 patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(runtime)}), \
                 patch.object(driver.time, 'monotonic', side_effect=[0, 151]):
                driver.watchdog(run, 'abcdef123456')
            self.assertEqual(p.snapshot(mime), values[0]['original'])
            self.assertFalse(entry.exists())
            result = json.loads((run / 'default-finished.json').read_text())
            self.assertTrue(result['success'])
            self.assertTrue(all(result['restoration'].values()))

    def test_foreign_mime_change_blocks_entry_removal_and_is_reported_incomplete(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run = root / 'run'
            run.mkdir(mode=0o700)
            runtime = root / 'runtime'
            runtime.mkdir(mode=0o700)
            mime, entry = root / 'mimeapps.list', root / 'owned.desktop'
            mime.write_bytes(b'caller')
            values = [p.plan(run, name, path, content) for name, path, content in
                      (('default-mime', mime, b'owned default'), ('default-entry', entry, b'owned entry'))]
            for value in values:
                p.publish(value)
            replacement = root / 'changed'
            replacement.write_bytes(b'new caller preference')
            os.replace(replacement, mime)
            foreign = p.snapshot(mime)
            with patch.object(driver, 'context', return_value=(run, {}, Mock())), \
                 patch.object(driver, 'capture_unit_origin', return_value={'origin': 'owned'}), \
                 patch.object(driver, 'wait_expiry'), patch.object(driver, 'close_history'), \
                 patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(runtime)}), \
                 patch.object(driver.time, 'monotonic', side_effect=[0, 151]):
                driver.watchdog(run, 'abcdef123456')
            self.assertEqual(p.snapshot(mime), foreign)
            self.assertEqual(p.snapshot(entry), values[1]['candidate'])
            result = json.loads((run / 'default-finished.json').read_text())
            self.assertFalse(result['success'])
            self.assertFalse(result['restoration']['default_mime_restored'])
            self.assertFalse(result['restoration']['default_entry_restored'])

    def test_foreign_notification_keeps_history_untouched_and_cleanup_failed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run = root / 'run'
            run.mkdir(mode=0o700)
            runtime = root / 'runtime'
            runtime.mkdir(mode=0o700)
            mime, entry = root / 'mimeapps.list', root / 'owned.desktop'
            mime.write_bytes(b'caller')
            values = [p.plan(run, name, path, content) for name, path, content in
                      (('default-mime', mime, b'owned default'), ('default-entry', entry, b'owned entry'))]
            for value in values:
                p.publish(value)
            with patch.object(driver, 'context', return_value=(run, {}, Mock())), \
                 patch.object(driver, 'capture_unit_origin', return_value={'origin': 'owned'}), \
                 patch.object(driver, 'wait_expiry', side_effect=ValueError('replaced_notification_revision_preserved')), \
                 patch.object(driver, 'close_history') as hide, \
                 patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(runtime)}), \
                 patch.object(driver.time, 'monotonic', side_effect=[0, 151]):
                driver.watchdog(run, 'abcdef123456')
            hide.assert_not_called()
            self.assertEqual(p.snapshot(mime), values[0]['original'])
            self.assertFalse(entry.exists())
            result = json.loads((run / 'default-finished.json').read_text())
            self.assertFalse(result['success'])
            self.assertFalse(result['restoration']['notifications_absent_after_expiry_window'])
            self.assertFalse(result['restoration']['notification_history_closed'])


if __name__ == '__main__':
    unittest.main()
