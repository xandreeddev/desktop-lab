"""Exercise actual native-test recovery functions in owned temporary files."""
import ast
import ctypes
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
import copy
import importlib.util

ROOT = Path(__file__).resolve().parents[1]


class DisplaySupervisionContractTests(unittest.TestCase):
    def setUp(self):
        spec=importlib.util.spec_from_file_location('actual_display_supervision',ROOT/'vm/display_supervision.py')
        self.module=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.module)
        self.before={'files':{'supervisor':{'sha256':'trusted'},'desktop':{'sha256':'desktop','identity':[1,2]},'settings':{'sha256':'saved','identity':[3,4]}},
                     'links':{'unchanged':True},'exports':{'unchanged':True},'media':{'unchanged':True},
                     'service':{'ActiveState':'active','MainPID':'100','NRestarts':'2','KillMode':'mixed'}}
        self.after=copy.deepcopy(self.before);self.after['service'].update(MainPID='300',NRestarts='3')
        self.unit_before={'properties':{'Id':'lucent.service','ActiveState':'active','SubState':'running','MainPID':'100','NRestarts':'2','KillMode':'mixed','FragmentPath':'/owned/lucent.service','DropInPaths':'','InvocationID':'0'*32},
                          'supervisor':{'pid':100,'birth':'10','argv':['python','session.py'],'exe':{'sha256':'python','identity':[5,6]},'cgroup':'0::/scope/lucent.service\n'},
                          'unit_files':{'/owned/lucent.service':{'sha256':'unit','identity':[7,8]}}}
        self.unit_after=copy.deepcopy(self.unit_before)
        self.unit_after['properties'].update(MainPID='300',NRestarts='3',InvocationID='1'*32)
        self.unit_after['supervisor'].update(pid=300,birth='30')
        self.ui_before={'pid':200,'start':'20','supervisor_pid':'100','cgroup':'/scope/lucent.service','exe_identity':[1,2]}
        self.ui_after={**self.ui_before,'pid':400,'start':'40','supervisor_pid':'300'}
        self.proof={'identity':copy.deepcopy(self.unit_after),'post':{'old_ui_retired':True,'current_ui_matches':True,'current_supervisor_matches':True,'native_surfaces_rendered':True,'ui_age_seconds':16,'old_ui_pid':200,'old_ui_birth':'20','ui_pid':400,'ui_birth':'40','supervisor_pid':300,'supervisor_birth':'30','desktop_sha256':'desktop','exe_identity':[1,2]}}

    def validate(self):
        return self.module.expected_after_ui_crash(self.before,self.after,self.ui_before,self.ui_after,self.unit_before,self.unit_after,self.proof,'trusted')

    def test_exact_single_restart_preserves_every_file_and_only_two_service_fields(self):
        before=copy.deepcopy(self.before)
        self.assertEqual(self.validate(),self.after)
        self.assertEqual(self.before,before)
        self.assertEqual(self.before['files'],self.after['files'])

    def test_changed_content_identity_media_or_unknown_fields_are_not_exempt(self):
        initial=copy.deepcopy(self.after)
        for category,change in [('files',{'unexpected':'content'}),('links',{'unexpected':'target'}),('exports',{'unexpected':'mtime'}),('media',{'unexpected':'history'})]:
            with self.subTest(category=category):
                self.after=copy.deepcopy(initial);self.after[category]=change
                with self.assertRaises(AssertionError):self.validate()
        self.after=copy.deepcopy(initial)
        for key,value in [('sha256','changed'),('identity',[3,5])]:
            with self.subTest(key=key):
                self.after=copy.deepcopy(initial);self.after['files']['settings'][key]=value
                with self.assertRaises(AssertionError):self.validate()
        self.after=copy.deepcopy(initial)
        self.before['service']['new_field']='unchanged';self.after['service']['new_field']='changed'
        with self.assertRaises(AssertionError):self.validate()

    def test_wrong_restart_count_or_unbound_current_supervisor_is_rejected(self):
        for key,value in [('NRestarts','4'),('NRestarts','2'),('MainPID','100'),('MainPID','900'),('KillMode','control-group')]:
            with self.subTest(key=key,value=value):
                saved=self.after['service'][key];self.after['service'][key]=value
                with self.assertRaises(AssertionError):self.validate()
                self.after['service'][key]=saved

    def test_changed_unit_argv_executable_cgroup_or_invocation_is_rejected(self):
        original=copy.deepcopy(self.unit_after)
        for key,value in [('argv',['other']),('exe',{'sha256':'other'}),('cgroup','0::/other'),('birth','9')]:
            with self.subTest(key=key):
                self.unit_after=copy.deepcopy(original);self.unit_after['supervisor'][key]=value;self.proof['identity']=copy.deepcopy(self.unit_after)
                with self.assertRaises(AssertionError):self.validate()
        self.unit_after=copy.deepcopy(original);self.unit_after['unit_files']['/owned/lucent.service']['identity']=[9,10];self.proof['identity']=copy.deepcopy(self.unit_after)
        with self.assertRaises(AssertionError):self.validate()
        self.unit_after=copy.deepcopy(original);self.unit_after['properties']['InvocationID']='0'*32;self.proof['identity']=copy.deepcopy(self.unit_after)
        with self.assertRaises(AssertionError):self.validate()

    def test_reused_ui_birth_or_executable_identity_cannot_establish_recovery(self):
        original=self.ui_after.copy()
        for key,value in [('pid',200),('start','29'),('exe_identity',[11,12]),('cgroup','/other')]:
            with self.subTest(key=key):
                self.ui_after={**original,key:value}
                with self.assertRaises(AssertionError):self.validate()

    def test_unrendered_or_not_retired_or_too_early_recovery_is_rejected(self):
        original=copy.deepcopy(self.proof)
        cases=[(key,False)for key in ('old_ui_retired','current_ui_matches','current_supervisor_matches','native_surfaces_rendered')]
        cases += [('ui_age_seconds',value)for value in (15,float('nan'),float('inf'))]
        cases += [('old_ui_pid',900),('old_ui_birth','900'),('ui_pid',900),('ui_birth','900'),('supervisor_pid',900),('supervisor_birth','900'),('desktop_sha256','other'),('exe_identity',[9,10])]
        for key,value in cases:
            with self.subTest(key=key,value=value):
                self.proof=copy.deepcopy(original);self.proof['post'][key]=value
                with self.assertRaises(AssertionError):self.validate()

    def test_actual_bounded_readonly_probe_script_compiles(self):
        compile(self.module.SCRIPT,'actual-supervisor-evidence','exec')


class DisplayRecoveryTests(unittest.TestCase):
    def setUp(self):
        source = ast.parse((ROOT / 'vm/test-display-settings.py').read_text())
        recovery = next(node.value.value for node in source.body if isinstance(node, ast.Assign)
                        and any(isinstance(target, ast.Name) and target.id == 'RECOVERY' for target in node.targets))
        functions = [node for node in ast.parse(recovery).body if isinstance(node, ast.FunctionDef)]
        self.temp = tempfile.TemporaryDirectory(prefix='lucent-display-recovery-')
        self.addCleanup(self.temp.cleanup)
        directory = Path(self.temp.name)
        self.config = directory / 'monitors.lua'
        self.backup = directory / 'original.lua'
        self.settings = directory / 'desktop.json'
        self.config.write_bytes(b'-- original user configuration\n')
        self.settings.write_bytes(b'{}\n')
        os.link(self.config, self.backup)
        self.monitor = dict(name='Virtual-1', width=1920, height=1080, refreshRate=60.,
                            x=0, y=0, scale=1, transform=0)
        self.observed = self.monitor.copy()
        self.commands = []
        self.ns = dict(ctypes=ctypes, hashlib=hashlib, json=json, os=os,
                       stat=__import__('stat'), time=__import__('time'), directory=directory,
                       config=self.config, backup=self.backup, state=self.settings)
        exec(compile(ast.Module(body=functions, type_ignores=[]), 'actual-native-recovery', 'exec'), self.ns)
        def session(*args):
            self.commands.append(args)
            self.observed = self.monitor.copy()
            return 'ok'
        self.ns['session'] = session
        self.ns['monitors'] = lambda: [self.observed]
        data, identity = self.ns['read'](self.config, 256 * 1024)
        settings, settings_id = self.ns['read'](self.settings, 4096)
        self.value = dict(monitor=self.monitor, allowed_native=[self.ns['signature'](self.monitor),
                          self.ns['signature']({**self.monitor, 'x': 16})],
                          config_sha256=hashlib.sha256(data).hexdigest(), config_identity=identity,
                          settings_sha256=hashlib.sha256(settings).hexdigest(), settings_identity=settings_id)
        self.original = data

    def kept(self):
        candidate = self.config.with_suffix('.kept')
        candidate.write_bytes(self.ns['expected_keep'](self.value, self.original))
        candidate.replace(self.config)
        data, identity = self.ns['read'](self.config, 256 * 1024)
        self.value['kept'] = dict(sha256=hashlib.sha256(data).hexdigest(), identity=identity)
        self.observed = {**self.monitor, 'x': 16}

    def test_actual_recovery_restores_original_inode_and_is_idempotent(self):
        self.kept()
        result = self.ns['restore'](self.value)
        self.assertTrue(all(result.values()))
        self.assertEqual(self.config.stat().st_ino, self.backup.stat().st_ino)
        self.assertEqual(self.config.read_bytes(), self.original)
        self.assertTrue(all(self.ns['restore'](self.value).values()))

    def test_same_bytes_in_a_foreign_inode_are_preserved_without_native_mutation(self):
        self.kept()
        replacement = self.config.with_suffix('.foreign')
        replacement.write_bytes(self.config.read_bytes())
        replacement.replace(self.config)
        identity = self.config.stat().st_ino
        with self.assertRaises(AssertionError):
            self.ns['restore'](self.value)
        self.assertEqual(self.config.stat().st_ino, identity)
        self.assertEqual(self.commands, [])

    def test_raced_foreign_symlink_returns_to_original_path(self):
        self.kept()
        actual_library = ctypes.CDLL(None, use_errno=True)
        actual_rename = actual_library.renameat2
        actual_rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
        actual_rename.restype = ctypes.c_int
        foreign_target = self.config.with_name('foreign-content')
        foreign_target.write_bytes(b'user content must stay untouched')
        captured = []
        config = self.config
        class RacedRename:
            calls = 0
            def __call__(self, *args):
                if self.calls == 0:
                    foreign = config.with_name('foreign-link')
                    foreign.symlink_to(foreign_target)
                    foreign.replace(config)
                    captured.append(config.lstat().st_ino)
                self.calls += 1
                return actual_rename(*args)
        rename = RacedRename()
        class Library:
            renameat2 = rename
        class Native:
            @staticmethod
            def CDLL(*args, **kwargs):
                return Library()
            get_errno = staticmethod(ctypes.get_errno)
            c_int = ctypes.c_int
            c_char_p = ctypes.c_char_p
            c_uint = ctypes.c_uint
        self.ns['ctypes'] = Native
        with self.assertRaises(AssertionError):
            self.ns['restore'](self.value)
        self.assertEqual(rename.calls, 2)
        self.assertTrue(config.is_symlink())
        self.assertEqual(config.lstat().st_ino, captured[0])
        self.assertEqual(config.readlink(), foreign_target)
        self.assertEqual(foreign_target.read_bytes(), b'user content must stay untouched')
        self.assertEqual(self.commands, [])

    def test_unrelated_cross_product_layout_is_preserved(self):
        self.kept()
        self.observed = {**self.monitor, 'x': 16, 'scale': 2}
        identity = self.config.stat().st_ino
        with self.assertRaises(AssertionError):
            self.ns['restore'](self.value)
        self.assertEqual(self.observed['scale'], 2)
        self.assertEqual(self.config.stat().st_ino, identity)
        self.assertEqual(self.commands, [])


if __name__ == '__main__':
    unittest.main()
