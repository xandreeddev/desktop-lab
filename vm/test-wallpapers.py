#!/usr/bin/env python3
"""Live wallpaper/palette test for the unlocked Lucent VM.

Uses real provider searches and downloads, so network/site failures fail the test.
Restores the original wallpaper and desktop settings in finally. Downloaded test
images remain in the guest's wallpaper library. Captures stay in reports/local.
Pass --login-file only to additionally exercise native secure lock/unlock.
"""
import argparse
import importlib.util
import sys
import json
import subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'vm'))
spec=importlib.util.spec_from_file_location('v', ROOT/'vm/test-framework.py');v=importlib.util.module_from_spec(spec);spec.loader.exec_module(v)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--login-file',type=Path)
    args=parser.parse_args()
    assert v.session('omarchy-shell','lock','isLocked').strip()=='false'
    state='/home/omarchy/.local/state/lucent/desktop.json'
    backup='/tmp/lucent-palette-original-settings.json'
    wallpaper=v.remote('readlink','-f','/home/omarchy/.local/state/lucent/wallpaper').strip()
    v.remote('cp',state,backup)
    def client():return v.inspect()['client']
    def exported():return json.loads(v.remote('cat','/home/omarchy/.local/state/lucent/theme/palette.json'))
    def applied():return client()['wallpaper_browser']['status']=='Applied'
    try:
        v.cli('launcher','close');v.key(125,29,57)
        v.eventually(lambda:client()['mode']=='Wallpapers','Wallpaper keybind did not open native browser')
        v.click('source-wallhaven');v.click('wallpaper-search');v.key(29,30);v.text('landscape');v.key(28)
        v.eventually(lambda:client()['wallpaper_browser']['count']>0,'Wallhaven results missing',timeout=50)
        v.click('online-wallpaper-0');v.click('download-theme')
        v.eventually(applied,'Wallhaven wallpaper/theme did not apply',timeout=60)
        v.eventually(lambda:exported()==client()['palette'],'Palette export differs from selected colors',timeout=35)
        generated=client()['palette'];assert generated['id'].startswith('wallpaper-')
        assert v.remote('readlink','-f','/home/omarchy/.local/state/lucent/wallpaper').strip()!=wallpaper
        v.screenshot('wallhaven-palette-local-only.png')
        print('Wallhaven: real search input, download, wallpaper activation and matching semantic exports passed',flush=True)
        v.click('source-alpha-coders');v.click('wallpaper-search-submit')
        v.eventually(lambda:client()['wallpaper_browser']['count']>0,'Alpha Coders results missing',timeout=60)
        v.click('catalog-next')
        v.eventually(lambda:client()['wallpaper_browser']['page']==2 and client()['wallpaper_browser']['count']>0,'Alpha Coders pagination failed',timeout=60)
        v.click('download-wallpaper');v.eventually(applied,'Alpha Coders wallpaper did not apply',timeout=60)
        assert client()['palette']==generated
        print('Alpha Coders: native search, second page and wallpaper-only apply preserving colors passed',flush=True)
        v.cli('themes','open');v.key(108);v.key(108);v.key(28)
        v.eventually(lambda:client()['palette']['id']=='ocean','Keyboard palette selection failed')
        v.eventually(lambda:exported()['id']=='ocean','Named palette was not exported',timeout=35)
        v.eventually(lambda:json.loads(v.remote('cat','/etc/greetd/lucent-palette.json'))['id']=='ocean','Greeter palette did not synchronize',timeout=15)
        v.session('systemctl','--user','restart','lucent.service')
        v.eventually(lambda:client()['background_ready'] and client()['palette']['id']=='ocean','Palette did not survive restart',timeout=35)
        assert not client()['theme_error']
        assert not v.remote('sh','-c','pgrep -x quickshell || true').strip()
        print('Named palette: keyboard selection, restart persistence and greeter synchronization passed',flush=True)
        if args.login_file:
            v.session('python3','/home/omarchy/.local/lib/lucent/lock.py')
            assert v.session('omarchy-shell','lock','isLocked').strip()=='true'
            assert v.remote('pgrep','-x','lucent-lock').strip()
            subprocess.run([sys.executable,str(ROOT/'vm/run.py'),'--type-password','--login-file',str(args.login_file),'lucent'],check=True)
            v.eventually(lambda:v.session('omarchy-shell','lock','isLocked').strip()=='false','Native unlock failed',timeout=20)
            print('Native secure lock/unlock passed with the selected palette',flush=True)
    finally:
        v.cli('wallpaper','set',wallpaper)
        v.eventually(lambda:v.remote('readlink','-f','/home/omarchy/.local/state/lucent/wallpaper').strip()==wallpaper,'Original wallpaper not restored')
        v.session('systemctl','--user','stop','lucent.service')
        v.remote('cp',backup,state)
        v.session('systemctl','--user','start','lucent.service')
        v.eventually(lambda:client()['background_ready'],'Restored desktop did not render',timeout=35)
        v.eventually(lambda:exported()==client()['palette'],'Original palette export did not restore',timeout=35)
        v.cli('wallpapers','open')
        print('Original wallpaper and preferences restored; native wallpaper browser left open',flush=True)

if __name__=='__main__':main()
