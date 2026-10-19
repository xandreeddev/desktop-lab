"""Exercise actual native-test recovery functions in owned temporary files."""
import ast
import ctypes
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


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
