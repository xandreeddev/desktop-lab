#!/usr/bin/env python3
"""Production BluetoothPort observation check in the existing Lucent guest.

Build bluetooth-service with the VM off first. This copies the consumer to an
owned temporary directory, runs as the regular SSH user against the system bus,
and removes only that directory. No Bluetooth controls, service restarts,
pairing agents, policy rules or desktop changes are performed. --start-service
may start an inactive BlueZ daemon for the fixture and restores that state.
--load-module may load its absent kernel prerequisite and removes it afterward.
Device metadata
is parsed locally and is never included in the report or terminal output.
"""
import argparse
import hashlib
import json
from pathlib import Path
import queue
import shlex
import subprocess
import threading
import time

import lab

PROFILE = 'lucent'


def remote(*args):
    return subprocess.check_output(lab.ssh_args(PROFILE) + [shlex.join(args)], text=True, timeout=15)


def counts(state):
    return {'adapters': len(state['adapters']), 'devices': len(state['devices']),
            'connected': sum(device['connected'] for device in state['devices']),
            'pairing_requests': len(state['pairing_requests'])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--start-service', action='store_true')
    parser.add_argument('--load-module', action='store_true')
    parser.add_argument('--login-file', type=Path)
    args = parser.parse_args()
    binary = lab.ROOT / 'lucent/target/release/bluetooth-service'
    expected = hashlib.sha256(binary.read_bytes()).hexdigest()
    directory = remote('mktemp', '-d', '/tmp/lucent-bluetooth.XXXXXXXX').strip()
    if not directory.startswith('/tmp/lucent-bluetooth.') or '/' in directory.removeprefix('/tmp/'):
        raise RuntimeError('Unexpected owned fixture directory')
    child = None
    reader = None
    result = {}
    service_started = False
    module_loaded = False
    cleanup_mask = False
    original_module = None
    original_modules = set()

    def module_present():
        return remote('python3', '-c', 'from pathlib import Path; print(int(Path("/sys/module/bluetooth").exists()))').strip() == '1'

    def privileged(*command):
        if args.login_file is None:
            raise RuntimeError('--start-service needs the existing private guest login file')
        secret = next(line.split(':', 1)[1].strip() for line in args.login_file.read_text().splitlines()
                      if line.startswith('Password:'))
        run = subprocess.run(lab.ssh_args(PROFILE) + [shlex.join([
            'sudo', '-S', '-p', '', *command
        ])], input=secret + '\n', text=True, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, timeout=15)
        if run.returncode:
            raise RuntimeError('Owned guest BlueZ fixture operation failed')

    def service(action):
        privileged('systemctl', action, 'bluetooth.service')

    try:
        original_module = module_present()
        original_modules = set(json.loads(remote('python3', '-c', 'from pathlib import Path; import json; print(json.dumps([p.name for p in Path("/sys/module").iterdir()]))')))
        executable = directory + '/bluetooth-service'
        with binary.open('rb') as stream:
            subprocess.run(lab.ssh_args(PROFILE) + [
                'umask 077; cat > ' + shlex.quote(executable) + ' && chmod 700 ' + shlex.quote(executable)
            ], stdin=stream, check=True, timeout=30)
        actual = remote('sha256sum', executable).split()[0]
        if actual != expected:
            raise RuntimeError('Guest binary differs from built consumer')
        version = remote('pacman', '-Q', 'bluez').strip()
        original_status = subprocess.run(lab.ssh_args(PROFILE) + [
            shlex.join(['systemctl', 'is-active', 'bluetooth.service'])
        ], capture_output=True, text=True, timeout=15).stdout.strip()
        if original_status != 'active':
            if not args.start_service or original_status != 'inactive':
                raise RuntimeError('Guest BlueZ must be active, or inactive with explicit --start-service')
            if not original_module:
                if not args.load_module:
                    raise RuntimeError('Guest needs explicit --load-module for the absent kernel prerequisite')
                module_loaded = True
                privileged('modprobe', 'bluetooth')
            # Claim cleanup before attempting a potentially ambiguous start.
            service_started = True
            service('start')
        observed = subprocess.run(lab.ssh_args(PROFILE) + [shlex.join([
            'timeout', '15s', executable, '--observe', '--seconds', '3'
        ])], capture_output=True, text=True, timeout=20, check=True)
        events = [json.loads(line) for line in observed.stdout.splitlines()]
        if not events or any(event['kind'] != 'state' for event in events) or observed.stderr:
            (lab.ROOT / 'reports/local/bluetooth-observation-failed.json').write_text(
                json.dumps({'events': events, 'stderr': observed.stderr}, indent=2) + '\n')
            raise RuntimeError('Real BlueZ count observation failed')
        for event in events:
            value = event['value']
            if set(value) != {'adapters', 'devices', 'connected', 'pairing_requests'} or any(
                    not isinstance(number, int) or number < 0 for number in value.values()):
                raise RuntimeError('Observation exposed metadata or invalid counts')
        result['observation'] = events[-1]['value']

        incoming = queue.Queue(maxsize=16)
        child = subprocess.Popen(lab.ssh_args(PROFILE) + [
            'exec ' + shlex.join(['timeout', '15s', executable, '--stdio'])
        ], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)

        def read():
            try:
                for line in child.stdout:
                    incoming.put(json.loads(line), timeout=1)
            except (ValueError, queue.Full):
                incoming.put({'kind': 'reader_failed'}, timeout=1)
            finally:
                incoming.put({'kind': 'ended'}, timeout=1)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        first = incoming.get(timeout=6)
        if first['kind'] != 'state':
            raise RuntimeError('Detailed consumer did not observe the real service')
        result['stdio_observation'] = counts(first['value'])
        # Closing input cancels observation without sending any control command.
        started = time.monotonic()
        child.stdin.close()
        if child.wait(timeout=2) != 0:
            raise RuntimeError('Consumer cancellation failed')
        result['eof_exit_seconds'] = round(time.monotonic() - started, 3)
        reader.join(timeout=2)
        if reader.is_alive():
            raise RuntimeError('Consumer reader did not stop')
        result.update(binary_sha256=expected, bluez_package=version,
                      binary_transfer_verified=True, regular_user_system_bus=True,
                      controls_sent=0, policy_changed=False,
                      service_temporarily_started=service_started,
                      kernel_module_temporarily_loaded=module_loaded,
                      pairing_agents_registered=False, metadata_published=False,
                      scope='Read-only actual BlueZ observation and executable cancellation; no device/profile/pairing acceptance')
        # Keep partial measurements private if owned-fixture cleanup later fails.
        (lab.ROOT / 'reports/local/bluetooth-native-progress.json').write_text(json.dumps(result, indent=2) + '\n')
    finally:
        if child is not None and child.poll() is None:
            if not child.stdin.closed:
                child.stdin.close()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=3)
        if reader is not None:
            reader.join(timeout=2)
        try:
            if service_started:
                if module_loaded:
                    # A logged-in PipeWire/BlueZ client may reactivate the
                    # daemon immediately after stop, retaining BNEP references.
                    existing = remote('python3', '-c',
                        'from pathlib import Path; p=Path("/run/systemd/system/bluetooth.service"); print(int(p.exists() or p.is_symlink()))').strip()
                    if existing != '0':
                        raise RuntimeError('Existing runtime Bluetooth override is not ours')
                    cleanup_mask = True
                    privileged('systemctl', 'mask', '--runtime', 'bluetooth.service')
                service('stop')
        finally:
            try:
                if module_loaded and module_present():
                    controllers = remote('python3', '-c', 'from pathlib import Path; print(len(list(Path("/sys/class/bluetooth").glob("hci*"))))').strip()
                    if controllers != '0':
                        raise RuntimeError('Unexpected controller arrived; refusing to unload its module')
                    removed = []

                    def unload(module, depth=0):
                        if module in original_modules or depth > 8:
                            raise RuntimeError('Refusing to remove a pre-existing or unexpected kernel dependency')
                        holders = json.loads(remote('python3', '-c',
                            'from pathlib import Path; import json,sys; print(json.dumps([p.name for p in (Path("/sys/module")/sys.argv[1]/"holders").iterdir()]))', module))
                        for holder in holders:
                            unload(holder, depth + 1)
                        # BlueZ may auto-load BNEP. Remove only new holders; never force.
                        privileged('rmmod', module)
                        removed.append(module)

                    unload('bluetooth')
                    result['owned_bluetooth_modules_removed'] = removed
            finally:
                try:
                    remote('rm', '-rf', '--', directory)
                finally:
                    if cleanup_mask:
                        owned = remote('python3', '-c',
                            'from pathlib import Path; p=Path("/run/systemd/system/bluetooth.service"); print(int(p.is_symlink() and p.readlink()==Path("/dev/null")))').strip()
                        if owned != '1':
                            raise RuntimeError('Runtime Bluetooth override changed; refusing to remove it')
                        privileged('systemctl', 'unmask', '--runtime', 'bluetooth.service')
                        result['owned_runtime_cleanup_mask_removed'] = True
    restored = subprocess.run(lab.ssh_args(PROFILE) + [
        shlex.join(['systemctl', 'is-active', 'bluetooth.service'])
    ], capture_output=True, text=True, timeout=15).stdout.strip()
    if restored != original_status:
        raise RuntimeError('Guest BlueZ service state was not restored')
    result['original_service_state_restored'] = True
    if module_present() != original_module:
        raise RuntimeError('Guest Bluetooth module state was not restored')
    result['original_kernel_module_state_restored'] = True
    result['owned_temporary_binary_removed'] = True
    output = lab.ROOT / 'reports/local/bluetooth-native.json'
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
