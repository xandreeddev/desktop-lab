#!/usr/bin/env python3
"""Owned virtual Wi-Fi fixture, run as root INSIDE the Lucent guest only.

Refuses existing Wi-Fi hardware or a loaded hwsim module. Creates two virtual
radios, a WPA2 AP and DHCP on a private benchmark subnet. Ethernet, routes,
existing profiles, system services and the global networking switch are untouched.
Private state contains a generated fixture password and is never a public report.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import time


def run(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL).strip()


def interfaces():
    return sorted(p.name for p in Path('/sys/class/net').iterdir() if (p / 'wireless').exists())


def save(directory, state):
    temporary = directory / 'state.tmp'
    temporary.write_text(json.dumps(state))
    temporary.chmod(0o600)
    temporary.replace(directory / 'state.json')


def launch(directory, state, name, args):
    with (directory / (name + '.log')).open('ab') as log:
        child = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    state['children'].append({'pid': child.pid, 'start': Path(f'/proc/{child.pid}/stat').read_text().split()[21]})
    save(directory, state)
    time.sleep(.3)
    if child.poll() is not None:
        raise RuntimeError('Virtual AP/DHCP fixture exited; inspect its private log')


def terminate(child):
    proc = Path(f'/proc/{child["pid"]}/stat')
    if proc.exists() and proc.read_text().split()[21] == child['start']:
        try:
            os.killpg(child['pid'], signal.SIGTERM)
        except ProcessLookupError:
            pass


def start(directory):
    if os.geteuid() != 0 or not Path('/etc/arch-release').exists() or run('systemd-detect-virt', '--vm') not in ('kvm', 'qemu'):
        raise SystemExit('This fixture requires root in the prepared Arch/Omarchy VM')
    if interfaces() or Path('/sys/module/mac80211_hwsim').exists():
        raise SystemExit('Refusing to change an existing Wi-Fi device or hwsim fixture')
    if (directory / 'state.json').exists():
        raise SystemExit('Fixture state already exists')
    os.chown(directory, 0, 0)
    directory.chmod(0o700)
    for program in ('hostapd', 'dnsmasq', 'iw'):
        run('sh', '-c', 'command -v "$1"', 'fixture', program)
    state = {'wifi_enabled': run('nmcli', 'radio', 'wifi') == 'enabled',
             'module': False, 'children': [], 'ssid': 'Lucent-test-' + secrets.token_hex(5),
             'password': secrets.token_hex(16), 'profile_keys': [],
             'original_profiles': run('nmcli', '-g', 'UUID', 'connection', 'show').splitlines()}
    save(directory, state)
    try:
        run('modprobe', 'mac80211_hwsim', 'radios=2')
        state['module'] = True
        save(directory, state)
        radios = interfaces()
        assert len(radios) == 2, 'Expected exactly two newly created virtual radios'
        station, ap = radios
        state.update(station=station, ap=ap)
        save(directory, state)
        run('nmcli', 'radio', 'wifi', 'on')
        run('nmcli', 'device', 'set', ap, 'managed', 'no')
        deadline = time.monotonic() + 10
        while not run('nmcli', '-g', 'GENERAL.STATE', 'device', 'show', ap).startswith('10 '):
            if time.monotonic() >= deadline:
                raise RuntimeError('NetworkManager did not release the owned AP interface')
            time.sleep(.1)
        # The property acknowledgement precedes asynchronous supplicant cleanup.
        # Wait for its disconnect before another nl80211 owner starts the AP.
        time.sleep(.5)
        namespace = directory.name
        run('ip', 'netns', 'add', namespace)
        state['namespace'] = namespace
        save(directory, state)
        phy = Path('/sys/class/net', ap, 'phy80211').resolve().name
        run('iw', 'phy', phy, 'set', 'netns', 'name', namespace)
        run('ip', '-n', namespace, 'link', 'set', 'lo', 'up')
        run('ip', '-n', namespace, 'address', 'add', '198.18.0.1/24', 'dev', ap)
        run('ip', '-n', namespace, 'link', 'set', ap, 'up')
        config = directory / 'hostapd.conf'
        config.write_text(f'interface={ap}\ndriver=nl80211\nssid={state["ssid"]}\nhw_mode=g\nchannel=1\nwpa=2\nwpa_key_mgmt=WPA-PSK\nwpa_passphrase={state["password"]}\nrsn_pairwise=CCMP\n')
        config.chmod(0o600)
        dhcp = directory / 'dnsmasq.conf'
        dhcp.write_text(f'port=0\ninterface={ap}\nbind-interfaces\nlog-dhcp\ndhcp-range=198.18.0.20,198.18.0.30,255.255.255.0,5m\ndhcp-option=3,198.18.0.1\ndhcp-option=6,198.18.0.1\ndhcp-leasefile={directory}/leases\n')
        for name, args in [('hostapd', ['hostapd', str(config)]), ('dnsmasq', ['dnsmasq', '--no-daemon', '--conf-file=' + str(dhcp)])]:
            launch(directory, state, name, ['ip', 'netns', 'exec', namespace, *args])
        deadline = time.monotonic() + 10
        while 'AP-ENABLED' not in (directory / 'hostapd.log').read_text():
            if time.monotonic() >= deadline:
                raise RuntimeError('Owned access point never became enabled')
            time.sleep(.1)
        if 'type AP' not in run('ip', 'netns', 'exec', namespace, 'iw', 'dev', ap, 'info'):
            raise RuntimeError('Another owner changed the AP interface mode')
        run('nmcli', 'device', 'set', station, 'managed', 'yes')
        print(json.dumps({'ready': True}))
    except BaseException:
        stop(directory)
        raise


def stop(directory):
    state = json.loads((directory / 'state.json').read_text())
    # Activation can fail before the harness registers its accepted profile.
    # Also recover newly created profiles for this exact owned SSID, preserving
    # all pre-fixture UUIDs and unrelated profiles created concurrently.
    owned = set(state['profile_keys'])
    for key in set(run('nmcli', '-g', 'UUID', 'connection', 'show').splitlines()) - set(state['original_profiles']) - owned:
        try:
            if run('nmcli', '-g', '802-11-wireless.ssid', 'connection', 'show', 'uuid', key) == state['ssid']:
                owned.add(key)
        except subprocess.CalledProcessError:
            pass
    for key in owned:
        subprocess.run(['nmcli', 'connection', 'delete', 'uuid', key], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    for child in reversed(state['children']):
        terminate(child)
    time.sleep(.2)
    if state.get('namespace'):
        run('ip', 'netns', 'delete', state['namespace'])
    if state['module']:
        run('modprobe', '-r', 'mac80211_hwsim')
    run('nmcli', 'radio', 'wifi', 'on' if state['wifi_enabled'] else 'off')
    assert not interfaces() and not Path('/sys/module/mac80211_hwsim').exists()
    # Keep private logs for diagnosis, but erase the credential-bearing files.
    for name in ('hostapd.conf', 'state.json', 'state.tmp'):
        (directory / name).unlink(missing_ok=True)
    print(json.dumps({'restored': True}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('start', 'info', 'register', 'wrong-secret', 'clear-secret', 'verify-secret', 'hide', 'stop'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('profile', nargs='?')
    args = parser.parse_args()
    directory = args.directory.resolve()
    if not directory.is_dir() or not directory.name.startswith('lucent-network-') or directory.parent != Path('/tmp'):
        raise SystemExit('Use an owned /tmp/lucent-network-* directory')
    if args.operation == 'start':
        start(directory)
    elif args.operation == 'info':
        print((directory / 'state.json').read_text())
    elif args.operation == 'register':
        if not args.profile or len(args.profile) != 36 or any(c not in '0123456789abcdef-' for c in args.profile):
            raise SystemExit('Expected a profile UUID')
        state = json.loads((directory / 'state.json').read_text())
        state['profile_keys'].append(args.profile)
        save(directory, state)
    elif args.operation in ('wrong-secret', 'clear-secret', 'verify-secret'):
        # Mutate/read only a profile created and explicitly registered by this
        # fixture. Never touch a host/guest user's pre-existing connection.
        state = json.loads((directory / 'state.json').read_text())
        key = args.profile
        if not key or key not in state['profile_keys'] or key in state['original_profiles']:
            raise SystemExit("Credential testing requires this fixture's registered new profile")
        if run('nmcli', '-g', '802-11-wireless.ssid', 'connection', 'show', 'uuid', key) != state['ssid']:
            raise SystemExit('Owned profile changed its network identity')
        if args.operation == 'verify-secret':
            # The actual credential stays inside this root fixture process;
            # stdout contains only a comparison result, never secret material.
            stored = run('nmcli', '--show-secrets', '-g', '802-11-wireless-security.psk', 'connection', 'show', 'uuid', key)
            print(json.dumps({'stored_secret_matches': stored == state['password']}))
        else:
            active = run('nmcli', '-g', 'GENERAL.CON-UUID', 'device', 'show', state['station'])
            if active == key:
                raise SystemExit('Disconnect this owned profile before invalidating its credentials')
            # A public, synthetic invalid fixture value is not the real secret.
            # Clearing exercises an immediately missing system-owned password.
            replacement = 'invalid-lucent-fixture-only' if args.operation == 'wrong-secret' else ''
            run('nmcli', 'connection', 'modify', 'uuid', key,
                '802-11-wireless-security.psk', replacement,
                '802-11-wireless-security.psk-flags', '0')
            print(json.dumps({'owned_profile_credentials_invalidated': True}))
    elif args.operation == 'hide':
        state = json.loads((directory / 'state.json').read_text())
        terminate(state['children'][0])
        time.sleep(.3)
        config = directory / 'hostapd.conf'
        config.write_text(config.read_text() + '\nignore_broadcast_ssid=1\n')
        launch(directory, state, 'hostapd', ['ip', 'netns', 'exec', state['namespace'], 'hostapd', str(config)])
        print(json.dumps({'hidden': True}))
    else:
        stop(directory)


if __name__ == '__main__':
    main()
