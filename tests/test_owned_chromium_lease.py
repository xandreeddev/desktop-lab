"""Original Chromium scope migration authority; no browser, VM or live D-Bus."""
import copy
import importlib.util
import os
from pathlib import Path
import signal
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'vm'))
import owned_chromium_lease as chromium


def scenario():
    unit={'load':'loaded','active':'active','transient':True,'main_pid':123,
          'invocation':'a'*32,'group':'/app.slice/run-owned.service',
          'executable':'/usr/bin/python3','argv':['/usr/bin/python3','/owned/fixture.py']}
    original={'pid':123,'start':'42','group':unit['group'],'executable':[1], 'sha256':'wrapper'}
    binary={'path':chromium.CHROMIUM,'identity':[2,3,4,0,319274072,6,7],'sha256':'binary'}
    declared={'argv':['/usr/bin/chromium','--user-data-dir=/owned/profile','--class=Owned',
              '--ozone-platform=wayland','--remote-debugging-address=127.0.0.1',
              '--remote-debugging-port=0','--new-window','about:blank'],
              'executable':binary,'launcher':{'path':'/usr/bin/chromium'},'profile':'/owned/profile','class':'Owned'}
    process=dict(original,group='/app.slice/actual-registered.scope',executable=binary['identity'],sha256='binary')
    scope={'load':'loaded','active':'active','transient':True,'invocation':'b'*32,
           'id':'actual-registered.scope','group':process['group']}
    return {'unit':unit,'process':original},declared,process,scope


class LeaseAuthority(unittest.TestCase):
    def capture(self, origin, declared, process, scope, expected=None):
        def snapshot(*_args,hashed=False,**_kwargs):
            return dict(process,sha256=process['sha256'] if hashed else None)
        with patch.object(chromium,'unit_snapshot',return_value=origin['unit']), \
             patch.object(chromium,'pinned_root_file',side_effect=lambda _path,value:value), \
             patch.object(chromium,'process_snapshot',side_effect=snapshot), \
             patch.object(chromium,'command_identity',return_value={'sha256':'opaque-title','bytes':123,'flattened':True}), \
             patch.object(chromium,'scope_snapshot',return_value=scope):
            return chromium.observe(origin,declared,expected)

    def test_same_original_birth_may_migrate_to_one_existing_typed_scope(self):
        origin,declared,process,scope=scenario()
        lease=self.capture(origin,declared,process,scope)
        self.assertEqual(lease['origin'],origin)
        self.assertEqual(lease['process']['pid'],origin['process']['pid'])
        self.assertNotEqual(lease['process']['group'],origin['process']['group'])
        self.assertEqual(lease['scope'],scope)
        self.assertEqual(self.capture(origin,declared,process,scope,lease),lease)

    def test_new_pid_birth_binary_scope_invocation_or_later_move_cannot_be_adopted(self):
        origin,declared,process,scope=scenario()
        lease=self.capture(origin,declared,process,scope)
        for change in ({'start':'43'},{'executable':[9]},{'group':'/app.slice/new.scope'}):
            with self.subTest(change=change),self.assertRaises(ValueError):
                self.capture(origin,declared,dict(process,**change),scope,lease)
        with self.assertRaises(ValueError):
            self.capture(origin,declared,process,dict(scope,invocation='c'*32),lease)
        with self.assertRaises(ValueError):
            self.capture(origin,declared,dict(process,sha256='foreign'),scope)

    def test_unregistered_or_wrong_cgroup_scope_is_not_pid_authority(self):
        origin,declared,process,scope=scenario()
        for changed in ({'load':'not-found'},dict(scope,group='/foreign'),dict(scope,transient=False),
                        dict(scope,id='guessed.scope'),dict(scope,invocation='')):
            with self.subTest(scope=changed),self.assertRaises(ValueError):
                self.capture(origin,declared,process,changed)

    def test_new_original_service_invocation_is_rejected_before_process_admission(self):
        origin,declared,_,_=scenario()
        with patch.object(chromium,'unit_snapshot',return_value=dict(origin['unit'],invocation='f'*32)), \
             patch.object(chromium,'process_snapshot') as process:
            with self.assertRaises(ValueError):chromium.observe(origin,declared)
        process.assert_not_called()


class OpaqueProcessTitle(unittest.TestCase):
    def check(self, raw):
        _,declared,_,_=scenario()
        with tempfile.TemporaryDirectory() as name:
            base=Path(name);(base/'123').mkdir();(base/'123/cmdline').write_bytes(raw)
            real=Path
            with patch.object(chromium,'Path',side_effect=lambda value:base if value=='/proc' else real(value)):
                return chromium.command_identity(123,declared)

    def test_flat_chromium_title_keeps_literal_startup_flags_without_shell_parsing(self):
        _,declared,_,_=scenario()
        flat=(chromium.CHROMIUM+' --normal-wrapper-flag '+' '.join(declared['argv'][1:])).encode()+b'\0'
        self.assertTrue(self.check(flat)['flattened'])
        structured=b'\0'.join(arg.encode() for arg in [chromium.CHROMIUM,*declared['argv'][1:]])+b'\0'
        self.assertFalse(self.check(structured)['flattened'])
        self.assertNotEqual(self.check(flat)['sha256'],self.check(structured)['sha256'])

    def test_profile_prefix_conflicting_flag_or_different_control_url_is_refused(self):
        _,declared,_,_=scenario()
        flat=(chromium.CHROMIUM+' '+' '.join(declared['argv'][1:])).encode()
        for raw in (flat.replace(b'/owned/profile',b'/owned/profile-other')+b'\0',
                    flat+b' --user-data-dir=/foreign\0',flat.replace(b'about:blank',b'https://example.invalid')+b'\0',
                    flat+b' --class=Other\0',flat+b'\0extra\0',b'x'*65537):
            with self.subTest(bytes=len(raw)),self.assertRaises(ValueError):self.check(raw)


class RegisteredScopeObservation(unittest.TestCase):
    def test_existing_kernel_named_scope_is_read_by_typed_properties_without_start_or_load(self):
        _,_,_,expected=scenario()
        manager,props,bus=Mock(),Mock(),Mock()
        manager.GetUnit.return_value='/observed/scope'
        fields={'Id':expected['id'],'LoadState':expected['load'],'ActiveState':expected['active'],
                'Transient':True,'InvocationID':bytes.fromhex(expected['invocation']),'ControlGroup':expected['group']}
        props.Get.side_effect=lambda _interface,key,**_kw:fields[key]
        dbus=types.SimpleNamespace(SessionBus=Mock(return_value=bus),
            Interface=lambda _obj,interface:manager if interface=='org.freedesktop.systemd1.Manager' else props,
            DBusException=RuntimeError)
        with patch.dict(sys.modules,{'dbus':dbus}):
            self.assertEqual(chromium.scope_snapshot(expected['group']),expected)
        manager.GetUnit.assert_called_once_with('actual-registered.scope',timeout=5)
        manager.LoadUnit.assert_not_called();manager.StartUnit.assert_not_called();manager.StopUnit.assert_not_called()
        self.assertIn((('org.freedesktop.systemd1.Scope','ControlGroup'),{'timeout':5}),
                      [(call.args,call.kwargs) for call in props.Get.call_args_list])
        bus.close.assert_called_once()


class PidfdRetirement(unittest.TestCase):
    def lease(self):
        origin,declared,process,scope=scenario()
        return {'origin':origin,'declared':declared,'unit':origin['unit'],'process':process,'scope':scope}

    def test_final_frozen_lease_check_precedes_the_only_pidfd_signal(self):
        lease=self.lease();order=[]
        with patch.object(chromium,'original_gone',return_value=False), \
             patch.object(chromium.os,'pidfd_open',return_value=9) as opened, \
             patch.object(chromium,'observe',side_effect=lambda *_args:order.append('recheck')), \
             patch.object(chromium.signal,'pidfd_send_signal',side_effect=lambda *_args:order.append('signal')) as sent, \
             patch.object(chromium,'retired',side_effect=[False,True]), \
             patch.object(chromium.time,'sleep'),patch.object(chromium.os,'close') as closed:
            chromium.retire(lease)
        self.assertEqual(order,['recheck','signal'])
        opened.assert_called_once_with(123);sent.assert_called_once_with(9,signal.SIGTERM)
        closed.assert_called_once_with(9)

    def test_raced_reborn_pid_or_scope_receives_no_signal(self):
        with patch.object(chromium,'original_gone',return_value=False), \
             patch.object(chromium.os,'pidfd_open',return_value=9), \
             patch.object(chromium,'observe',side_effect=ValueError('successor')), \
             patch.object(chromium.signal,'pidfd_send_signal') as sent,patch.object(chromium.os,'close') as closed:
            with self.assertRaises(ValueError):chromium.retire(self.lease())
        sent.assert_not_called();closed.assert_called_once_with(9)

    def test_already_exited_original_requires_both_registrations_and_recursive_drain(self):
        lease=self.lease()
        original=lease['origin']['unit']
        inactive=dict(original,active='inactive',main_pid=0,invocation='',group='')
        with patch.object(chromium,'unit_snapshot',return_value=inactive), \
             patch.object(chromium,'scope_snapshot',return_value={'load':'not-found'}), \
             patch.object(chromium,'original_gone',return_value=True), \
             patch.object(chromium,'group_drained',side_effect=[True,False]):
            self.assertFalse(chromium.retired(lease))
        with patch.object(chromium,'unit_snapshot',return_value=dict(inactive,invocation='f'*32)):
            with self.assertRaises(ValueError):chromium.retired(lease)

    def test_original_exit_racing_pidfd_open_requires_drain_and_sends_no_signal(self):
        with patch.object(chromium,'original_gone',side_effect=[False,True]), \
             patch.object(chromium.os,'pidfd_open',side_effect=ProcessLookupError), \
             patch.object(chromium,'retired',return_value=True) as drained, \
             patch.object(chromium.signal,'pidfd_send_signal') as sent:
            chromium.retire(self.lease())
        drained.assert_called_once();sent.assert_not_called()

    def test_descendants_that_do_not_drain_fail_within_the_deadline_without_extra_signals(self):
        with patch.object(chromium,'original_gone',return_value=True), \
             patch.object(chromium,'retired',return_value=False), \
             patch.object(chromium.time,'monotonic',side_effect=[0,1,21]), \
             patch.object(chromium.time,'sleep'),patch.object(chromium.signal,'pidfd_send_signal') as sent:
            with self.assertRaises(ValueError):chromium.retire(self.lease())
        sent.assert_not_called()


if __name__=='__main__':unittest.main()
