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
                 patch.object(fixture,'retire_original_unit') as generic,patch('builtins.print'):
                fixture.retire(directory,'abcdef')
            leased.assert_called_once_with(directory,'abcdef',event,retiring=True)
            retired.assert_called_once_with(lease);generic.assert_not_called()


if __name__=='__main__':unittest.main()
