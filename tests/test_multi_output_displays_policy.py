"""Pure/owned tempfile tests only. Never connect to VM, bus or native display."""
import ast,copy,hashlib,importlib.util,io,json,os,stat,sys,tempfile,unittest
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
        qualify.assert_called_once_with(('/tmp/private-lab/reports/local/proof.json','a'*64))

    def test_default_unbound_refuses_before_source_or_transport(self):
        with patch.object(c,'read',side_effect=AssertionError('no_io')):
            with self.assertRaisesRegex(ValueError,'reviewed_execution_bindings_UNBOUND'): c.qualify()

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

if __name__=='__main__':unittest.main()
