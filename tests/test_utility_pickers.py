"""Utility adapter behavior with temporary history and mocked OS commands only."""
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import clipboard_store as store


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


controls = load('utility_controls', 'lucent-controls.py')
menu = load('utility_menu', 'lucent-menu.py')
routes = load('utility_routes', 'lucent-menu-routes.py')
compat = load('utility_compat', 'lucent-compat.py')


class UtilityRecovery(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = patch.object(store, 'ROOT', Path(temporary.name) / 'history')
        root.start()
        self.addCleanup(root.stop)
        store.capture('image/png', store.PNG + b'fixture')
        (store.ROOT / 'history.json').write_bytes(b'{private fixture: unreadable')
        self.saved = {path.name: path.read_bytes() for path in store.ROOT.iterdir()}

    def test_unreadable_history_has_an_explicit_reset_and_cancel_preserves_exact_files(self):
        for returncode, selection in [(1, ''), (0, '2'), (0, '0')]:
            with self.subTest(returncode=returncode, selection=selection), \
                    patch.object(controls.picker, 'pick', return_value=SimpleNamespace(
                        returncode=returncode, stdout=selection)) as pick, \
                    patch.object(controls, 'paste') as paste:
                controls.clipboard()
                rows = pick.call_args.args[0]['entries']
                self.assertTrue(rows[0]['disabled'])
                self.assertEqual(rows[1]['label'], 'Reset saved clipboard history')
                self.assertEqual(rows[2]['label'], 'Cancel')
                self.assertNotIn('private fixture', json.dumps(rows))
                self.assertEqual({path.name: path.read_bytes() for path in store.ROOT.iterdir()}, self.saved)
                paste.assert_not_called()

    def test_explicit_reset_recovers_and_next_selection_pastes_exact_text(self):
        with patch.object(controls.picker, 'pick', return_value=SimpleNamespace(returncode=0, stdout='1')), \
                patch.object(controls, 'paste') as paste:
            controls.clipboard()
            paste.assert_not_called()
        self.assertEqual(store.read(), [])
        self.assertFalse(list(store.ROOT.glob('*.png')))
        text = 'Recovered\n😀 trailing space '
        store.capture('text', text.encode())
        with patch.object(controls.picker, 'pick', return_value=SimpleNamespace(returncode=0, stdout='0')), \
                patch.object(controls, 'paste') as paste:
            controls.clipboard()
            paste.assert_called_once_with(text=text)

    def test_failed_explicit_reset_is_visible_and_keeps_saved_files(self):
        with patch.object(controls.picker, 'pick', return_value=SimpleNamespace(returncode=0, stdout='1')), \
                patch.object(store, 'clear', side_effect=OSError('Could not reset saved history')), \
                patch.object(controls, 'information') as information, patch.object(controls, 'paste') as paste:
            self.assertEqual(controls.main('clipboard'), 1)
            information.assert_called_once_with('Clipboard', 'Could not reset saved history')
            paste.assert_not_called()
        self.assertEqual({path.name: path.read_bytes() for path in store.ROOT.iterdir()}, self.saved)

    def test_invalid_recovery_selection_cannot_clear_or_paste(self):
        with patch.object(controls.picker, 'pick', return_value=SimpleNamespace(returncode=0, stdout='reset')), \
                patch.object(store, 'clear') as clear, patch.object(controls, 'paste') as paste:
            with self.assertRaises(ValueError):
                controls.clipboard()
            clear.assert_not_called()
            paste.assert_not_called()

    def test_image_selection_revalidates_owned_bytes_after_picker_exits(self):
        store.clear()
        data = store.PNG + b'image fixture'
        store.capture('image/png', data)
        path = Path(store.read()[0]['path'])
        with patch.object(controls.picker, 'pick', return_value=SimpleNamespace(returncode=0, stdout='0')), \
                patch.object(controls, 'paste') as paste:
            controls.clipboard()
            paste.assert_called_once_with(image=data)

        def select_after_eviction(*args, **kwargs):
            path.unlink()
            return SimpleNamespace(returncode=0, stdout='0')

        with patch.object(controls.picker, 'pick', side_effect=select_after_eviction), \
                patch.object(controls, 'paste') as paste:
            with self.assertRaises(store.HistoryError):
                controls.clipboard()
            paste.assert_not_called()

    def test_copy_failure_never_types_into_the_returned_application(self):
        for arguments in [{'text': 'literal $(fixture)'}, {'image': store.PNG + b'fixture'}]:
            with self.subTest(arguments=arguments), patch.object(controls.subprocess, 'run',
                    side_effect=subprocess.CalledProcessError(1, ['fixture-copy'])) as run, \
                    patch.object(controls.time, 'sleep') as sleep:
                with self.assertRaises(subprocess.CalledProcessError):
                    controls.paste(**arguments)
                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args.args[0][0], 'wl-copy')
                sleep.assert_not_called()

    def test_paste_copies_literal_bounded_payload_before_shift_insert(self):
        for arguments, expected in [({'text': '😀\n$(fixture)'}, '😀\n$(fixture)'.encode()),
                                    ({'image': store.PNG + b'fixture'}, store.PNG + b'fixture')]:
            with self.subTest(arguments=arguments), patch.object(controls.subprocess, 'run') as run, \
                    patch.object(controls.time, 'sleep'):
                controls.paste(**arguments)
                self.assertEqual(run.call_count, 2)
                self.assertEqual(run.call_args_list[0].kwargs['input'], expected)
                self.assertEqual(run.call_args_list[1].args[0],
                                 ['wtype', '-M', 'shift', '-k', 'Insert', '-m', 'shift'])


class UtilityDismissal(unittest.TestCase):
    def test_shared_picker_hides_panels_before_launch_and_preserves_response_contract(self):
        result = SimpleNamespace(returncode=0, stdout='literal value\n')
        for mode in ['select', 'input']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary, \
                    patch.dict(menu.os.environ, {'XDG_STATE_HOME': temporary}), \
                    patch.object(menu.subprocess, 'run', side_effect=[SimpleNamespace(returncode=0), result]) as run:
                payload = {'mode': mode, 'prompt': 'Capture fixture',
                           'entries': [{'label': 'Literal', 'value': '$(fixture)'}]}
                self.assertIs(menu.pick(payload, capture=True), result)
                hide, pick = run.call_args_list
                self.assertEqual(hide.args[0], [str(Path.home() / '.local/bin/lucent-cli'), 'shell', 'hide'])
                self.assertTrue(hide.kwargs['check'])
                self.assertEqual(hide.kwargs['timeout'], 3)
                self.assertEqual(pick.args[0], [str(Path.home() / '.local/bin/lucent-menu')])
                self.assertEqual(json.loads(pick.kwargs['input']), payload)
                self.assertEqual(pick.kwargs['stdout'], subprocess.PIPE)

    def test_failed_hide_blocks_picker_and_reports_no_captured_command_output(self):
        failures = [FileNotFoundError('private fixture path'),
                    subprocess.CalledProcessError(1, ['fixture'], stderr='private fixture'),
                    subprocess.TimeoutExpired(['fixture'], 3, output='private fixture')]
        for failure in failures:
            with self.subTest(failure=type(failure)), tempfile.TemporaryDirectory() as temporary, \
                    patch.dict(menu.os.environ, {'XDG_STATE_HOME': temporary}), \
                    patch.object(menu.subprocess, 'run', side_effect=failure) as run:
                with self.assertRaises(menu.DismissalError) as error:
                    menu.pick({'prompt': 'Fixture', 'entries': []})
                self.assertEqual(run.call_count, 1)
                self.assertNotIn('private fixture', str(error.exception))

    def test_utility_hide_failure_does_not_try_another_picker_to_report_it(self):
        with tempfile.TemporaryDirectory() as temporary, \
                patch.dict(controls.os.environ, {'XDG_STATE_HOME': temporary}), \
                patch.object(controls.picker, 'dismiss_shell', side_effect=controls.picker.DismissalError('Try again')), \
                patch.object(controls.subprocess, 'run') as run, \
                patch.object(controls.sys, 'stderr', new_callable=io.StringIO) as stderr:
            self.assertEqual(controls.main('clock'), 1)
            run.assert_not_called()
            self.assertIn('Try again', stderr.getvalue())

    def test_clipboard_and_emoji_reach_the_same_hide_before_picker_boundary(self):
        for name in ['clipboard', 'emojis']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                emojis = root / 'shell/plugins/emojis/emojis.json'
                emojis.parent.mkdir(parents=True)
                emojis.write_text(json.dumps([{'k': 'Smile', 'e': '😀'}]))
                calls = []

                def run(argv, **kwargs):
                    calls.append(argv)
                    return SimpleNamespace(returncode=0 if argv[-2:] == ['shell', 'hide'] else 1, stdout='')

                with patch.object(store, 'ROOT', root / 'history'), \
                        patch.dict(controls.os.environ, {'OMARCHY_PATH': temporary, 'XDG_STATE_HOME': temporary}), \
                        patch.object(controls.subprocess, 'run', side_effect=run), patch.object(controls, 'paste') as paste:
                    self.assertEqual(controls.main(name), 0)
                    self.assertEqual(calls, [[str(Path.home() / '.local/bin/lucent-cli'), 'shell', 'hide'],
                                             [str(Path.home() / '.local/bin/lucent-menu')]])
                    paste.assert_not_called()

    def test_compat_utility_open_hides_all_panels_before_launch(self):
        for name in ['clipboard', 'emojis', 'reminders']:
            selector = SimpleNamespace(picker_command=Mock(return_value=None))
            calls = []
            with self.subTest(name=name), patch.dict(sys.modules, {'menu_routes': selector}), \
                    patch.object(compat, 'cli', side_effect=lambda *a: calls.append(('cli', a))), \
                    patch.object(compat, 'launch', side_effect=lambda *a: calls.append(('launch', a))):
                self.assertEqual(compat.panel(name), 'ok')
                self.assertEqual(calls, [('cli', ('shell', 'hide')), ('launch', ('controls.py', name))])

    def test_compat_toggle_existing_utility_only_closes_it(self):
        selector = SimpleNamespace(picker_command=Mock(return_value=json.dumps({'client': {'prompt': 'Emoji'}})))
        with patch.dict(sys.modules, {'menu_routes': selector}), patch.object(compat, 'cli') as cli, \
                patch.object(compat, 'launch') as launch:
            self.assertEqual(compat.panel('emojis', toggle=True), 'ok')
            self.assertEqual(selector.picker_command.call_args_list[-1].args, ('close',))
            cli.assert_not_called()
            launch.assert_not_called()

    def test_compat_hide_failure_cannot_launch_a_utility(self):
        selector = SimpleNamespace(picker_command=Mock(return_value=None))
        with patch.dict(sys.modules, {'menu_routes': selector}), \
                patch.object(compat, 'cli', side_effect=subprocess.CalledProcessError(1, ['fixture'])), \
                patch.object(compat, 'launch') as launch:
            with self.assertRaises(subprocess.CalledProcessError):
                compat.panel('clipboard')
            launch.assert_not_called()

    def test_direct_capture_route_hides_before_the_trusted_owner_is_dispatched(self):
        action = ['omarchy-cmd-screenshot', 'region']
        model = routes.Model({'capture': {'label': 'Capture', 'action': action}}, {})
        calls = []
        with tempfile.TemporaryDirectory() as temporary, patch.dict(routes.os.environ, {'XDG_RUNTIME_DIR': temporary}), \
                patch.object(routes, 'load', return_value=model), patch.object(routes, 'picker_command', return_value=None), \
                patch.object(routes, 'condition', side_effect=lambda c, d: d), \
                patch.object(routes.subprocess, 'run', side_effect=lambda argv, **kw: calls.append(('hide', argv))), \
                patch.object(routes.subprocess, 'Popen', side_effect=lambda argv, **kw: calls.append(('action', argv))):
            pick = Mock()
            self.assertEqual(routes.run(['summon', 'capture'], pick, menu.dismiss_shell), 0)
            self.assertEqual(calls, [('hide', [str(Path.home() / '.local/bin/lucent-cli'), 'shell', 'hide']),
                                     ('action', action)])
            pick.assert_not_called()

    def test_route_hide_failure_prevents_picker_and_direct_dispatch(self):
        model = routes.Model({'capture': {'label': 'Capture', 'action': ['fixture-capture']}}, {})
        for route in ['root', 'capture']:
            with self.subTest(route=route), tempfile.TemporaryDirectory() as temporary, \
                    patch.dict(routes.os.environ, {'XDG_RUNTIME_DIR': temporary}), \
                    patch.object(routes, 'load', return_value=model), patch.object(routes, 'picker_command', return_value=None), \
                    patch.object(routes, 'condition', side_effect=lambda c, d: d), patch.object(routes, 'dispatch') as dispatch:
                pick = Mock()
                with self.assertRaises(menu.DismissalError):
                    routes.run(['summon', route], pick, Mock(side_effect=menu.DismissalError('Try again')))
                pick.assert_not_called()
                dispatch.assert_not_called()

    def test_nonopening_or_invalid_routes_do_not_dismiss_panels(self):
        model = routes.Model({'disabled': {'label': 'Disabled', 'disabled': 'yes', 'action': 'fixture'}}, {})
        for arguments, opened, fails in [(['help'], False, False), (['ping'], False, False),
                                         (['refresh'], False, False), (['close'], False, False),
                                         (['toggle'], True, False), (['invalid'], False, True),
                                         (['summon', 'missing'], False, True),
                                         (['summon', 'disabled'], False, True)]:
            with self.subTest(arguments=arguments), tempfile.TemporaryDirectory() as temporary, \
                    patch.dict(routes.os.environ, {'XDG_RUNTIME_DIR': temporary}), patch.object(routes, 'load', return_value=model), \
                    patch.object(routes, 'condition', side_effect=lambda c, d: True if c == 'yes' else d), \
                    patch.object(routes, 'picker_command', return_value='existing' if opened else None), \
                    patch.object(sys, 'stdout', new_callable=io.StringIO):
                before_open, pick = Mock(), Mock()
                if fails:
                    with self.assertRaises(ValueError):
                        routes.run(arguments, pick, before_open)
                else:
                    self.assertEqual(routes.run(arguments, pick, before_open), 0)
                before_open.assert_not_called()
                pick.assert_not_called()

    def test_menu_entrypoint_passes_shared_dismissal_to_routes_and_rollback_skips_it(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(menu.Path, 'home', return_value=Path(temporary)), \
                patch.object(menu.sys, 'argv', ['menu.py', 'routes', 'summon', 'capture']):
            enabled = Path(temporary) / '.local/state/lucent/integration-backup/menu-enabled'
            enabled.parent.mkdir(parents=True)
            enabled.touch()
            adapter = SimpleNamespace(run=Mock(return_value=0))
            with patch.dict(sys.modules, {'menu_routes': adapter}):
                self.assertEqual(menu.main(), 0)
                adapter.run.assert_called_once_with(['summon', 'capture'], menu.pick, menu.dismiss_shell)
            enabled.unlink()
            with patch.object(menu.os, 'execv', side_effect=SystemExit(0)) as stock, \
                    patch.object(menu, 'dismiss_shell') as hide:
                with self.assertRaises(SystemExit):
                    menu.main()
                stock.assert_called_once_with('/usr/bin/omarchy-menu', ['omarchy-menu', 'summon', 'capture'])
                hide.assert_not_called()
