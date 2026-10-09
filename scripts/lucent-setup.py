#!/usr/bin/env python3
"""Install, activate, or roll back Lucent using only Omarchy user configuration."""
import argparse
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


def edit(path, block=None):
    text = path.read_text() if path.exists() else ''
    if BEGIN in text:
        before, rest = text.split(BEGIN, 1)
        if END not in rest:
            raise SystemExit(f'Malformed managed block: {path}')
        text = before + rest.split(END, 1)[1]
    if block:
        text = text.rstrip() + '\n\n' + BEGIN + block + END
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def install(binary_dir):
    for name in ('lucent-desktop', 'lucent-cli'):
        source = binary_dir / name
        if not source.is_file():
            raise SystemExit(f'Build {source} first')
    for name in ('lucent-desktop', 'lucent-cli'):
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
    run('systemctl', '--user', 'daemon-reload')
    print('Installed. Test with systemctl --user start lucent.service before activating login startup.')


def activate():
    # A running, renderable client is required before changing login integration.
    run(str(HOME / '.local/bin/lucent-cli'), 'inspect')
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
''')
    edit(HOME / '.config/hypr/looknfeel.lua', (ROOT / 'configs/lucent/window-rules.lua').read_text())
    edit(HOME / '.config/hypr/autostart.lua', 'o.launch_on_start("systemctl --user start lucent.service")\n')
    run('hyprctl', 'reload')
    print('Lucent login integration active. Super+Space opens apps; Super+Ctrl+Space opens wallpapers.')


def rollback():
    # Remove only our blocks, preserving subsequent user edits.
    edit(HOME / '.config/hypr/bindings.lua')
    edit(HOME / '.config/hypr/autostart.lua')
    edit(HOME / '.config/hypr/looknfeel.lua')
    subprocess.run(['systemctl', '--user', 'stop', 'lucent.service'], check=False)
    run('hyprctl', 'reload')
    print('Restored stock bar and startup. Lucent settings and binaries retained.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'activate', 'rollback'))
    parser.add_argument('--binary-dir', type=Path, default=ROOT / 'lucent/target/release')
    args = parser.parse_args()
    {'install': lambda: install(args.binary_dir), 'activate': activate, 'rollback': rollback}[args.action]()
