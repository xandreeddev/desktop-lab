"""Pure policy and owned temporary files only; no VM, compositor, bus or browser."""
import contextlib
import ast
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
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


class FixturePreparation(unittest.TestCase):
    def prepare(self, root):
        run = root / 'lucent-application-intent.abcdef123456'
        config, data = root / 'config', root / 'data'
        for path in (run, config, data):
            path.mkdir(mode=0o700)
        apps = data / 'applications'
        apps.mkdir(mode=0o700)
        canonical = module('canonical_entry_encoding', ROOT / 'vm/fixtures/application-intent-fixture.py')
        encode = Mock(wraps=canonical.encoded)
        base = types.SimpleNamespace(encoded=encode, http_authority=Mock(return_value={
            'targets': [{'uri': 'http://127.0.0.1:1234/owned'}]}))
        environment = {'XDG_CURRENT_DESKTOP': 'Hyprland', 'BROWSER': '',
                       'XDG_CONFIG_HOME': str(config), 'XDG_DATA_HOME': str(data)}
        with patch.object(driver, 'context', return_value=(run, {}, base)), \
             patch.dict(os.environ, environment), \
             patch.object(subprocess, 'run', side_effect=AssertionError('preparation must not run applications')):
            prepared = driver.prepare(run, 'abcdef123456')
        return run, config, apps, prepared, canonical.encoded, encode

    def test_only_reviewed_fixed_arguments_are_bare_literals_and_uri_remains_a_field_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            run, config, apps, prepared, encoded, encode = self.prepare(Path(temporary))
            tail = ['-I', str(run / 'default-uri.py'), 'open', str(run), 'abcdef123456']
            entry = p.read(run / 'default-entry.payload').decode()
            command = next(line for line in entry.splitlines() if line.startswith('Exec='))
            self.assertEqual(command, 'Exec=/usr/bin/python3 ' + ' '.join(tail) + ' %u')
            encode.assert_not_called()
            self.assertIn('MimeType=x-scheme-handler/http;', entry)
            self.assertIn(prepared['app_id'].encode(), p.read(run / 'default-mime.payload'))
            self.assertFalse((apps / prepared['app_id']).exists())
            self.assertFalse((config / 'hyprland-mimeapps.list').exists())

    def test_unsupported_literal_paths_refuse_before_any_configuration_plan(self):
        for character in (' ', '\t', '\n', '"', "'", '\\', '$', '`', '%', ';', '&', '|',
                          '<', '>', '*', '?', '[', ']', '(', ')', '{', '}', ':', '雪'):
            with self.subTest(character=repr(character)), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary) / ('unsupported' + character + 'path')
                root.mkdir(mode=0o700)
                with self.assertRaisesRegex(ValueError, '^fixed_default_uri_exec_literals$'):
                    self.prepare(root)
                self.assertEqual(list((root / 'lucent-application-intent.abcdef123456').iterdir()), [])
                self.assertEqual(list((root / 'config').iterdir()), [])
                self.assertEqual(list((root / 'data/applications').iterdir()), [])

    def test_fixed_command_rejects_relative_parent_and_unpinned_run_identity(self):
        for run, run_id in ((Path('lucent-application-intent.abcdef123456'), 'abcdef123456'),
                            (Path('/tmp/../lucent-application-intent.abcdef123456'), 'abcdef123456'),
                            (Path('/tmp/unrelated'), 'abcdef123456'),
                            (Path('/tmp/lucent-application-intent.ABCDEF123456'), 'ABCDEF123456'),
                            (Path('/tmp/lucent-application-intent.abcdef1234567'), 'abcdef1234567')):
            with self.subTest(run=str(run), run_id=run_id):
                with self.assertRaisesRegex(ValueError, '^fixed_default_uri_exec_literals$'):
                    driver.fixed_desktop_command(run, run_id)

    def test_installed_xdg_mime_prefers_prepared_entry_over_an_isolated_generic_fallback(self):
        tool = shutil.which('xdg-mime')
        if tool is None:
            self.skipTest('xdg-mime is not installed; isolated default-query regression unavailable')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run, config, apps, prepared, _, _ = self.prepare(root)
            fallback = 'org.lucent.IsolatedFallback.desktop'
            (apps / fallback).write_text('[Desktop Entry]\nType=Application\nName=Isolated fallback\n'
                                         'MimeType=x-scheme-handler/http;\nExec=/usr/bin/python3\n')
            (config / 'mimeapps.list').write_text('[Default Applications]\nx-scheme-handler/http=' + fallback + ';\n')
            (config / 'hyprland-mimeapps.list').write_bytes(p.read(run / 'default-mime.payload'))
            config_dirs, data_dirs = root / 'empty-config-dirs', root / 'empty-data-dirs'
            config_dirs.mkdir(mode=0o700)
            data_dirs.mkdir(mode=0o700)
            environment = {**os.environ, 'DE': 'generic', 'XDG_CURRENT_DESKTOP': 'Hyprland',
                           'XDG_CONFIG_HOME': str(config), 'XDG_CONFIG_DIRS': str(config_dirs),
                           'XDG_DATA_HOME': str(apps.parent), 'XDG_DATA_DIRS': str(data_dirs),
                           'XDG_MENU_PREFIX': ''}
            self.assertEqual(environment.get('HOME'), os.environ.get('HOME'))
            candidate = p.read(run / 'default-entry.payload')
            self.assertEqual(candidate.count(b'Exec=/usr/bin/python3 '), 1)
            entry = apps / prepared['app_id']
            def query(content):
                entry.write_bytes(content)
                result = subprocess.run([tool, 'query', 'default', 'x-scheme-handler/http'],
                                        env=environment, stdin=subprocess.DEVNULL,
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stderr, '')
                return result.stdout.strip()
            prior = query(candidate.replace(b'Exec=/usr/bin/python3 ', b'Exec="/usr/bin/python3" ', 1))
            self.assertIn(prior, (fallback, prepared['app_id']))
            if hashlib.sha256(Path(tool).read_bytes()).hexdigest() == 'a587107ff995a5f907ab1b731316a169348c6b0383caf8fd151ff21f89615f3f':
                self.assertEqual(prior, fallback)
            self.assertEqual(query(candidate), prepared['app_id'])

    def test_installed_xdg_open_generic_dispatches_exact_fixed_argv_and_uri_as_data(self):
        tool, mime = shutil.which('xdg-open'), shutil.which('xdg-mime')
        if tool is None or mime is None:
            self.skipTest('xdg-open/xdg-mime not installed; isolated generic-dispatch regression unavailable')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run, config, apps, prepared, encoded, _ = self.prepare(root)
            output = run / 'recorded.json'
            script = run / 'default-uri.py'
            script.write_text('import json, sys\nfrom pathlib import Path\n'
                              "Path(__file__).with_name('recorded.json').write_text(json.dumps(sys.argv))\n")
            fallback_script = root / 'fallback.py'
            fallback_output = root / 'fallback.json'
            fallback_script.write_text('import json, sys\nfrom pathlib import Path\n'
                                       "Path(__file__).with_name('fallback.json').write_text(json.dumps(sys.argv))\n")
            fallback = 'org.lucent.IsolatedFallback.desktop'
            (apps / fallback).write_text('[Desktop Entry]\nType=Application\nName=Isolated fallback\n'
                                         'MimeType=x-scheme-handler/http;\nExec=/usr/bin/python3 -I '
                                         + str(fallback_script) + ' %u\n')
            (config / 'mimeapps.list').write_text('[Default Applications]\nx-scheme-handler/http=' + fallback + ';\n')
            config_dirs, data_dirs = root / 'empty-config-dirs', root / 'empty-data-dirs'
            config_dirs.mkdir(mode=0o700)
            data_dirs.mkdir(mode=0o700)
            # No real display/bus or browser fallback can be reached. The generic
            # path only checks that a display variable is present; URI recording
            # happens solely in these temporary, controlled Python executables.
            environment = {'HOME': os.environ['HOME'], 'PATH': '/usr/bin:/bin', 'LC_ALL': 'C',
                           'DE': 'generic', 'XDG_CURRENT_DESKTOP': 'Hyprland',
                           'XDG_CONFIG_HOME': str(config), 'XDG_CONFIG_DIRS': str(config_dirs),
                           'XDG_DATA_HOME': str(apps.parent), 'XDG_DATA_DIRS': str(data_dirs),
                           'XDG_MENU_PREFIX': '', 'DISPLAY': '', 'WAYLAND_DISPLAY': 'owned-test-no-compositor',
                           'DBUS_SESSION_BUS_ADDRESS': 'unix:path=' + str(root / 'absent-bus'),
                           'BROWSER': '/usr/bin/false'}
            self.assertEqual(environment['HOME'], os.environ['HOME'])
            uri = 'http://127.0.0.1:1234/owned%20file?literal=$(unused);&home=$HOME&quote=%22%E9%9B%AA%22&percent=%25'
            def query():
                result = subprocess.run([mime, 'query', 'default', 'x-scheme-handler/http'],
                                        env=environment, cwd=root, stdin=subprocess.DEVNULL,
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0)
                return result.stdout.strip()
            def dispatch():
                return subprocess.run([tool, uri], env=environment, cwd=root,
                                      stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=5)
            self.assertEqual(query(), fallback)
            self.assertEqual(dispatch().returncode, 0)
            self.assertEqual(json.loads(fallback_output.read_text()), [str(fallback_script), uri])
            fallback_output.unlink()
            candidate = p.read(run / 'default-entry.payload')
            entry = apps / prepared['app_id']
            prior = '/usr/bin/python3 ' + ' '.join(encoded(value) for value in
                ('-I', str(script), 'open', str(run), 'abcdef123456')) + ' %u'
            entry.write_bytes(candidate.replace(driver.fixed_desktop_command(run, 'abcdef123456').encode() + b' %u',
                                                prior.encode(), 1))
            (config / 'hyprland-mimeapps.list').write_bytes(p.read(run / 'default-mime.payload'))
            self.assertEqual(query(), prepared['app_id'])
            previous = dispatch()
            if hashlib.sha256(Path(tool).read_bytes()).hexdigest() == '7d759d54f0d4ce1beeb34ee62c587985b5042374e8de237b0dcb8cd47aa698cf':
                self.assertFalse(output.exists())
                self.assertIn('can\'t open file', previous.stderr)
                self.assertIn('"-I"', previous.stderr)
            elif output.exists():
                self.assertEqual(json.loads(output.read_text()),
                                 [str(script), 'open', str(run), 'abcdef123456', uri])
                output.unlink()
            self.assertFalse(fallback_output.exists())
            entry.write_bytes(candidate)
            self.assertEqual(query(), prepared['app_id'])
            self.assertEqual(dispatch().returncode, 0)
            self.assertEqual(json.loads(output.read_text()),
                             [str(script), 'open', str(run), 'abcdef123456', uri])
            self.assertFalse(fallback_output.exists())


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


class NativeFlowDiagnostics(unittest.TestCase):
    def history(self, expanded=False):
        run_id = 'abcdef123456'
        uri = 'http://127.0.0.1:1234/owned'
        group = {'id': 'notification-group-name:Lucent owned URI ' + run_id,
                 'x': 20, 'y': 30, 'width': 200, 'height': 32,
                 'clip': {'x': 0, 'y': 0, 'width': 1920, 'height': 1080}}
        link = {**group, 'id': 'notification-body-7-2-0/link/' + uri, 'y': 80}
        surface = {'id': 'notifications', 'role': 'notifications', 'instance': 'original-history',
                   'output': 'Virtual-1', 'keyboard': 'exclusive', 'pointer_input': 'all',
                   'frames': 3, 'native_frames': 2, 'density': 1, 'width': 1920, 'height': 1080,
                   'buffer_width': 1920, 'buffer_height': 1080, 'hits': [group, link] if expanded else [group]}
        layer = {'namespace': 'lucent-notifications', 'address': '0xabc', 'pid': 123,
                 'x': 0, 'y': 0, 'w': 1920, 'h': 1080}
        return {'surfaces': [surface]}, {'Virtual-1': {'levels': {'3': [layer]}}}, run_id, uri

    def test_group_hit_matches_actual_client_and_fixture_declarations_before_a_link_exists(self):
        view = (ROOT / 'lucent/apps/lucent-desktop/src/notifications/view.rs').read_text()
        grouping = (ROOT / 'lucent/crates/lucent-usecases/src/notifications.rs').read_text()
        self.assertIn('.id(format!("notification-group-{}", group.key))', view)
        self.assertIn('format!("name:{}", note.app)', grouping)
        tree = ast.parse((ROOT / 'vm/fixtures/default-uri-fixture.py').read_text())
        notify = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'notify')
        call = next(node for node in ast.walk(notify) if isinstance(node, ast.Call)
                    and isinstance(node.func, ast.Attribute) and node.func.attr == 'Notify')
        self.assertEqual(ast.literal_eval(call.args[0].left), 'Lucent owned URI ')
        snapshot, layers, run_id, uri = self.history()
        value = controller.group_target(snapshot, layers, run_id, 123)
        self.assertEqual(value['hit']['id'], 'notification-group-name:Lucent owned URI ' + run_id)
        self.assertEqual(value['point'], (120, 46))
        with self.assertRaisesRegex(ValueError, 'one_current_selected_notification_link'):
            controller.target(snapshot, layers, 7, uri, 123)

    def test_group_hit_rejects_foreign_duplicate_unsubmitted_scaled_and_clipped_targets(self):
        snapshot, layers, run_id, _ = self.history()
        surface = snapshot['surfaces'][0]
        for changed in ({'hits': []}, {'hits': surface['hits'] * 2}, {'native_frames': 0},
                        {'keyboard': 'none'}, {'density': 2}, {'output': 'foreign'}):
            with self.subTest(changed=tuple(changed)), self.assertRaises(ValueError):
                controller.group_target({'surfaces': [{**surface, **changed}]}, layers, run_id, 123)
        with self.assertRaises(RuntimeError):
            controller.group_target(snapshot, layers, run_id, 456)
        clipped = {**surface['hits'][0], 'clip': {'x': 0, 'y': 0, 'width': 4, 'height': 4}}
        with self.assertRaises(RuntimeError):
            controller.group_target({'surfaces': [{**surface, 'hits': [clipped]}]}, layers, run_id, 123)
        with self.assertRaisesRegex(ValueError, 'owned_notification_group_run_required'):
            controller.group_target(snapshot, layers, 'private_lowercase_secret', 123)

    def pointer_fixture(self, kind, case):
        snapshot, layers, run_id, uri = self.history(expanded=True)
        target = (controller.group_target(snapshot, layers, run_id, 123) if kind == 'group'
                  else controller.target(snapshot, layers, 7, uri, 123))
        current = {'x': 1, 'y': 1}
        from application_intent_policy import CursorOwnership
        cursor = CursorOwnership(dict(current))
        report = {'native_input': {}}
        controller.begin_native_case(report, case)
        events = []
        focused = False
        def observe():
            events.append('observe')
            return {**target, 'focus': target['hit']['id'] if focused else None}
        def move(point):
            events.append('cursor_dispatch');current.update(zip(('x', 'y'), point))
        def button(down):
            nonlocal focused
            events.append('button_down' if down else 'button_up')
            focused = down
        def wait(predicate, label, timeout):
            self.assertEqual(timeout, 3)
            self.assertTrue(predicate(), label)
        held = []
        return report, cursor, lambda: dict(current), observe, move, button, wait, held, events

    def test_actual_group_setup_pointer_flow_is_separate_from_pointer_and_keyboard_link_proof(self):
        for case in ('pointer', 'keyboard'):
            with self.subTest(case=case):
                report, cursor, read, observe, move, button, wait, held, events = self.pointer_fixture('group', case)
                with patch.object(controller.time, 'monotonic', return_value=0):
                    controller.native_pointer_control(report, 'group', observe, cursor, read, move, button, wait, 10, held.append)
                self.assertEqual(held, [True, False])
                self.assertEqual([event for event in events if event.startswith('button_')], ['button_down', 'button_up'])
                self.assertEqual(cursor.expected, (120, 46))
                self.assertTrue(report['native_flow']['group_dispatch_attempted'])
                self.assertTrue(report['native_flow']['group_dispatch_returned'])
                self.assertFalse(report['native_flow']['pointer_dispatch_attempted'])
                self.assertEqual(report['native_input'], {})
                self.assertNotIn('failure', report)

    def test_failure_before_button_dispatch_retains_the_exact_phase_without_claiming_input(self):
        for kind in ('group', 'pointer'):
            with self.subTest(kind=kind):
                report, cursor, read, _, move, button, wait, held, events = self.pointer_fixture(kind, 'pointer')
                observe = Mock(side_effect=ValueError('one_current_selected_notification_link'))
                with self.assertRaises(ValueError):
                    controller.native_pointer_control(report, kind, observe, cursor, read, move, button, wait, 10, held.append)
                failure = report['failure']
                self.assertEqual(failure['phase'], kind + '_target_capture')
                self.assertEqual(failure['current_case'], 'pointer')
                self.assertFalse(failure[kind + '_dispatch_attempted'])
                self.assertEqual(events, []);self.assertEqual(held, [])

    def test_failed_focus_after_dispatch_is_not_replaced_by_release_failure(self):
        for kind in ('group', 'pointer'):
            with self.subTest(kind=kind):
                report, cursor, read, observe, move, button, _, held, _ = self.pointer_fixture(kind, 'pointer')
                label = 'native_pointer_focus_on_owned_group' if kind == 'group' else 'native_pointer_focus_on_owned_link'
                def release_fails(down):
                    if not down:
                        raise ValueError('private_lowercase_secret')
                    return button(down)
                with patch.object(controller.time, 'monotonic', return_value=0), self.assertRaises(ValueError):
                    controller.native_pointer_control(report, kind, observe, cursor, read, move, release_fails,
                        Mock(side_effect=ValueError(label)), 10, held.append)
                failure = report['failure']
                self.assertEqual(failure['phase'], kind + '_focus_wait')
                self.assertEqual(failure['fixed_reason'], label)
                self.assertTrue(failure[kind + '_dispatch_attempted']);self.assertTrue(failure[kind + '_dispatch_returned'])
                self.assertEqual(held, [True])
                self.assertNotIn('private', json.dumps(failure))

    def test_failed_dispatch_reports_attempt_without_claiming_a_successful_return(self):
        report, cursor, read, observe, move, _, wait, held, _ = self.pointer_fixture('pointer', 'pointer')
        def button(down):
            if down:
                raise TimeoutError('private_lowercase_secret')
        with patch.object(controller.time, 'monotonic', return_value=0), self.assertRaises(TimeoutError):
            controller.native_pointer_control(report, 'pointer', observe, cursor, read, move, button, wait, 10, held.append)
        failure = report['failure']
        self.assertEqual(failure['phase'], 'pointer_button_down')
        self.assertTrue(failure['pointer_dispatch_attempted']);self.assertFalse(failure['pointer_dispatch_returned'])
        self.assertEqual(held, [True, False]);self.assertNotIn('fixed_reason', failure)

    def test_native_case_phase_and_reason_labels_are_explicit_and_private_values_are_redacted(self):
        for case in ('private_lowercase_secret', None, 1):
            report = {}
            with self.assertRaisesRegex(ValueError, 'known_native_case_required'):
                controller.begin_native_case(report, case)
            self.assertEqual(report, {})
        report = {};controller.begin_native_case(report, 'keyboard');action = Mock()
        for phase in ('private_lowercase_secret', 'pointer_button_down'):
            with self.assertRaises(ValueError):controller.native_step(report, phase, action)
        action.assert_not_called()
        with self.assertRaisesRegex(ValueError, 'known_native_pointer_control_required'):
            controller.native_pointer_control(report, 'pointer', *([action] * 8))
        controller.record_failure(report, ValueError('private_lowercase_secret'), 'private_lowercase_secret')
        self.assertEqual(report['failure']['phase'], 'setup')
        self.assertNotIn('private', json.dumps(report))

    def test_every_literal_controller_and_pinned_native_policy_failure_label_is_admitted(self):
        labels = {'bounded_subprocess_failed'}
        for relative in ('vm/test-default-uri-notification.py', 'vm/application_intent_policy.py', 'vm/multi_output_policy.py'):
            for node in ast.walk(ast.parse((ROOT / relative).read_text())):
                if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name):
                    continue
                index = {'require': 1, 'wait': 1, 'unique': 2}.get(node.func.id)
                if index is not None and len(node.args) > index:
                    argument = node.args[index]
                    values = [argument] if isinstance(argument, ast.Constant) else list(ast.walk(argument)) if isinstance(argument, ast.IfExp) else []
                    labels.update(value.value for value in values if isinstance(value, ast.Constant) and isinstance(value.value, str))
                if node.func.id == 'ValueError' and len(node.args) == 1 and isinstance(node.args[0], ast.Constant):
                    labels.add(node.args[0].value)
        self.assertTrue(labels <= set(controller.FAILURE_REASONS), sorted(labels - set(controller.FAILURE_REASONS)))
        for label in labels:
            report = {};controller.begin_native_case(report, 'pointer')
            controller.record_failure(report, ValueError(label), 'case_preflight')
            self.assertEqual(report['failure']['fixed_reason'], label)


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
            self.assertIn('--property=RuntimeMaxSec=' + str(p.BROWSER_RUNTIME_SECONDS), argv)
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


class ControlledTestTiming(unittest.TestCase):
    def test_finite_budgets_fit_canonical_http_browser_and_guardian_lifetimes(self):
        self.assertEqual((p.NOTIFICATION_TTL_MS, p.NOTIFICATION_EXPIRY_SECONDS, p.BRIDGE_EXPIRY_SECONDS), (60000, 65, 70))
        self.assertEqual((p.NATIVE_INPUT_SECONDS, p.CANONICAL_HTTP_SECONDS, p.BROWSER_RUNTIME_SECONDS), (180, 210, 240))
        self.assertEqual((p.GUARDIAN_OPEN_SECONDS, p.GUARDIAN_CLEANUP_RESERVE_SECONDS,
                          p.GUARDIAN_UNIT_RUNTIME_SECONDS, p.FINISH_RECEIPT_SECONDS), (210, 120, 345, 90))
        seconds = (p.NOTIFICATION_EXPIRY_SECONDS, p.NOTIFICATION_POST_EXPIRY_SECONDS,
                   p.BRIDGE_COMMAND_SECONDS, p.BRIDGE_EXPIRY_SECONDS, p.NATIVE_INPUT_SECONDS,
                   p.CANONICAL_HTTP_SECONDS, p.HTTP_DELIVERY_MARGIN_SECONDS, p.BROWSER_RUNTIME_SECONDS,
                   p.GUARDIAN_OPEN_SECONDS, p.GUARDIAN_CLEANUP_RESERVE_SECONDS,
                   p.GUARDIAN_STARTUP_MARGIN_SECONDS, p.GUARDIAN_UNIT_RUNTIME_SECONDS, p.FINISH_RECEIPT_SECONDS)
        self.assertTrue(all(type(value) is int and 0 < value <= 345 for value in seconds))
        self.assertLess(p.NOTIFICATION_TTL_MS / 1000, p.NOTIFICATION_EXPIRY_SECONDS)
        self.assertLess(p.NOTIFICATION_EXPIRY_SECONDS, p.BRIDGE_EXPIRY_SECONDS)
        self.assertLessEqual(p.NATIVE_INPUT_SECONDS + p.HTTP_DELIVERY_MARGIN_SECONDS, p.CANONICAL_HTTP_SECONDS)
        self.assertGreaterEqual(p.BROWSER_RUNTIME_SECONDS, p.NATIVE_INPUT_SECONDS + p.HTTP_DELIVERY_MARGIN_SECONDS)
        self.assertGreaterEqual(p.GUARDIAN_OPEN_SECONDS, p.NATIVE_INPUT_SECONDS)
        self.assertGreaterEqual(p.GUARDIAN_UNIT_RUNTIME_SECONDS, p.GUARDIAN_OPEN_SECONDS
                                + p.GUARDIAN_CLEANUP_RESERVE_SECONDS + p.GUARDIAN_STARTUP_MARGIN_SECONDS)
        self.assertLessEqual(p.BRIDGE_COMMAND_SECONDS + p.FINISH_RECEIPT_SECONDS, p.GUARDIAN_CLEANUP_RESERVE_SECONDS)
        canonical = ROOT / 'vm/fixtures/application-intent-fixture.py'
        self.assertEqual(hashlib.sha256(canonical.read_bytes()).hexdigest(),
                         '25e7332ef60c67797f0823104d5a617f9178bc59df900d49479cbcb0ebd566a7')
        wait = next(node for node in ast.parse(canonical.read_text()).body
                    if isinstance(node, ast.FunctionDef) and node.name == 'http_wait')
        self.assertEqual(ast.literal_eval(wait.args.defaults[-1]), p.CANONICAL_HTTP_SECONDS)

    def test_only_readonly_expiry_operation_gets_the_longer_bridge_timeout(self):
        response = types.SimpleNamespace(returncode=0, stdout=b'{}', stderr=b'')
        for operation in p.BRIDGE_OPERATIONS:
            with self.subTest(operation=operation), patch.object(controller.subprocess, 'run', return_value=response) as execute:
                controller.bridge_command(['owned-transport'], operation)
            self.assertEqual(execute.call_args.kwargs['timeout'],
                             p.BRIDGE_EXPIRY_SECONDS if operation == 'wait-expiry' else 30)
        with patch.object(controller.subprocess, 'run') as execute:
            with self.assertRaisesRegex(ValueError, 'known_default_uri_bridge_operation'):
                controller.bridge_command(['unused'], 'unknown')
        execute.assert_not_called()

    def test_both_cases_fit_with_a_modeled_setup_and_complete_expiry_postchecks(self):
        self.assertEqual(controller.NATIVE_CASES, p.NOTIFICATION_CASES)
        self.assertEqual(controller.NATIVE_CASES, ('pointer', 'keyboard'))
        now = 25  # Model elapsed setup; this is not a measured startup result.
        for _case in controller.NATIVE_CASES:
            self.assertTrue(p.admit_notification_case(now, p.NATIVE_INPUT_SECONDS, p.CANONICAL_HTTP_SECONDS))
            now += p.NOTIFICATION_TTL_MS / 1000 + p.NOTIFICATION_POST_EXPIRY_SECONDS
        self.assertLess(now, p.NATIVE_INPUT_SECONDS)

    def test_late_case_fails_actual_controller_admission_before_any_notify(self):
        tree = ast.parse((ROOT / 'vm/test-default-uri-notification.py').read_text())
        run = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'run')
        loop = next(node for node in ast.walk(run) if isinstance(node, ast.For)
                    and isinstance(node.target, ast.Name) and node.target.id == 'sequence')
        report = {};notify = Mock(side_effect=AssertionError('late case must not emit a notification'))
        namespace = {'sequence': 'pointer', 'report': report, 'policy': p,
                     'begin_native_case': controller.begin_native_case, 'native_step': controller.native_step,
                     'native_guard': Mock(), 'call': notify, 'deadline': 180, 'http_deadline': 210,
                     'time': types.SimpleNamespace(monotonic=lambda: 106)}
        with self.assertRaisesRegex(ValueError, 'notification_case_budget_exhausted'):
            exec(compile(ast.Module(body=loop.body[:4], type_ignores=[]), 'actual_case_admission', 'exec'), namespace)
        notify.assert_not_called()
        self.assertEqual(report['failure']['phase'], 'notification_budget_admission')
        self.assertEqual(report['failure']['fixed_reason'], 'notification_case_budget_exhausted')
        with self.assertRaisesRegex(ValueError, 'notification_case_budget_exhausted'):
            p.admit_notification_case(16, 180, 90)
        for value in (None, float('inf'), float('nan'), True):
            with self.assertRaisesRegex(ValueError, 'finite_notification_case_budget_required'):
                p.admit_notification_case(value, 180, 210)

    def test_real_notify_arguments_and_private_expiry_receipt_use_shared_sixty_second_ttl(self):
        proxy = Mock();proxy.Notify.side_effect = [7, 8]
        bus = Mock()
        fake = types.SimpleNamespace(SessionBus=lambda **kwargs: bus, Interface=lambda *_: proxy,
                                     Array=lambda value, **_: value, Dictionary=lambda value, **_: value, Boolean=bool)
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / 'default-plan.json').write_text(json.dumps({'http': {'targets': [{'uri': 'http://127.0.0.1:1234/owned'}]}}))
            with patch.dict(sys.modules, {'dbus': fake}), \
                 patch.object(driver, 'context', return_value=(run, {'desktop_pid': 123}, Mock())), \
                 patch.object(driver, 'admitted'), patch.object(driver, 'notification_owner', return_value=':1.8'), \
                 patch.object(driver.time, 'monotonic', return_value=100):
                for case in controller.NATIVE_CASES:
                    driver.notify(run, 'abcdef123456', case)
                    record = json.loads((run / ('notification-' + case + '.json')).read_text())
                    self.assertEqual(record['expires_after'], 160)
            self.assertEqual([call.args[7] for call in proxy.Notify.call_args_list], [60000, 60000])
            self.assertEqual(bus.close.call_count, 2)

    def test_expiry_cleanup_admission_keeps_canonical_desktop_authority_without_live_http(self):
        base = types.SimpleNamespace(admission=Mock(), http_authority=Mock(side_effect=AssertionError('HTTP may be retired')))
        header = {'desktop_pid': 123, 'desktop_sha256': 'a' * 64}
        admitted = {'baseline': {'desktop': {'pid': 123}}}
        with patch.object(driver, 'context', return_value=(Path('/unused-run'), header, base)), \
             patch.object(driver, 'guardian'), patch.object(driver, 'capture_print', return_value=admitted) as capture:
            self.assertEqual(driver.admitted(Path('/unused-run'), 'abcdef123456', config=False), admitted)
        base.http_authority.assert_not_called()
        capture.assert_called_once_with(base.admission, Path('/unused-run'), 'abcdef123456',
                                        {'desktop_sha256': header['desktop_sha256'], 'example_sha256': None})


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
        records[0]['expires_after'] = p.NOTIFICATION_TTL_MS / 1000
        self.assertEqual(self.run_wait(records, [absent, absent], [0, 1, p.NOTIFICATION_TTL_MS / 1000]),
                         {'absent_after_expiry_window': True})

    def test_unidentified_remaining_notice_times_out_without_removing_it(self):
        records, hidden = self.fixture()
        hidden['surfaces'] = []
        with self.assertRaisesRegex(ValueError, 'remaining_notification_preserved_after_expiry_window'):
            self.run_wait(records, [hidden], [0, p.NOTIFICATION_EXPIRY_SECONDS + 1])
        self.assertEqual(hidden['client']['notification_count'], 1)

    def test_absence_arriving_after_the_readonly_budget_is_not_accepted_late(self):
        records, absent = self.fixture(count=0)
        with self.assertRaisesRegex(ValueError, 'remaining_notification_preserved_after_expiry_window'):
            self.run_wait(records, [absent], [0, p.NOTIFICATION_EXPIRY_SECONDS + 1])

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
                 patch.object(driver.time, 'monotonic', side_effect=[0, p.GUARDIAN_OPEN_SECONDS + 1]):
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
                 patch.object(driver.time, 'monotonic', side_effect=[0, p.GUARDIAN_OPEN_SECONDS + 1]):
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
                 patch.object(driver.time, 'monotonic', side_effect=[0, p.GUARDIAN_OPEN_SECONDS + 1]):
                driver.watchdog(run, 'abcdef123456')
            hide.assert_not_called()
            self.assertEqual(p.snapshot(mime), values[0]['original'])
            self.assertFalse(entry.exists())
            result = json.loads((run / 'default-finished.json').read_text())
            self.assertFalse(result['success'])
            self.assertFalse(result['restoration']['notifications_absent_after_expiry_window'])
            self.assertFalse(result['restoration']['notification_history_closed'])


class BrowserKeyHandoff(unittest.TestCase):
    def event(self, **changes):
        return {'count': 1, 'event': {'trusted': True, 'key': 'a', 'code': 'KeyA', 'ctrl': False,
                'alt': False, 'meta': False, 'shift': False, 'repeat': False, **changes}}

    def snapshot(self, opened=False, frames=11, keyboard='none', pointer='none', instance='original'):
        return {'client': {'notification_history_open': opened}, 'surfaces': [{
            'id': 'notifications', 'role': 'notifications', 'instance': instance,
            'native_frames': frames, 'keyboard': keyboard, 'pointer_input': pointer}]}

    def flow(self, case='pointer', observed=None, snapshot=None, initial=None, guard_snapshots=None, preflight=None):
        report = {};controller.begin_native_case(report, case)
        operations = []
        reads = iter([{'count': 0, 'event': None} if initial is None else initial,
                      *(observed if observed is not None else [self.event()])])
        snapshots = iter(guard_snapshots) if guard_snapshots is not None else None
        def call(operation, packet):
            self.assertEqual(json.loads(packet), {'case': case})
            operations.append(operation)
            return {'armed': True, 'count': 0} if operation == 'browser-key-arm' else next(reads)
        def guard():
            operations.append('guard')
            return (next(snapshots) if snapshots is not None else
                    self.snapshot() if snapshot is None else snapshot), []
        def wait(predicate, label):
            for _ in range(3):
                result = predicate()
                if result:
                    return result
            raise ValueError(label)
        send = Mock(side_effect=lambda: operations.append('KEY_A'))
        def input_preflight():
            operations.append('input_preflight')
            if preflight is not None:
                preflight()
        def run():
            with patch.object(controller.time, 'monotonic', return_value=1):
                controller.assert_browser_key_handoff(report, {'case': case},
                    {'id': 'notifications', 'instance': 'original', 'native_frames': 10},
                    call, guard, wait, input_preflight, send, 100)
        return report, operations, send, run

    def test_both_cases_send_exactly_one_key_after_arming_and_normal_input_release(self):
        for case in ('pointer', 'keyboard'):
            with self.subTest(case=case):
                report, operations, send, run = self.flow(case)
                run()
                send.assert_called_once_with()
                proof = report['browser_key_handoff'][case]
                self.assertTrue(proof['accepted'])
                self.assertEqual(proof['event'], self.event())
                self.assertEqual(proof['release_observation']['native_frames_advanced'], True)
                self.assertEqual(operations, ['browser-key-arm', 'guard', 'browser-key-read',
                                              'input_preflight', 'guard', 'KEY_A', 'browser-key-read', 'guard'])
                self.assertEqual(proof['pre_dispatch_release']['notification_policy'], 'exit-paint')
                self.assertEqual(proof['post_event_release']['notification_policy'], 'exit-paint')

    def test_real_normal_toast_policy_is_nonmodal_after_close_and_still_requires_actual_key(self):
        toast = self.snapshot(keyboard='on-demand', pointer='content')
        for case in ('pointer', 'keyboard'):
            with self.subTest(case=case):
                report, _, send, run = self.flow(case, guard_snapshots=[self.snapshot(), toast, toast])
                run()
                send.assert_called_once_with()
                proof = report['browser_key_handoff'][case]
                self.assertTrue(proof['accepted'])
                self.assertEqual(proof['pre_dispatch_release']['notification_policy'], 'normal-toast')
                self.assertEqual(proof['post_event_release']['notification_policy'], 'normal-toast')

    def test_reopened_modal_or_changed_surface_in_last_guard_blocks_key_after_release_and_cdp_read(self):
        for changed in (self.snapshot(opened=True, keyboard='exclusive', pointer='all'),
                        self.snapshot(keyboard='exclusive', pointer='all'), self.snapshot(instance='newer'),
                        self.snapshot(frames=10)):
            with self.subTest(changed=changed):
                report, operations, send, run = self.flow(guard_snapshots=[self.snapshot(), changed])
                with self.assertRaises(ValueError):
                    run()
                send.assert_not_called()
                self.assertEqual(operations, ['browser-key-arm', 'guard', 'browser-key-read', 'input_preflight', 'guard'])
                self.assertFalse(report['browser_key_handoff']['pointer']['key_dispatch_attempted'])
                self.assertEqual(report['failure']['phase'], 'browser_key_before_release_assert')

    def test_reopened_modal_or_changed_final_surface_rejects_successful_key_receipt(self):
        for changed in (self.snapshot(opened=True, keyboard='exclusive', pointer='all'),
                        self.snapshot(keyboard='exclusive', pointer='all'), self.snapshot(instance='newer'),
                        self.snapshot(frames=10)):
            with self.subTest(changed=changed):
                report, _, send, run = self.flow(guard_snapshots=[self.snapshot(), self.snapshot(), changed])
                with self.assertRaises(ValueError):
                    run()
                send.assert_called_once_with()
                proof = report['browser_key_handoff']['pointer']
                self.assertFalse(proof['accepted'])
                self.assertEqual(proof['event'], self.event())
                self.assertEqual(report['failure']['phase'], 'browser_key_after_release_assert')

    def test_repeat_original_us_caps_and_binding_admission_is_after_cdp_read_before_last_ui_guard(self):
        tree = ast.parse((ROOT / 'vm/test-application-actions.py').read_text())
        original = next(node for node in tree.body if isinstance(node, ast.FunctionDef)
                        and node.name == 'keyboard_preflight')
        for changes, bindings, expected in (({'layout': 'se'}, [], 'plain_us_main_keyboard_with_caps_off'),
                                            ({'capsLock': True}, [], 'plain_us_main_keyboard_with_caps_off'),
                                            ({}, [{'modmask': 0, 'key': 'a'}], 'typed_query_keys_have_no_compositor_bindings')):
            with self.subTest(changes=changes, bindings=bindings):
                main = {'main': True, 'layout': 'us', 'variant': '', 'capsLock': False, **changes}
                session = Mock(side_effect=[json.dumps({'keyboards': [main]}), json.dumps(bindings)])
                namespace = {'json': json, 'session': session, 'require': controller.require}
                exec(compile(ast.Module(body=[original], type_ignores=[]), '<reviewed-original-preflight>', 'exec'), namespace)
                report, operations, send, run = self.flow(preflight=namespace['keyboard_preflight'])
                with self.assertRaisesRegex(ValueError, '^' + expected + '$'):
                    run()
                send.assert_not_called()
                self.assertEqual(operations, ['browser-key-arm', 'guard', 'browser-key-read', 'input_preflight'])
                self.assertEqual(report['failure']['phase'], 'browser_key_input_preflight')
                self.assertEqual(report['failure']['fixed_reason'], expected)

    def test_missing_key_fails_without_retry_and_preserves_first_failure(self):
        report, _, send, run = self.flow(observed=[{'count': 0, 'event': None}] * 3)
        with self.assertRaisesRegex(ValueError, '^owned_browser_received_native_key_a$'):
            run()
        send.assert_called_once_with()
        self.assertFalse(report['browser_key_handoff']['pointer']['accepted'])
        self.assertEqual(report['failure']['phase'], 'browser_key_delivery_wait')
        first = dict(report['failure'])
        controller.record_failure(report, ValueError('cleanup'), 'setup')
        self.assertEqual(report['failure'], first)

    def test_preexisting_key_or_failed_dispatch_never_becomes_owned_success(self):
        report, _, send, run = self.flow(initial=self.event())
        with self.assertRaisesRegex(ValueError, '^no_key_before_owned_native_dispatch$'):
            run()
        send.assert_not_called()
        report, _, send, run = self.flow()
        send.side_effect = ValueError('bounded_subprocess_failed')
        with self.assertRaisesRegex(ValueError, '^bounded_subprocess_failed$'):
            run()
        send.assert_called_once_with()
        proof = report['browser_key_handoff']['pointer']
        self.assertTrue(proof['key_dispatch_attempted'])
        self.assertFalse(proof['key_dispatch_returned'])
        self.assertFalse(proof['accepted'])
        self.assertEqual(report['failure']['phase'], 'browser_key_dispatch')

    def test_open_history_or_unreleased_input_or_unsubmitted_exit_never_sends_key(self):
        for snapshot in (self.snapshot(opened=True), self.snapshot(keyboard='exclusive'),
                         self.snapshot(pointer='all'), self.snapshot(keyboard='on-demand', pointer='all'),
                         self.snapshot(keyboard='exclusive', pointer='content'), self.snapshot(frames=10)):
            with self.subTest(snapshot=snapshot):
                report, _, send, run = self.flow(snapshot=snapshot)
                with self.assertRaisesRegex(ValueError, '^natural_notification_input_release_required$'):
                    run()
                send.assert_not_called()
                self.assertFalse(report['browser_key_handoff']['pointer']['key_dispatch_attempted'])

    def test_retired_surface_is_recorded_without_inventing_native_frame_proof(self):
        value = controller.notification_handoff({'client': {'notification_history_open': False}, 'surfaces': []}, {})
        self.assertTrue(value['surface_absent'])
        self.assertIsNone(value['native_frames_advanced'])
        with self.assertRaisesRegex(ValueError, '^notification_handoff_surface_identity_changed$'):
            controller.notification_handoff(self.snapshot(instance='newer'),
                {'id': 'notifications', 'instance': 'original', 'native_frames': 10})

    def test_only_one_trusted_exact_unmodified_key_a_can_pass(self):
        for changes in ({'trusted': False}, {'key': 'other'}, {'ctrl': True}, {'alt': True},
                        {'meta': True}, {'shift': True}, {'repeat': True}):
            with self.subTest(changes=changes), self.assertRaisesRegex(ValueError, '^trusted_unmodified_key_a_required$'):
                p.received_browser_key(self.event(**changes))
        with self.assertRaises(ValueError):p.received_browser_key({**self.event(), 'count': 2})
        for value in ({**self.event(), 'private': 'secret'}, self.event(key='secret'),
                      self.event(code='private'), self.event(trusted=1), {'count': True, 'event': None}):
            with self.assertRaisesRegex(ValueError, '^bounded_browser_key_result_required$'):
                p.browser_key_observation(value)

    def test_js_records_only_fixed_key_data_and_never_focuses_or_synthesizes_input(self):
        target = {'url': 'http://127.0.0.1:1234/owned?literal=%22', 'title': 'Owned " document'}
        for operation in ('arm', 'read'):
            expression = p.browser_key_expression(operation, target, 'abcdef123456', 'pointer')
            for forbidden in ('.focus(', 'bringToFront', 'activateTarget', 'dispatchEvent', 'Input.dispatch',
                              'navigator.clipboard', 'innerHTML', 'textContent'):
                self.assertNotIn(forbidden, expression)
            self.assertIn(json.dumps(target['url']), expression)
        expression = p.browser_key_expression('arm', target, 'abcdef123456', 'pointer')
        self.assertIn('if(event.code!=="KeyA")return;', expression)
        self.assertIn('key:event.key==="a"?"a":"other"', expression)
        self.assertIn('trusted:event.isTrusted', expression)

    def test_optional_mode_is_inert_and_adds_key_binding_checks_only_when_requested(self):
        with patch.object(sys, 'argv', ['controller', '--assert-browser-key-handoff']), \
             patch.object(controller, 'run', side_effect=AssertionError('native forbidden')), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(controller.main(), 0)
        self.assertFalse(json.loads(output.getvalue())['guest_actions'])
        for binding in ({'modmask': 0, 'key': 'a'}, {'modmask': 0, 'keycode': 30}, {'modmask': 0, 'key': 'code:38'}):
            controller.keyboard_preflight([binding])
            with self.assertRaisesRegex(ValueError, '^plain_browser_key_a_must_be_unbound$'):
                controller.keyboard_preflight([binding], browser_key=True)

    def test_additional_cdp_transport_is_exact_pinned_source_and_default_never_loads_it(self):
        self.assertEqual(hashlib.sha256(controller.key_transport()).hexdigest(), controller.KEY_CDP_SHA)
        with patch.object(controller.Path, 'read_bytes', return_value=b'locally changed transport'):
            with self.assertRaisesRegex(ValueError, '^reviewed_browser_key_transport_required$'):
                controller.key_transport()
        with patch.object(controller, 'key_transport', side_effect=AssertionError('optional transport forbidden')), \
             patch.object(sys, 'argv', ['controller']), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(controller.main(), 0)

    def test_real_mode_wires_one_linux_key_a_command_and_no_dom_key_dispatch(self):
        tree = ast.parse((ROOT / 'vm/test-default-uri-notification.py').read_text())
        calls = [node for node in ast.walk(tree) if isinstance(node, ast.Call)
                 and isinstance(node.func, ast.Attribute) and node.func.attr == 'virsh'
                 and any(isinstance(arg, ast.Constant) and arg.value == '30' for arg in node.args)]
        self.assertEqual(len(calls), 1)
        self.assertEqual([arg.value for arg in calls[0].args if isinstance(arg, ast.Constant)],
                         ['send-key', '--codeset', 'linux', '--holdtime', '50', '30'])
        self.assertEqual([(item.arg, ast.literal_eval(item.value)) for item in calls[0].keywords], [('capture', True)])
        handoff = next(node for node in ast.walk(tree) if isinstance(node, ast.Call)
                       and isinstance(node.func, ast.Name) and node.func.id == 'assert_browser_key_handoff')
        callback = handoff.args[6]
        self.assertIsInstance(callback, ast.Lambda)
        self.assertIn("old.a['keyboard_preflight']()", ast.unparse(callback))
        self.assertIn("keyboard_preflight(json.loads(session('hyprctl', '-j', 'binds')), browser_key=True)", ast.unparse(callback))


class OwnedBrowserKeyBridge(unittest.TestCase):
    def fixture(self, run):
        control = {'id': 'blank', 'url': 'about:blank', 'title': ''}
        target = {'id': 'fresh', 'url': 'http://127.0.0.1:1234/owned', 'title': 'Owned'}
        endpoint = {'port': 12345, 'browser': '/devtools/browser/original'}
        identity = {'control': control, 'endpoint': endpoint}
        request = {'identity': identity, 'before': [control], 'targets': [target], 'case': 'pointer'}
        from application_intent_browser import pages, fresh_documents
        base = types.SimpleNamespace(browser_authority=Mock(return_value=identity),
            context=Mock(return_value=(run, {})), browser_pages=Mock(return_value=[control, target]),
            request_documents=Mock(return_value=[{'uri': target['url'], 'title': target['title']}]),
            fresh_documents=fresh_documents, browser_endpoint=Mock(return_value=endpoint), pages=pages,
            browser_http=Mock(return_value=json.dumps([{'type': 'page', **control},
                {'type': 'page', **target, 'webSocketDebuggerUrl': 'ws://127.0.0.1:12345/devtools/page/fresh'}])))
        peer = Mock()
        peer.call.return_value = {'result': {'type': 'object', 'value': {'armed': True, 'count': 0}}}
        transport = types.SimpleNamespace(Peer=types.SimpleNamespace(connect=Mock(return_value=peer)))
        return base, request, peer, transport

    def test_actual_bridge_arms_and_reads_only_exact_original_fresh_target_without_user_gesture(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary);base, request, peer, transport = self.fixture(run)
            with patch.object(driver, 'context', return_value=(run, {'assert_browser_key_handoff': True}, base)), \
                 patch.object(driver, 'admitted'), patch.dict(sys.modules, {'cdp_peer': transport}):
                self.assertEqual(driver.browser_key(run, 'abcdef123456', request, True), {'armed': True, 'count': 0})
                transport.Peer.connect.assert_called_once_with('ws://127.0.0.1:12345/devtools/page/fresh', 12345, 'fresh')
                args = peer.call.call_args.args
                self.assertEqual(args[0], 'Runtime.evaluate')
                self.assertIs(args[1]['userGesture'], False)
                self.assertIs(args[1]['returnByValue'], True)
                peer.call.return_value = {'result': {'value': {'count': 0, 'event': None}}}
                self.assertEqual(driver.browser_key(run, 'abcdef123456', request, False), {'count': 0, 'event': None})
                with self.assertRaisesRegex(ValueError, '^new_key_capture_required$'):
                    driver.browser_key(run, 'abcdef123456', request, True)
                self.assertEqual(peer.close.call_count, 2)

    def test_actual_cli_dispatches_both_key_operations_with_exact_json_request(self):
        request = {'case': 'pointer', 'identity': {}, 'before': [], 'targets': []}
        for operation, arm in (('browser-key-arm', True), ('browser-key-read', False)):
            with self.subTest(operation=operation), \
                 patch.object(sys, 'argv', ['fixture', operation, '/owned-run', 'abcdef123456', json.dumps(request)]), \
                 patch.object(driver.signal, 'signal'), \
                 patch.object(driver, 'context', return_value=(Path('/owned-run'), {}, Mock())), \
                 patch.object(driver, 'browser_key', return_value={'controlled': True}) as handler, \
                 contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(driver.main(), 0)
                self.assertEqual(json.loads(output.getvalue()), {'controlled': True})
                handler.assert_called_once_with(Path('/owned-run'), 'abcdef123456', request, arm)

    def test_default_mode_foreign_target_and_changed_endpoint_refuse_before_cdp(self):
        for mutation in ('default', 'foreign_target', 'endpoint'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                run = Path(temporary);base, request, _, transport = self.fixture(run)
                header = {'assert_browser_key_handoff': mutation != 'default'}
                if mutation == 'foreign_target':request['targets'][0] = {**request['targets'][0], 'id': 'foreign'}
                if mutation == 'endpoint':base.browser_endpoint.return_value = {'port': 23456}
                with patch.object(driver, 'context', return_value=(run, header, base)), \
                     patch.object(driver, 'admitted'), patch.dict(sys.modules, {'cdp_peer': transport}):
                    with self.assertRaises(ValueError):driver.browser_key(run, 'abcdef123456', request, True)
                transport.Peer.connect.assert_not_called()
                self.assertEqual(list(run.iterdir()), [])

    def test_target_navigation_after_evaluation_cannot_create_a_capture_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary);base, request, _, transport = self.fixture(run)
            base.browser_pages.side_effect = [[request['before'][0], request['targets'][0]], []]
            with patch.object(driver, 'context', return_value=(run, {'assert_browser_key_handoff': True}, base)), \
                 patch.object(driver, 'admitted'), patch.dict(sys.modules, {'cdp_peer': transport}):
                with self.assertRaisesRegex(ValueError, '^exact_owned_key_target_endpoint_required$'):
                    driver.browser_key(run, 'abcdef123456', request, True)
            self.assertEqual(list(run.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
