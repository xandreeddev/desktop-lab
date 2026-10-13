#!/usr/bin/env python3
"""Two owned virtual HCI controllers, as root inside the prepared guest only.

Uses an explicitly supplied pinned BlueZ btvirt binary. No host Bluetooth or USB
passthrough. Refuses existing controllers/modules/runtime overrides, journals
ownership before mutations, and removes only new fixture storage/dependencies.
Private logs and state must never be published as hardware acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time


def run(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL).strip()


def controllers():
    return sorted(p.name for p in Path('/sys/class/bluetooth').glob('hci*')
                  if re.fullmatch(r'hci\d+', p.name))


def save(directory, state):
    temporary = directory / 'state.tmp'
    temporary.write_text(json.dumps(state))
    temporary.chmod(0o600)
    temporary.replace(directory / 'state.json')


def require_guest():
    if os.geteuid() != 0 or not Path('/etc/arch-release').exists() or run('systemd-detect-virt', '--vm') not in ('kvm', 'qemu'):
        raise RuntimeError('Fixture requires root in the prepared Arch guest')


def runtime_override():
    p = Path('/run/systemd/system/bluetooth.service')
    return p.exists() or p.is_symlink()


def process_stat(pid):
    try:
        # The comm field may contain spaces; fields after its closing parenthesis
        # begin at field 3. starttime is field 22.
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        return fields[0], fields[19]
    except FileNotFoundError:
        return None


def process_start(pid):
    stat = process_stat(pid)
    return stat[1] if stat else None


def terminate(child):
    if not child['start']:
        raise RuntimeError('Missing emulator birth identity; refusing to signal a process')
    stat = process_stat(child['pid'])
    if stat is None or stat[1] != child['start'] or stat[0] == 'Z':
        return
    try:
        os.killpg(child['pid'], signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        stat = process_stat(child['pid'])
        if stat is None or stat[1] != child['start'] or stat[0] == 'Z':
            return
        time.sleep(.1)
    stat = process_stat(child['pid'])
    if stat is not None and stat[1] == child['start'] and stat[0] != 'Z':
        raise RuntimeError('Owned emulator did not stop; no force-kill or module removal')


def start(directory, expected):
    require_guest()
    if controllers() or Path('/sys/module/bluetooth').exists() or Path('/sys/module/hci_vhci').exists():
        raise RuntimeError('Existing controller or Bluetooth module is not ours')
    if runtime_override() or (directory / 'state.json').exists():
        raise RuntimeError('Existing runtime override or fixture state is not ours')
    status = subprocess.run(['systemctl', 'is-active', 'bluetooth.service'], capture_output=True, text=True).stdout.strip()
    if status != 'inactive':
        raise RuntimeError('Expected the original inactive BlueZ service')
    binary = directory / 'btvirt'
    if binary.is_symlink() or not re.fullmatch(r'[a-f0-9]{64}', expected) or hashlib.sha256(binary.read_bytes()).hexdigest() != expected:
        raise RuntimeError('Virtual-controller executable hash mismatch')
    os.chown(directory, 0, 0)
    directory.chmod(0o700)
    store = Path('/var/lib/bluetooth')
    state = {'modules_before': [p.name for p in Path('/sys/module').iterdir()],
             'storage_before': [p.name for p in store.iterdir()] if store.exists() else [],
             'module_claimed': False, 'service_claimed': False, 'cleanup_mask': False,
             'children': [], 'addresses': [], 'controllers': [], 'btvirt_sha256': expected}
    save(directory, state)
    try:
        state['module_claimed'] = True
        save(directory, state)
        run('modprobe', 'hci_vhci')
        with (directory / 'btvirt.log').open('ab') as log:
            child = subprocess.Popen([str(binary), '-l2'], stdin=subprocess.DEVNULL,
                                     stdout=log, stderr=log, start_new_session=True)
        state['children'].append({'pid': child.pid, 'start': process_start(child.pid)})
        save(directory, state)
        deadline = time.monotonic() + 10
        while len(controllers()) != 2:
            if child.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError('Owned emulator did not create two controllers')
            time.sleep(.1)
        state['controllers'] = controllers()
        state['service_claimed'] = True
        save(directory, state)
        run('systemctl', 'start', 'bluetooth.service')
        deadline = time.monotonic() + 10
        while True:
            addresses = re.findall(r'^Controller ([0-9A-Fa-f:]{17}) ', run('bluetoothctl', 'list'), re.M)
            if len(addresses) == 2:
                break
            if time.monotonic() >= deadline:
                raise RuntimeError('BlueZ did not observe the owned controllers')
            time.sleep(.1)
        state['addresses'] = [a.upper() for a in addresses]
        save(directory, state)
        if any(a in state['storage_before'] for a in state['addresses']):
            raise RuntimeError('Virtual address collides with pre-existing storage')
        print(json.dumps({'ready': True, 'controllers': 2}))
    except BaseException:
        stop(directory)
        raise


def stop(directory):
    require_guest()
    state = json.loads((directory / 'state.json').read_text())
    try:
        if state['module_claimed']:
            if runtime_override() and not state['cleanup_mask']:
                raise RuntimeError('Runtime override is not ours')
            state['cleanup_mask'] = True
            save(directory, state)
            run('systemctl', 'mask', '--runtime', 'bluetooth.service')
            run('systemctl', 'stop', 'bluetooth.service')
        for child in reversed(state['children']):
            terminate(child)
        deadline = time.monotonic() + 5
        while controllers() and time.monotonic() < deadline:
            time.sleep(.1)
        if controllers():
            raise RuntimeError('A controller remains; refusing module or storage removal')
        before = set(state['modules_before'])

        def unload(name, depth=0):
            p = Path('/sys/module') / name
            if not p.exists() or name in before:
                return
            if depth > 8:
                raise RuntimeError('Unexpected module dependency depth')
            for holder in (p / 'holders').iterdir():
                if holder.name in before:
                    raise RuntimeError('Pre-existing holder prevents owned module cleanup')
                unload(holder.name, depth + 1)
            run('rmmod', name)

        if state['module_claimed']:
            unload('hci_vhci')
            unload('bluetooth')
        for address in state['addresses']:
            if not re.fullmatch(r'(?:[0-9A-F]{2}:){5}[0-9A-F]{2}', address) or address in state['storage_before']:
                raise RuntimeError('Refusing non-owned controller storage')
            p = Path('/var/lib/bluetooth') / address
            if p.is_symlink():
                raise RuntimeError('Refusing a storage symlink')
            if p.exists():
                shutil.rmtree(p)
    finally:
        if state['cleanup_mask']:
            p = Path('/run/systemd/system/bluetooth.service')
            if not p.is_symlink() or p.readlink() != Path('/dev/null'):
                raise RuntimeError('Runtime cleanup override changed; refusing to remove it')
            run('systemctl', 'unmask', '--runtime', 'bluetooth.service')
            state['cleanup_mask'] = False
            save(directory, state)
    assert not controllers() and not Path('/sys/module/bluetooth').exists()
    assert not runtime_override()
    assert subprocess.run(['systemctl', 'is-active', 'bluetooth.service'], capture_output=True, text=True).stdout.strip() == 'inactive'
    state['restored'] = True
    save(directory, state)
    print(json.dumps({'restored': True}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('start', 'info', 'stop'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('--sha256')
    args = parser.parse_args()
    directory = args.directory
    if directory.is_symlink() or directory.resolve().parent != Path('/tmp') or not directory.name.startswith('lucent-bluetooth-vhci-') or not directory.is_dir():
        raise SystemExit('Use an owned /tmp/lucent-bluetooth-vhci-* directory')
    if args.operation == 'start':
        start(directory, args.sha256 or '')
    elif args.operation == 'info':
        require_guest()
        print((directory / 'state.json').read_text())
    else:
        stop(directory)


if __name__ == '__main__':
    main()
