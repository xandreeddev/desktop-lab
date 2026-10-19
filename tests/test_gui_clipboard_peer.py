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
            barriers = [];real_fsync = os.fsync
            def sync(descriptor):
                real_fsync(descriptor);barriers.append(descriptor)
            def execute(*_args):
                self.assertEqual(json.loads((path/'origin.json').read_bytes()), proof)
                self.assertEqual(len(barriers), 2)  # Receipt and parent durable before exec.
                raise RuntimeError('no real exec')
            with patch.object(peer, 'context', return_value=(path, manifest)), \
                 patch.object(peer, 'capture_unit_origin', return_value=proof), \
                 patch.object(peer.os, 'fsync', side_effect=sync), \
                 patch.object(peer.os, 'execv', side_effect=execute):
                with self.assertRaisesRegex(RuntimeError, 'no real exec'):
                    peer.serve(path, self.run_id)

    def test_input_rejects_same_argv_successor_or_reborn_executable(self):
        proof = self.proof()
        process = dict(proof['process'], sha256=None)
        with patch.object(peer, 'regular', return_value=json.dumps(proof).encode()), \
             patch.object(peer, 'unit_snapshot', return_value=proof['unit']) as unit, \
             patch.object(peer, 'process_snapshot', return_value=process) as snapshot:
            peer.owned_unit(Path('/owned'), self.run_id, proof)
            self.assertEqual(snapshot.call_args.kwargs, {'max_executable_bytes': 512*1024*1024})
            for changed in (dict(proof['unit'], invocation='2'*32), dict(proof['unit'], main_pid=999)):
                unit.return_value = changed
                with self.subTest(unit=changed), self.assertRaises(ValueError):
                    peer.owned_unit(Path('/owned'), self.run_id, proof)
            unit.return_value = proof['unit']
            for changed in (dict(process, start='88'), dict(process, executable=[9]), dict(process, group='/foreign')):
                snapshot.return_value = changed
                with self.subTest(process=changed), self.assertRaises(ValueError):
                    peer.owned_unit(Path('/owned'), self.run_id, proof)

    def test_admission_waits_for_owned_page_and_hashes_original_chromium_once(self):
        proof = self.proof()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory);(path/'origin.json').write_text(json.dumps(proof))
            connection = Mock()
            with patch.object(peer, 'context', return_value=(path, {})), \
                 patch.object(peer, 'page', return_value=connection) as page, \
                 patch.object(peer, 'unit_snapshot', return_value=proof['unit']), \
                 patch.object(peer, 'assert_unit_origin', return_value=proof['process']) as origin, \
                 patch.object(peer, 'owned_unit'), patch('builtins.print'):
                peer.admit(path, self.run_id)
            page.assert_called_once();connection.close.assert_called_once()
            self.assertEqual(origin.call_args.kwargs, {'max_executable_bytes': 512*1024*1024, 'hashed': True})
            self.assertEqual(json.loads((path/'admission.json').read_bytes()), proof)

    def test_chromium_client_must_be_original_main_pid_on_its_private_workspace(self):
        proof = self.proof()
        value = {'class': 'Owned', 'pid': 123, 'workspace': {'name': 'lucent-clipboard-browser-'+self.run_id}}
        with patch.object(peer, 'context', return_value=(Path('/owned'), {'class': 'Owned'})), \
             patch.object(peer, 'owned_unit', return_value=proof['unit']), \
             patch.object(peer.Path, 'stat') as stat, patch.object(peer, 'regular', return_value=('0::'+proof['unit']['group']+'\n').encode()), \
             patch('builtins.print'):
            stat.return_value.st_uid = os.getuid()
            peer.clients('/owned', self.run_id, {'identity': proof, 'clients': [value]})
            for changed in (dict(value, pid=999), dict(value, workspace={'name': 'notes'})):
                with self.subTest(client=changed), self.assertRaises(ValueError):
                    peer.clients('/owned', self.run_id, {'identity': proof, 'clients': [changed]})

    def test_retirement_always_carries_original_receipt_or_full_captured_identity(self):
        proof = self.proof()
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'regular', return_value=json.dumps(proof).encode()), \
             patch.object(peer, 'retire_unit') as retire, patch.object(peer, 'retire_original_unit') as original:
            peer.retire_owned('/owned', self.run_id, {'identity': proof})
            self.assertEqual(retire.call_args.kwargs['captured'], proof)
            peer.retire_owned('/owned', self.run_id, {'identity': None, 'origin': proof})
            self.assertEqual(original.call_args.args[2], proof)
            original.reset_mock();retire.reset_mock()
            with self.assertRaises(ValueError):
                peer.retire_owned('/owned', self.run_id, {'identity': None, 'origin': None})
            original.assert_not_called();retire.assert_not_called()

    def test_successor_refusal_preserves_private_profile_and_receipts(self):
        proof = self.proof()
        with patch.object(peer, 'context', return_value=(Path('/owned'), {})), \
             patch.object(peer, 'regular', return_value=json.dumps(proof).encode()), \
             patch.object(peer, 'retire_unit', side_effect=AssertionError('successor')), \
             patch.object(peer.shutil, 'rmtree') as remove:
            with self.assertRaises(AssertionError):
                peer.cleanup('/owned', self.run_id, {'identity': proof})
        remove.assert_not_called()


if __name__ == '__main__':
    unittest.main()
