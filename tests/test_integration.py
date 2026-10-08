"""Regression checks for recovery state and the real upstream configuration contracts."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('guest', ROOT / 'scripts/guest.py')
guest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guest)

class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.old_home, self.old_state = guest.HOME_DIR, guest.STATE
        guest.HOME_DIR = self.home
        guest.STATE = self.home / '.local/state/desktop-lab'

    def tearDown(self):
        guest.HOME_DIR, guest.STATE = self.old_home, self.old_state
        self.temp.cleanup()

    def test_backup_keeps_first_original_and_restores_absence(self):
        a = self.home/'existing'
        a.write_text('original')
        guest.backup('existing')
        a.write_text('modified')
        guest.backup('existing')
        guest.backup('absent')
        (self.home/'absent').write_text('new')
        with patch.object(guest, 'run'):
            guest.rollback()
        self.assertEqual(a.read_text(), 'original')
        self.assertFalse((self.home/'absent').exists())

    def test_symlink_is_restored_without_editing_its_target(self):
        target = self.home/'target'
        target.write_text('untouched')
        link = self.home/'link'
        link.symlink_to('target')
        guest.backup('link')
        link.unlink()
        link.write_text('replacement')
        with patch.object(guest, 'run'):
            guest.rollback()
        self.assertTrue(link.is_symlink())
        self.assertEqual(target.read_text(), 'untouched')

    def test_config_directory_and_its_symlinks_survive_rollback(self):
        config = self.home/'config'
        config.mkdir()
        (config/'settings').write_text('user settings')
        (config/'alias').symlink_to('settings')
        guest.backup('config')
        (config/'settings').write_text('lab settings')
        with patch.object(guest, 'run'):
            guest.rollback()
        self.assertEqual((config/'alias').read_text(), 'user settings')

class ManifestTests(unittest.TestCase):
    def test_upstream_sources_are_pinned_and_hashed(self):
        refs = json.loads((ROOT/'manifests/upstream-lock.json').read_text())
        for ref in refs.values():
            self.assertRegex(ref['revision'], r'^[a-f0-9]{40}$')
            self.assertRegex(ref['archive_sha256'], r'^[a-f0-9]{64}$')
        self.assertRegex(refs['lucid']['installer_sha256'], r'^[a-f0-9]{64}$')

    def test_binding_overrides_do_not_depend_on_newer_omarchy_helper(self):
        for profile in ('lucid','noctalia'):
            content = guest.bindings_for(profile)
            self.assertNotIn('o.rebind', content)
            self.assertIn('hl.unbind', content)
            self.assertNotIn('SUPER + 1', content)

    def test_noctalia_config_is_toml(self):
        import tomllib
        data = tomllib.loads((ROOT/'configs/noctalia/config.toml').read_text())
        self.assertTrue(data['dock']['enabled'])
        self.assertEqual(data['bar']['main']['background_opacity'], 0.0)

if __name__ == '__main__':
    unittest.main()
