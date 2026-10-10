#!/usr/bin/env python3
"""Install, activate, or roll back Lucent using only Omarchy user configuration."""
import argparse
import json
import time
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
HOME = Path.home()
BEGIN = '-- BEGIN DESKTOP LAB LUCENT\n'
END = '-- END DESKTOP LAB LUCENT\n'
BACKUP = HOME / '.local/state/lucent/integration-backup'


def run(*args):
    subprocess.run(args, check=True)


def edit(path, block=None, begin=BEGIN, end=END):
    text = path.read_text() if path.exists() else ''
    if begin in text:
        before, rest = text.split(begin, 1)
        if end not in rest:
            raise SystemExit(f'Malformed managed block: {path}')
        text = before + rest.split(end, 1)[1]
    if block:
        text = text.rstrip() + '\n\n' + begin + block + end
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def install(binary_dir):
    for name in ('lucent-desktop', 'lucent-cli', 'lucent-lock', 'lucent-greeter', 'lucent-menu', 'lucent-polkit'):
        source = binary_dir / name
        if not source.is_file():
            raise SystemExit(f'Build {source} first')
    for name in ('lucent-desktop', 'lucent-cli', 'lucent-lock', 'lucent-greeter', 'lucent-menu', 'lucent-polkit'):
        target = HOME / '.local/bin' / name
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_suffix('.new')
        shutil.copy2(binary_dir / name, temporary)
        temporary.chmod(0o755)
        temporary.replace(target)
    helper = HOME / '.local/lib/lucent/session.py'
    helper.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / 'scripts/lucent-session.py', helper)
    unit = HOME / '.config/systemd/user/lucent.service'
    unit.parent.mkdir(parents=True, exist_ok=True)
    unit.write_text('''[Unit]
Description=Lucent Rust desktop
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/python3 %h/.local/lib/lucent/session.py
TimeoutStopSec=10
Restart=on-failure
RestartSec=3
''')
    for source,target in [('lucent-theme.py','theme.py'),('lucent-controls.py','controls.py'),
                          ('lucent-catalog.py','catalog.py'),('lucent-compat.py','compat.py'),('lucent-idle.py','idle.py'),
                          ('lucent-clipboard.py','clipboard.py'),('clipboard_store.py','clipboard_store.py'),
                          ('lucent-stop-stock.py','stop_stock.py')]:
        shutil.copy2(ROOT/'scripts'/source,helper.parent/target)
    shutil.copytree(ROOT/'configs/lucent/themes',helper.parent/'themes',dirs_exist_ok=True)
    shutil.copy2(ROOT/'configs/lucent/palettes.json',helper.parent/'palettes.json')
    state=HOME/'.local/state/lucent'
    state.mkdir(parents=True,exist_ok=True)
    wallpaper=state/'wallpaper'
    legacy=HOME/'.local/state/omarchy/current/background'
    if not wallpaper.exists() and legacy.exists():
        wallpaper.unlink(missing_ok=True)
        wallpaper.symlink_to(legacy.resolve())
    # Emergency secure locker exists before the first theme-export effect.
    theme=state/'theme';theme.mkdir(exist_ok=True)
    settings=state/'desktop.json'
    light=json.loads(settings.read_text()).get('light',False) if settings.exists() else False
    mode='light' if light else 'dark'
    shutil.copy2(helper.parent/'themes'/mode/'hyprlock.conf',theme/'hyprlock.conf')
    (theme/'mode').write_text(mode+'\n')
    for name,command in [('polkit','%h/.local/bin/lucent-polkit'),
                         ('idle','/usr/bin/python3 %h/.local/lib/lucent/idle.py'),
                         ('clipboard','/usr/bin/python3 %h/.local/lib/lucent/clipboard.py')]:
        (unit.parent/f'lucent-{name}.service').write_text(f"""[Unit]
Description=Lucent {name} adapter
PartOf=lucent.service graphical-session.target
After=graphical-session.target
StartLimitIntervalSec=0
[Service]
Type=exec
ExecStart={command}
Restart=on-failure
RestartSec=3
UMask=0077
""")
    entry = HOME / '.local/share/applications/lucent-desktop.desktop'
    entry.parent.mkdir(parents=True, exist_ok=True)
    entry.write_text('[Desktop Entry]\nType=Application\nName=Lucent Desktop\nComment=Native Rust desktop shell\nExec=systemctl --user start lucent.service\nIcon=preferences-desktop\nCategories=System;\nTerminal=false\n')
    shutil.copy2(ROOT/'scripts/lucent-lock.py', HOME/'.local/lib/lucent/lock.py')
    shutil.copy2(ROOT/'scripts/lucent-wallpaper.py', HOME/'.local/lib/lucent/wallpaper.py')
    shutil.copy2(ROOT/'scripts/lucent-menu.py', HOME/'.local/lib/lucent/menu.py')
    shutil.copy2(ROOT/'scripts/lucent-menu-routes.py', HOME/'.local/lib/lucent/menu_routes.py')
    shutil.copy2(ROOT/'scripts/menu_model.py', HOME/'.local/lib/lucent/menu_model.py')
    (unit.parent/'lucent-wallpaper-sync.service').write_text('''[Unit]
Description=Publish the selected wallpaper for Lucent login
StartLimitIntervalSec=0
[Service]
Type=oneshot
ExecStart=/usr/bin/python3 %h/.local/lib/lucent/wallpaper.py
''')
    (unit.parent/'lucent-wallpaper-sync.path').write_text('''[Unit]
Description=Publish Lucent wallpaper and appearance for login
[Path]
PathChanged=%h/.local/state/lucent/wallpaper
PathChanged=%h/.local/state/lucent/theme/mode
PathChanged=%h/.local/state/lucent/theme/palette.json
Unit=lucent-wallpaper-sync.service
[Install]
WantedBy=default.target
''')
    lock_unit=HOME/'.config/systemd/user/lucent-lock.service'
    lock_unit.write_text("""[Unit]
Description=Lucent secure Wayland locker
PartOf=graphical-session.target
StartLimitIntervalSec=30
StartLimitBurst=3
[Service]
Type=exec
ExecStart=%h/.local/bin/lucent-lock
Restart=on-failure
RestartSec=1
""")
    run('systemctl', '--user', 'daemon-reload')
    subprocess.run(['systemctl','--user','reset-failed','lucent-wallpaper-sync.service','lucent-wallpaper-sync.path'],check=False,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    run('systemctl','--user','enable','--now','lucent-wallpaper-sync.path')
    run('systemctl','--user','start','lucent-wallpaper-sync.service')
    print('Installed. Test with systemctl --user start lucent.service before activating login startup.')


def native_ready():
    result=subprocess.run([str(HOME/'.local/bin/lucent-cli'),'inspect'],check=True,capture_output=True,text=True,timeout=3)
    state=json.loads(result.stdout)
    rendered={surface['id'] for surface in state.get('surfaces',[]) if surface.get('frames',0)>0}
    if not {'background','dock','widgets'}<=rendered or 'theme_error' not in state.get('client',{}):
        raise SystemExit('Run the current native desktop and wait for its first frames before activation')


def activate():
    for dependency in ('swayidle','hyprlock'):
        if not shutil.which(dependency): raise SystemExit('Install '+dependency+' before activating native shell ownership')
    # A running, renderable client is required before changing login integration.
    native_ready()
    subprocess.run(['systemctl','--user','reset-failed','lucent-wallpaper-sync.service','lucent-wallpaper-sync.path'],check=False,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    run('systemctl','--user','enable','--now','lucent-wallpaper-sync.path')
    run('systemctl','--user','start','lucent-wallpaper-sync.service')
    BACKUP.mkdir(parents=True, exist_ok=True)
    for name in ('bindings.lua', 'autostart.lua', 'looknfeel.lua'):
        source = HOME / '.config/hypr' / name
        target = BACKUP / name
        if not target.exists() and source.exists():
            shutil.copy2(source, target)
    edit(HOME / '.config/hypr/bindings.lua', '''hl.unbind("SUPER + SPACE")
o.bind("SUPER + SPACE", "Lucent launcher", os.getenv("HOME") .. "/.local/bin/lucent-cli launcher toggle")
hl.unbind("SUPER + CTRL + SPACE")
o.bind("SUPER + CTRL + SPACE", "Lucent wallpapers", os.getenv("HOME") .. "/.local/bin/lucent-cli wallpapers open")
hl.unbind("SUPER + CTRL + L")
o.bind("SUPER + CTRL + L", "Lucent secure lock", "python3 " .. os.getenv("HOME") .. "/.local/lib/lucent/lock.py")
''')
    edit(HOME / '.config/hypr/looknfeel.lua', (ROOT / 'configs/lucent/window-rules.lua').read_text())
    wrappers=HOME/'.local/lib/lucent/bin';wrappers.mkdir(parents=True,exist_ok=True)
    for mode in ('select', 'input', 'routes'):
        script = wrappers / ('omarchy-menu' if mode == 'routes' else 'omarchy-menu-' + mode)
        script.write_text('#!/usr/bin/env bash\nexec python3 "$HOME/.local/lib/lucent/menu.py" ' + mode + ' "$@"\n')
        script.chmod(0o755)
    for name in ('omarchy-shell','omarchy-launch-shell','omarchy-restart-shell','omarchy-theme-set',
                 'omarchy-theme-switcher','omarchy-theme-bg-switcher','omarchy-launch-screensaver'):
        script=wrappers/name
        script.write_text('#!/usr/bin/env bash\nexec python3 "$HOME/.local/lib/lucent/compat.py" '+name+' "$@"\n')
        script.chmod(0o755)
    (BACKUP/'menu-enabled').touch()
    for name,mode in [('omarchy-system-lock',''),('omarchy-system-sleep-lock',' sleep')]:
        script=wrappers/name
        script.write_text('#!/usr/bin/env bash\nexec python3 "$HOME/.local/lib/lucent/lock.py"'+mode+' "$@"\n')
        script.chmod(0o755)
    profile=HOME/'.bash_profile'
    if not (BACKUP/'bash_profile.saved').exists():
        (BACKUP/'bash_profile.saved').write_text(profile.read_text() if profile.exists() else '')
    edit(profile,'export PATH="$HOME/.local/lib/lucent/bin:$PATH"\n',begin='# BEGIN DESKTOP LAB LUCENT\n',end='# END DESKTOP LAB LUCENT\n')
    original_path=':'.join(p for p in os.environ.get('PATH','/usr/local/bin:/usr/bin').split(':') if p!=str(wrappers))
    (BACKUP/'session-path').write_text(original_path)
    run('systemctl','--user','set-environment','PATH='+str(wrappers)+':'+original_path)
    edit(HOME / '.config/hypr/autostart.lua', 'hl.env("PATH", os.getenv("HOME") .. "/.local/lib/lucent/bin:" .. os.getenv("PATH"))\no.launch_on_start("systemctl --user start lucent.service")\n')
    first_native=not (BACKUP/'native-shell-enabled').exists()
    (BACKUP/'native-shell-enabled').touch()
    run('hyprctl', 'reload')
    try:
        run('python3',str(HOME/'.local/lib/lucent/stop_stock.py'))
        if first_native:
            (HOME/'.local/state/omarchy/toggles/bar-off').unlink(missing_ok=True)
            run(str(HOME/'.local/bin/lucent-cli'),'bar','show')
        run('systemctl','--user','start','lucent-polkit.service','lucent-idle.service','lucent-clipboard.service')
        time.sleep(2)
        run('systemctl','--user','is-active','--quiet','lucent-polkit.service','lucent-idle.service','lucent-clipboard.service')
    except (OSError,subprocess.SubprocessError):
        rollback()
        raise
    print('Lucent login integration active. Super+Space opens apps; Super+Ctrl+Space opens wallpapers.')


def rollback():
    # Remove only our blocks, preserving subsequent user edits.
    (BACKUP/'menu-enabled').unlink(missing_ok=True)
    (BACKUP/'native-shell-enabled').unlink(missing_ok=True)
    edit(HOME / '.config/hypr/bindings.lua')
    edit(HOME / '.config/hypr/autostart.lua')
    edit(HOME / '.config/hypr/looknfeel.lua')
    subprocess.run(['systemctl','--user','disable','--now','lucent-wallpaper-sync.path'],check=False)
    edit(HOME/'.bash_profile',begin='# BEGIN DESKTOP LAB LUCENT\n',end='# END DESKTOP LAB LUCENT\n')
    if (BACKUP/'session-path').exists():
        run('systemctl','--user','set-environment','PATH='+(BACKUP/'session-path').read_text())
    subprocess.run(['systemctl', '--user', 'stop', 'lucent.service'], check=False)
    theme=HOME/'.local/lib/lucent/theme.py'
    if theme.exists(): run('python3',str(theme),'rollback')
    run('hyprctl', 'reload')
    env=dict(os.environ)
    if (BACKUP/'session-path').exists(): env['PATH']=(BACKUP/'session-path').read_text()
    # These two capabilities were owned by Lucent; restore the stock owners.
    (HOME/'.local/state/omarchy/toggles/bar-off').unlink(missing_ok=True)
    config=HOME/'.config/omarchy/shell.json'
    if config.exists():
        data=json.loads(config.read_text())
        data['disabledPlugins']=[p for p in data.get('disabledPlugins',[]) if p!='omarchy.notifications']
        config.write_text(json.dumps(data,indent=2)+'\n')
    subprocess.Popen(['/usr/bin/omarchy-launch-shell'],env=env,start_new_session=True,
                     stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    print('Restored stock bar and startup. Lucent settings and binaries retained.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'activate', 'rollback'))
    parser.add_argument('--binary-dir', type=Path, default=ROOT / 'lucent/target/release')
    args = parser.parse_args()
    {'install': lambda: install(args.binary_dir), 'activate': activate, 'rollback': rollback}[args.action]()
