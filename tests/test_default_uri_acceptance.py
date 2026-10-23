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
