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
        with patch.object(g.base,'inspect',return_value={'client':{'displays':child}}),\
             patch.object(g.time,'monotonic',side_effect=[0,21]),patch.object(g,'exchange_restore') as restore:
            with self.assertRaisesRegex(g.p.Blocked,'display_transaction_retirement_deadline'):
                owner.cleanup()
        restore.assert_not_called()

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
        owner.monitors=Mock(return_value=[self.original,{**self.peer,'scale':1.25}]);owner.input_preflight=Mock()
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

if __name__=='__main__':unittest.main()
