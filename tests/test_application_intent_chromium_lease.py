"""App fixture lease persistence and input/retirement separation; no native work."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock,patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'vm'))
spec=importlib.util.spec_from_file_location('intent_chromium_fixture',ROOT/'vm/fixtures/application-intent-fixture.py')
fixture=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixture)


class DurableBrowserLease(unittest.TestCase):
    def scenario(self):
        event={'operation':'open','uris':[],'cgroup':'/app.slice/run-owned.service',
               'origin':{'unit':{'invocation':'a'*32},'process':{'pid':123,'start':'42'}}}
        manifest={'browser_startup':{'owned':'startup'},'class':'Owned'}
        lease={'origin':event['origin'],'declared':manifest['browser_startup'],
               'process':{'pid':123,'start':'42','group':'/app.slice/registered.scope'}}
        return event,manifest,lease

    def test_first_lease_is_durable_then_every_guard_rechecks_that_exact_lifetime(self):
        event,manifest,lease=self.scenario()
        with tempfile.TemporaryDirectory() as name:
            directory=Path(name)
            with patch.object(fixture,'context',return_value=(directory,manifest)), \
                 patch.object(fixture,'events',return_value=[event]), \
                 patch.object(fixture.chromium,'observe',return_value=lease) as observe, \
                 patch.object(fixture.chromium,'original_gone',return_value=False):
                self.assertEqual(fixture.browser_lease(directory,'abcdef',event),lease)
                saved=json.loads((directory/'chromium-lease.json').read_bytes())
                self.assertEqual(saved,lease)
                observe.assert_called_once_with(event['origin'],manifest['browser_startup'])
                observe.reset_mock()
                self.assertEqual(fixture.browser_lease(directory,'abcdef',event),lease)
                observe.assert_called_once_with(event['origin'],manifest['browser_startup'],lease)

    def test_retired_original_cannot_authorize_native_input_even_if_pid_is_reused(self):
        event,manifest,lease=self.scenario()
        with tempfile.TemporaryDirectory() as name:
            directory=Path(name)
            fixture.original_receipt(directory,'chromium-lease.json',lease)
            with patch.object(fixture,'context',return_value=(directory,manifest)), \
                 patch.object(fixture,'events',return_value=[event]), \
                 patch.object(fixture.chromium,'original_gone',return_value=True), \
                 patch.object(fixture.chromium,'retired',return_value=True) as retired:
                with self.assertRaises(ValueError):fixture.browser_lease(directory,'abcdef',event)
                retired.assert_not_called()
                self.assertEqual(fixture.browser_lease(directory,'abcdef',event,retiring=True),lease)

    def test_managed_window_can_follow_original_pid_into_the_frozen_scope(self):
        event,manifest,lease=self.scenario()
        with patch.object(fixture,'context',return_value=(Path('/owned'),manifest)), \
             patch.object(fixture,'events',return_value=[event]), \
             patch.object(fixture,'browser_lease',return_value=lease) as leased, \
             patch.object(fixture.Path,'stat') as meta,patch('builtins.print'):
            meta.return_value.st_uid=os.getuid()
            fixture.owned_clients('/owned','abcdef',[{'class':'Owned','pid':123}])
            leased.assert_called_once()
            with self.assertRaises(ValueError):
                fixture.owned_clients('/owned','abcdef',[{'class':'Owned','pid':124}])

    def test_primary_browser_retirement_uses_only_its_lease_not_generic_scope_stop(self):
        event,manifest,lease=self.scenario()
        with tempfile.TemporaryDirectory() as name:
            directory=Path(name);fixture.private(directory/'events.lock',b'')
            with patch.object(fixture,'context',return_value=(directory,manifest)), \
                 patch.object(fixture,'events',return_value=[event]),patch.object(fixture,'retire_example'), \
                 patch.object(fixture,'browser_lease',return_value=lease) as leased, \
                 patch.object(fixture.chromium,'retire') as retired, \
                 patch.object(fixture,'retire_entry'), \
                 patch.object(fixture,'retire_original_unit') as generic,patch('builtins.print'):
                fixture.retire(directory,'abcdef')
            leased.assert_called_once_with(directory,'abcdef',event,retiring=True)
            retired.assert_called_once_with(lease);generic.assert_not_called()


class FixedFixtureDiagnostics(unittest.TestCase):
    def test_known_lease_failure_keeps_operation_class_and_source_location_without_payload(self):
        try:
            fixture.chromium.require(False)
        except ValueError as error:
            value=fixture.failure_details(error,'guard')
        self.assertEqual(value['operation'],'guard')
        self.assertEqual(value['error_class'],'ValueError')
        self.assertEqual(value['fixed_reason'],'owned_chromium_lease_refused')
        self.assertEqual(value['trace_frames'][0]['source'],'owned_chromium_lease.py')
        self.assertEqual(value['trace_frames'][0]['function'],'require')
        self.assertGreater(value['trace_frames'][0]['line'],0)
        self.assertFalse(value['trace_truncated'])
        self.assertNotIn(str(ROOT),json.dumps(value))

    def test_unknown_reason_operation_chain_and_locals_never_enter_diagnostics(self):
        secret='credential profile title /private/path about:private'
        try:
            try:raise ValueError(secret)
            except ValueError as cause:raise RuntimeError(secret) from cause
        except RuntimeError as error:
            value=fixture.failure_details(error,secret)
        self.assertEqual(value['operation'],'unknown')
        self.assertEqual(value['error_class'],'OtherError')
        self.assertEqual(value['fixed_reason'],'unclassified_fixture_failure')
        self.assertEqual(value['trace_frames'],[])
        self.assertNotIn(secret,json.dumps(value))

    def test_trace_has_fixed_sources_and_bounded_frames_without_code_or_locals(self):
        namespace={}
        exec(compile('def recurse(depth):\n if depth: recurse(depth-1)\n else: raise ValueError("private payload")\n',
                     str(ROOT/'vm/fixtures/application-intent-fixture.py'),'exec'),namespace)
        try:namespace['recurse'](100)
        except ValueError as error:value=fixture.failure_details(error,'retire')
        self.assertEqual(len(value['trace_frames']),12)
        self.assertTrue(value['trace_truncated'])
        self.assertLess(len(json.dumps(value)),4096)
        self.assertNotIn('private payload',json.dumps(value))
        self.assertTrue(all(set(frame)=={'source','function','line'} for frame in value['trace_frames']))

    def test_actual_staged_fixture_basename_normalizes_to_source_without_private_path(self):
        namespace={}
        exec(compile('def guarded():\n raise ValueError("private fixture payload")\n',
                     '/private/test-run/fixture.py','exec'),namespace)
        try:namespace['guarded']()
        except ValueError as error:value=fixture.failure_details(error,'admission')
        self.assertEqual(value['trace_frames'],[
            {'source':'application-intent-fixture.py','function':'guarded','line':2}])
        self.assertNotIn('/private',json.dumps(value))
        self.assertNotIn('private fixture payload',json.dumps(value))


if __name__=='__main__':unittest.main()
