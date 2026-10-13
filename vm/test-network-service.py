#!/usr/bin/env python3
"""Validate the production NetworkPort adapter against guest NetworkManager.

Run with the capped Lucent VM and an already built network-service binary.
The guest needs hostapd, dnsmasq, iw and mac80211_hwsim. Root is used only for
the owned fixture; commands run as the regular guest user over the system bus.
The desktop, Ethernet connection and wallpaper are preserved. Raw local network
metadata and the generated fixture secret are never printed or put in reports.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import threading
import time
import uuid
import lab

PROFILE = 'lucent'


def remote(*args):
    return subprocess.check_output(lab.ssh_args(PROFILE) + [shlex.join(args)], text=True)


class Probe:
    def __init__(self, binary):
        self.unit = 'lucent-network-test-' + uuid.uuid4().hex
        executable = remote('realpath', binary).strip()
        command = ['python3', 'desktop-lab/scripts/in-session.py', 'systemd-run', '--user', '--pipe', '--wait', '--collect', '--quiet', '--unit=' + self.unit, '--property=MemoryMax=256M', '--property=TasksMax=64', executable, '--stdio']
        self.child = subprocess.Popen(lab.ssh_args(PROFILE) + ['exec ' + shlex.join(command)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.condition = threading.Condition()
        self.state = None
        self.results = {}
        self.ended = False
        self.number = 0
        self.failures = []
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        try:
            for line in self.child.stdout:
                event = json.loads(line)
                with self.condition:
                    if event['event'] == 'state':
                        self.state = event['snapshot']
                    elif event['event'] == 'result':
                        self.results[event.get('request_id')] = event
                    elif event['event'] == 'unavailable':
                        self.state = None
                    self.condition.notify_all()
        finally:
            with self.condition:
                self.ended = True
                self.condition.notify_all()

    def until(self, predicate, description, timeout=35):
        deadline = time.monotonic() + timeout
        with self.condition:
            while True:
                if self.state is not None and predicate(self.state):
                    return self.state
                if self.ended or time.monotonic() >= deadline:
                    raise RuntimeError(description)
                self.condition.wait(min(.2, deadline - time.monotonic()))

    def command(self, command, expect=True):
        with self.condition:
            self.number += 1
            number = self.number
            self.child.stdin.write(json.dumps(dict(command, request_id=number)) + '\n')
            self.child.stdin.flush()
            deadline = time.monotonic() + 7
            while number not in self.results:
                if self.ended or time.monotonic() >= deadline:
                    raise RuntimeError('Production adapter command was not acknowledged')
                self.condition.wait(.1)
            result = self.results.pop(number)
            if not result['ok']:
                self.failures.append({'error': result['error'], 'snapshot': self.state})
            if expect and not result['ok']:
                raise RuntimeError(result['error'])
            return result['ok']

    def close(self):
        self.child.stdin.close()
        try:
            self.child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            remote('python3', 'desktop-lab/scripts/in-session.py', 'systemctl', '--user', 'stop', self.unit)
            self.child.terminate()
            self.child.wait(timeout=3)
            raise RuntimeError('Network subscription failed to stop after EOF')
        self.reader.join(timeout=1)
        assert self.child.returncode == 0, 'Network service example exited unsuccessfully'
        result = subprocess.run(lab.ssh_args(PROFILE) + [shlex.join(['python3', 'desktop-lab/scripts/in-session.py', 'systemctl', '--user', 'is-active', self.unit])], capture_output=True, text=True)
        assert result.returncode != 0, 'Owned network-service unit remained active'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--login-file', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=lab.ROOT / 'lucent/target/release/network-service')
    args = parser.parse_args()
    password = next(line.split(':', 1)[1].strip() for line in args.login_file.read_text().splitlines() if line.startswith('Password:'))

    def root(*command):
        return subprocess.check_output(lab.ssh_args(PROFILE) + ['sudo -S -p "" ' + shlex.join(command)], input=password + '\n', text=True, stderr=subprocess.DEVNULL)

    assert remote('systemd-detect-virt', '--vm').strip() in ('kvm', 'qemu')
    assert remote('systemctl', 'is-active', 'NetworkManager').strip() == 'active'
    before = remote('nmcli', '-t', '-f', 'UUID,NAME,TYPE', 'connection', 'show').splitlines()
    routes = json.loads(remote('ip', '-j', 'route', 'show', 'default'))
    desktop_pid = remote('pgrep', '-x', 'lucent-desktop').strip()
    tag = uuid.uuid4().hex
    directory = '/tmp/lucent-network-' + tag
    stage = '.cache/lucent-network-' + tag
    remote('mkdir', '-m', '700', directory, stage)
    fixture = (Path(__file__).with_name('network_fixture.py')).read_bytes()
    subprocess.run(lab.ssh_args(PROFILE) + ['cat > ' + shlex.quote(directory + '/fixture.py')], input=fixture, check=True)
    data = args.binary.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    subprocess.run(lab.ssh_args(PROFILE) + ['cat > ' + shlex.quote(stage + '/network-service')], input=data, check=True)
    remote('chmod', '700', stage + '/network-service')
    assert remote('sha256sum', stage + '/network-service').split()[0] == digest
    probe = None
    started = False
    checks = []
    try:
        probe = Probe(stage + '/network-service')
        initial = probe.until(lambda s: bool(s['devices']), 'Initial system-bus observation unavailable')
        assert not any(d['kind'] == 'Wifi' for d in initial['devices']), 'VM already has a Wi-Fi device'
        root('python3', directory + '/fixture.py', 'start', directory)
        started = True
        state = json.loads(root('python3', directory + '/fixture.py', 'info', directory))
        observed = probe.until(lambda s: any(d['interface'] == state['station'] and d['state'] == 'Disconnected' for d in s['devices']), 'Virtual Wi-Fi did not become ready after hotplug')
        device = next(d for d in observed['devices'] if d['interface'] == state['station'])
        checks.append('system_bus_initial_state_and_virtual_wifi_hotplug')
        # NM may reject a scan while automatic discovery is already in progress.
        for _ in range(5):
            observed = probe.until(lambda s: any(d['interface'] == state['station'] and d['state'] == 'Disconnected' for d in s['devices']), 'Wi-Fi was unavailable before scan')
            device = next(d for d in observed['devices'] if d['interface'] == state['station'])
            if probe.command({'command': 'scan', 'device': device['id']}, expect=False):
                break
            time.sleep(3)
        else:
            diagnostics = subprocess.run(lab.ssh_args(PROFILE) + [shlex.join(['nmcli', 'device', 'wifi', 'rescan', 'ifname', state['station']])], capture_output=True, text=True)
            (lab.ROOT / 'reports/local/network-scan-diagnostic.log').write_text(diagnostics.stdout + diagnostics.stderr)
            raise RuntimeError('Guest NetworkManager never accepted a scan request')
        observed = probe.until(lambda s: any(d['id'] == device['id'] and d['scan_generation'] is not None for d in s['devices']), 'Completed scan generation not observed')
        ssid = list(state['ssid'].encode())
        observed = probe.until(lambda s: any(a['device'] == device['id'] and a['ssid'] == ssid for a in s['access_points']), 'WPA2 AP was not discovered')
        ap = next(a for a in observed['access_points'] if a['device'] == device['id'] and a['ssid'] == ssid)
        assert ap['security'] == 'WpaPersonal'
        checks.append('scan_completion_and_real_wpa2_discovery')
        probe.command({'command': 'join', 'access_point': ap['id'], 'security': 'wpa', 'password': state['password'], 'remember': True})
        observed = probe.until(lambda s: any(p['ssid'] == ssid for p in s['profiles']), 'Accepted Wi-Fi profile not observed')
        profile = next(p for p in observed['profiles'] if p['ssid'] == ssid)
        root('python3', directory + '/fixture.py', 'register', directory, profile['key'])

        def connected(s):
            return any(d['id'] == device['id'] and d['state'] == 'Connected' for d in s['devices']) and any(c['state'] == 'Connected' and device['id'] in c['devices'] for c in s['connections'])

        observed = probe.until(connected, 'WPA2 association/address setup did not complete')
        assert state['ssid'] in remote('iw', 'dev', state['station'], 'link')
        assert '198.18.0.' in remote('nmcli', '-g', 'IP4.ADDRESS', 'device', 'show', state['station'])
        checks.append('regular_user_wpa2_join_and_independent_association_dhcp')
        connection = next(c for c in observed['connections'] if device['id'] in c['devices'])
        probe.command({'command': 'disconnect', 'connection': connection['id']})
        observed = probe.until(lambda s: any(d['id'] == device['id'] and d['state'] == 'Disconnected' for d in s['devices']) and not any(device['id'] in c['devices'] for c in s['connections']), 'Targeted disconnect did not finish')
        probe.command({'command': 'activate', 'profile': profile['id'], 'device': device['id']})
        probe.until(connected, 'Saved-profile reactivation did not complete')
        assert remote('nmcli', '-g', 'GENERAL.CON-UUID', 'device', 'show', state['station']).strip() == profile['key']
        probe.command({'command': 'forget', 'profile': profile['id']})
        probe.until(lambda s: any(d['id'] == device['id'] and d['state'] == 'Disconnected' for d in s['devices']) and not any(device['id'] in c['devices'] for c in s['connections']) and not any(p['key'] == profile['key'] for p in s['profiles']), 'Forget did not remove the active profile and finish disconnecting')
        checks.append('targeted_disconnect_saved_reactivation_and_forget')
        root('python3', directory + '/fixture.py', 'hide', directory)
        probe.command({'command': 'hidden', 'device': device['id'], 'ssid': state['ssid'], 'security': 'wpa', 'password': state['password'], 'remember': False})
        observed = probe.until(connected, 'Hidden WPA2 association did not complete')
        hidden = next(p for p in observed['profiles'] if p['ssid'] == ssid)
        root('python3', directory + '/fixture.py', 'register', directory, hidden['key'])
        assert remote('nmcli', '-g', '802-11-wireless.hidden', 'connection', 'show', 'uuid', hidden['key']).strip() == 'yes'
        assert state['ssid'] in remote('iw', 'dev', state['station'], 'link')
        checks.append('hidden_wpa2_join_and_volatile_profile')
        connection = next(c for c in observed['connections'] if device['id'] in c['devices'])
        probe.command({'command': 'disconnect', 'connection': connection['id']})
        probe.until(lambda s: not any(p['key'] == hidden['key'] for p in s['profiles']), 'Volatile profile survived disconnect')
        # Only the Wi-Fi radio is toggled; Ethernet/default route stay present.
        probe.command({'command': 'wifi', 'enabled': False})
        probe.until(lambda s: not s['wifi_enabled'], 'Wi-Fi disable not observed')
        probe.command({'command': 'wifi', 'enabled': True})
        probe.until(lambda s: s['wifi_enabled'], 'Wi-Fi enable not observed')
        checks.append('wifi_radio_toggle_without_global_network_disable')
        root('python3', directory + '/fixture.py', 'stop', directory)
        started = False
        probe.until(lambda s: not any(d['kind'] == 'Wifi' for d in s['devices']), 'Owned device removal not observed')
        assert not probe.command({'command': 'scan', 'device': device['id']}, expect=False), 'Removed device handle was accepted'
        checks.append('device_removal_and_stale_handle_rejection')
    except BaseException:
        if probe:
            (lab.ROOT / 'reports/local/network-native-failure.json').write_text(json.dumps({'snapshot': probe.state, 'failures': probe.failures}, indent=2))
        if started:
            # Private diagnostic logs only; never read the credential-bearing
            # state/config files into a log or the public report.
            (lab.ROOT / 'reports/local/network-ap-diagnostic.log').write_text(root('tail', '-100', directory + '/hostapd.log'))
            (lab.ROOT / 'reports/local/network-radio-diagnostic.log').write_text(root('iw', 'dev'))
            (lab.ROOT / 'reports/local/network-dhcp-diagnostic.log').write_text(root('tail', '-100', directory + '/dnsmasq.log'))
        raise
    finally:
        if started:
            root('python3', directory + '/fixture.py', 'stop', directory)
        if probe:
            probe.close()
        remote('rm', '-r', stage)
        # Known private fixture directory only; root-owned after creation.
        root('rm', '-r', directory)
    assert sorted(remote('nmcli', '-t', '-f', 'UUID,NAME,TYPE', 'connection', 'show').splitlines()) == sorted(before), 'Existing profiles were not restored'
    assert json.loads(remote('ip', '-j', 'route', 'show', 'default')) == routes, 'Ethernet/default route changed'
    assert remote('pgrep', '-x', 'lucent-desktop').strip() == desktop_pid, 'Desktop process changed during the service test'
    checks.append('original_profiles_routes_desktop_and_fixture_cleanup_verified')
    report = {'checks': checks, 'binary_sha256': digest, 'scope': 'real guest NetworkManager; simulated nl80211 radios, WPA2/hidden association and DHCP; regular-user service commands from a bounded user unit matching desktop permissions. No native panel, physical Wi-Fi, enterprise/SAE/OWE, captive portal or suspend proof.'}
    destination = lab.ROOT / 'reports/local/lucent-network-native.json'
    destination.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
