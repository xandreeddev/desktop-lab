"""Owned temporary Unix sockets/files only; no VM, D-Bus or browser actions."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import unittest
from unittest.mock import Mock,patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'vm'))
spec=importlib.util.spec_from_file_location('intent_artifact_fixture',ROOT/'vm/fixtures/application-intent-fixture.py')
fixture=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixture)


def socket_stamp(path):
    value=path.lstat()
    return [value.st_dev,value.st_ino,value.st_mode,value.st_uid]


def bind(path,live=False):
    value=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    try:
        value.bind(str(path));path.chmod(0o600)
        if live:value.listen(1)
        else:value.close()
    except BaseException:
        value.close();raise
    return value


class SocketRetirement(unittest.TestCase):
    def setUp(self):
        self.temp=self.enterContext(tempfile.TemporaryDirectory())
        self.directory=Path(self.temp);self.path=self.directory/'application-intent.sock'
        self.enterContext(patch.dict(os.environ,{'XDG_RUNTIME_DIR':str(self.directory)}))

    def test_actual_refused_socket_is_removed_only_after_drain_and_absence_is_idempotent(self):
        bind(self.path);original={'socket':socket_stamp(self.path)}
        with patch.object(fixture,'retirement_drained',return_value=True) as drained:
            fixture.retire_example_socket(original)
            self.assertFalse(self.path.exists())
            fixture.retire_example_socket(original)
        self.assertGreaterEqual(drained.call_count,3)

    def test_live_listener_is_never_removed_even_if_original_process_is_drained(self):
        listener=bind(self.path,live=True);self.addCleanup(listener.close)
        original={'socket':socket_stamp(self.path)}
        with patch.object(fixture,'retirement_drained',return_value=True),self.assertRaises(ValueError):
            fixture.retire_example_socket(original)
        self.assertEqual(socket_stamp(self.path),original['socket'])

    def test_replaced_foreign_and_non_socket_entries_are_refused(self):
        bind(self.path);original={'socket':socket_stamp(self.path)}
        self.path.rename(self.directory/'original.sock');bind(self.path)
        replacement=socket_stamp(self.path)
        with patch.object(fixture,'retirement_drained',return_value=True),self.assertRaises(ValueError):
            fixture.retire_example_socket(original)
        self.assertEqual(socket_stamp(self.path),replacement)
        foreign=copy.deepcopy(original);foreign['socket'][3]=os.getuid()+1
        with patch.object(fixture,'retirement_drained') as drained,self.assertRaises(ValueError):
            fixture.retire_example_socket(foreign)
        drained.assert_not_called()
        self.path.unlink();self.path.write_bytes(b'foreign regular file');self.path.chmod(0o600)
        with patch.object(fixture,'retirement_drained',return_value=True),self.assertRaises(ValueError):
            fixture.retire_example_socket(original)
        self.assertEqual(self.path.read_bytes(),b'foreign regular file')

    def test_replacement_during_refused_connect_survives_final_identity_check(self):
        bind(self.path);original={'socket':socket_stamp(self.path)}
        real_socket=socket.socket
        connection=Mock();connection.__enter__=Mock(return_value=connection)
        connection.__exit__=Mock(return_value=False)
        def refused(_path):
            self.path.rename(self.directory/'original.sock')
            replacement=real_socket(socket.AF_UNIX,socket.SOCK_STREAM)
            replacement.bind(str(self.path));self.path.chmod(0o600);replacement.close()
            raise ConnectionRefusedError()
        connection.connect.side_effect=refused
        with patch.object(fixture,'retirement_drained',return_value=True), \
             patch.object(fixture.socket,'socket',return_value=connection),self.assertRaises(ValueError):
            fixture.retire_example_socket(original)
        self.assertTrue(self.path.exists());self.assertNotEqual(socket_stamp(self.path),original['socket'])
        connection.settimeout.assert_called_once_with(1)

    def test_undrained_process_blocks_socket_contact_and_removal(self):
        bind(self.path);original={'socket':socket_stamp(self.path)}
        with patch.object(fixture,'retirement_drained',return_value=False), \
             patch.object(fixture.socket,'socket') as contacted,self.assertRaises(ValueError):
            fixture.retire_example_socket(original)
        contacted.assert_not_called();self.assertTrue(self.path.exists())

    def test_unknown_socket_without_admission_is_not_adopted_when_unit_is_absent(self):
        bind(self.path);before=socket_stamp(self.path)
        with patch.object(fixture,'context',return_value=(self.directory,{})), \
             patch.object(fixture,'owned_argv',return_value=['owned']), \
             patch.object(fixture,'unit_snapshot',return_value={'load':'not-found'}), \
             patch.object(fixture,'retire_original_unit') as stopped,self.assertRaises(ValueError):
            fixture.retire_example(self.directory,'abcdef')
        stopped.assert_not_called();self.assertEqual(socket_stamp(self.path),before)

    def test_absent_unit_still_requires_original_drain_and_retires_admitted_socket(self):
        bind(self.path)
        origin={'unit':{'invocation':'a'*32,'group':'/owned.service'},
                'process':{'pid':123,'start':'42','group':'/owned.service'}}
        admitted=dict(origin,socket=socket_stamp(self.path))
        fixture.original_receipt(self.directory,'example-origin.json',origin)
        fixture.original_receipt(self.directory,'example-admission.json',admitted)
        with patch.object(fixture,'context',return_value=(self.directory,{})), \
             patch.object(fixture,'owned_argv',return_value=['owned']), \
             patch.object(fixture,'unit_snapshot',return_value={'load':'not-found'}), \
             patch.object(fixture,'retirement_drained',return_value=True), \
             patch.object(fixture,'retire_unit') as stop:
            self.assertEqual(fixture.retire_example(self.directory,'abcdef',admitted),
                             {'retired':True,'socket_admitted':True,'socket_retired':True})
            self.assertEqual(fixture.retire_example(self.directory,'abcdef',admitted)['socket_retired'],True)
        stop.assert_not_called();self.assertFalse(self.path.exists())

    def test_absent_unadmitted_unit_with_durable_origin_still_requires_original_process_drain(self):
        origin={'unit':{'invocation':'a'*32,'group':'/owned.service'},
                'process':{'pid':123,'start':'42','group':'/owned.service'}}
        fixture.original_receipt(self.directory,'example-origin.json',origin)
        with patch.object(fixture,'context',return_value=(self.directory,{})), \
             patch.object(fixture,'owned_argv',return_value=['owned']), \
             patch.object(fixture,'unit_snapshot',return_value={'load':'not-found'}), \
             patch.object(fixture,'retirement_drained',return_value=False) as drained, \
             self.assertRaises(ValueError):fixture.retire_example(self.directory,'abcdef')
        drained.assert_called_once_with(origin)


class EntryRetirement(unittest.TestCase):
    def setUp(self):
        self.temp=self.enterContext(tempfile.TemporaryDirectory());self.directory=Path(self.temp)
        self.data=self.directory/'data';self.apps=self.data/'applications';self.apps.mkdir(parents=True)
        self.enterContext(patch.dict(os.environ,{'XDG_DATA_HOME':str(self.data)}))
        self.entry=self.apps/'org.lucent.ApplicationIntentabcdef.desktop';self.content=b'[Desktop Entry]\nName=Owned\n'
        self.manifest={'app_id':self.entry.name,'entry':str(self.entry),'entry_sha256':fixture.digest(self.content)}
        created=fixture.private(self.entry,self.content)
        fixture.original_receipt(self.directory,'entry-origin.json',
            {'path':str(self.entry),'identity':created,'sha256':fixture.digest(self.content)})

    def test_exact_entry_retires_on_failure_without_deleting_private_receipts_and_repeats_safely(self):
        fixture.retire_entry(self.directory,'abcdef',self.manifest)
        self.assertFalse(self.entry.exists());self.assertTrue((self.directory/'entry-origin.json').exists())
        fixture.retire_entry(self.directory,'abcdef',self.manifest)

    def test_same_hash_replacement_or_modified_entry_is_not_removed(self):
        self.entry.rename(self.apps/'original.desktop');fixture.private(self.entry,self.content)
        with self.assertRaises(ValueError):fixture.retire_entry(self.directory,'abcdef',self.manifest)
        self.assertEqual(self.entry.read_bytes(),self.content)
        self.entry.write_bytes(b'foreign changed contents')
        with self.assertRaises(ValueError):fixture.retire_entry(self.directory,'abcdef',self.manifest)
        self.assertEqual(self.entry.read_bytes(),b'foreign changed contents')

    def test_uncaptured_or_redirected_entry_never_grants_unlink_authority(self):
        with self.assertRaises(ValueError):fixture.retire_entry(self.directory,'other12',self.manifest)
        (self.directory/'entry-origin.json').unlink()
        with self.assertRaises(FileNotFoundError):fixture.retire_entry(self.directory,'abcdef',self.manifest)
        self.assertTrue(self.entry.exists())

    def test_failed_example_quiescence_prevents_entry_retirement(self):
        fixture.private(self.directory/'events.lock',b'')
        with patch.object(fixture,'context',return_value=(self.directory,self.manifest)), \
             patch.object(fixture,'retire_example',side_effect=ValueError('not drained')), \
             patch.object(fixture,'events',return_value=[]),patch.object(fixture,'retire_entry') as entry, \
             self.assertRaises(ValueError):fixture.retire(self.directory,'abcdef')
        entry.assert_not_called();self.assertTrue(self.entry.exists())

    def test_example_and_browser_retire_before_the_published_entry(self):
        fixture.private(self.directory/'events.lock',b'');order=[]
        event={'cgroup':'/app.slice/run-owned.service','operation':'open','uris':[]}
        with patch.object(fixture,'context',return_value=(self.directory,self.manifest)), \
             patch.object(fixture,'retire_example',side_effect=lambda *_:order.append('example')), \
             patch.object(fixture,'events',return_value=[event]), \
             patch.object(fixture,'browser_lease',return_value={'owned':'lease'}), \
             patch.object(fixture.chromium,'retire',side_effect=lambda _:order.append('browser')), \
             patch.object(fixture,'retire_entry',side_effect=lambda *_:order.append('entry')),patch('builtins.print'):
            fixture.retire(self.directory,'abcdef')
        self.assertEqual(order,['example','browser','entry'])


if __name__=='__main__':unittest.main()
