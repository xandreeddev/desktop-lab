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

class LucentIntegrationTests(unittest.TestCase):
    def test_managed_blocks_are_idempotent_and_preserve_later_user_edits(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('lucent_setup', ROOT / 'scripts/lucent-setup.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'bindings.lua'
            config.write_text('-- original\n')
            module.edit(config, 'o.bind("SUPER + SPACE", "Lucent", "lucent-cli launcher toggle")\n')
            once = config.read_text()
            module.edit(config, 'o.bind("SUPER + SPACE", "Lucent", "lucent-cli launcher toggle")\n')
            self.assertEqual(config.read_text(), once)
            config.write_text(config.read_text() + '-- later user edit\n')
            module.edit(config)
            self.assertIn('-- original', config.read_text())
            self.assertIn('-- later user edit', config.read_text())
            self.assertNotIn('lucent-cli', config.read_text())

    def test_malformed_managed_block_is_not_overwritten(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('lucent_setup', ROOT / 'scripts/lucent-setup.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'bindings.lua'
            original = module.BEGIN + '-- unrelated user content\n'
            config.write_text(original)
            with self.assertRaises(SystemExit):
                module.edit(config, 'replacement\n')
            self.assertEqual(config.read_text(), original)


    def test_window_defaults_activation_and_rollback_preserve_user_appearance(self):
        spec = importlib.util.spec_from_file_location('lucent_setup_corners', ROOT / 'scripts/lucent-setup.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            module.HOME = Path(directory)
            module.BACKUP = module.HOME / 'backup'
            path = module.HOME / '.config/hypr/looknfeel.lua'
            path.parent.mkdir(parents=True)
            original = '-- User appearance\nhl.config({general={gaps_in=6}})\n'
            path.write_text(original)
            with patch.object(module, 'run'), patch.object(module.subprocess, 'run'):
                module.activate()
                once = path.read_text()
                self.assertTrue((module.BACKUP/'menu-enabled').exists())
                self.assertIn('menu.py', (module.HOME/'.local/lib/lucent/bin/omarchy-menu-select').read_text())
                module.activate()
                self.assertEqual(path.read_text(), once)
                self.assertIn('rounding = 20', once)
                self.assertEqual((module.BACKUP/'looknfeel.lua').read_text(), original)
                path.write_text(once+'-- Later user setting\n')
                module.rollback()
                self.assertFalse((module.BACKUP/'menu-enabled').exists())
            self.assertIn(original.strip(), path.read_text())
            self.assertIn('-- Later user setting', path.read_text())
            self.assertNotIn('rounding = 20', path.read_text())
