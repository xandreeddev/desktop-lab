"""Pure/owned tempfile tests only. Never connect to VM, bus or native display."""
import ast,copy,hashlib,importlib.util,io,json,os,stat,sys,tempfile,types,unittest
from pathlib import Path
from unittest.mock import Mock,patch
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'vm'
sys.path.insert(0,str(ROOT/'vm'))
def module(name):
    spec=importlib.util.spec_from_file_location('display_review_'+name,HERE/({'controller':'test-multi-output-displays.py','guest':'multi_output_display_guard.py'}[name]))
    value=importlib.util.module_from_spec(spec);spec.loader.exec_module(value);return value
c,g=module('controller'),module('guest')

class Boundaries(unittest.TestCase):
    def test_private_receipt_arguments_require_a_pair_before_execution(self):
        for args in (['--fresh-off','/tmp/private-proof.json'],['--fresh-off-sha256','a'*64]):
            with patch.object(sys,'argv',['test-multi-output-displays.py','--run',*args]),patch.object(c,'run',side_effect=AssertionError('no_transport')):
                with self.assertRaisesRegex(ValueError,'fresh_receipt_and_sha256_required_together'):c.main()

    def test_relative_private_receipt_resolves_against_lab_root_without_transport(self):
        with patch.object(sys,'argv',['test-multi-output-displays.py','--lab-root','/tmp/private-lab','--fresh-off','reports/local/proof.json','--fresh-off-sha256','a'*64]),patch.object(c,'qualify') as qualify,patch('sys.stdout',new=io.StringIO()):
            self.assertEqual(c.main(),0)
        qualify.assert_called_once_with(('/tmp/private-lab/reports/local/proof.json','a'*64),
                                        c.NORMAL_OFF_IDENTITY,c.RELEASE_MANIFEST)

    def test_collector_binding_and_helper_require_a_pair_before_execution(self):
        for args in (['--fresh-off-binding-sha256','a'*64],['--fresh-off-helper-sha256','b'*64]):
            with patch.object(sys,'argv',['test-multi-output-displays.py','--run',*args]),patch.object(c,'run',side_effect=AssertionError('no_transport')):
                with self.assertRaisesRegex(ValueError,'fresh_collector_binding_and_helper_required_together'):c.main()

    def test_private_manifest_and_collector_identity_are_explicit_arguments(self):
        argv=['test-multi-output-displays.py','--lab-root','/tmp/private-lab',
              '--fresh-off','reports/local/proof.json','--fresh-off-sha256','a'*64,
              '--fresh-off-binding-sha256','b'*64,'--fresh-off-helper-sha256','c'*64,
              '--release-manifest','.cache/releases/manifest.json']
        with patch.object(sys,'argv',argv),patch.object(c,'qualify') as qualify,patch('sys.stdout',new=io.StringIO()):
            self.assertEqual(c.main(),0)
        qualify.assert_called_once_with(('/tmp/private-lab/reports/local/proof.json','a'*64),
                                        ('b'*64,'c'*64),'/tmp/private-lab/.cache/releases/manifest.json')

    def test_default_unbound_refuses_before_source_or_transport(self):
        with patch.object(c,'read',side_effect=AssertionError('no_io')):
            with self.assertRaisesRegex(ValueError,'reviewed_execution_bindings_UNBOUND'): c.qualify()

    def test_missing_collector_or_release_binding_refuses_before_io(self):
        for fresh,identity,manifest in ((('/tmp/unit-proof.json','a'*64),c.NORMAL_OFF_IDENTITY,'/tmp/unit-manifest.json'),
                                        (('/tmp/unit-proof.json','a'*64),('b'*64,'c'*64),c.RELEASE_MANIFEST)):
            with patch.object(c,'read',side_effect=AssertionError('no_io')):
                with self.assertRaisesRegex(ValueError,'reviewed_execution_bindings_UNBOUND'):
                    c.qualify(fresh,identity,manifest)

    def test_native_run_requires_an_exact_vm_uuid_before_transport(self):
        for value in (None,'desktop-lab-lucent','a'*36,'../elsewhere','00000000-0000-0000-0000-000000000000'):
            with patch.object(c,'qualify',side_effect=AssertionError('no_source_io')):
                with self.assertRaisesRegex(ValueError,'exact_vm_uuid_required'):
                    c.run(Path('/tmp/private-lab'),vm_uuid=value)

    def test_archived_source_and_actual_receipt_qualification_has_no_native_side_effects(self):
        # Synthetic unit fixtures exercise the parser only; they are never native evidence.
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);guest=root/'multi_output_display_guard.py'
            guest.write_bytes(b'unit-only guest fixture\n')
            receipt=root/'unit-only-off.json';manifest=root/'unit-only-manifest.json'
            release={'source_commit':c.SOURCE,'passed':True,'source_working_tree_changed':False,
                     'native_activated':False,'binaries':{'lucent-desktop':{'sha256':c.DESKTOP}}}
            fresh={'success':True,'read_only':True,'source_commit':c.SOURCE,
                   'binding_sha256':'b'*64,'helper_sha256':'c'*64,
                   'facts':{'runtime_profiling':{'frames':False,'presentation':False,'source_pts':False},
                            'authority':{'desktop':{'sha256':c.DESKTOP}}}}
            def bind(value,path):
                path.write_text(json.dumps(value));return hashlib.sha256(path.read_bytes()).hexdigest()
            manifest_sha=bind(release,manifest)
            with patch.multiple(c,ROOT=root,HERE=root,PINS={},RELEASE_MANIFEST_SHA=manifest_sha,
                                GUEST_SHA=hashlib.sha256(guest.read_bytes()).hexdigest()):
                receipt_sha=bind(fresh,receipt)
                raw,facts,actual=c.qualify((str(receipt),receipt_sha),('b'*64,'c'*64),str(manifest))
                self.assertEqual(raw,{})
                self.assertEqual(facts,fresh['facts']);self.assertEqual(actual,guest.read_bytes())
                for key,value in (('source_commit','0'*40),('binding_sha256','d'*64),('helper_sha256','e'*64)):
                    changed={**fresh,key:value};receipt_sha=bind(changed,receipt)
                    with self.assertRaisesRegex(ValueError,'exact_normal_off_receipt_required'):
                        c.qualify((str(receipt),receipt_sha),('b'*64,'c'*64),str(manifest))
                changed=copy.deepcopy(fresh);changed['facts']['runtime_profiling']['frames']=True
                receipt_sha=bind(changed,receipt)
                with self.assertRaisesRegex(ValueError,'exact_normal_off_receipt_required'):
                    c.qualify((str(receipt),receipt_sha),('b'*64,'c'*64),str(manifest))
                receipt_sha=bind(fresh,receipt)
                bind({**release,'source_commit':'0'*40},manifest)
                with self.assertRaisesRegex(ValueError,'frozen_source_changed'):
                    c.qualify((str(receipt),receipt_sha),('b'*64,'c'*64),str(manifest))
                manifest_sha=bind({**release,'native_activated':True},manifest)
                with patch.object(c,'RELEASE_MANIFEST_SHA',manifest_sha):
                    with self.assertRaisesRegex(ValueError,'exact_archived_build_manifest_required'):
                        c.qualify((str(receipt),receipt_sha),('b'*64,'c'*64),str(manifest))

    def test_configuration_restore_never_races_a_closed_but_pending_display_guardian(self):
        owner=g.Owner.__new__(g.Owner)
        child={'open':False,'visible':False,'busy':True,'transaction':{'phase':'confirmation'}}
        owner.cleanup_state=Mock(return_value={'client':{'displays':child}})
        with \
             patch.object(g.time,'monotonic',side_effect=[0,21]),patch.object(g,'exchange_restore') as restore:
            with self.assertRaisesRegex(g.p.Blocked,'display_transaction_retirement_deadline'):
                owner.cleanup()
        restore.assert_not_called()

    def test_lost_cursor_still_restores_actual_original_inode_and_removes_only_owned_peer(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary);config=directory/'monitors.lua'
            config.write_bytes(b'original user rules\n');original=g.read(config)
            os.link(config,directory/'monitors.original')
            staged=directory/'staged';staged.write_bytes(b'captured owned keep\n');os.replace(staged,config)
            owner=g.Owner.__new__(g.Owner);owner.directory=directory;owner.config=config
            owner.original=original;owner.kept=g.read(config);owner.workspace=1;owner.pointer={'x':960,'y':540}
            owner.cursor=g.p.CursorOwnership((10,20));owner.cursor.lost=True
            original_monitor={'id':0,'name':'Virtual-1','description':'original','width':1920,'height':1080,
                'x':0,'y':0,'scale':1,'transform':0,'refreshRate':60.,'disabled':False,'dpmsStatus':True,
                'activeWorkspace':{'id':1}}
            owner.baseline=[g.p.monitor_signature(original_monitor)]
            name='LUCENT-TEST-'+'a'*24
            peer={**original_monitor,'id':1,'name':name,'width':1280,'height':960,'x':1968}
            owner.owned=g.p.monitor_signature(peer);owner.allowed=[owner.owned]
            owner.peer_identity={key:peer[key] for key in ('id','name','description')}
            owner.lease=types.SimpleNamespace(name=name,expected=owner.owned,global_id=9)
            child={'open':False,'visible':False,'busy':False,'transaction':{'phase':'reverted'}}
            state={'client':{'named_monitor_hosts':True,'displays':child,'monitor_hosts':[
                {'id':name,'instance':'owned-instance'}]},'outputs':[{'name':name,'global':9}],
                'surfaces':[{'id':'owned-bar','instance':'owned-instance','output':name}],
                'retiring_native_surfaces':0}
            present=True
            def hypr(*args):
                nonlocal present
                if args==('-j','cursorpos'):return json.dumps(owner.pointer)
                if args==('output','remove',name):
                    present=False;state['client']['monitor_hosts']=[];state['outputs']=[];state['surfaces']=[]
                    return 'ok'
                self.fail('cleanup must not send cursor or focus input: '+repr(args))
            owner.cleanup_state=Mock(side_effect=lambda:state);owner.resource_state=Mock(side_effect=lambda:state)
            owner.all_monitors=Mock(side_effect=lambda:[original_monitor,peer] if present else [original_monitor])
            owner.monitors=Mock(return_value=[original_monitor]);owner.admit_file=Mock()
            owner.preserve=Mock();owner.security_sources=Mock()
            owner.restore_focus=Mock(side_effect=AssertionError('lost_cursor_has_no_input_authority'))
            with patch.object(g.base,'hypr',side_effect=hypr) as native:
                result=owner.cleanup()
            self.assertTrue(all(result.values()));self.assertIsNone(owner.owned)
            self.assertTrue(g.original_file(original,g.read(config)))
            self.assertEqual(config.stat().st_ino,(directory/'monitors.original').stat().st_ino)
            self.assertEqual([call.args for call in native.call_args_list if call.args[:2]==('output','remove')],
                             [('output','remove',name)])
            owner.restore_focus.assert_not_called()
            self.assertFalse(owner.cleanup_cursor['restored']);self.assertFalse(owner.cleanup_cursor['authority_retained'])
            self.assertTrue(owner.cleanup_cursor['at_original_position']);self.assertTrue(owner.cursor.lost)

    def test_foreign_ui_still_blocks_resource_cleanup_before_any_mutation(self):
        idle={'launcher':False,'settings_writable':True,'settings_save_status':'saved',
              'background_ready':True,'named_monitor_hosts':True,'displays':{'open':True}}
        for foreign in ({'notification_count':1},{'launcher':True},{'audio':{'open':True}},
                        {'network':{'busy':True}}):
            owner=g.Owner.__new__(g.Owner)
            owner.resource_state=Mock(return_value={'client':{**idle,**foreign}})
            with patch.object(g,'exchange_restore') as restore,patch.object(g.base.Native,'remove') as remove,\
                 patch.object(g.base,'command') as command:
                with self.assertRaises(g.p.Blocked):owner.cleanup()
            restore.assert_not_called();remove.assert_not_called();command.assert_not_called()

    def test_foreign_window_auth_or_source_rejection_prevents_cleanup_mutations(self):
        for label in ('no_windows_on_any_workspace_required','no_authorization_surface_required','source_or_security_changed'):
            owner=g.Owner.__new__(g.Owner);owner.resource_state=Mock(side_effect=g.p.Blocked(label))
            with patch.object(g,'exchange_restore') as restore,patch.object(g.base.Native,'remove') as remove,\
                 patch.object(g.base,'command') as command:
                with self.assertRaisesRegex(g.p.Blocked,label):owner.cleanup()
            restore.assert_not_called();remove.assert_not_called();command.assert_not_called()

    def test_global_run_failure_never_claims_no_guest_actions_after_admission(self):
        tree=ast.parse(Path(c.__file__).read_text())
        outer=tree.body[-1]
        self.assertIsInstance(outer,ast.If)
        code=compile(ast.Module(body=outer.body,type_ignores=[]),'conservative_entrypoint','exec')
        for argv,expected in ((['controller.py','--run'],'unknown'),(['controller.py'],False)):
            namespace={'main':Mock(side_effect=ValueError('post_admission_failure')),'sys':sys,'json':json}
            with patch.object(sys,'argv',argv),patch('sys.stdout',io.StringIO()) as output:
                with self.assertRaises(SystemExit):exec(code,namespace)
            reply=json.loads(output.getvalue())
            self.assertEqual(reply['guest_actions'],expected)
            self.assertFalse(reply['success'])

    def test_complete_keep_tail_preserves_prefix_and_contains_both_outputs(self):
        mode={'width':1920,'height':1080,'refreshRate':60,'scale':1,'x':0,'y':0,'disabled':False,'transform':0}
        rows=[dict(mode,name='Virtual-1'),dict(mode,name='LUCENT-TEST-'+'a'*24,x=1968,width=1280,height=960)]
        prefix=b'-- user rules\nhl.monitor({ output="", mode="preferred" })\n'
        content=g.keep_content(prefix,rows)
        self.assertTrue(content.startswith(prefix+g.BEGIN));self.assertTrue(content.endswith(g.END))
        self.assertEqual(content.count(b'hl.monitor('),3)
        self.assertIn(b'position = "1968x0"',content)
        self.assertEqual(g.keep_content(content,rows),content)
        for malformed in (prefix+g.END,prefix+g.BEGIN+b'bad',prefix+g.BEGIN+g.BEGIN+g.END):
            with self.assertRaises(g.p.Blocked):g.keep_content(malformed,rows)

    def test_actual_exchange_restores_original_inode_and_preserves_siblings(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);config=root/'monitors.lua';backup=root/'original'
            config.write_bytes(b'original');original=g.read(config);os.link(config,backup)
            staging=root/'staged';staging.write_bytes(b'controlled keep');os.replace(staging,config)
            kept=g.read(config);sibling=root/'unrelated';sibling.write_bytes(b'keep')
            g.exchange_restore(config,backup,original,kept,root)
            self.assertTrue(g.original_file(original,g.read(config)))
            self.assertEqual(config.stat().st_ino,backup.stat().st_ino)
            self.assertEqual(sibling.read_bytes(),b'keep')
            self.assertFalse((root/'monitors.restore').exists())

    def test_changed_identical_bytes_inode_or_foreign_content_is_never_overwritten(self):
        for foreign in (b'controlled keep',b'foreign change'):
            with tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary);config=root/'monitors.lua';backup=root/'original'
                config.write_bytes(b'original');original=g.read(config);os.link(config,backup)
                tmp=root/'tmp';tmp.write_bytes(b'controlled keep');os.replace(tmp,config);kept=g.read(config)
                tmp.write_bytes(foreign);os.replace(tmp,config);before=g.read(config)
                with self.assertRaisesRegex(g.p.Blocked,'independent_monitor_change_preserved'):
                    g.exchange_restore(config,backup,original,kept,root)
                self.assertEqual(before,g.read(config))

    def test_uncaptured_keep_refuses_restoration_and_keeps_checkpoint(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);config=root/'monitors.lua';backup=root/'original'
            config.write_bytes(b'original');original=g.read(config);os.link(config,backup)
            tmp=root/'tmp';tmp.write_bytes(b'unknown new inode');os.replace(tmp,config)
            with self.assertRaisesRegex(g.p.Blocked,'independent_monitor_change_preserved'):
                g.exchange_restore(config,backup,original,None,root)
            self.assertEqual(config.read_bytes(),b'unknown new inode');self.assertTrue(backup.is_file())

    def test_keyboard_and_pointer_bindings_cannot_intercept_owned_native_inputs(self):
        g.keyboard_bindings([])
        for row in ({'modmask':0,'key':'Tab'},{'modmask':4,'key':'a'},
                    {'modmask':0,'key':'period'},{'modmask':0,'key':'mouse:272'},
                    {'modmask':0,'key':'other','catch_all':True}):
            with self.assertRaises(g.p.Blocked):g.keyboard_bindings([row])
        g.keyboard_bindings([{'modmask':64,'key':'a'}])

    def panel_warp(self,move_on_dispatch):
        owner=g.Owner.__new__(g.Owner)
        owner.original_host={'generation':4};owner.original_global=9;owner.baseline=[{}]
        owner.panel_target='Virtual-1'
        owner.lease=types.SimpleNamespace(name='LUCENT-TEST-'+'a'*24)
        owner.cursor=g.p.CursorOwnership((10,20))
        current={'x':10,'y':20}
        owner.status=Mock(return_value={'panel':{'id':'displays'},'layer':{},'cursor':dict(current),
            'host':owner.original_host,'global':owner.original_global,'monitors':[{'name':'Virtual-1'}],
            'state':{'selected':owner.lease.name}})
        def hypr(*args):
            if args==('dispatch','hl.dsp.cursor.move({x=30,y=40})'):
                if move_on_dispatch:current.update(x=30,y=40)
                return 'ok'
            if args==('eval','hl.dsp.cursor.move({x=30,y=40})'):
                return 'ok'  # Constructing a descriptor does not dispatch it.
            if args==('-j','cursorpos'):return json.dumps(current)
            self.fail('unexpected_native_cursor_operation')
        request={'operation':'warp','monitor':'Virtual-1','generation':4,'global':9,'identifier':'display-reset'}
        return owner,hypr,request

    def test_panel_warp_dispatches_before_confirming_declared_cursor_postimage(self):
        owner,hypr,request=self.panel_warp(True)
        with patch.object(g.p,'hit_center',return_value=(30,40)),patch.object(g.base,'hypr',side_effect=hypr) as native:
            result=owner.operation(request)
        self.assertEqual(result,{'point':(30,40),'surface':'displays'})
        self.assertEqual(native.call_args_list[0].args,('dispatch','hl.dsp.cursor.move({x=30,y=40})'))
        self.assertEqual(owner.cursor.expected,(30,40));self.assertIsNone(owner.cursor.pending)
        self.assertFalse(owner.cursor.lost)

    def test_panel_warp_ack_without_actual_move_never_recovers_uncertain_cursor(self):
        owner,hypr,request=self.panel_warp(False)
        with patch.object(g.p,'hit_center',return_value=(30,40)),patch.object(g.base,'hypr',side_effect=hypr):
            with self.assertRaisesRegex(g.p.Blocked,'native_cursor_postimage_differs_from_declared_transition'):
                owner.operation(request)
        self.assertTrue(owner.cursor.lost)
        self.assertEqual(owner.cursor.expected,(10,20));self.assertEqual(owner.cursor.pending,(30,40))
        for actual in ({'x':10,'y':20},{'x':30,'y':40}):
            with self.assertRaisesRegex(g.p.Blocked,'foreign_or_uncertain_cursor_movement_blocks_warp'):
                owner.cursor.check(actual)

    def test_headless_lifetime_and_original_output_cannot_be_adopted(self):
        original={'id':0,'name':'Virtual-1','description':'original','width':1920,'height':1080,
            'x':0,'y':0,'scale':1,'transform':0,'refreshRate':60,'disabled':False,'dpmsStatus':True}
        owned=dict(original,id=2,name='LUCENT-TEST-'+'a'*24,width=1280,height=960,x=1952)
        g.p.unchanged_layout([original,owned],[original],owned['name'],owned)
        for mutate in (lambda rows:rows[0].update(x=16),lambda rows:rows[1].update(id=3)):
            changed=copy.deepcopy([original,owned]);mutate(changed)
            with self.assertRaises(g.p.Blocked):g.p.unchanged_layout(changed,[original],owned['name'],owned)

    def test_security_stamp_detects_identical_content_replacement_and_literal_link_changes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);path=root/'source';path.write_bytes(b'unchanged')
            before=g.stamp(path);self.assertEqual(g.metadata(before),g.stamp(path,False))
            new=root/'new';new.write_bytes(b'unchanged');os.replace(new,path)
            self.assertNotEqual(g.metadata(before),g.stamp(path,False))
            link=root/'enabled';link.symlink_to(path);captured=g.stamp(link)
            self.assertEqual(captured['resolved_target']['stamp'],g.stamp(path))
            link.unlink();link.symlink_to(new)
            with self.assertRaises(FileNotFoundError):g.stamp(link)


class ExtendedDisplayCases(unittest.TestCase):
    def setUp(self):
        self.original={'id':0,'name':'Virtual-1','description':'original','width':1920,'height':1080,
            'x':0,'y':0,'scale':1,'transform':0,'refreshRate':60,'disabled':False,'dpmsStatus':True}
        self.peer=dict(self.original,id=2,name='LUCENT-TEST-'+'a'*24,description='owned headless',
                       width=1280,height=960,x=1952)
        self.identity={key:self.peer[key] for key in ('id','name','description')}
        self.disabled={**self.identity,'disabled':True}

    def test_rotated_and_disabled_peer_do_not_weaken_the_original_native_policy(self):
        rotated={**self.peer,'transform':1}
        with self.assertRaises(g.p.Blocked):g.p.monitor_signature(rotated)
        self.assertEqual(g.p.display_peer_signature(rotated,self.identity),rotated)
        native_disabled={**self.peer,'width':0,'height':0,'refreshRate':0,'disabled':True}
        with self.assertRaises(g.p.Blocked):g.p.monitor_signature(native_disabled)
        self.assertEqual(g.p.display_layout([self.original,native_disabled],[self.original],self.identity),self.disabled)
        for mutation in ({'id':3},{'name':'Virtual-1'},{'description':'replacement'}):
            with self.assertRaisesRegex(g.p.Blocked,'owned_display_native_identity_changed'):
                g.p.display_peer_signature({**native_disabled,**mutation},self.identity)
        for mutation in ({'transform':1},{'disabled':True},{'x':16}):
            with self.assertRaises(g.p.Blocked):
                g.p.display_layout([{**self.original,**mutation},native_disabled],[self.original],self.identity)
        with self.assertRaisesRegex(g.p.Blocked,'inventory_required'):
            g.p.display_layout([self.original,native_disabled,dict(self.peer,name='foreign')],[self.original],self.identity)

    def test_fractional_logical_viewport_is_separate_from_physical_buffer_and_rotation(self):
        self.assertEqual(g.p.display_logical_size({**self.peer,'scale':1.25}),(1024,768))
        self.assertEqual(g.p.display_logical_size({**self.peer,'scale':1.25,'transform':1}),(768,1024))
        for mutation in ({'disabled':True},{'scale':float('nan')},{'transform':7}):
            with self.assertRaises(g.p.Blocked):g.p.display_logical_size({**self.peer,**mutation})

    def test_real_mode_index_preserves_native_order_deduplication_and_disabled_fallback(self):
        native={**self.peer,'availableModes':['invalid','1280x960@60.000Hz','1280x960@60',
                                           '1920x1080@60Hz','1280x720@nanHz']}
        self.assertEqual(g.p.display_mode_index(native,1280,960,60000),0)
        self.assertEqual(g.p.display_mode_index({**native,'width':1024,'height':768},1280,960,60000),1)
        inactive={**native,'disabled':True,'width':0,'height':0,'refreshRate':0}
        self.assertEqual(g.p.display_mode_index(inactive,1920,1080,60000),1)
        with self.assertRaisesRegex(g.p.Blocked,'not_advertised'):
            g.p.display_mode_index(inactive,800,600,60000)

    def test_saved_disabled_rule_is_allowed_only_for_the_exact_owned_peer(self):
        inactive={**self.peer,'disabled':True,'width':0,'height':0,'refreshRate':0}
        content=g.keep_content(b'-- original\n',[self.original,inactive],self.peer['name'])
        self.assertIn(('hl.monitor({ output = '+json.dumps(self.peer['name'])+', disabled = true })\n').encode(),content)
        self.assertIn(b'output = "Virtual-1", disabled = false',content)
        with self.assertRaisesRegex(g.p.Blocked,'original_display_must_remain_enabled'):
            g.keep_content(b'', [{**self.original,'disabled':True},self.peer],self.peer['name'])
        with self.assertRaisesRegex(g.p.Blocked,'rotation_must_remain_unchanged'):
            g.keep_content(b'', [{**self.original,'transform':1},self.peer],self.peer['name'])

    def test_physical_original_draft_never_receives_mutating_control_input(self):
        name=self.peer['name']
        for identifier in ('display-enabled','display-rotation','display-apply','display-mode-0','display-field-0-1'):
            with self.assertRaisesRegex(g.p.Blocked,'only_owned_display_draft'):
                g.p.require_owned_display_control(identifier,'Virtual-1',name)
            g.p.require_owned_display_control(identifier,name,name)
        for identifier in ('display-reset','display-select-'+name):
            g.p.require_owned_display_control(identifier,'Virtual-1',name)
        with self.assertRaises(g.p.Blocked):
            g.p.require_owned_display_control('display-select-Virtual-1',name,name)

    def owner(self,disabled=False):
        owner=g.Owner.__new__(g.Owner)
        owner.baseline=[self.original];owner.peer_identity=self.identity
        owner.allowed=[self.peer,self.disabled];owner.peer_global=9;owner.peer_disabled=disabled
        owner.peer_retired=disabled;owner.peer_ready=False;owner.lifecycle='enable' if disabled else 'disable'
        owner.lifecycle_transaction=1
        owner.peer_host={'id':self.peer['name'],'instance':'peer-instance','generation':4}
        owner.peer_surfaces={'background-peer','bar-peer'}
        owner.lease=types.SimpleNamespace(name=self.peer['name'],global_id=9)
        owner.pid=123
        row=self.peer if disabled else {**self.peer,'disabled':True,'width':0,'height':0}
        owner.all_monitors=Mock(return_value=[self.original,row])
        state={'client':{'named_monitor_hosts':True,'monitor_hosts':[],
                         'displays':{'busy':False,'transaction':{'id':2,'phase':'confirmation'}}},
               'outputs':[],'surfaces':[],'retiring_native_surfaces':0}
        return owner,state

    def test_disabled_native_peer_requires_our_pending_display_transaction_and_full_retirement(self):
        owner,state=self.owner()
        old=copy.deepcopy(state);old['client']['displays']['transaction']['id']=1
        with self.assertRaisesRegex(g.p.Blocked,'without_admitted_native_transaction'):owner.peer_state(old)
        self.assertEqual(owner.peer_state(state),self.disabled)
        self.assertTrue(owner.peer_ready and owner.peer_disabled and owner.peer_retired)
        self.assertEqual(owner.peer_global,9)

    def enabled_state(self):
        owner,state=self.owner(True)
        state['outputs']=[{'name':self.peer['name'],'global':10}]
        state['client']['monitor_hosts']=[{**owner.peer_host,'generation':5,'viewport':[1280,960],'density':1}]
        state['surfaces']=[{'id':role+'-peer','role':role,'output':self.peer['name'],
                            'instance':'peer-instance','native_frames':1,'frames':1,'density':1,
                            'width':1280,'height':960 if role=='background' else 144,
                            'buffer_width':1280,'buffer_height':960 if role=='background' else 144}
                           for role in ('background','bar')]
        return owner,state

    def test_reenable_adopts_only_a_new_global_generation_after_actual_peer_submissions(self):
        for mutation in ('same_global','same_generation','old_transaction'):
            owner,state=self.enabled_state()
            if mutation=='same_global':state['outputs'][0]['global']=9
            if mutation=='same_generation':state['client']['monitor_hosts'][0]['generation']=4
            if mutation=='old_transaction':state['client']['displays']['transaction']['id']=1
            with patch.object(g.base,'hypr',return_value='{}'),patch.object(g.p,'layer_for',return_value={}):
                with self.assertRaises(g.p.Blocked):owner.peer_state(state)
        owner,state=self.enabled_state();state['surfaces'][0]['native_frames']=0
        with patch.object(g.base,'hypr',return_value='{}'),patch.object(g.p,'layer_for',return_value={}):
            owner.peer_state(state)
        self.assertFalse(owner.peer_ready);self.assertTrue(owner.peer_disabled)
        state['surfaces'][0]['native_frames']=1
        with patch.object(g.base,'hypr',return_value='{}'),patch.object(g.p,'layer_for',return_value={}):
            self.assertEqual(owner.peer_state(state),self.peer)
        self.assertTrue(owner.peer_ready);self.assertFalse(owner.peer_disabled)
        self.assertEqual(owner.peer_global,10);self.assertEqual(owner.lease.global_id,10)

    def test_fractional_panel_status_requires_actual_density_buffer_and_exact_output_layer(self):
        owner=g.Owner.__new__(g.Owner);owner.pid=123;owner.panel_target=self.peer['name']
        owner.owned=self.peer;owner.peer_ready=True;owner.peer_state=Mock(return_value=self.peer)
        owner.monitors=Mock(return_value=[self.original,{**self.peer,'scale':1.25}]);owner.input_preflight=Mock(return_value=None)
        panel={'id':'displays','output':self.peer['name'],'instance':None,'pointer_input':'all','keyboard':'exclusive',
               'frames':3,'native_frames':2,'width':1024,'height':768,'density':1.25,'buffer_width':1280,
               'buffer_height':960,'hits':[]}
        state={'client':{'named_monitor_hosts':True,'active_monitor':self.peer['name'],'displays':{'open':True},
                        'monitor_hosts':[{'id':self.peer['name'],'instance':'peer-instance','generation':5}]},
               'outputs':[{'name':self.peer['name'],'global':10}],'surfaces':[panel]}
        owner.checked=Mock(return_value=state)
        layer={'namespace':'lucent-displays','address':'0x123','pid':123,'x':1952,'y':0,'w':1024,'h':768}
        layers={self.peer['name']:{'levels':{'3':[layer]}}}
        def hypr(*args):
            if args==('-j','layers'):return json.dumps(layers)
            if args==('-j','cursorpos'):return json.dumps({'x':2200,'y':200})
            self.fail('unexpected_native_operation')
        with patch.object(g.base,'hypr',side_effect=hypr):
            value=owner.status()
            self.assertEqual(value['host']['generation'],5);self.assertEqual(value['global'],10)
            for mutation in ({'density':2},{'buffer_width':1024},{'output':'Virtual-1'}):
                original=dict(panel);panel.update(mutation)
                with self.assertRaises(g.p.Blocked):owner.status()
                panel.clear();panel.update(original)

class DeclaredDisableCursor(unittest.TestCase):
    def fixture(self):
        original={'id':0,'name':'Virtual-1','description':'original','width':1920,'height':1080,
            'x':0,'y':0,'scale':1,'transform':0,'refreshRate':60.,'disabled':False,'dpmsStatus':True}
        identity={'id':1,'name':'LUCENT-TEST-'+'a'*24,'description':''}
        original_host={'id':'Virtual-1','instance':'original-instance','generation':1}
        peer_host={'id':identity['name'],'instance':'peer-instance','generation':2}
        cursor=g.p.CursorOwnership((1200,900))
        declaration=g.p.DisplayDisableCursor(cursor,[original],identity,peer_host,{'peer-bar'},
                                              original_host,71,4,10.)
        state={'client':{'named_monitor_hosts':True,'monitor_hosts':[original_host],
                         'displays':{'selected':identity['name'],'open':True,'visible':True,'busy':False,
                                     'transaction':{'id':5,'phase':'confirmation'}}},
               'outputs':[{'name':'Virtual-1','global':71}],'surfaces':[],
               'retiring_native_surfaces':0}
        return cursor,declaration,state,{**identity,'disabled':True}

    def test_declared_center_grants_nothing_until_exact_transaction_and_full_retirement(self):
        cursor,declaration,state,peer=self.fixture()
        self.assertEqual(cursor.expected,(1200,900));self.assertEqual(declaration.destination,(960,540))
        state['client']['monitor_hosts'].append({'id':peer['name'],'instance':'peer-instance','generation':2})
        self.assertFalse(declaration.confirm(cursor,{'x':960,'y':540},state,peer,11.))
        self.assertEqual(cursor.expected,(1200,900));self.assertFalse(declaration.consumed)
        state['client']['monitor_hosts'].pop()
        self.assertTrue(declaration.confirm(cursor,{'x':960,'y':540},state,peer,11.1))
        self.assertEqual(cursor.expected,(960,540));self.assertTrue(declaration.consumed)
        with self.assertRaisesRegex(g.p.Blocked,'expired_or_consumed'):
            declaration.confirm(cursor,{'x':960,'y':540},state,peer,11.2)

    def test_wrong_point_native_peer_transaction_backup_or_expired_window_never_confirms(self):
        for case in ('point','floating_point','enabled','different_id','old_transaction','wrong_phase','panel_closed',
                     'wrong_selection','changed_backup','expired','clock_before_declaration','lost_cursor'):
            cursor,declaration,state,peer=self.fixture();point={'x':960,'y':540};now=11.
            if case=='point':point['x']=961
            if case=='floating_point':point['x']=960.
            if case=='enabled':peer['disabled']=False
            if case=='different_id':peer['id']=2
            if case=='old_transaction':state['client']['displays']['transaction']['id']=4
            if case=='wrong_phase':state['client']['displays']['transaction']['phase']='reverted'
            if case=='panel_closed':state['client']['displays']['open']=False
            if case=='wrong_selection':state['client']['displays']['selected']='Virtual-1'
            if case=='changed_backup':state['outputs'][0]['global']=72
            if case=='expired':now=15.001
            if case=='clock_before_declaration':now=9.
            if case=='lost_cursor':cursor.lost=True
            with self.subTest(case=case),self.assertRaises(g.p.Blocked):
                declaration.confirm(cursor,point,state,peer,now)
            self.assertEqual(cursor.expected,(1200,900));self.assertFalse(declaration.consumed)

    def test_native_disable_before_client_reply_waits_without_admitting_input(self):
        cursor,declaration,state,peer=self.fixture()
        child=state['client']['displays'];child['transaction']['id']=4;child['busy']=True
        self.assertFalse(declaration.confirm(cursor,{'x':960,'y':540},state,peer,11.))
        self.assertEqual(cursor.expected,(1200,900));self.assertFalse(declaration.consumed)
        child['transaction']={'id':5,'phase':'confirmation'};child['busy']=False
        self.assertTrue(declaration.confirm(cursor,{'x':960,'y':540},state,peer,11.1))

    def test_guard_accepts_only_proven_disable_and_sticky_rejects_wrong_point_or_state(self):
        for case in ('proved','point','state','unretired_expired'):
            cursor,declaration,state,peer=self.fixture();point={'x':960,'y':540}
            owner=g.Owner.__new__(g.Owner);owner.cursor=cursor;owner.disable_cursor=declaration
            owner.directory=Path('/unit-only-not-written');owner.checked=Mock(return_value=state)
            owner.panel_observation=Mock()
            owner.peer_state=Mock(return_value=peer)
            if case=='point':point['x']=961
            if case=='state':state['client']['displays']['transaction']['id']=4
            if case=='unretired_expired':state['retiring_native_surfaces']=1
            def hypr(*args):
                if args==('-j','binds'):return '[]'
                if args==('-j','cursorpos'):return json.dumps(point)
                self.fail('cursor retirement must not dispatch input')
            clock=[11.,16.] if case=='unretired_expired' else [11.]
            with patch.object(g.base,'hypr',side_effect=hypr),patch.object(g.time,'monotonic',side_effect=clock),\
                 patch.object(g.time,'sleep'),patch.object(g.base,'private_json') as saved:
                if case=='proved':
                    self.assertIs(owner.input_preflight(),state)
                    self.assertFalse(cursor.lost);self.assertEqual(cursor.expected,(960,540));saved.assert_called_once()
                else:
                    with self.assertRaises(g.p.Blocked):owner.input_preflight()
                    self.assertTrue(cursor.lost);self.assertEqual(cursor.expected,(1200,900));saved.assert_not_called()
                    point.update(x=1200,y=900)
                    with self.assertRaisesRegex(g.p.Blocked,'foreign_or_uncertain_cursor_movement_blocks_warp'):
                        owner.input_preflight()
            self.assertIsNone(owner.disable_cursor)

    def test_foreign_panel_rejection_precedes_cursor_authority_commit(self):
        cursor,declaration,state,peer=self.fixture()
        owner=g.Owner.__new__(g.Owner);owner.cursor=cursor;owner.disable_cursor=declaration
        owner.checked=Mock(return_value=state)
        owner.panel_observation=Mock(side_effect=g.p.Blocked('no_foreign_panel_required'))
        owner.peer_state=Mock(return_value=peer)
        with patch.object(g.base,'hypr',side_effect=['[]','{"x":960,"y":540}']),\
             patch.object(g.base,'private_json') as saved:
            with self.assertRaisesRegex(g.p.Blocked,'no_foreign_panel_required'):owner.input_preflight()
        self.assertTrue(cursor.lost);self.assertEqual(cursor.expected,(1200,900))
        self.assertFalse(declaration.consumed);saved.assert_not_called();owner.peer_state.assert_not_called()

    def test_cursor_movement_during_source_and_panel_probes_cannot_confirm_stale_center(self):
        cursor,declaration,state,peer=self.fixture()
        observations=[]
        owner=g.Owner.__new__(g.Owner);owner.cursor=cursor;owner.disable_cursor=declaration
        owner.checked=Mock(side_effect=lambda:observations.append('sources') or state)
        owner.panel_observation=Mock(side_effect=lambda _:observations.append('panel') or {'cursor':{'x':961,'y':540}})
        owner.peer_state=Mock(side_effect=lambda _:observations.append('peer') or peer)
        reads=['[]','{"x":960,"y":540}','{"x":961,"y":540}']
        def hypr(*args):
            self.assertIn(args,(('-j','binds'),('-j','cursorpos')))
            observations.append(args[-1])
            return reads.pop(0)
        with patch.object(g.base,'hypr',side_effect=hypr) as native,\
             patch.object(g.time,'monotonic',return_value=11.),patch.object(g.base,'private_json') as saved:
            with self.assertRaisesRegex(g.p.Blocked,'does_not_match_declared_disable_postimage'):
                owner.input_preflight()
            self.assertEqual(native.call_args_list[-1].args,('-j','cursorpos'))
            self.assertFalse(reads)
            self.assertEqual(observations,['binds','cursorpos','sources','panel','peer','cursorpos'])
        self.assertTrue(cursor.lost);self.assertEqual(cursor.expected,(1200,900))
        self.assertFalse(declaration.consumed);self.assertIsNone(owner.disable_cursor);saved.assert_not_called()
        owner.checked.assert_called_once();owner.panel_observation.assert_called_once();owner.peer_state.assert_called_once()
        with patch.object(g.base,'hypr',side_effect=['[]','{"x":1200,"y":900}']):
            with self.assertRaisesRegex(g.p.Blocked,'foreign_or_uncertain_cursor_movement_blocks_warp'):
                owner.input_preflight()


if __name__=='__main__':unittest.main()
