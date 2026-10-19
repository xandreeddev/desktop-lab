"""Owned browser lifetime and real-file read regressions; no browser/VM/D-Bus."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'vm'))
spec = importlib.util.spec_from_file_location('gui_clipboard_fixture', ROOT/'vm/fixtures/gui-clipboard-peer.py')
peer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(peer)


class RegularFileReads(unittest.TestCase):
    def test_real_cold_read_changes_access_time_but_keeps_all_eight_protected_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'manifest.json';data = b'owned cold fixture'*1024
            path.write_bytes(data);os.utime(path, ns=(1, path.stat().st_mtime_ns))
            before = path.stat()
            self.assertEqual(peer.regular(path), data)
            after = path.stat()
            self.assertEqual(peer.stable_file_identity(before), peer.stable_file_identity(after))
            self.assertEqual(len(peer.stable_file_identity(before)), 8)
            if before.st_atime_ns == after.st_atime_ns:
                self.skipTest('filesystem does not update atime on actual cold reads')

    def read_with_mutation(self, path, mutation):
        real_fdopen = os.fdopen

        class Hook:
            def __init__(self, stream):
                self.stream = stream
            def __enter__(self):
                self.stream.__enter__();return self
            def __exit__(self, *args):
                return self.stream.__exit__(*args)
            def fileno(self):
                return self.stream.fileno()
            def read(self, amount):
                data = self.stream.read(amount);mutation();return data

        with patch.object(peer.os, 'fdopen', side_effect=lambda *a, **k: Hook(real_fdopen(*a, **k))):
            peer.regular(path)

    def test_same_size_rewrite_during_read_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'manifest.json';path.write_bytes(b'original')
            before = path.stat()
            def mutate():
                path.write_bytes(b'modified');os.utime(path, ns=(1, before.st_mtime_ns+1))
            with self.assertRaises(ValueError):
                self.read_with_mutation(path, mutate)

    def test_replacement_and_oversize_or_symlink_reads_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'manifest.json';path.write_bytes(b'original')
            replacement = Path(directory)/'replacement';replacement.write_bytes(b'original')
            with self.assertRaises(ValueError):
                self.read_with_mutation(path, lambda: os.replace(replacement, path))
            with self.assertRaises(ValueError):
                peer.regular(path, 3)
            link = Path(directory)/'link';link.symlink_to(path)
            with self.assertRaises(OSError):
                peer.regular(link)


class OriginalBrowserAuthority(unittest.TestCase):
    run_id = 'a'*24

    def proof(self):
        unit = {'load': 'loaded', 'active': 'active', 'transient': True,
                'executable': '/usr/bin/python3', 'argv': peer.argv(Path('/owned'), self.run_id),
                'group': '/owned/'+peer.unit_name(self.run_id), 'main_pid': 123,
                'invocation': '1'*32}
        process = {'pid': 123, 'start': '77', 'group': unit['group'],
                   'executable': [1,2,3,4,5,6,7], 'sha256': 'a'*64}
        return {'unit': unit, 'process': process}

    def lease(self):
        proof=self.proof()
        process=dict(proof['process'],group='/owned/actual-registered.scope')
        return {'lease':{'unit':proof['unit'],'process':process,'origin':proof,
                'declared':{'owned':'startup'},'scope':{'invocation':'2'*32,'group':process['group']}},
                'endpoint':{'port':12345,'browser':'/devtools/browser/owned'},
                'control':{'id':'owned','title':'Owned','url':'file:///owned/document.html'}}

    def test_existing_name_refuses_initialization_before_creating_any_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            with patch.object(peer, 'unit_snapshot', return_value={'load': 'loaded'}):
                with self.assertRaises(ValueError):
                    peer.initialize(path, self.run_id)
            self.assertEqual(list(path.iterdir()), [])

    def test_second_absence_preflight_refuses_name_reuse_before_launch(self):
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'unit_snapshot', return_value={'load': 'loaded'}), patch('builtins.print') as output:
            with self.assertRaises(ValueError):
                peer.absent('/owned', self.run_id)
        output.assert_not_called()

    def test_wrapper_persists_original_receipt_before_exec(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory);proof = self.proof()
            manifest = {'class': 'Owned', 'uri': 'file:///owned/document.html'}
            (path/'watchdog-origin.json').write_text(json.dumps(proof))
            barriers = [];real_fsync = os.fsync
            def sync(descriptor):
                real_fsync(descriptor);barriers.append(descriptor)
            def execute(*_args):
                self.assertEqual(json.loads((path/'origin.json').read_bytes()), proof)
                self.assertEqual(len(barriers), 2)  # Receipt and parent durable before exec.
                raise RuntimeError('no real exec')
            with patch.object(peer, 'context', return_value=(path, manifest)), \
                 patch.object(peer, 'capture_unit_origin', return_value=proof), \
                 patch.object(peer, 'unit_snapshot', return_value={'load':'not-found'}), \
                 patch.object(peer, 'assert_unit_origin'),patch.object(peer.subprocess,'run') as started, \
                 patch.object(peer.os, 'fsync', side_effect=sync), \
                 patch.object(peer.os, 'execv', side_effect=execute):
                with self.assertRaisesRegex(RuntimeError, 'no real exec'):
                    peer.serve(path, self.run_id)
            self.assertIn('--property=RuntimeMaxSec=240s',started.call_args.args[0])
            self.assertIn('--unit='+peer.watchdog_name(self.run_id),started.call_args.args[0])

    def test_input_rejects_same_argv_successor_or_reborn_executable(self):
        lease=self.lease()
        def read(path):return json.dumps(lease if path.name=='admission.json' else lease['lease']['origin']).encode()
        with patch.object(peer,'context',return_value=(Path('/owned'),{'startup':lease['lease']['declared']})), \
             patch.object(peer,'browser_endpoint',return_value=lease['endpoint']), \
             patch.object(peer,'regular',side_effect=read),patch.object(peer,'browser_lease',return_value=lease['lease']) as observe:
            self.assertEqual(peer.owned_unit(Path('/owned'),self.run_id,lease),lease)
            observe.assert_called_once_with(Path('/owned'),self.run_id)
            observe.side_effect=ValueError('scope_or_birth_replaced')
            with self.assertRaises(ValueError):peer.owned_unit(Path('/owned'),self.run_id,lease)
            observe.reset_mock()
            with self.assertRaises(ValueError):
                peer.owned_unit(Path('/owned'),self.run_id,dict(lease,lease={'process':{'pid':999}}))
            observe.assert_not_called()

    def test_admission_waits_for_owned_page_and_hashes_original_chromium_once(self):
        proof = self.lease()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory);(path/'origin.json').write_text(json.dumps(proof['lease']['origin']))
            connection = Mock()
            with patch.object(peer, 'context', return_value=(path, {'startup':proof['lease']['declared']})), \
                 patch.object(peer, 'page', return_value=(connection,{'endpoint':proof['endpoint'],'control':proof['control']})) as page, \
                 patch.object(peer, 'browser_lease', return_value=proof['lease']) as origin, \
                 patch.object(peer, 'owned_unit'), patch('builtins.print'):
                peer.admit(path, self.run_id)
            page.assert_called_once();connection.close.assert_called_once()
            origin.assert_called_once_with(path,self.run_id)
            self.assertEqual(json.loads((path/'admission.json').read_bytes()), proof)

    def test_chromium_client_must_be_original_main_pid_on_its_private_workspace(self):
        proof = self.lease()
        value = {'class': 'Owned', 'pid': 123, 'workspace': {'name': 'lucent-clipboard-browser-'+self.run_id}}
        with patch.object(peer, 'context', return_value=(Path('/owned'), {'class': 'Owned'})), \
             patch.object(peer, 'owned_unit', return_value=proof), \
             patch.object(peer.Path, 'stat') as stat, patch.object(peer, 'regular', return_value=('0::'+proof['lease']['process']['group']+'\n').encode()), \
             patch('builtins.print'):
            stat.return_value.st_uid = os.getuid()
            peer.clients('/owned', self.run_id, {'identity': proof, 'clients': [value]})
            for changed in (dict(value, pid=999), dict(value, workspace={'name': 'notes'})):
                with self.subTest(client=changed), self.assertRaises(ValueError):
                    peer.clients('/owned', self.run_id, {'identity': proof, 'clients': [changed]})

    def test_retirement_always_carries_original_receipt_or_full_captured_identity(self):
        proof = self.lease()
        def read(path):return json.dumps(proof if path.name=='admission.json' else proof['lease']['origin']).encode()
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'regular', side_effect=read),patch.object(peer,'browser_lease',return_value=proof['lease']), \
             patch.object(peer.chromium, 'retire') as retire, patch.object(peer, 'retire_original_unit') as original:
            peer.retire_owned('/owned', self.run_id, {'identity': proof})
            retire.assert_called_once_with(proof['lease'])
            self.assertEqual(original.call_args.args[0],peer.watchdog_name(self.run_id))
            peer.retire_owned('/owned', self.run_id, {'identity': None, 'origin': proof['lease']['origin']})
            self.assertEqual(retire.call_args.args[0],proof['lease'])
            self.assertEqual(original.call_args.args[2],proof['lease']['origin'])
            original.reset_mock();retire.reset_mock()
            with self.assertRaises(ValueError):
                peer.retire_owned('/owned', self.run_id, {'identity': None, 'origin': None})
            original.assert_not_called();retire.assert_not_called()

    def test_successor_refusal_preserves_private_profile_and_receipts(self):
        proof = self.lease()
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'regular', return_value=json.dumps(proof).encode()), \
             patch.object(peer, 'browser_lease', return_value=proof['lease']), \
             patch.object(peer.chromium, 'retire', side_effect=AssertionError('successor')), \
             patch.object(peer.shutil, 'rmtree') as remove:
            with self.assertRaises(AssertionError):
                peer.cleanup('/owned', self.run_id, {'identity': proof})
        remove.assert_not_called()

    def test_watchdog_origin_is_durable_before_admission_and_uses_same_lease(self):
        proof=self.proof();lease=self.lease()
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)
            def admitted(*_args,**_kw):
                self.assertEqual(json.loads((path/'watchdog-origin.json').read_bytes()),proof)
                return lease['lease']
            with patch.object(peer,'context',return_value=(path,{})), \
                 patch.object(peer,'capture_unit_origin',return_value=proof), \
                 patch.object(peer,'browser_lease',side_effect=admitted),patch.object(peer.signal,'alarm'), \
                 patch.object(peer.time,'sleep'),patch.object(peer.chromium,'retire') as retired, \
                 patch.object(peer,'admit') as page_admission,patch.object(peer,'page') as page:
                peer.watchdog(path,self.run_id)
            retired.assert_called_once_with(lease['lease'])
            page_admission.assert_not_called();page.assert_not_called()

    def test_replaced_endpoint_file_is_refused_before_any_http_request(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);(path/'profile').mkdir()
            port=path/'profile/DevToolsActivePort';port.write_bytes(b'12345\n/devtools/browser/owned\n')
            expected={'endpoint':peer.browser_endpoint(path)}
            port.rename(path/'original-port');port.write_bytes(b'12345\n/devtools/browser/owned\n')
            with patch.object(peer.urllib.request,'build_opener') as opener,self.assertRaises(ValueError):
                peer.page(path,{},expected=expected)
            opener.assert_not_called()

    def test_same_document_in_a_replacement_target_is_not_the_original_control(self):
        manifest={'uri':'file:///owned/document.html','title':'Owned'}
        target={'type':'page','url':manifest['uri'],'title':'Owned','id':'replacement',
                'webSocketDebuggerUrl':'ws://127.0.0.1:12345/devtools/page/replacement'}
        expected={'endpoint':{'port':12345},'control':dict(target,id='original')}
        response=Mock();response.status=200;response.geturl.return_value='http://127.0.0.1:12345/json/list'
        response.read.return_value=json.dumps([target]).encode()
        opener=Mock();opener.open.return_value.__enter__=Mock(return_value=response)
        opener.open.return_value.__exit__=Mock(return_value=False)
        with patch.object(peer,'browser_endpoint',return_value=expected['endpoint']), \
             patch.object(peer.urllib.request,'build_opener',return_value=opener), \
             patch.object(peer.Peer,'connect') as connect,self.assertRaises(ValueError):
            peer.page(Path('/owned'),manifest,expected=expected)
        connect.assert_not_called()


class SeparateProcessLease(unittest.TestCase):
    run_id=OriginalBrowserAuthority.run_id

    def proof(self, path):
        value=OriginalBrowserAuthority().lease()
        value['lease']['origin']['unit']['argv']=peer.argv(path,self.run_id)
        return value

    def test_original_receipt_remains_available_without_browser_page_or_process_admission(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);proof=self.proof(path)['lease']['origin']
            peer.private(path/'origin.json',json.dumps(proof).encode())
            with patch.object(peer,'context',return_value=(path,{})),patch.object(peer,'admit') as admit, \
                 patch.object(peer,'page') as page,patch.object(peer.chromium,'observe') as observe, \
                 patch('builtins.print') as output:
                peer.origin(path,self.run_id)
            self.assertEqual(json.loads(output.call_args.args[0]),proof)
            admit.assert_not_called();page.assert_not_called();observe.assert_not_called()

    def test_process_lease_is_durable_and_revalidated_without_any_cdp_dependency(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);proof=self.proof(path);lease=proof['lease']
            peer.private(path/'origin.json',json.dumps(lease['origin']).encode())
            with patch.object(peer,'context',return_value=(path,{'startup':lease['declared']})), \
                 patch.object(peer.chromium,'observe',return_value=lease) as observe, \
                 patch.object(peer.chromium,'original_gone',return_value=False),patch.object(peer,'page') as page:
                self.assertEqual(peer.browser_lease(path,self.run_id),lease)
                self.assertEqual(json.loads((path/'browser-lease.json').read_bytes()),lease)
                observe.reset_mock();self.assertEqual(peer.browser_lease(path,self.run_id),lease)
            observe.assert_called_once_with(lease['origin'],lease['declared'],lease)
            page.assert_not_called();self.assertFalse((path/'admission.json').exists())

    def test_initial_lease_waits_for_registered_scope_without_freezing_the_wrapper_group(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);ready=self.proof(path)['lease']
            starting=json.loads(json.dumps(ready));starting['scope']=None
            starting['process']['group']=starting['origin']['unit']['group']
            peer.private(path/'origin.json',json.dumps(ready['origin']).encode())
            with patch.object(peer,'context',return_value=(path,{'startup':ready['declared']})), \
                 patch.object(peer.chromium,'observe',side_effect=[starting,ready,ready]) as observe, \
                 patch.object(peer,'page') as page:
                with self.assertRaises(ValueError):peer.browser_lease(path,self.run_id)
                self.assertFalse((path/'browser-lease.json').exists())
                self.assertFalse((path/'admission.json').exists())
                self.assertEqual(peer.browser_lease(path,self.run_id),ready)
                self.assertEqual(json.loads((path/'browser-lease.json').read_bytes()),ready)
            self.assertEqual(observe.call_count,3)
            self.assertEqual(observe.call_args.args,(ready['origin'],ready['declared'],ready))
            page.assert_not_called()

    def test_page_admission_waits_without_input_or_durable_lease_before_scope_readiness(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);starting=self.proof(path)['lease'];starting['scope']=None
            peer.private(path/'origin.json',json.dumps(starting['origin']).encode())
            with patch.object(peer,'context',return_value=(path,{'startup':starting['declared']})), \
                 patch.object(peer.chromium,'observe',return_value=starting),patch.object(peer,'page') as page:
                with self.assertRaises(ValueError):peer.admit(path,self.run_id)
            page.assert_not_called()
            self.assertFalse((path/'browser-lease.json').exists())
            self.assertFalse((path/'admission.json').exists())

    def test_failed_page_admission_neither_grants_input_nor_blocks_process_only_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);proof=self.proof(path);lease=proof['lease']
            peer.private(path/'origin.json',json.dumps(lease['origin']).encode())
            peer.private(path/'watchdog-origin.json',json.dumps(lease['origin']).encode())
            with patch.object(peer,'context',return_value=(path,{'startup':lease['declared']})), \
                 patch.object(peer,'browser_lease',return_value=lease), \
                 patch.object(peer,'page',side_effect=ValueError('not ready')) as page, \
                 patch.object(peer.chromium,'retire') as retire,patch.object(peer,'retire_original_unit') as watched:
                with self.assertRaises(ValueError):peer.admit(path,self.run_id)
                self.assertFalse((path/'admission.json').exists());page.reset_mock()
                with self.assertRaises(FileNotFoundError):
                    peer.dom(path,self.run_id,'read',{'identity':proof})
                page.assert_not_called()
                peer.retire_owned(path,self.run_id,{'identity':None,'origin':lease['origin']})
            retire.assert_called_once_with(lease);watched.assert_called_once()
            self.assertTrue((path/'origin.json').exists())

    def test_retired_lease_cannot_grant_input_or_adopt_a_successor(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);lease=self.proof(path)['lease']
            peer.private(path/'origin.json',json.dumps(lease['origin']).encode())
            peer.private(path/'browser-lease.json',json.dumps(lease).encode())
            with patch.object(peer,'context',return_value=(path,{'startup':lease['declared']})), \
                 patch.object(peer.chromium,'original_gone',return_value=True), \
                 patch.object(peer.chromium,'retired',return_value=True) as retired, \
                 patch.object(peer.chromium,'observe') as observe:
                with self.assertRaises(ValueError):peer.browser_lease(path,self.run_id)
                self.assertEqual(peer.browser_lease(path,self.run_id,retiring=True),lease)
            retired.assert_called_once_with(lease);observe.assert_not_called()

    def test_replaced_lease_origin_or_frozen_lifetime_never_becomes_new_cleanup_authority(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory);lease=self.proof(path)['lease']
            peer.private(path/'origin.json',json.dumps(lease['origin']).encode())
            peer.private(path/'browser-lease.json',json.dumps(lease).encode())
            with patch.object(peer,'context',return_value=(path,{'startup':lease['declared']})), \
                 patch.object(peer.chromium,'original_gone',return_value=False), \
                 patch.object(peer.chromium,'observe',side_effect=ValueError('successor')), \
                 patch.object(peer.chromium,'retire') as retire:
                with self.assertRaises(ValueError):
                    peer.retire_owned(path,self.run_id,{'identity':None,'origin':lease['origin']})
            retire.assert_not_called()


class FixedPeerDiagnostics(unittest.TestCase):
    def test_staged_source_and_safe_http_status_survive_without_any_private_payload(self):
        from cdp_peer import PeerError
        error=PeerError('websocket_upgrade_required')
        error.upgrade_status={'http_version':'1.1','status_code':101}
        namespace={'error':error}
        exec(compile('def fail():\n raise error\n','/private/run/fixture.py','exec'),namespace)
        try:namespace['fail']()
        except Exception as observed:details=peer.failure_details(observed,'admit')
        self.assertEqual(details['upgrade_status'],{'http_version':'1.1','status_code':101})
        self.assertEqual(details['fixed_reason'],'websocket_upgrade_required')
        self.assertEqual(details['trace_frames'],[{'source':'gui-clipboard-peer.py','function':'fail','line':2}])
        self.assertNotIn('/private',json.dumps(details))

    def test_unknown_reason_operation_or_forged_status_never_emits_payload(self):
        error=ValueError('private document and credential')
        error.upgrade_status={'http_version':'private header','status_code':999}
        details=peer.failure_details(error,'private operation')
        self.assertEqual(details['operation'],'unknown')
        self.assertEqual(details['fixed_reason'],'unclassified_peer_failure')
        self.assertNotIn('upgrade_status',details)
        self.assertNotIn('private',json.dumps(details))


if __name__ == '__main__':
    unittest.main()


class ClipboardOfferDiagnostics(unittest.TestCase):
    def observation(self, types, payloads):
        commands = []
        def read(command, limit, deadline):
            commands.append(command)
            self.assertGreater(deadline, peer.time.monotonic())
            if command == ['wl-paste', '--list-types']:
                self.assertEqual(limit, 16384)
                return 0, ('\n'.join(types) + '\n').encode(), b''
            self.assertEqual(limit, 65536)
            return 0, payloads[command[-1]], b''
        return peer.clipboard_observation(read), commands

    def test_extra_browser_mimes_are_counted_but_never_requested_or_disclosed(self):
        result, commands = self.observation(
            ['text/plain;charset=utf-8', 'text/html', 'private/type-bearing-sensitive-value'],
            {'text/plain;charset=utf-8': peer.SOURCE_TEXT.encode()})
        self.assertEqual(result['payloads'], {'text/plain;charset=utf-8': 'fixture_source'})
        self.assertEqual(result['other_mime_count'], 2)
        self.assertFalse(result['strict_text_only'])
        self.assertTrue(result['all_read_alias_payloads_identical'])
        self.assertEqual(len(commands), 2)
        self.assertNotIn('text/html', json.dumps(result))
        self.assertNotIn('private/', json.dumps(result))

    def test_legacy_alias_differences_are_reported_without_decoding_or_exposing_them(self):
        secret = b'private original clipboard\xff'
        result, _commands = self.observation(
            ['text/plain', 'STRING'], {'text/plain': peer.SOURCE_TEXT.encode(), 'STRING': secret})
        self.assertEqual(result['payloads'], {'text/plain': 'fixture_source', 'STRING': 'other_non_utf8'})
        self.assertFalse(result['all_read_alias_payloads_identical'])
        self.assertTrue(result['strict_text_only'])
        self.assertNotIn('private original', json.dumps(result))
        self.assertNotIn(peer.digest(secret), json.dumps(result))

    def test_fixture_line_ending_variants_are_diagnostic_only_and_unknown_data_is_private(self):
        for fixture_name, fixture in (('source', peer.SOURCE_TEXT), ('destination', peer.DESTINATION_TEXT)):
            for suffix, value in (('', fixture), ('_crlf', fixture.replace('\n', '\r\n')),
                                  ('_without_final_lf', fixture.removesuffix('\n'))):
                with self.subTest(fixture=fixture_name, suffix=suffix):
                    self.assertEqual(peer.fixture_payload_status(value.encode()), 'fixture_' + fixture_name + suffix)
        self.assertEqual(peer.fixture_payload_status(b''), 'empty')
        self.assertEqual(peer.fixture_payload_status(b'private original text'), 'other')

    def test_empty_clipboard_and_failed_list_do_not_expose_stderr(self):
        result = peer.clipboard_observation(lambda *_args: (1, b'', b'Nothing is copied'))
        self.assertEqual(result['list_status'], 'empty')
        self.assertEqual(result['payloads'], {})
        for response in ((2, b'private output', b'private error'),
                         (0, b'\xff', b'private error'),
                         (0, b'text/plain\ntext/plain\n', b''),
                         (0, ('\n'.join('type/' + str(i) for i in range(65))).encode(), b'')):
            with self.subTest(response=response):
                result = peer.clipboard_observation(lambda *_args: response)
                self.assertEqual(result['list_status'], 'unavailable')
                self.assertNotIn('private', json.dumps(result))

    def test_unavailable_alias_does_not_hide_other_alias_fixture_or_leak_exception(self):
        def read(command, _limit, _deadline):
            if command[-1] == '--list-types':
                return 0, b'text/plain\nSTRING\n', b''
            if command[-1] == 'STRING':
                raise ValueError('private clipboard exception')
            return 0, peer.SOURCE_TEXT.encode(), b''
        result = peer.clipboard_observation(read)
        self.assertEqual(result['payloads'], {'text/plain': 'fixture_source', 'STRING': 'unavailable'})
        self.assertNotIn('private', json.dumps(result))

    def test_owned_browser_lease_is_checked_before_and_after_read_only_observation(self):
        proof = {'identity': {'owned': True}}
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'owned_unit') as owned, \
             patch.object(peer, 'clipboard_observation', return_value={'list_status': 'empty'}) as read, \
             patch('builtins.print') as output:
            peer.clipboard_observe('/owned', 'a' * 24, proof)
        self.assertEqual(owned.call_args_list, [unittest.mock.call(Path('/owned'), 'a' * 24, proof['identity'])] * 2)
        read.assert_called_once_with()
        self.assertEqual(json.loads(output.call_args.args[0]), {'list_status': 'empty'})
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'owned_unit', side_effect=ValueError('retired')), \
             patch.object(peer, 'clipboard_observation') as read:
            with self.assertRaises(ValueError):
                peer.clipboard_observe('/owned', 'a' * 24, proof)
        read.assert_not_called()

    def test_bounded_reader_drains_both_pipes_and_refuses_oversize_or_expired_child(self):
        command = [sys.executable, '-c', 'import sys;sys.stdout.buffer.write(b"owned");sys.stderr.write("status")']
        self.assertEqual(peer.bounded_clipboard_read(command, 5, peer.time.monotonic() + 3),
                         (0, b'owned', b'status'))
        with self.assertRaises(ValueError):
            peer.bounded_clipboard_read(command, 4, peer.time.monotonic() + 3)
        with self.assertRaises(ValueError):
            peer.bounded_clipboard_read(command, 5, peer.time.monotonic() - 1)


class BrowserEventObservation(unittest.TestCase):
    def observe(self, events):
        client = Mock()
        client.call.return_value = {'result': {'value': {'value': peer.SOURCE_TEXT,
            'focused': True, 'start': 0, 'end': len(peer.SOURCE_TEXT.encode('utf-16-le')) // 2,
            'events': events}}}
        with patch.object(peer, 'context', return_value=(Path('/owned'), {
                'uri': 'file:///owned/document.html', 'title': 'Owned'})), \
             patch.object(peer, 'owned_unit'), patch.object(peer, 'page', return_value=client), \
             patch('builtins.print') as output:
            peer.dom('/owned', 'a' * 24, 'read', {'identity': {'owned': True}})
        client.close.assert_called_once_with()
        expression = client.call.call_args.args[1]['expression']
        self.assertNotIn('navigator.clipboard', expression)
        self.assertNotIn('dispatchKeyEvent', expression)
        self.assertNotIn('execCommand', expression)
        return json.loads(output.call_args.args[0])

    def test_observed_trusted_copy_counts_are_bounded_data_and_not_input_synthesis(self):
        events = dict.fromkeys(peer.EVENT_KEYS, 0)
        events.update(trusted_copy=1, ctrl_c_down=1)
        result = self.observe(events)
        self.assertEqual(result['events'], events)
        self.assertTrue(result['selection_complete'])
        self.assertNotIn(peer.SOURCE_TEXT, json.dumps(result, ensure_ascii=False))

    def test_missing_extra_boolean_negative_and_overbound_event_counters_are_refused(self):
        valid = dict.fromkeys(peer.EVENT_KEYS, 0)
        for events in (None, {}, dict(valid, private_unknown_counter=1),
                       dict(valid, trusted_copy=True), dict(valid, trusted_copy=-1),
                       dict(valid, trusted_copy=4097)):
            with self.subTest(events=events), self.assertRaises(ValueError):
                self.observe(events)
