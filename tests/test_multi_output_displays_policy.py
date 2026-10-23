"""Pure/owned tempfile tests only. Never connect to VM, bus or native display."""
import ast,contextlib,copy,hashlib,importlib.util,io,json,os,stat,struct,subprocess,sys,tempfile,types,unittest
from pathlib import Path
from unittest.mock import Mock,patch
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'vm'
sys.path.insert(0,str(ROOT/'vm'))
def module(name):
    spec=importlib.util.spec_from_file_location('display_review_'+name,HERE/({'controller':'test-multi-output-displays.py','guest':'multi_output_display_guard.py'}[name]))
    value=importlib.util.module_from_spec(spec);spec.loader.exec_module(value);return value
c,g=module('controller'),module('guest')

class Boundaries(unittest.TestCase):
    def test_shared_native_policy_keeps_the_reviewed_canonical_identity(self):
        raw=(HERE/'multi_output_policy.py').read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(),
                         '25c0e45f87c25363220cffdfd901774856c8792f7e4298946fc9011dbb2ebb58')
        self.assertEqual(c.PINS['multi_output_policy'][1],hashlib.sha256(raw).hexdigest())
        names={'display_peer_signature','display_layout','display_logical_size',
               'display_mode_index','require_owned_display_control','DisplayDisableCursor'}
        self.assertTrue(names.isdisjoint(vars(g.p)))
        self.assertEqual(set(g.d.__all__),names)
        self.assertIs(g.d.host,g.p.host);self.assertIs(g.d.require,g.p.require)
        self.assertIs(g.d.retired,g.p.retired)

    def test_staged_header_binds_both_policies_and_all_eight_guest_sources(self):
        raw={name:(ROOT/pin[0]).read_bytes() for name,pin in c.PINS.items()}
        guest=(HERE/'multi_output_display_guard.py').read_bytes();header={'output':'unit-only'}
        files=c.stage_files(raw,guest,header)
        expected={'guest.py','multi_output_guard.py','multi_output_policy.py','multi_output_display_policy.py',
                  'multi_output_display_timing.py',
                  'preservation_fingerprint.py','display_supervision.py','owned_application_units.py'}
        self.assertEqual(set(files),expected|{'header.json'})
        self.assertEqual(set(header['source_sha256']),expected)
        self.assertEqual(json.loads(files['header.json'])['source_sha256'],header['source_sha256'])
        for name in expected:self.assertEqual(header['source_sha256'][name],hashlib.sha256(files[name]).hexdigest())
        stage=ast.parse(c.STAGE)
        allowed=next(node.comparators[0] for node in ast.walk(stage) if isinstance(node,ast.Compare)
                     and isinstance(node.left,ast.Name) and node.left.id=='name'
                     and len(node.ops)==1 and isinstance(node.ops[0],ast.In))
        self.assertEqual(set(ast.literal_eval(allowed)),set(files))

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
        self.assertEqual(g.d.display_peer_signature(rotated,self.identity),rotated)
        native_disabled={**self.peer,'width':0,'height':0,'refreshRate':0,'disabled':True}
        with self.assertRaises(g.p.Blocked):g.p.monitor_signature(native_disabled)
        self.assertEqual(g.d.display_layout([self.original,native_disabled],[self.original],self.identity),self.disabled)
        for mutation in ({'id':3},{'name':'Virtual-1'},{'description':'replacement'}):
            with self.assertRaisesRegex(g.p.Blocked,'owned_display_native_identity_changed'):
                g.d.display_peer_signature({**native_disabled,**mutation},self.identity)
        for mutation in ({'transform':1},{'disabled':True},{'x':16}):
            with self.assertRaises(g.p.Blocked):
                g.d.display_layout([{**self.original,**mutation},native_disabled],[self.original],self.identity)
        with self.assertRaisesRegex(g.p.Blocked,'inventory_required'):
            g.d.display_layout([self.original,native_disabled,dict(self.peer,name='foreign')],[self.original],self.identity)

    def test_fractional_logical_viewport_is_separate_from_physical_buffer_and_rotation(self):
        self.assertEqual(g.d.display_logical_size({**self.peer,'scale':1.25}),(1024,768))
        self.assertEqual(g.d.display_logical_size({**self.peer,'scale':1.25,'transform':1}),(768,1024))
        for mutation in ({'disabled':True},{'scale':float('nan')},{'transform':7}):
            with self.assertRaises(g.p.Blocked):g.d.display_logical_size({**self.peer,**mutation})

    def test_real_mode_index_preserves_native_order_deduplication_and_disabled_fallback(self):
        native={**self.peer,'availableModes':['invalid','1280x960@60.000Hz','1280x960@60',
                                           '1920x1080@60Hz','1280x720@nanHz']}
        self.assertEqual(g.d.display_mode_index(native,1280,960,60000),0)
        self.assertEqual(g.d.display_mode_index({**native,'width':1024,'height':768},1280,960,60000),1)
        inactive={**native,'disabled':True,'width':0,'height':0,'refreshRate':0}
        self.assertEqual(g.d.display_mode_index(inactive,1920,1080,60000),1)
        with self.assertRaisesRegex(g.p.Blocked,'not_advertised'):
            g.d.display_mode_index(inactive,800,600,60000)

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
                g.d.require_owned_display_control(identifier,'Virtual-1',name)
            g.d.require_owned_display_control(identifier,name,name)
        for identifier in ('display-reset','display-select-'+name):
            g.d.require_owned_display_control(identifier,'Virtual-1',name)
        with self.assertRaises(g.p.Blocked):
            g.d.require_owned_display_control('display-select-Virtual-1',name,name)

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
        declaration=g.d.DisplayDisableCursor(cursor,[original],identity,peer_host,{'peer-bar'},
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


class DisplayRequestWire(unittest.TestCase):
    def client(self,chunks=(b'{"ok":true,"value":{}}\n',)):
        client=Mock();client.__enter__=Mock(return_value=client);client.__exit__=Mock(return_value=False)
        client.recv.side_effect=list(chunks)
        return client

    def main_request(self,directory,operation,client,extra=()):
        with patch.object(sys,'argv',['guest.py','request',str(directory),operation,*extra]),\
             patch.object(g.socket,'socket',return_value=client),contextlib.redirect_stdout(io.StringIO()) as output:
            g.main()
        return output.getvalue()

    def test_existing_cli_frames_remain_byte_equivalent_to_canonical_serializer(self):
        operations=('create','status','capture-keep','expect-disable','expect-enable','arm-keep','warp','finish')
        with tempfile.TemporaryDirectory(prefix='native-two-output-displays-') as temporary:
            directory=Path(temporary)
            for operation in operations:
                with self.subTest(operation=operation):
                    original=self.client()
                    with patch.object(g.base.socket,'socket',return_value=original),contextlib.redirect_stdout(io.StringIO()):
                        g.base.request(directory,operation,None,'Virtual-1','display-reset',5,19)
                    current=self.client()
                    self.main_request(directory,operation,current,('--monitor','Virtual-1','--identifier','display-reset',
                                      '--generation','5','--global-id','19'))
                    self.assertEqual(current.sendall.call_args,original.sendall.call_args)
                    current.connect.assert_called_once_with(str(directory/'control.sock'))
                    self.assertEqual(current.settimeout.call_args_list[0].args,(35,))

    def bar_owner(self):
        name='LUCENT-TEST-'+'a'*24
        owner=g.Owner.__new__(g.Owner);owner.lease=types.SimpleNamespace(name=name)
        owner.pid=123;owner.peer_ready=True;owner.peer_disabled=False;owner.panel_target='Virtual-1'
        current={'x':10,'y':20};owner.cursor=g.p.CursorOwnership((10,20))
        owner.status=Mock(return_value={'state':{'visible':False},'cursor':dict(current)})
        owner.monitors=Mock(return_value=[{'name':name,'width':1280,'height':960,'x':1968,'y':0,'scale':1.25,'transform':0}])
        source={'id':'bar-peer','output':name,'role':'bar','instance':'peer-instance','width':1024,'height':48,
                'density':1.25,'pointer_input':'all','hits':[{'id':'bar-audio','x':20,'y':4,'width':24,'height':24,
                                                          'clip':{'x':0,'y':0,'width':1024,'height':48}}]}
        state={'client':{'named_monitor_hosts':True,'monitor_hosts':[{'id':name,'instance':'peer-instance','generation':5}]},
               'outputs':[{'name':name,'global':19}],'surfaces':[source]}
        layers={name:{'levels':{'2':[{'namespace':'lucent-bar-peer','pid':123,'address':'0xabc',
                                    'x':1968,'y':0,'w':1024,'h':48}]}}}
        def hypr(*args):
            if args==('-j','layers'):return json.dumps(layers)
            if args==('dispatch','hl.dsp.cursor.move({x=2000,y=16})'):
                current.update(x=2000,y=16);return 'ok'
            if args==('-j','cursorpos'):return json.dumps(current)
            self.fail('unexpected_native_operation')
        return owner,state,hypr

    def test_actual_bar_request_cli_passes_complete_wire_capture_into_owner(self):
        owner,state,hypr=self.bar_owner();client=self.client();seen=[];reply=[]
        def deliver(raw):
            frame=json.loads(raw);seen.append(frame)
            reply.append((json.dumps({'ok':True,'value':owner.operation(frame)})+'\n').encode())
        client.sendall.side_effect=deliver;client.recv.side_effect=lambda _:reply.pop(0)
        with tempfile.TemporaryDirectory(prefix='native-two-output-displays-') as temporary:
            with patch.object(g.base,'inspect',return_value=state),patch.object(g.base,'hypr',side_effect=hypr):
                output=self.main_request(Path(temporary),'bar-warp',client,('--monitor',owner.lease.name,
                                         '--generation','5','--global-id','19'))
        self.assertEqual(seen,[{'operation':'bar-warp','monitor':owner.lease.name,'generation':5,'global':19}])
        self.assertEqual(json.loads(output)['value'],{'point':[2000,16],'surface':'bar-peer','generation':5,'global':19})
        self.assertEqual(owner.panel_target,owner.lease.name)
        self.assertEqual(owner.cursor.expected,(2000,16));self.assertFalse(owner.cursor.lost)

    def test_carried_bar_lifetime_values_do_not_bypass_owner_admission(self):
        for key,value in (('generation',4),('global',18)):
            with self.subTest(key=key):
                owner,state,_=self.bar_owner()
                frame=g.display_request_packet('bar-warp',owner.lease.name,None,5,19);frame[key]=value
                with patch.object(g.base,'inspect',return_value=state),patch.object(g.base,'hypr') as native:
                    with self.assertRaisesRegex(g.p.Blocked,'exact_bar_host_lifetime_required'):
                        owner.operation(frame)
                native.assert_not_called();self.assertEqual(owner.cursor.expected,(10,20))

    def test_socket_frames_keep_newline_size_and_absolute_response_deadline_guards(self):
        prefix,suffix=b'{"value":"',b'"}\n'
        boundary=prefix+b'x'*(65536-len(prefix)-len(suffix))+suffix
        client=self.client((boundary[:100],boundary[100:]))
        with patch.object(g.socket,'socket',return_value=client),contextlib.redirect_stdout(io.StringIO()) as output:
            g.display_request(Path('/unused-stage'),'status')
        self.assertEqual(len(boundary),65536);self.assertIn('value',json.loads(output.getvalue()))
        for chunks,label in (((b'x'*65537+b'\n',),'oversized_guard_response'),
                             ((b'partial',b''),'truncated_guard_response')):
            client=self.client(chunks)
            with self.subTest(label=label),patch.object(g.socket,'socket',return_value=client),\
                 contextlib.redirect_stdout(io.StringIO()) as output:
                with self.assertRaisesRegex(g.p.Blocked,label):g.display_request(Path('/unused-stage'),'status')
            self.assertEqual(output.getvalue(),'');client.__exit__.assert_called_once()
        client=self.client()
        with patch.object(g.socket,'socket',return_value=client),patch.object(g.time,'monotonic',side_effect=[0,36]):
            with self.assertRaisesRegex(g.p.Blocked,'absolute_guard_response_deadline_expired'):
                g.display_request(Path('/unused-stage'),'status')
        client.recv.assert_not_called();client.__exit__.assert_called_once()

    def test_unknown_operations_and_socket_timeout_never_claim_a_response(self):
        with patch.object(g.socket,'socket') as native:
            with self.assertRaisesRegex(g.p.Blocked,'known_display_request_operation_required'):
                g.display_request(Path('/unused-stage'),'unknown')
        native.assert_not_called()
        client=self.client();client.recv.side_effect=TimeoutError('injected_socket_timeout')
        with patch.object(g.socket,'socket',return_value=client),contextlib.redirect_stdout(io.StringIO()) as output:
            with self.assertRaises(TimeoutError):g.display_request(Path('/unused-stage'),'status')
        self.assertEqual(output.getvalue(),'');client.__exit__.assert_called_once()


class DisplayObservationTiming(unittest.TestCase):
    def no_alarm(self):
        return patch.object(g,'phase_deadline',side_effect=lambda _:contextlib.nullcontext())

    def test_complete_timing_contract_covers_startup_cleanup_and_unit_without_relaxing_input(self):
        t=g.timing
        self.assertEqual((t.PHASE_SECONDS['before_preservation'],t.PHASE_SECONDS['after_preservation']),(45,45))
        self.assertEqual((t.PHASE_SECONDS['before_supervision'],t.PHASE_SECONDS['after_supervision']),(15,15))
        self.assertEqual(t.STARTUP_PHASE_SECONDS,175)
        self.assertEqual(t.CLEANUP_PHASE_SECONDS,190)
        self.assertLess(t.STARTUP_PHASE_SECONDS+t.STATE_OBSERVATION_SECONDS,t.STARTUP_READY_SECONDS)
        self.assertLess(t.CLEANUP_PHASE_SECONDS+t.STATE_OBSERVATION_SECONDS,t.CLEANUP_RECEIPT_SECONDS)
        self.assertLess(t.STARTUP_PHASE_SECONDS+t.OWNER_LEASE_SECONDS+t.CLEANUP_PHASE_SECONDS+t.UNIT_STOP_SECONDS,
                        t.UNIT_RUNTIME_SECONDS)
        self.assertEqual((t.INTERACTION_SECONDS,t.OWNER_LEASE_SECONDS),(420,480))
        expected={'multi_output_guard':'05cc165f9b823becd28e8a631920e120ceb75afd73e026a3b91a64404729d765',
                  'multi_output_policy':'25c0e45f87c25363220cffdfd901774856c8792f7e4298946fc9011dbb2ebb58',
                  'preservation_fingerprint':'ecd796f7bce24e4ea153633089595c7ba4d5604959ca5f0131367832b173e2a3',
                  'display_supervision':'1399eebb03af1ae2408124c0b46706f109ed99cc49f8cd00165021d935cab0be',
                  'owned_application_units':'0082d9c4c87c36a7cc65f361304a2fbda174af83f30d6713720d709a9193d5b7'}
        for name,digest in expected.items():
            self.assertEqual(hashlib.sha256((HERE/(name+'.py')).read_bytes()).hexdigest(),digest)
        tree=ast.parse((HERE/'multi_output_guard.py').read_text())
        command=next(node for node in tree.body if isinstance(node,ast.FunctionDef) and node.name=='command')
        call=next(node for node in ast.walk(command) if isinstance(node,ast.Call) and isinstance(node.func,ast.Attribute)
                  and node.func.attr=='run')
        self.assertEqual(next(ast.literal_eval(kw.value) for kw in call.keywords if kw.arg=='timeout'),6)

    def test_fixed_complete_snapshot_commands_have_separate_before_and_after_budgets(self):
        for phase in ('before_preservation','after_preservation','before_supervision','after_supervision'):
            with tempfile.TemporaryDirectory() as temporary:
                directory=Path(temporary);diagnostics=[]
                preserved=phase.endswith('_preservation')
                stdout='{"inventory":"complete"}' if preserved else '{"identity":{"unit":"original"}}'
                with self.no_alarm(),patch.object(g.subprocess,'run',return_value=types.SimpleNamespace(returncode=0,stdout=stdout)) as run,\
                     patch.object(g.base,'command',side_effect=AssertionError('canonical input runner not used')):
                    result=g.snapshot(directory,diagnostics,phase)
                expected=('python3','-c',g.preservation.SCRIPT) if preserved else ('python3','-c',g.supervision.SCRIPT,'identity')
                self.assertEqual(run.call_args.args[0],expected)
                self.assertEqual(run.call_args.kwargs['timeout'],45 if preserved else 15)
                self.assertEqual(result,{'inventory':'complete'} if preserved else {'unit':'original'})
                self.assertEqual(diagnostics[0]['status'],'passed')
                self.assertEqual(stat.S_IMODE((directory/(phase+'-result.json')).stat().st_mode),0o600)

    def test_readonly_callback_cannot_run_an_arbitrary_command_with_the_long_budget(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.no_alarm(),patch.object(g.preservation,'fingerprint',side_effect=lambda callback:callback('sh','-c','private command')),\
                 patch.object(g.subprocess,'run') as run:
                with self.assertRaisesRegex(g.p.Blocked,'fixed_read_only_snapshot_command_required'):
                    g.snapshot(Path(temporary),[],'before_preservation')
            run.assert_not_called()

    def test_snapshot_timeout_records_only_fixed_phase_class_budget_and_elapsed(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary);diagnostics=[]
            error=subprocess.TimeoutExpired(['PRIVATE_ARGV_SENTINEL'],45,output='PRIVATE_STDOUT_SENTINEL',stderr='PRIVATE_STDERR_SENTINEL')
            with self.no_alarm(),patch.object(g.subprocess,'run',side_effect=error):
                with self.assertRaises(subprocess.TimeoutExpired):
                    g.snapshot(directory,diagnostics,'before_preservation')
            record=json.loads((directory/'before_preservation-result.json').read_text())
            self.assertEqual(set(record),{'phase','status','budget_seconds','elapsed_seconds','error_class'})
            self.assertEqual((record['phase'],record['budget_seconds'],record['error_class'],record['status']),
                             ('before_preservation',45,'TimeoutExpired','failed'))
            self.assertGreaterEqual(record['elapsed_seconds'],0)
            self.assertEqual(diagnostics,[record])
            self.assertNotIn('PRIVATE_', ''.join(path.read_text() for path in directory.iterdir()))

    def test_expired_phase_cannot_be_reported_successful_if_action_catches_its_timer(self):
        with tempfile.TemporaryDirectory() as temporary:
            diagnostics=[]
            with self.no_alarm(),patch.object(g.time,'monotonic',side_effect=[0,91,91.5]):
                with self.assertRaises(g.PhaseDeadline):
                    g.run_phase(Path(temporary),diagnostics,'owner_initialization',lambda:'late result')
            self.assertEqual(diagnostics[0]['error_class'],'PhaseDeadline')
            self.assertEqual(diagnostics[0]['status'],'failed')

    def test_phase_timer_restores_handler_and_never_takes_over_an_existing_timer(self):
        previous=object()
        with patch.object(g.signal,'getsignal',return_value=previous),\
             patch.object(g.signal,'getitimer',return_value=(0.0,0.0)),\
             patch.object(g.signal,'signal') as handler,patch.object(g.signal,'setitimer') as timer:
            with self.assertRaises(g.PhaseDeadline):
                with g.phase_deadline(90):raise g.PhaseDeadline('injected')
        self.assertEqual(timer.call_args_list[0].args,(g.signal.ITIMER_REAL,90,.1))
        self.assertEqual(timer.call_args_list[-1].args,(g.signal.ITIMER_REAL,0))
        self.assertEqual(handler.call_args_list[-1].args,(g.signal.SIGALRM,previous))
        with patch.object(g.signal,'getitimer',return_value=(1.0,0.0)),patch.object(g.signal,'setitimer') as timer:
            with self.assertRaisesRegex(g.p.Blocked,'no_preexisting_display_phase_timer'):
                with g.phase_deadline(90):self.fail('must not enter')
            timer.assert_not_called()

    def test_arming_failure_still_cancels_and_restores_previous_handler(self):
        previous=object()
        with patch.object(g.signal,'getsignal',return_value=previous),\
             patch.object(g.signal,'getitimer',return_value=(0.0,0.0)),\
             patch.object(g.signal,'signal') as handler,\
             patch.object(g.signal,'setitimer',side_effect=[OSError('injected arming failure'),None]) as timer:
            with self.assertRaisesRegex(OSError,'injected arming failure'):
                with g.phase_deadline(90):self.fail('must not enter after failed arm')
        self.assertEqual(timer.call_args_list[-1].args,(g.signal.ITIMER_REAL,0))
        self.assertEqual(handler.call_args_list[-1].args,(g.signal.SIGALRM,previous))

    def test_alarm_during_arm_cannot_skip_timer_and_handler_cleanup(self):
        previous=object();current=[previous]
        def install(_signum,handler):current[0]=handler
        def arm(_timer,seconds,*_):
            if seconds:current[0](g.signal.SIGALRM,None)
        with patch.object(g.signal,'getsignal',return_value=previous),\
             patch.object(g.signal,'getitimer',return_value=(0.0,0.0)),\
             patch.object(g.signal,'signal',side_effect=install) as handler,\
             patch.object(g.signal,'setitimer',side_effect=arm) as timer:
            with self.assertRaises(g.PhaseDeadline):
                with g.phase_deadline(90):self.fail('must not enter after alarm at arm')
        self.assertIs(current[0],previous)
        self.assertEqual(timer.call_args_list[-1].args,(g.signal.ITIMER_REAL,0))
        self.assertEqual(handler.call_args_list[-1].args,(g.signal.SIGALRM,previous))

    def test_alarm_immediately_after_handler_install_is_inside_cleanup_scope(self):
        previous=object();current=[previous]
        def install(_signum,handler):
            current[0]=handler
            if handler is not previous:handler(g.signal.SIGALRM,None)
        with patch.object(g.signal,'getsignal',return_value=previous),\
             patch.object(g.signal,'getitimer',return_value=(0.0,0.0)),\
             patch.object(g.signal,'signal',side_effect=install),patch.object(g.signal,'setitimer') as timer:
            with self.assertRaises(g.PhaseDeadline):
                with g.phase_deadline(90):self.fail('must not enter after immediate alarm')
        self.assertIs(current[0],previous)
        timer.assert_called_once_with(g.signal.ITIMER_REAL,0)

    def test_cancel_failure_still_restores_previous_handler(self):
        previous=object()
        with patch.object(g.signal,'getsignal',return_value=previous),\
             patch.object(g.signal,'getitimer',return_value=(0.0,0.0)),\
             patch.object(g.signal,'signal') as handler,\
             patch.object(g.signal,'setitimer',side_effect=[None,OSError('injected cancel failure')]):
            with self.assertRaisesRegex(OSError,'injected cancel failure'):
                with g.phase_deadline(90):pass
        self.assertEqual(handler.call_args_list[-1].args,(g.signal.SIGALRM,previous))

    def test_terminal_failure_beats_stale_ready_and_stops_without_any_input(self):
        finished={'success':False,'restoration':None,'phases':[{'phase':'before_preservation','error_class':'TimeoutExpired'}]}
        observe=Mock(return_value={'ready':True,'finished':finished});pause=Mock()
        with self.assertRaises(c.OwnerStartupFailed) as caught:
            c.wait_owner_state(observe,g.timing,True,clock=lambda:0,pause=pause)
        self.assertEqual(caught.exception.result,finished)
        observe.assert_called_once_with(g.timing.STATE_OBSERVATION_SECONDS);pause.assert_not_called()

    def test_cleanup_observation_has_one_absolute_deadline_and_no_ready_shortcut(self):
        class Clock:
            value=0
            def __call__(self):return self.value
            def sleep(self,seconds):self.value+=seconds
        clock=Clock();observe=Mock(return_value={'ready':True,'finished':None})
        with self.assertRaisesRegex(ValueError,'independent_cleanup_receipt_deadline'):
            c.wait_owner_state(observe,g.timing,False,clock=clock,pause=clock.sleep)
        self.assertEqual(clock.value,g.timing.CLEANUP_RECEIPT_SECONDS)
        self.assertTrue(all(0<call.args[0]<=g.timing.STATE_OBSERVATION_SECONDS for call in observe.call_args_list))
        done={'success':False,'restoration':None}
        self.assertIs(c.wait_owner_state(lambda _: {'ready':False,'finished':done},g.timing,False,
                                        clock=lambda:0,pause=self.fail),done)

    def test_late_ready_or_cleanup_reply_cannot_be_accepted_after_its_deadline(self):
        for ready in (True,False):
            now=[0]
            budget=g.timing.STARTUP_READY_SECONDS if ready else g.timing.CLEANUP_RECEIPT_SECONDS
            def late(_):
                now[0]=budget+1
                return {'ready':ready,'finished':None if ready else {'success':True}}
            with self.assertRaisesRegex(ValueError,'independent_owner_ready_deadline' if ready else 'independent_cleanup_receipt_deadline'):
                c.wait_owner_state(late,g.timing,ready,clock=lambda:now[0],pause=self.fail)

    def test_single_readonly_observation_requires_publish_phase_and_reports_terminal(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)
            def observe():
                with patch.object(sys,'argv',['owner-state',str(directory)]),patch('sys.stdout',new=io.StringIO()) as out:
                    exec(compile(c.OWNER_STATE,'owner-state','exec'),{})
                return json.loads(out.getvalue())
            (directory/'ready.json').write_text('{"ready":true,"desktop_pid":123}')
            self.assertEqual(observe(),{'ready':False,'finished':None})
            (directory/'publish_ready-result.json').write_text('{"phase":"publish_ready","status":"passed"}')
            self.assertTrue(observe()['ready'])
            (directory/'finished.json').write_text('{"success":false,"restoration":null}')
            value=observe();self.assertEqual(value['finished'],{'success':False,'restoration':None})
            with self.assertRaises(c.OwnerStartupFailed):
                c.wait_owner_state(lambda _:value,g.timing,True,clock=lambda:0,pause=self.fail)

    def test_startup_snapshot_failure_never_constructs_owner_or_publishes_ready(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)
            with self.no_alarm(),patch.object(g.socket,'socket',return_value=Mock()),\
                 patch.object(g.subprocess,'run',side_effect=subprocess.TimeoutExpired(['PRIVATE_ARGV_SENTINEL'],45)),\
                 patch.object(g,'Owner',side_effect=AssertionError('owner must not exist')) as owner:
                g.serve(directory,{'unit':'unit-only'})
            owner.assert_not_called()
            result=json.loads((directory/'finished.json').read_text())
            self.assertFalse(result['success']);self.assertIsNone(result['restoration'])
            self.assertEqual(result['failure'],'TimeoutExpired')
            self.assertEqual(result['phases'][0]['phase'],'before_preservation')
            self.assertFalse((directory/'ready.json').exists());self.assertFalse((directory/'before-facts.json').exists())
            self.assertNotIn('PRIVATE_',json.dumps(result))

    def serve_fixture(self,directory,cleanup_error=None):
        owner=types.SimpleNamespace(files={},baseline=[],workspace=1,pointer={},original_global=1,
            pid=123,original=(b'',[]),cleanup_cursor={'restored':True},
            cleanup=Mock(return_value={'owned':True}),security_sources=Mock())
        if cleanup_error is not None:owner.cleanup.side_effect=cleanup_error
        client=Mock();client.__enter__=Mock(return_value=client);client.__exit__=Mock(return_value=False)
        client.getsockopt.return_value=struct.pack('3i',123,os.getuid(),os.getgid())
        client.recv.return_value=b'{"operation":"finish"}\n'
        server=Mock();server.accept.return_value=(client,None)
        owner.operation=Mock(return_value={'finishing':True})
        def command(argv,**kwargs):
            payload='{"identity":{"unit":"original"}}' if argv[-1]=='identity' else '{"inventory":"complete"}'
            return types.SimpleNamespace(returncode=0,stdout=payload)
        with self.no_alarm(),patch.object(g.socket,'socket',return_value=server),\
             patch.object(g.os,'chmod'),patch.object(g.subprocess,'run',side_effect=command) as run,\
             patch.object(g.units,'capture_unit_origin',return_value={'process':{'pid':os.getpid()}}),\
             patch.object(g,'Owner',return_value=owner):
            g.serve(directory,{'unit':'unit-only'})
        return owner,run,json.loads((directory/'finished.json').read_text())

    def test_before_after_snapshots_and_security_comparison_keep_exact_contract(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary);owner,run,result=self.serve_fixture(directory)
            self.assertTrue(result['success']);self.assertTrue(result['resource_cleanup_completed'])
            self.assertEqual([call.kwargs['timeout'] for call in run.call_args_list],[45,15,45,15])
            self.assertEqual([row['phase'] for row in result['phases']],list(g.timing.STARTUP_PHASES+g.timing.CLEANUP_PHASES))
            self.assertTrue(all(row['status']=='passed' for row in result['phases']))
            self.assertEqual(json.loads((directory/'before-facts.json').read_text()),json.loads((directory/'after-facts.json').read_text()))
            owner.security_sources.assert_called_once_with(True)

    def test_cleanup_timeout_is_retained_and_cannot_mark_resource_restoration_complete(self):
        with tempfile.TemporaryDirectory() as temporary:
            _,_,result=self.serve_fixture(Path(temporary),subprocess.TimeoutExpired(['PRIVATE_ARGV_SENTINEL'],120))
            self.assertFalse(result['success']);self.assertIsNone(result['restoration'])
            self.assertEqual(result['cleanup_blocked'],'TimeoutExpired')
            self.assertNotIn('resource_cleanup_completed',result)
            phase=result['phases'][-1]
            self.assertEqual((phase['phase'],phase['budget_seconds'],phase['status'],phase['error_class']),
                             ('resource_cleanup',120,'failed','TimeoutExpired'))
            self.assertNotIn('PRIVATE_',json.dumps(result))


if __name__=='__main__':unittest.main()
