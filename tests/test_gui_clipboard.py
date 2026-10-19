"""Input authority checks for the real-browser clipboard acceptance driver."""

import importlib.util
import copy
import json
from pathlib import Path
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('gui_clipboard_driver', ROOT / 'vm/test-gui-clipboard.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class InputAdmissionTests(unittest.TestCase):
    def harness(self, focused=True):
        h = driver.Harness.__new__(driver.Harness)
        h.input_deadline = None
        h.browser_started = True
        h.browser_attempted = h.start_acknowledged = True
        h.browser_retired = False
        h.browser_identity = {'captured': True}
        h.desktop_pid = 42
        h.cursor_owner = None
        h.original_monitor = {'name': 'Virtual-1', 'width': 1920, 'height': 1080, 'scale': 1}
        h.workspace, h.notes_workspace, h.browser_workspace = 1, 'owned-notes', 'owned-browser'
        h.target = 'browser'
        h.verify_binary = lambda: 42
        self.native_client = {'pid': 100, 'address': '0x123', 'class': 'owned',
                              'workspace': {'id': 2, 'name': 'owned-browser'}}
        h.browser_window = dict(self.native_client)
        self.current_workspace = {'id': -2, 'name': 'owned-browser'}
        self.document_focused = focused
        self.peer_calls = []
        def session(*argv, **_kwargs):
            if argv == ('omarchy-shell', 'lock', 'isLocked'):
                return b'false'
            values = {'layers': {}, 'clients': [self.native_client],
                      'binds': [],
                      'monitors': [h.original_monitor], 'activeworkspace': self.current_workspace,
                      'activewindow': self.native_client}
            return json.dumps(values[argv[-1]]).encode()
        def peer(operation, *_args, **_kwargs):
            self.peer_calls.append(operation)
            return {'owned_clients': 1} if operation == 'clients' else {'focused': self.document_focused}
        h.session, h.call_peer = session, peer
        h.inspect = lambda: {'client': {'launcher': False, 'query': '', 'widgets_editing': False}}
        return h

    def test_native_activewindow_does_not_authorize_keys_to_blurred_document(self):
        h = self.harness(focused=False)
        with self.assertRaisesRegex(driver.CheckFailure, '^owned_browser_document_has_keyboard_focus$'):
            h.input_guard()
        self.assertEqual(self.peer_calls, ['clients', 'dom'])

    def test_document_focus_must_be_rechecked_after_an_accepted_input(self):
        h = self.harness()
        h.input_guard()
        self.document_focused = False
        with self.assertRaisesRegex(driver.CheckFailure, 'owned_browser_document_has_keyboard_focus'):
            h.input_guard()
        self.assertEqual(self.peer_calls.count('dom'), 2)

    def test_unowned_workspace_is_rejected_before_dom_input_preparation(self):
        h = self.harness()
        self.current_workspace = {'id': 5, 'name': 'foreign'}
        with self.assertRaisesRegex(driver.CheckFailure, 'only_owned_or_original_workspace'):
            h.input_guard()
        self.assertNotIn('dom', self.peer_calls)

    def test_timeout_cannot_be_extended_by_successful_previous_checks(self):
        h = self.harness()
        h.input_deadline = 0
        with self.assertRaisesRegex(driver.CheckFailure, 'bounded_native_input_window'):
            h.input_guard()
        self.assertEqual(self.peer_calls, [])

    def test_same_class_replacement_window_cannot_inherit_input_authority(self):
        h = self.harness()
        self.native_client = dict(self.native_client, address='0x456')
        with self.assertRaisesRegex(driver.CheckFailure, 'same_admitted_browser_native_window'):
            h.input_guard()

    def test_old_logical_notes_focus_cannot_authorize_keys_to_active_browser(self):
        h = self.harness()
        h.target = 'notes'
        self.current_workspace = {'id': 3, 'name': h.notes_workspace}
        h.notes_surface = lambda: ({'focus': 'notes-input'}, {})
        with self.assertRaisesRegex(driver.CheckFailure, 'no_managed_window_has_notes_keyboard_focus'):
            h.input_guard()

    def test_refused_or_lost_start_blocks_every_further_input(self):
        h = self.harness()
        h.start_acknowledged = False
        h.browser_started = False
        with self.assertRaisesRegex(driver.CheckFailure, 'browser_admission_or_original_retirement_required'):
            h.input_guard()
        self.assertEqual(self.peer_calls, [])

    def test_independent_cursor_movement_remains_lost_when_it_returns(self):
        h = self.harness()
        h.cursor_owner = driver.input_policy.CursorOwnership({'x': 10, 'y': 20})
        original = h.session
        position = {'x': 11, 'y': 20}
        h.session = lambda *args, **kwargs: json.dumps(position).encode() if args[-1] == 'cursorpos' else original(*args, **kwargs)
        with self.assertRaisesRegex(ValueError, 'application_cursor_authority_lost'):
            h.input_guard()
        position['x'] = 10
        with self.assertRaisesRegex(ValueError, 'application_cursor_authority_lost'):
            h.input_guard()

    def test_foreign_or_secure_state_never_stops_desktop_for_restoration(self):
        h = self.harness()
        h.input_guard = Mock(side_effect=driver.CheckFailure('foreign'))
        h.restoring_desktop = False
        with patch.object(driver.notes.Harness, 'stop_quiescent') as stop:
            with self.assertRaisesRegex(driver.CheckFailure, 'foreign'):
                h.stop_quiescent()
        stop.assert_not_called()
        self.assertFalse(h.restoring_desktop)

    def test_foreign_workspace_blocks_restore_before_dispatch(self):
        h = self.harness()
        h.target = None
        self.current_workspace = {'id': 6, 'name': 'foreign'}
        with self.assertRaisesRegex(driver.CheckFailure, 'only_owned_or_original_workspace'):
            h.restore_workspace()

    def test_original_notes_harness_also_refuses_foreign_workspace_restore(self):
        h = driver.notes.Harness.__new__(driver.notes.Harness)
        h.workspace, h.tag = 1, 'abc'
        calls = []
        def session(*argv):
            calls.append(argv)
            return b'{"id":7,"name":"foreign"}'
        h.session = session
        with self.assertRaisesRegex(driver.CheckFailure, 'workspace_still_original_or_owned_before_restore'):
            h.restore_workspace()
        self.assertEqual(calls, [('hyprctl', '-j', 'activeworkspace')])


class BindingAdmissionTests(unittest.TestCase):
    def test_plain_text_pointer_control_and_catchall_bindings_are_rejected(self):
        for value in ({'key': 'n'}, {'key': 'space'}, {'key': 'Return'}, {'key': 'mouse:272'},
                      {'key': 'code:57'}, {'keycode': 65}, {'key': 'catchall'},
                      {'key': 'unused', 'catch_all': True}, {'key': 'any'}, {'key': 'mouse:left'}, {'key': 'Control_L'},
                      {'modmask': 4, 'key': 'v'}, {'modmask': 4, 'key': 'catchall'}):
            with self.subTest(value=value), self.assertRaises(driver.CheckFailure):
                driver.require_safe_bindings([value])

    def test_unrelated_omarchy_window_management_binding_remains_usable(self):
        driver.require_safe_bindings([{'modmask': 64, 'key': 'Return'},
                                      {'modmask': 64, 'key': 'n'}, {'modmask': 4, 'key': 'q'}])


class NativeLayerAdmissionTests(unittest.TestCase):
    def test_foreign_same_namespace_layer_cannot_intercept_input(self):
        layers = {'Virtual-1': {'levels': {'2': [
            {'namespace': 'lucent-bar', 'pid': 42}, {'namespace': 'lucent-panel', 'pid': 99}]}}}
        with self.assertRaisesRegex(driver.CheckFailure, 'no_foreign_native_layer_before_input'):
            driver.require_desktop_layers(layers, 42)

    def test_current_desktop_layers_on_nested_native_levels_are_admitted(self):
        driver.require_desktop_layers({'Virtual-1': {'levels': {'0': [
            {'namespace': 'lucent-background', 'pid': 42}], '2': [
            {'namespace': 'lucent-bar', 'pid': 42}]}}}, 42)

    def test_authorization_layer_is_refused_even_with_same_process_owner(self):
        with self.assertRaises(driver.CheckFailure):
            driver.require_desktop_layers({'namespace': 'lucent-authorization-test', 'pid': 42}, 42)


class NotesPointerLifetimeTests(unittest.TestCase):
    def fixture(self):
        surface = {'id': 'widgets', 'instance': 'one', 'output': 'Virtual-1', 'frames': 1,
                   'native_frames': 1, 'pointer_input': 'content', 'width': 640, 'height': 480}
        hit = {'id': 'notes-input', 'x': 24, 'y': 60, 'width': 592, 'height': 240,
               'clip': {'x': 0, 'y': 0, 'width': 640, 'height': 480}}
        layer = {'namespace': 'lucent-widgets', 'pid': 42, 'address': '0xabc',
                 'x': 640, 'y': 100, 'w': 640, 'h': 480}
        return surface, hit, layer

    def target(self):
        return driver.notes_pointer_target(*self.fixture(), 42)

    def test_target_binds_layer_instance_geometry_clip_and_submitted_surface(self):
        surface, hit, layer = self.fixture()
        target = self.target()
        self.assertEqual(target['point'], (960, 280))
        for source, key, value in ((surface, 'instance', 'two'), (surface, 'output', 'Virtual-2'),
                                   (layer, 'address', '0xdef'), (layer, 'x', 650), (hit, 'x', 25)):
            changed = [copy.deepcopy(item) for item in (surface, hit, layer)]
            changed[(surface, hit, layer).index(source)][key] = value
            with self.subTest(key=key, value=value):
                self.assertNotEqual(driver.notes_pointer_target(*changed, 42), target)
        for index, key, value in ((0, 'native_frames', 0), (0, 'pointer_input', 'none'),
                                   (2, 'pid', 99), (2, 'address', ''), (2, 'w', 630),
                                   (1, 'clip', {'x': 0, 'y': 0, 'width': 640, 'height': 200})):
            changed = [copy.deepcopy(item) for item in (surface, hit, layer)];changed[index][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(driver.CheckFailure):
                driver.notes_pointer_target(*changed, 42)

    def harness(self):
        h = driver.Harness.__new__(driver.Harness)
        h.input_guard = Mock();h.move = Mock();h.qmp = Mock();h.eventually = Mock()
        h.notes_press_target = None
        h.notes_surface = lambda: ({'focus': 'notes-input'}, {})
        return h

    def test_moved_recreated_clipped_or_retired_notes_never_gets_physical_down(self):
        target = self.target()
        variants = []
        for part, key, value in (('layer', 'address', '0xdef'), ('layer', 'x', 650), ('hit', 'x', 25)):
            changed = copy.deepcopy(target);changed[part][key] = value;variants.append(changed)
        changed = copy.deepcopy(target);changed['instance'] = 'replacement';variants.append(changed)
        variants += [driver.CheckFailure('fully_visible_notes_pointer_target'),
                     driver.CheckFailure('submitted_notes_pointer_target')]
        for changed in variants:
            h = self.harness();h.current_notes_target = Mock(side_effect=[target, target, changed])
            with self.subTest(changed=changed), patch.object(driver.time, 'sleep'), self.assertRaises(driver.CheckFailure):
                h.focus()
            h.qmp.assert_not_called()
            h.move.assert_called_once_with(*target['point'])

    def test_changed_target_after_slow_guard_is_rejected_before_qmp_command(self):
        h = InputAdmissionTests().harness();h.target = None
        h.notes_press_target = self.target();changed = copy.deepcopy(h.notes_press_target)
        changed['layer']['address'] = '0xdef'
        h.current_notes_target = Mock(return_value=changed);h.run = Mock()
        with self.assertRaisesRegex(driver.CheckFailure, 'same_notes_hit_and_native_lifetime_before_press'):
            h.qmp([{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
        h.run.assert_not_called()

    def test_lost_down_reply_still_balances_release_and_retires_pending_target(self):
        h = self.harness();target = self.target()
        h.current_notes_target = Mock(return_value=target)
        h.qmp.side_effect = [driver.CheckFailure('lost_down_reply'), None]
        with patch.object(driver.time, 'sleep'), self.assertRaisesRegex(driver.CheckFailure, 'lost_down_reply'):
            h.focus()
        self.assertEqual([call.args[0][0]['data']['down'] for call in h.qmp.call_args_list], [True, False])
        self.assertIsNone(h.notes_press_target)


class RestorationAdmissionTests(unittest.TestCase):
    def harness(self):
        h = driver.Harness.__new__(driver.Harness)
        h.browser_attempted = True
        h.start_acknowledged = False
        h.browser_origin = h.browser_identity = None
        h.browser_started = True
        h.browser_retired = False
        h.peer_initialized = True
        h.original_cursor = None
        h.call_peer = Mock()
        h.input_guard = Mock()
        return h

    def test_failed_or_uncertain_start_does_not_retire_any_unit_or_restore_notes(self):
        h = self.harness();report = {'restoration': {}}
        with patch.object(driver.notes, 'restore') as restore:
            driver.restore(h, report)
        h.call_peer.assert_not_called()
        restore.assert_not_called()
        self.assertFalse(report['restoration']['owned_browser_retired'])
        self.assertFalse(report['restoration']['guarded_notes_restoration'])

    def test_failed_original_browser_retirement_keeps_notes_guard_and_profile(self):
        h = self.harness();h.start_acknowledged = True;h.browser_origin = {'original': True}
        h.call_peer.side_effect = driver.CheckFailure('original_invocation_changed')
        report = {'restoration': {}, 'completed_checks': True}
        with patch.object(driver.notes, 'restore') as restore:
            driver.restore(h, report)
        self.assertEqual(h.call_peer.call_args_list, [unittest.mock.call('retire')])
        restore.assert_not_called()
        self.assertTrue(h.browser_started)
        self.assertFalse(h.browser_retired)
        self.assertFalse(report['restoration']['owned_browser_retired'])

    def test_successfully_retired_browser_still_requires_live_idle_guard(self):
        h = self.harness();h.start_acknowledged = True;h.browser_origin = {'original': True}
        h.eventually = lambda predicate, *_args, **_kwargs: self.assertTrue(predicate())
        h.session = lambda *_args, **_kwargs: b'[]'
        h.input_guard.side_effect = driver.CheckFailure('authorization_open')
        report = {'restoration': {}}
        with patch.object(driver.notes, 'restore') as restore:
            driver.restore(h, report)
        restore.assert_not_called()
        self.assertTrue(report['restoration']['owned_browser_retired'])
        self.assertFalse(report['restoration']['guarded_notes_restoration'])


if __name__ == '__main__':
    unittest.main()
