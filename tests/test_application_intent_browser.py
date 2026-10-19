"""Fresh real-browser target proof; pure policy and mocked HTTP/owner boundaries."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import application_intent_browser as policy
spec = importlib.util.spec_from_file_location('intent_browser_fixture', ROOT / 'vm/fixtures/application-intent-fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)

CONTROL = {'id': 'control', 'url': 'about:blank', 'title': ''}
FILES = [{'uri': 'file:///owned/a%20b%25%E9%9B%AA%24%28unused%29%3B.html', 'title': 'first'},
         {'uri': 'file:///owned/second%20file.html', 'title': 'second'}]
NEW = [{'id': str(index), 'url': item['uri'], 'title': item['title']}
       for index, item in enumerate(FILES)]


class FreshDocumentEvidence(unittest.TestCase):
    def test_both_new_literal_documents_qualify_by_target_id_url_and_title(self):
        self.assertEqual(policy.fresh_documents([CONTROL], [CONTROL, *NEW], FILES, CONTROL), NEW)

    def test_old_matching_documents_cannot_prove_a_later_request(self):
        before = [CONTROL, *NEW]
        self.assertIsNone(policy.fresh_documents(before, before, FILES, CONTROL))

    def test_one_fresh_document_cannot_reuse_the_other_old_document(self):
        before = [CONTROL, NEW[0]]
        current = [*before, NEW[1]]
        self.assertIsNone(policy.fresh_documents(before, current, FILES, CONTROL))

    def test_duplicate_wrong_pending_or_extra_pages_do_not_qualify(self):
        for new in ([NEW[0]], [NEW[0], dict(NEW[0], id='other')],
                    [NEW[0], dict(NEW[1], title='pending')],
                    [NEW[0], dict(NEW[1], url='file:///foreign')],
                    [*NEW, {'id': 'extra', 'url': 'about:blank', 'title': ''}]):
            with self.subTest(new=new):
                self.assertIsNone(policy.fresh_documents([CONTROL], [CONTROL, *new], FILES, CONTROL))

    def test_replaced_control_or_changed_preexisting_tab_is_refused(self):
        for control in (dict(CONTROL, id='replacement'), dict(CONTROL, title='changed')):
            self.assertIsNone(policy.fresh_documents([CONTROL], [control, *NEW], FILES, CONTROL))

    def test_duplicate_or_path_shaped_target_ids_are_never_accepted(self):
        values = [dict(item, type='page') for item in [CONTROL, *NEW]]
        self.assertEqual(policy.pages(values), sorted([CONTROL, *NEW], key=lambda item: item['id']))
        for changed in (values + [values[0]], [dict(values[0], id='../close/control')]):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                policy.pages(changed)


class ExactTargetRetirement(unittest.TestCase):
    def request(self):
        identity = {'control': CONTROL, 'unit': {'invocation': '1' * 32}}
        return {'identity': identity, 'before': [CONTROL], 'targets': NEW}

    def test_close_touches_only_two_new_targets_and_keeps_original_control(self):
        request = self.request()
        current = [CONTROL, *NEW]
        def close(directory, suffix):
            identifier = suffix.removeprefix('/json/close/')
            self.assertIn(identifier, [item['id'] for item in NEW])
            current[:] = [item for item in current if item['id'] != identifier]
            return b'Target is closing'
        with patch.object(fixture, 'context', return_value=(Path('/owned'), {'files': FILES})), \
             patch.object(fixture, 'browser_authority', return_value=request['identity']) as authority, \
             patch.object(fixture, 'browser_pages', side_effect=lambda *_: sorted(current, key=lambda item: item['id'])), \
             patch.object(fixture, 'browser_http', side_effect=close) as http, patch('builtins.print'):
            fixture.browser_close('/owned', 'abcdef', request)
        self.assertEqual([call.args[1] for call in http.call_args_list], ['/json/close/0', '/json/close/1'])
        self.assertEqual(current, [CONTROL])
        self.assertEqual(authority.call_count, 4)

    def test_old_or_partial_proof_cannot_authorize_any_close(self):
        for current in ([CONTROL, NEW[0]], [CONTROL], [CONTROL, *NEW]):
            request = self.request()
            if len(current) == 3:
                request['targets'] = [CONTROL, NEW[0]]
            with self.subTest(current=current), \
                 patch.object(fixture, 'context', return_value=(Path('/owned'), {'files': FILES})), \
                 patch.object(fixture, 'browser_authority', return_value=request['identity']), \
                 patch.object(fixture, 'browser_pages', return_value=current), \
                 patch.object(fixture, 'browser_http') as http:
                with self.assertRaises(ValueError):
                    fixture.browser_close('/owned', 'abcdef', request)
            http.assert_not_called()

    def test_browser_successor_refusal_sends_no_target_close(self):
        with patch.object(fixture, 'context', return_value=(Path('/owned'), {'files': FILES})), \
             patch.object(fixture, 'browser_authority', side_effect=AssertionError('invocation_changed')), \
             patch.object(fixture, 'browser_http') as http:
            with self.assertRaises(AssertionError):
                fixture.browser_close('/owned', 'abcdef', self.request())
        http.assert_not_called()

    def test_changed_endpoint_inode_is_refused_before_http_even_with_identical_port(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            (directory / 'profile').mkdir()
            path = directory / 'profile/DevToolsActivePort'
            path.write_bytes(b'12345\n/devtools/browser/owned\n')
            endpoint = fixture.browser_endpoint(directory)
            fixture.original_receipt(directory, 'browser-admission.json', {'endpoint': endpoint})
            path.rename(directory / 'original-port')
            path.write_bytes(b'12345\n/devtools/browser/owned\n')
            with patch.object(fixture.urllib.request, 'build_opener') as opener:
                with self.assertRaises(ValueError):
                    fixture.browser_http(directory, '/json/close/0')
            opener.assert_not_called()

    def test_page_change_before_second_close_preserves_foreign_target(self):
        request = self.request()
        snapshots = [[CONTROL, *NEW], [CONTROL, *NEW], [CONTROL, NEW[1]],
                     [CONTROL, dict(NEW[1], url='file:///foreign')]]
        snapshots = [sorted(items, key=lambda item: item['id']) for items in snapshots]
        with patch.object(fixture, 'context', return_value=(Path('/owned'), {'files': FILES})), \
             patch.object(fixture, 'browser_authority', return_value=request['identity']), \
             patch.object(fixture, 'browser_pages', side_effect=snapshots), \
             patch.object(fixture, 'browser_http', return_value=b'Target is closing') as http:
            with self.assertRaises(ValueError):
                fixture.browser_close('/owned', 'abcdef', request)
        self.assertEqual([call.args[1] for call in http.call_args_list], ['/json/close/0'])


if __name__ == '__main__':
    unittest.main()
