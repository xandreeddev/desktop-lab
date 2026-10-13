#!/usr/bin/env python3
"""Verify native pairing on the two explicitly supplied owned guest vHCI controllers.

No host Bluetooth or physical controller use. The fixture owner must run stop
and cleanup afterwards; a successful accept leaves only the owned test bond.
A separate BlueZ reference agent stays alive while native rejection/disconnect
termination is verified. Accept requires explicit visual code comparison via
stdin. Captures, full reference logs and credentials remain private; only the
owned fixture comparison code is displayed locally for review.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import threading
import time

import lab

spec = importlib.util.spec_from_file_location('bluetooth_radio', Path(__file__).with_name('test-bluetooth-radio.py'))
radio = importlib.util.module_from_spec(spec)
spec.loader.exec_module(radio)
v = radio.v


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture-directory', type=Path, required=True)
    parser.add_argument('--login-file', type=Path, required=True)
    parser.add_argument('mode', choices=('reject', 'close', 'disconnect', 'accept'))
    args = parser.parse_args()
    radio.require_fixture(args.fixture_directory, args.login_file)
    local, peer = radio.PATHS

    def prop(path, name, interface='org.bluez.Adapter1'):
        return v.remote('busctl', '--system', 'get-property', 'org.bluez', path, interface, name).strip()

    def address(path):
        return json.loads(prop(path, 'Address')[2:])

    def device_properties(path):
        managed = json.loads(v.remote('busctl', '--system', '--json=short', 'call',
                                     'org.bluez', '/', 'org.freedesktop.DBus.ObjectManager', 'GetManagedObjects'))
        return managed['data'][0].get(path, {}).get('org.bluez.Device1')

    local_address, peer_address = address(local), address(peer)
    assert {local_address, peer_address} == radio.EXPECTED_ADDRESSES
    assert v.inspect()['client']['bluetooth']['controllers'] == 2
    assert 'pairing' in v.inspect()['client']['bluetooth'], 'Install the phase-inspection client before pairing verification'
    original_open = v.inspect()['client']['bluetooth']['open']
    mode = args.mode
    output = bytearray()
    lock = threading.Lock()
    peer_process = subprocess.Popen(lab.ssh_args('lucent') + [shlex.join(['bluetoothctl', '--agent', 'KeyboardDisplay'])],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT)

    def read_output():
        while True:
            data = os.read(peer_process.stdout.fileno(), 4096)
            if not data:
                break
            with lock:
                output.extend(data)
                del output[:-65536]
                (v.REPORT / 'bluetooth-reference-peer.log').write_bytes(output)

    threading.Thread(target=read_output, daemon=True).start()

    def text():
        with lock:
            return re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', output.decode(errors='replace'))

    def command(value):
        peer_process.stdin.write((value + '\n').encode())
        peer_process.stdin.flush()

    def state():
        return v.inspect()['client']['bluetooth']

    def hits():
        return [h['id'] for s in v.inspect()['surfaces'] if s['id'] == 'bluetooth' for h in s['hits']]

    def visible_click(identifier):
        for _ in range(len(hits()) + 2):
            panel = next(s for s in v.inspect()['surfaces'] if s['id'] == 'bluetooth')
            hit = next(h for h in panel['hits'] if h['id'] == identifier)
            clip = hit['clip']
            if hit['y'] >= clip['y'] and hit['y'] + hit['height'] <= clip['y'] + clip['height'] + .01:
                v.click(identifier)
                return
            v.key(15)
        raise RuntimeError('Native device control was not reachable')

    try:
        v.eventually(lambda: 'Agent registered' in text(), 'Reference peer agent did not register')
        command('select ' + peer_address)
        v.eventually(lambda: 'default' in text() and peer_address in text(), 'Reference controller not selected')
        command('power on')
        command('pairable on')
        command('discoverable on')
        command('default-agent')
        v.eventually(lambda: prop(peer, 'Pairable') == 'b true' and prop(peer, 'Discoverable') == 'b true',
                     'Reference peer radio did not become pairable/discoverable')
        v.eventually(lambda: 'Default agent request successful' in text(), 'Reference agent not default')
        v.cli('bluetooth', 'open')
        v.click('bluetooth-tab-devices')
        controllers = [h for h in hits() if h.startswith('bluetooth-controller-')]
        assert len(controllers) == 2
        v.click(controllers[0])
        if prop(local, 'Powered') != 'b true':
            v.click('bluetooth-power')
            v.eventually(lambda: prop(local, 'Powered') == 'b true', 'Selected native radio did not power on')
            v.eventually(lambda: not state()['busy'], 'Native power command did not finish')
        v.click('bluetooth-discovery')
        v.eventually(lambda: any(h.startswith('bluetooth-pair-') for h in hits()), 'No owned peer in native discovery', timeout=30)
        targets = [h for h in hits() if h.startswith('bluetooth-pair-')]
        assert len(targets) == 1, targets
        v.click(targets[0])
        v.eventually(lambda: state()['requests'], 'Native pairing prompt did not appear', timeout=30)
        v.eventually(lambda: re.search(r'Confirm passkey (\d{6})', text()), 'Reference confirmation did not appear', timeout=15)
        code = re.search(r'Confirm passkey (\d{6})', text()).group(1)
        request = state()['requests'][0]['id']
        device = local + '/dev_' + peer_address.replace(':', '_')
        if mode != 'accept':
            if mode == 'reject':
                v.click('bluetooth-decline-' + str(request))
            elif mode == 'close':
                v.key(1)
                v.eventually(lambda: not state()['visible'], 'Escape did not close the active native prompt')
            else:
                visible_click('bluetooth-connect-' + str(state()['requests'][0]['device']))
            v.eventually(lambda: not state()['requests'] and not state()['answering'], 'Retired prompt remained reachable', timeout=15)
            v.eventually(lambda: state()['pairing'] is not None and state()['pairing']['phase'] in ('cancelled', 'failed'),
                         'Owned pairing method did not reach observed termination', timeout=15)
            assert peer_process.poll() is None, 'Reference peer must remain alive during terminal observation'
            properties = device_properties(device)
            assert properties is None or not properties['Paired']['data']
            assert properties is None or not properties['Trusted']['data']
            if mode == 'close':
                time.sleep(1)
                assert not state()['open'] and not state()['requests'], 'Late prompt reopened the closed panel'
            v.screenshot('bluetooth-virtual-' + mode + '.png')
            checks = {'native_' + mode: True, 'prompt_retired': True, 'not_paired': True,
                      'not_trusted': True, 'link_connected': properties is not None and properties['Connected']['data'],
                      'device_retired': properties is None, 'pairing_observed_phase': state()['pairing']['phase'],
                      'peer_still_running_at_terminal_observation': peer_process.poll() is None,
                      'physical_hardware': False}
            (v.REPORT / ('bluetooth-vhci-' + mode + '.json')).write_text(json.dumps(checks, indent=2) + '\n')
            print(json.dumps(checks), flush=True)
            sys.exit(0)
        v.screenshot('bluetooth-virtual-confirmation.png')
        print(json.dumps({'step': 'compare_owned_virtual_codes', 'reference_code': code,
                          'request': request, 'private_screenshot': 'reports/local/bluetooth-virtual-confirmation.png'}), flush=True)
        entered = input('Enter the matching six digits after inspecting the native prompt: ').strip()
        if entered != code:
            raise RuntimeError('Visual comparison was not verified; refusing acceptance')
        v.click('bluetooth-accept-' + str(request))
        command('yes')
        v.eventually(lambda: prop(device, 'Paired', 'org.bluez.Device1') == 'b true',
                     'BlueZ did not complete pairing', timeout=30)
        v.eventually(lambda: not state()['requests'] and not state()['busy'] and state()['pairing']['phase'] == 'succeeded', 'Native pairing did not reach observed success', timeout=15)
        checks = {'controllers': 2, 'native_discovery': True, 'native_confirmation': True,
                  'visual_code_comparison': True, 'bluez_paired': True,
                  'bluez_bonded': prop(device, 'Bonded', 'org.bluez.Device1') == 'b true',
                  'automatic_trust': prop(device, 'Trusted', 'org.bluez.Device1') == 'b true',
                  'pairing_observed_phase': state()['pairing']['phase'],
                  'prompts_retired': not state()['requests'], 'physical_hardware': False,
                  'profile_interoperability_verified': False}
        assert not checks['automatic_trust'], checks
        v.screenshot('bluetooth-virtual-paired.png')
        (v.REPORT / 'bluetooth-vhci-pairing.json').write_text(json.dumps(checks, indent=2) + '\n')
        print(json.dumps(checks), flush=True)
    finally:
        try:
            command('quit')
            peer_process.stdin.close()
            peer_process.wait(timeout=8)
        except (BrokenPipeError, subprocess.TimeoutExpired):
            peer_process.terminate()
            peer_process.wait(timeout=5)
        v.cli('bluetooth', 'close')
        v.eventually(lambda: not radio.property_value(local, 'Discovering'), 'Owned discovery did not stop')
        if original_open:
            v.cli('bluetooth', 'open')


if __name__ == '__main__':
    main()
