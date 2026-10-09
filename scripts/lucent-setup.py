#!/usr/bin/env python3
"""Install, activate, or roll back Lucent using only Omarchy user configuration."""
import argparse
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
    for name in ('lucent-desktop', 'lucent-cli', 'lucent-lock', 'lucent-greeter', 'lucent-menu'):
        source = binary_dir / name
        if not source.is_file():
            raise SystemExit(f'Build {source} first')
    for name in ('lucent-desktop', 'lucent-cli', 'lucent-lock', 'lucent-greeter', 'lucent-menu'):
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
ExecStopPost=/usr/bin/python3 %h/.local/lib/lucent/session.py restore
TimeoutStopSec=10
Restart=on-failure
RestartSec=3
''')
    entry = HOME / '.local/share/applications/lucent-desktop.desktop'
    entry.parent.mkdir(parents=True, exist_ok=True)
    entry.write_text('[Desktop Entry]\nType=Application\nName=Lucent Desktop\nComment=Native Rust desktop shell\nExec=systemctl --user start lucent.service\nIcon=preferences-desktop\nCategories=System;\nTerminal=false\n')
    shutil.copy2(ROOT/'scripts/lucent-lock.py', HOME/'.local/lib/lucent/lock.py')
    shutil.copy2(ROOT/'scripts/lucent-wallpaper.py', HOME/'.local/lib/lucent/wallpaper.py')
    shutil.copy2(ROOT/'scripts/lucent-menu.py', HOME/'.local/lib/lucent/menu.py')
    (unit.parent/'lucent-wallpaper-sync.service').write_text('''[Unit]
Description=Publish the selected wallpaper for Lucent login
[Service]
Type=oneshot
ExecStart=/usr/bin/python3 %h/.local/lib/lucent/wallpaper.py
''')
    (unit.parent/'lucent-wallpaper-sync.path').write_text('''[Unit]
Description=Follow Omarchy wallpaper and theme changes for Lucent login
[Path]
PathChanged=%h/.local/state/omarchy/current
PathChanged=%h/.local/state/omarchy/current/background
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
    run('systemctl','--user','enable','--now','lucent-wallpaper-sync.path')
    run('systemctl','--user','start','lucent-wallpaper-sync.service')
    print('Installed. Test with systemctl --user start lucent.service before activating login startup.')


def activate():
    # A running, renderable client is required before changing login integration.
    subprocess.run([str(HOME / '.local/bin/lucent-cli'), 'inspect'], check=True, stdout=subprocess.DEVNULL)
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
    for mode in ('select', 'input'):
        script = wrappers / ('omarchy-menu-' + mode)
        script.write_text('#!/usr/bin/env bash\nexec python3 "$HOME/.local/lib/lucent/menu.py" ' + mode + ' "$@"\n')
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
    run('hyprctl', 'reload')
    print('Lucent login integration active. Super+Space opens apps; Super+Ctrl+Space opens wallpapers.')


def rollback():
    # Remove only our blocks, preserving subsequent user edits.
    (BACKUP/'menu-enabled').unlink(missing_ok=True)
    edit(HOME / '.config/hypr/bindings.lua')
    edit(HOME / '.config/hypr/autostart.lua')
    edit(HOME / '.config/hypr/looknfeel.lua')
    subprocess.run(['systemctl','--user','disable','--now','lucent-wallpaper-sync.path'],check=False)
    edit(HOME/'.bash_profile',begin='# BEGIN DESKTOP LAB LUCENT\n',end='# END DESKTOP LAB LUCENT\n')
    if (BACKUP/'session-path').exists():
        run('systemctl','--user','set-environment','PATH='+(BACKUP/'session-path').read_text())
    subprocess.run(['systemctl', '--user', 'stop', 'lucent.service'], check=False)
    run('hyprctl', 'reload')
    print('Restored stock bar and startup. Lucent settings and binaries retained.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'activate', 'rollback'))
    parser.add_argument('--binary-dir', type=Path, default=ROOT / 'lucent/target/release')
    args = parser.parse_args()
    {'install': lambda: install(args.binary_dir), 'activate': activate, 'rollback': rollback}[args.action]()
