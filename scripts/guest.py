#!/usr/bin/env python3
"""Audited, reversible shell integration for disposable Omarchy test guests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
HOME_DIR = Path.home()
STATE = HOME_DIR / '.local/state/desktop-lab'
CONFIG = HOME_DIR / '.config/desktop-lab'
DATA = HOME_DIR / '.local/share/desktop-lab'
PACKAGED = Path('/usr/share/omarchy/default/hypr/autostart.lua')
STARTUP = HOME_DIR / '.config/default/hypr/autostart.lua'


def run(*args, check=True, capture=False, **kwargs):
    return subprocess.run(args, check=check, text=True, capture_output=capture, **kwargs)


def output(*args):
    return run(*args, check=False, capture=True).stdout.strip()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, content, executable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(path.name + '.tmp')
    temp.write_text(content)
    temp.chmod(0o755 if executable else 0o600)
    temp.replace(path)


def backup(relative):
    """Record absence as well as bytes; never overwrite an original backup."""
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    index = STATE / 'backup.json'
    records = json.loads(index.read_text()) if index.exists() else {}
    if relative in records:
        return
    src = HOME_DIR / relative
    dest = STATE / 'original' / relative
    exists = src.exists() or src.is_symlink()
    records[relative] = exists
    if exists:
        dest.parent.mkdir(parents=True, exist_ok=True)
        if src.is_dir() and not src.is_symlink():
            shutil.copytree(src, dest, symlinks=True)
        else:
            shutil.copy2(src, dest, follow_symlinks=False)
    write(index, json.dumps(records, indent=2) + '\n')


def doctor():
    data = {
        'virtualization': output('systemd-detect-virt'),
        'packages': output('pacman', '-Q', 'omarchy', 'hyprland', 'quickshell', 'noctalia', 'hyprlock', 'hypridle'),
        'startup_sha256': sha(PACKAGED) if PACKAGED.exists() else None,
        'startup': PACKAGED.read_text() if PACKAGED.exists() else None,
        'processes': output('ps', '-u', str(os.getuid()), '-o', 'comm='),
        'session': bool(os.environ.get('WAYLAND_DISPLAY')),
        'monitors': output('hyprctl', '-j', 'monitors'),
        'sleep_service': output('systemctl', '--user', 'is-enabled', 'omarchy-sleep-lock.service'),
    }
    STATE.mkdir(parents=True, exist_ok=True)
    write(STATE / 'doctor.json', json.dumps(data, indent=2) + '\n')
    print(json.dumps(data, indent=2))


def gate(args):
    if os.getuid() == 0:
        raise SystemExit('Run as the desktop user, using sudo only for package installation.')
    if not PACKAGED.is_file():
        raise SystemExit('This integration requires Omarchy with Hyprland Lua.')
    if args.yes:
        if output('systemd-detect-virt') not in ('kvm', 'qemu'):
            raise SystemExit('--yes is limited to disposable KVM/QEMU guests.')
    elif input(f'Prepare {args.profile} on this Omarchy installation? [y/N] ').lower() != 'y':
        raise SystemExit('Cancelled.')


def install(args):
    if args.profile == 'lucent':
        raise SystemExit('Use scripts/lucent-setup.py and lucent/README.md for the Rust framework/client integration.')
    gate(args)
    doctor()
    for relative in ('.config/quickshell', '.config/lucid', '.config/noctalia',
                     '.config/desktop-lab', '.local/share/desktop-lab',
                     '.local/share/fonts/lucid', '.config/cava/quickshell.conf', '.cache/quickshell/matugen.json'):
        backup(relative)
    write(STATE / 'packages-before.txt', output('pacman', '-Q') + '\n') if not (STATE / 'packages-before.txt').exists() else None
    if not args.skip_packages:
        packages = (ROOT / f'manifests/{args.profile}-dependencies.txt').read_text().split()
        cmd = ['sudo', 'pacman', '-S', '--needed']
        if args.yes:
            cmd.append('--noconfirm')
        run(*(cmd + packages))
        if output('systemd-detect-virt') in ('kvm', 'qemu'):
            run('sudo', 'pacman', '-S', '--needed', *(['--noconfirm'] if args.yes else []), 'qemu-guest-agent')
            run('sudo', 'systemctl', 'start', 'qemu-guest-agent')
    CONFIG.mkdir(parents=True, exist_ok=True)
    DATA.mkdir(parents=True, exist_ok=True)
    for name in ('hyprlock.conf', 'hypridle.conf'):
        shutil.copy2(ROOT / 'configs' / name, CONFIG / name)
    shutil.copy2(ROOT / 'scripts/session.sh', DATA / 'session.sh')
    (DATA / 'session.sh').chmod(0o755)
    if args.profile == 'lucid':
        run(sys.executable, str(ROOT / 'scripts/fetch.py'), 'lucid')
        src = ROOT / '.cache/upstream/lucid'
        lock = json.loads((ROOT / 'manifests/upstream-lock.json').read_text())['lucid']
        if sha(src / 'install.sh') != lock['installer_sha256']:
            raise SystemExit('Lucid installer changed; review it and update the audit before running.')
        run('bash', str(src / 'install.sh'), '--no-hypr', '--no-apps', '--no-look',
            '--no-theming', '--no-wallpapers', '--no-plugins', '--skip-deps',
            *(['--yes'] if args.yes else []))
        run('patch', '-p1', '-i', str(ROOT / 'patches/lucid-bluetooth-timeout.patch'),
            cwd=HOME_DIR / '.config/quickshell')
        prefs = HOME_DIR / '.config/quickshell/lucidprefs/prefs.json'
        values = json.loads(prefs.read_text())
        values.update(json.loads((ROOT / 'configs/lucid/prefs.json').read_text()))
        write(prefs, json.dumps(values, indent=2) + '\n')
        # Keep all bundled palette helpers local to Lucid; no app-theme hooks.
        shutil.copytree(src / 'support/lucid', HOME_DIR / '.config/lucid', dirs_exist_ok=True)
    elif args.profile == 'noctalia':
        version = output('noctalia', '--version')
        if not any(s.removeprefix('v').startswith('5.') for s in version.split()):
            raise SystemExit(f'Expected Noctalia v5, got {version!r}')
        cfg = HOME_DIR / '.config/noctalia/config.toml'
        write(cfg, (ROOT / 'configs/noctalia/config.toml').read_text())
        run('noctalia', 'config', 'validate')
        write(STATE / 'noctalia-effective.toml', output('noctalia', 'config', 'export', 'full') + '\n')
    write(STATE / 'profile', args.profile + '\n')
    if (STATE / 'weather.json').is_file():
        run(sys.executable, str(ROOT / 'scripts/set-weather.py'), args.profile, str(STATE / 'weather.json'))
    write(STATE / 'packages-after.txt', output('pacman', '-Q') + '\n')
    print('Installed. Verify Hyprlock in the graphical session before activation.')


def activate(args):
    gate(args)
    if not (STATE / 'lock-verified.json').is_file():
        raise SystemExit('Lock verification required: scripts/verify.sh --lock in a graphical terminal.')
    if (STATE / 'profile').read_text().strip() != args.profile:
        raise SystemExit('Install this profile first.')
    expected = 'hl.exec_cmd("omarchy-launch-shell")'
    source = PACKAGED.read_text()
    if source.count(expected) != 1:
        raise SystemExit('Unsupported startup hook; inspect doctor report before adapting.')
    for relative in ('.config/default/hypr/autostart.lua', '.config/hypr/autostart.lua',
                     '.config/desktop-lab/bindings.lua',
                     '.config/systemd/user/desktop-lab-shell.service',
                     '.config/systemd/user/desktop-lab-idle.service',
                     '.config/systemd/user/omarchy-sleep-lock.service'):
        backup(relative)
    # Omarchy bootstrap resolves ~/.config/?.lua before /usr/share/omarchy/?.lua.
    replacement = 'hl.exec_cmd("systemctl --user start desktop-lab-shell.service")'
    write(STARTUP, '-- User startup override generated by desktop-lab. See doctor.json.\n' + source.replace(expected, replacement))
    write(STATE / 'startup-sha256', sha(PACKAGED) + '\n')
    bindings = bindings_for(args.profile)
    write(CONFIG / 'bindings.lua', bindings)
    user_start = HOME_DIR / '.config/hypr/autostart.lua'
    marker = 'dofile(os.getenv("HOME") .. "/.config/desktop-lab/bindings.lua")'
    text = user_start.read_text() if user_start.exists() else ''
    if marker not in text:
        write(user_start, text.rstrip() + '\n\n-- desktop-lab shortcuts\n' + marker + '\n')
    units = HOME_DIR / '.config/systemd/user'
    write(units / 'desktop-lab-shell.service', '''[Unit]
Description=Desktop Lab alternative shell
PartOf=graphical-session.target
Requires=desktop-lab-idle.service
After=desktop-lab-idle.service
[Service]
Type=simple
ExecStart=%h/.local/share/desktop-lab/session.sh
Restart=on-failure
RestartSec=3
''')
    write(units / 'desktop-lab-idle.service', '''[Unit]
Description=Desktop Lab idle and secure lock handling
PartOf=graphical-session.target
[Service]
ExecStart=/usr/bin/hypridle -c %h/.config/desktop-lab/hypridle.conf
Restart=on-failure
RestartSec=3
''')
    # The stock monitor invokes Omarchy's removed shell; hypridle takes over sleep locking.
    target = units / 'omarchy-sleep-lock.service'
    if target.exists() or target.is_symlink():
        target.unlink()
    target.symlink_to('/dev/null')
    run('systemctl', '--user', 'daemon-reload')
    print('Activation staged for next login. Log out and back in; no current shell was terminated.')


def bindings_for(profile):
    lock = 'hyprlock -c ~/.config/desktop-lab/hyprlock.conf'
    lucid = {
        'SUPER + SPACE': 'launcher toggle', 'SUPER + ALT + SPACE': 'launcher toggle',
        'SUPER + CTRL + V': 'launcher clipboard', 'SUPER + CTRL + E': 'moji toggle',
        'SUPER + ESCAPE': 'session toggle', 'SUPER + SHIFT + SPACE': 'settings bar',
        'SUPER + CTRL + SPACE': 'launcher wallpaper', 'SUPER + comma': 'notifs clear',
        'SUPER + SHIFT + ALT + comma': 'notifs toggle',
        'SUPER + ALT + S': 'settings open', 'PRINT': 'snap toggle',
    }
    noctalia = {
        'SUPER + SPACE': 'panel-toggle launcher', 'SUPER + ALT + SPACE': 'panel-toggle launcher',
        'SUPER + CTRL + V': 'panel-toggle clipboard', 'SUPER + ESCAPE': 'panel-toggle session',
        'SUPER + SHIFT + SPACE': 'bar-toggle', 'SUPER + CTRL + SPACE': 'panel-toggle wallpaper',
        'SUPER + SHIFT + ALT + comma': 'panel-toggle notifications',
        'SUPER + ALT + S': 'settings-open', 'PRINT': 'screenshot-region',
    }
    mapping = {key: ('qs ipc call -- ' + value) for key, value in lucid.items()} if profile == 'lucid' else {key: ('noctalia msg ' + value) for key, value in noctalia.items()}
    mapping['SUPER + CTRL + L'] = lock
    mapping['SUPER + L'] = lock
    result = '-- Only shell actions are rebound; Omarchy tiling/input/monitor bindings stay loaded.\n'
    for key, cmd in mapping.items():
        result += f'hl.unbind({json.dumps(key)})\no.bind({json.dumps(key)}, "Desktop Lab", {json.dumps(cmd)})\n'
    return result


def verify_lock():
    if not os.environ.get('WAYLAND_DISPLAY'):
        raise SystemExit('Run lock verification inside a graphical terminal.')
    start = time.monotonic()
    run('hyprlock', '-c', str(CONFIG / 'hyprlock.conf'))
    if time.monotonic() - start < 2:
        raise SystemExit('Lock exited too quickly; no verification recorded.')
    write(STATE / 'lock-verified.json', json.dumps({'locker': 'hyprlock', 'unlocked': True}) + '\n')
    print('Hyprlock returned successfully after lock/unlock.')


def verify(args):
    if args.lock:
        verify_lock()
        return
    profile = (STATE / 'profile').read_text().strip()
    if not os.environ.get('WAYLAND_DISPLAY'):
        raise SystemExit('Run verification inside the graphical session.')
    run('systemctl', '--user', 'is-active', 'desktop-lab-shell.service', 'desktop-lab-idle.service')
    if (STATE / 'startup-sha256').read_text().strip() != sha(PACKAGED):
        raise SystemExit('Packaged startup changed: re-audit the user override before updating.')
    processes = output('ps', '-u', str(os.getuid()), '-o', 'args=')
    if 'quickshell -n -p /usr/share/omarchy/shell' in processes:
        raise SystemExit('Competing stock shell detected.')
    errors = output('hyprctl', 'configerrors')
    if errors.strip() not in ('', 'ok'):
        raise SystemExit('Hyprland config errors: ' + errors)
    if profile == 'noctalia':
        run('noctalia', 'config', 'validate')
        run('noctalia', 'msg', 'status')
    else:
        run('qs', 'ipc', 'call', '--', 'widgets', 'list')
    run('notify-send', 'Desktop Lab', f'{profile} verification notification')
    print('Automatic session checks passed. Hardware, lock, resume, and visual checks are separate.')


def rollback():
    index = STATE / 'backup.json'
    if not index.exists():
        raise SystemExit('No backup exists.')
    print('Restoring original user files; installed packages remain for an explicit package review.')
    for relative, existed in json.loads(index.read_text()).items():
        dest = HOME_DIR / relative
        if dest.is_symlink() or dest.is_file():
            dest.unlink()
        elif dest.is_dir():
            shutil.rmtree(dest)
        if existed:
            src = STATE / 'original' / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            if src.is_dir() and not src.is_symlink():
                shutil.copytree(src, dest, symlinks=True)
            else:
                shutil.copy2(src, dest, follow_symlinks=False)
    run('systemctl', '--user', 'daemon-reload')
    print('Original configuration restored. Log out and back in to restart the stock session.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('doctor', 'install', 'activate', 'verify', 'rollback'))
    parser.add_argument('profile', nargs='?', choices=('lucid', 'noctalia', 'lucent'))
    parser.add_argument('--yes', action='store_true')
    parser.add_argument('--skip-packages', action='store_true')
    parser.add_argument('--lock', action='store_true')
    args = parser.parse_args()
    if args.action in ('install', 'activate') and not args.profile:
        parser.error('profile is required')
    {'doctor': doctor, 'install': lambda: install(args), 'activate': lambda: activate(args),
     'verify': lambda: verify(args), 'rollback': rollback}[args.action]()


if __name__ == '__main__':
    main()
