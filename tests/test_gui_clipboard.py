"""Input authority checks for the real-browser clipboard acceptance driver."""

import importlib.util
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('gui_clipboard_driver', ROOT / 'vm/test-gui-clipboard.py')
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class InputAdmissionTests(unittest.TestCase):
    def harness(self, focused=True):
        h = driver.Harness.__new__(driver.Harness)
        h.input_deadline = None
        h.browser_started = True
        h.original_monitor = {'name': 'Virtual-1', 'width': 1920, 'height': 1080, 'scale': 1}
        h.workspace, h.notes_workspace, h.browser_workspace = 1, 'owned-notes', 'owned-browser'
        h.target = 'browser'
        self.native_client = {'pid': 100, 'address': '0x123', 'class': 'owned'}
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
                      {'modmask': 4, 'key': 'v'}, {'modmask': 4, 'key': 'catchall'}):
            with self.subTest(value=value), self.assertRaises(driver.CheckFailure):
                driver.require_safe_bindings([value])

    def test_unrelated_omarchy_window_management_binding_remains_usable(self):
        driver.require_safe_bindings([{'modmask': 64, 'key': 'Return'},
                                      {'modmask': 64, 'key': 'n'}, {'modmask': 4, 'key': 'q'}])


if __name__ == '__main__':
    unittest.main()
