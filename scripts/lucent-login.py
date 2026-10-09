#!/usr/bin/env python3
"""Install/test Lucent's greetd greeter; preserve the previous display manager."""
import argparse
import json
import os
import pwd
from pathlib import Path
import shutil
import subprocess

ROOT=Path(__file__).resolve().parent.parent
BACKUP=Path('/var/lib/lucent/login-backup')
RECORD=BACKUP/'manager.json'

def run(*cmd):
    return subprocess.run(cmd,check=True,text=True,capture_output=True).stdout.strip()

def install(binary_dir, wallpaper_user=None):
    if not shutil.which('greetd'):
        raise SystemExit('Install the official greetd package before this step.')
    binary=binary_dir/'lucent-greeter'
    if not binary.is_file(): raise SystemExit('Build lucent-greeter first.')
    BACKUP.mkdir(parents=True,exist_ok=True)
    config=Path('/etc/greetd/config.toml')
    if not RECORD.exists():
        manager=Path('/etc/systemd/system/display-manager.service')
        RECORD.write_text(json.dumps({'manager': manager.resolve().name if manager.exists() else None,
                                     'had_config': config.exists()}))
        if config.exists(): shutil.copy2(config,BACKUP/'config.toml')
    for source,target in [(binary,Path('/usr/local/bin/lucent-greeter')),
                          (ROOT/'configs/lucent/greetd/run-greeter',Path('/usr/local/lib/lucent/run-greeter')),
                          (ROOT/'configs/lucent/greetd/greeter.lua',Path('/etc/greetd/lucent.lua'))]:
        target.parent.mkdir(parents=True,exist_ok=True)
        temporary=target.with_suffix('.new');shutil.copy2(source,temporary)
        temporary.chmod(0o644 if target.suffix=='.lua' else 0o755);temporary.replace(target)
    # The greeter user has / as its packaged home; give its compositor a writable private home.
    home=Path('/var/lib/lucent/greeter');home.mkdir(parents=True,exist_ok=True)
    shutil.chown(home,user='greeter',group='greeter');home.chmod(0o700)
    user=wallpaper_user or os.environ.get('SUDO_USER')
    wallpaper_env=''
    if user and user!='root':
        account=pwd.getpwnam(user)
        directory=Path('/var/lib/lucent/wallpapers')/str(account.pw_uid)
        directory.mkdir(parents=True,exist_ok=True)
        shutil.chown(directory,user=user,group='greeter');directory.chmod(0o2750)
        # Run as the desktop account: never read a user-selected file with root privileges.
        run('runuser','-u',user,'--','/usr/bin/python3',str(ROOT/'scripts/lucent-wallpaper.py'))
        link=Path('/etc/greetd/lucent-wallpaper')
        temporary=link.with_suffix('.new');temporary.unlink(missing_ok=True)
        temporary.symlink_to(directory/'wallpaper');temporary.replace(link)
        wallpaper_env=f'LUCENT_GREETER_WALLPAPER={directory}/wallpaper '
    text=(ROOT/'configs/lucent/greetd/config.toml').read_text().replace('command = "','command = "env HOME=/var/lib/lucent/greeter '+wallpaper_env,1)
    Path('/etc/greetd/lucent.toml').write_text(text)
    # Refresh this installation's active config without changing the selected manager.
    if config.exists() and '/etc/greetd/lucent.lua' in config.read_text():
        config.write_text(text)
    print('Installed native greeter. Existing login manager unchanged.')

def test():
    text=Path('/etc/greetd/lucent.toml').read_text().replace('vt = 1','vt = 9')
    Path('/etc/greetd/lucent-test.toml').write_text(text)
    run('systemd-run','--unit=lucent-greeter-test','--collect','/usr/bin/greetd','--config','/etc/greetd/lucent-test.toml')
    print('Greeter test runs on VT9. Stop with: sudo systemctl stop lucent-greeter-test')

def activate():
    if not RECORD.exists(): raise SystemExit('Install and test the greeter first.')
    previous=json.loads(RECORD.read_text())
    shutil.copy2('/etc/greetd/lucent.toml','/etc/greetd/config.toml')
    if previous['manager']: run('systemctl','disable',previous['manager'])
    run('systemctl','enable','greetd.service')
    print('Lucent greeter selected for the next boot. Current session preserved.')

def rollback():
    if not RECORD.exists(): raise SystemExit('No Lucent login backup exists.')
    previous=json.loads(RECORD.read_text())
    subprocess.run(['systemctl','stop','lucent-greeter-test'],check=False,capture_output=True)
    run('systemctl','disable','greetd.service')
    if previous['had_config']: shutil.copy2(BACKUP/'config.toml','/etc/greetd/config.toml')
    else: Path('/etc/greetd/config.toml').unlink(missing_ok=True)
    if previous['manager']: run('systemctl','enable',previous['manager'])
    print('Previous login manager selected for next boot. No active session was stopped.')

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('action',choices=['install','test','activate','rollback']);p.add_argument('--binary-dir',type=Path,default=ROOT/'lucent/target/release');p.add_argument('--wallpaper-user',help='Desktop account whose chosen wallpaper is shown before login (defaults to SUDO_USER)');args=p.parse_args()
    if os.geteuid()!=0: raise SystemExit('This display-manager integration requires root in the test VM.')
    if args.action=='install':install(args.binary_dir,args.wallpaper_user)
    else:globals()[args.action]()
