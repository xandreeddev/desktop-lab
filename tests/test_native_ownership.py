"""Ownership/migration contracts; never touches a running desktop."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))

def load(name,filename):
    spec=importlib.util.spec_from_file_location(name,ROOT/'scripts'/filename)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    return module


class Appearance(unittest.TestCase):
    def test_tokens_propagate_outward_preserving_wallpaper_and_user_css(self):
        theme=load('theme_test','lucent-theme.py')
        with tempfile.TemporaryDirectory() as temporary:
            theme.HOME=Path(temporary);theme.STATE=theme.HOME/'state';theme.BACKUP=theme.STATE/'backup'
            current=theme.HOME/'.local/state/omarchy/current/theme'
            current.mkdir(parents=True);(current/'colors.toml').write_text('original')
            wallpaper=current.parent/'background';wallpaper.write_text('wallpaper untouched')
            css=theme.HOME/'.config/gtk-3.0/gtk.css';css.parent.mkdir(parents=True);css.write_text('/* user CSS */\n')
            def run(*args,**kwargs):
                if args[0]=='/usr/bin/omarchy-theme-set':
                    self.assertEqual(kwargs['env']['OMARCHY_THEME_HEADLESS'],'1')
                    self.assertEqual(kwargs['env']['OMARCHY_THEME_SKIP_BACKGROUND'],'1')
                    (current/'colors.toml').write_text('exported')
            with patch.object(theme,'run',side_effect=run), patch.object(theme.shutil,'which',return_value=None), \
                    patch.object(theme.subprocess,'run',return_value=SimpleNamespace(stdout="'original-setting'\n")):
                theme.apply('light');theme.apply('dark')
                self.assertEqual((theme.BACKUP/'theme/colors.toml').read_text(),'original')
                self.assertEqual(css.read_text().count(theme.BEGIN),1)
                css.write_text(css.read_text()+'/* later edit */\n')
                self.assertEqual((theme.STATE/'theme/mode').read_text(),'dark\n')
                theme.rollback()
            self.assertEqual((current/'colors.toml').read_text(),'original')
            self.assertEqual(wallpaper.read_text(),'wallpaper untouched')
            self.assertEqual(css.read_text(),'/* user CSS */\n/* later edit */\n')
            self.assertFalse((theme.HOME/'.config/gtk-4.0/gtk.css').exists())

    def test_invalid_modes_and_failed_backups_cannot_apply_partial_theme(self):
        theme=load('theme_failure','lucent-theme.py')
        with tempfile.TemporaryDirectory() as temporary:
            theme.HOME=Path(temporary);theme.STATE=theme.HOME/'state';theme.BACKUP=theme.STATE/'backup'
            with patch.object(theme,'run') as apply, patch.object(theme.subprocess,'run',side_effect=RuntimeError('gsettings unavailable')):
                with self.assertRaises(ValueError): theme.apply('../../other')
                with self.assertRaises(RuntimeError): theme.apply('light')
                self.assertFalse(theme.BACKUP.exists())
                apply.assert_not_called()


class Compatibility(unittest.TestCase):
    def test_migration_rejects_old_or_unrendered_clients(self):
        setup=load('readiness_test','lucent-setup.py')
        for state in [{}, {'client':{'theme_error':''},'surfaces':[{'id':'background','frames':0}]}]:
            with patch.object(setup.subprocess,'run',return_value=SimpleNamespace(stdout=json.dumps(state))):
                with self.assertRaises(SystemExit):setup.native_ready()
        state={'client':{'theme_error':''},'surfaces':[{'id':name,'frames':1} for name in ('background','dock','widgets')]}
        with patch.object(setup.subprocess,'run',return_value=SimpleNamespace(stdout=json.dumps(state))):setup.native_ready()

    def test_process_matching_never_stops_other_quickshell_profiles(self):
        stop=load('stop_test','lucent-stop-stock.py');root=Path('/usr/share/omarchy')
        self.assertEqual(stop.role(['/bin/bash','/usr/bin/omarchy-launch-shell'],root),'supervisor')
        self.assertEqual(stop.role(['quickshell','-n','-p',str(root/'shell')],root),'shell')
        self.assertIsNone(stop.role(['quickshell','-p','/home/fixture/.config/lucid'],root))
        self.assertIsNone(stop.role(['python3','script.py','omarchy-launch-shell'],root))

    def test_stock_theme_cannot_override_lucent_tokens_and_unknown_ipc_does_not_start_shell(self):
        compat=load('compat_test','lucent-compat.py')
        with tempfile.TemporaryDirectory() as temporary:
            compat.ENABLED=Path(temporary)/'enabled';compat.ENABLED.touch()
            with patch.object(compat,'cli',return_value='ok') as cli, patch.object(compat.subprocess,'Popen') as popen:
                self.assertEqual(compat.main('omarchy-theme-set',['lucent-tokens-light']),'ok')
                cli.assert_called_once_with('theme','light')
                with self.assertRaises(ValueError): compat.main('omarchy-theme-set',['external-theme'])
                with self.assertRaises(ValueError): compat.shell(['shell','summon','unknown.plugin'])
                popen.assert_not_called()

    def test_native_picker_cannot_execute_an_unoffered_value(self):
        controls=load('controls_test','lucent-controls.py')
        for value in ['-1','100','$(command)','0; command']:
            with patch.object(controls.picker,'pick',return_value=SimpleNamespace(returncode=0,stdout=value)):
                with self.assertRaises(ValueError): controls.choose('Fixture',[('Action','',('safe','argument'))])
        self.assertEqual(controls.nm_fields(r'uuid:Name\: with\\slash:wifi'),['uuid','Name: with\\slash','wifi'])

    def test_idle_stay_awake_retains_sleep_and_logind_lock_handlers(self):
        idle=load('idle_test','lucent-idle.py')
        with tempfile.TemporaryDirectory() as temporary:
            idle.HOME=Path(temporary)
            self.assertNotIn('timeout',idle.command(False))
            self.assertIn('before-sleep',idle.command(False))
            self.assertIn('lock',idle.command(False))
            self.assertIn('300',idle.command(True))


class Clipboard(unittest.TestCase):
    def test_private_bounded_history_ignores_sensitive_offers_and_deduplicates(self):
        import clipboard_store as store
        with tempfile.TemporaryDirectory() as temporary, patch.object(store,'ROOT',Path(temporary)/'history'),patch.object(store,'LIMIT',2):
            store.capture('text',b'never-save',sensitive=True)
            self.assertFalse(store.ROOT.exists())
            for text in [b'one',b'two',b'one',b'three']: store.capture('text',text)
            self.assertEqual([item['text'] for item in store.read()],['three','one'])
            self.assertEqual((store.ROOT/'history.json').stat().st_mode & 0o777,0o600)
            store.capture('text',b'x'*(store.BYTES+1))
            self.assertEqual(len(store.read()),2)
            store.clear();self.assertEqual(store.read(),[])
