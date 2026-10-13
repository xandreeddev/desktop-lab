#!/usr/bin/env python3
"""Native radio and foreign-property observation checks on an owned vHCI fixture.

Requires bluetooth_fixture.py to have started two isolated guest controllers.
Refuses hardware addresses, active scans, retired fixtures and other modal UI.
Restores radio flags and panel state, keeping captures and credentials private.
The fixture owner remains responsible for stop/cleanup. No host Bluetooth access.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import shlex
import subprocess

import lab

spec = importlib.util.spec_from_file_location('bluetooth_radio', Path(__file__).with_name('test-framework.py'))
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)

EXPECTED_ADDRESSES = {'00:AA:01:00:00:00', '00:AA:01:01:00:01'}
PATHS = ('/org/bluez/hci0', '/org/bluez/hci1')


def state():
    return v.inspect()['client']['bluetooth']


def property_value(path, name):
    value = v.remote('busctl', '--system', 'get-property', 'org.bluez', path, 'org.bluez.Adapter1', name).strip()
    if value not in ('b true', 'b false'):
        raise RuntimeError('Expected a native boolean radio property')
    return value == 'b true'


def restore_property(path, name, value):
    v.remote('busctl', '--system', 'set-property', 'org.bluez', path,
             'org.bluez.Adapter1', name, 'b', 'true' if value else 'false')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--login-file', type=Path, required=True)
    parser.add_argument('--fixture-directory', type=Path, required=True)
    args = parser.parse_args()
    directory = args.fixture_directory
    if directory.parent != Path('/tmp') or not directory.name.startswith('lucent-bluetooth-vhci-'):
        raise SystemExit('Expected the owned guest vHCI fixture directory')
    secret = next(line.split(':', 1)[1].strip() for line in args.login_file.read_text().splitlines()
                  if line.startswith('Password:'))
    owned = json.loads(subprocess.check_output(lab.ssh_args('lucent') + [shlex.join([
        'sudo', '-S', '-p', '', 'python3', str(directory / 'fixture.py'), 'info', str(directory)
    ])], input=secret + '\n', text=True, stderr=subprocess.DEVNULL))
    if owned.get('restored') or set(owned['addresses']) != EXPECTED_ADDRESSES or owned['controllers'] != ['hci0', 'hci1']:
        raise SystemExit('Expected the two live owned synthetic controllers')
    actual = {json.loads(v.remote('busctl', '--system', 'get-property', 'org.bluez', path,
                                  'org.bluez.Adapter1', 'Address').strip()[2:]) for path in PATHS}
    if actual != EXPECTED_ADDRESSES:
        raise SystemExit('Controllers changed; refusing non-owned radios')
    if v.session('omarchy-shell', 'lock', 'isLocked').strip() != 'false':
        raise SystemExit('Unlock the guest before native radio input')
    monitors = json.loads(v.session('hyprctl', '-j', 'monitors'))
    if len(monitors) != 1 or tuple(monitors[0][key] for key in ('width', 'height', 'x', 'y', 'scale')) != (1920, 1080, 0, 0, 1):
        raise SystemExit('This input harness requires the prepared single 1920x1080 scale-1 output')
    v.eventually(lambda: state()['controllers'] == 2, 'Native controller snapshot did not arrive')
    before = v.inspect()
    if any(s['id'] != 'bluetooth' and s['pointer_input'] == 'all' for s in before['surfaces']):
        raise SystemExit('Close other modal panels before radio acceptance')
    if any(property_value(path, 'Discovering') for path in PATHS):
        raise SystemExit('An existing discovery session is not part of this check')
    original = {path: {name: property_value(path, name) for name in ('Powered', 'Pairable', 'Discoverable')}
                for path in PATHS}
    original_open = state()['open']
    original_tab = state()['tab']
    original_controller = state()['controller']
    original_incoming = state()['incoming']
    native_power_checked = False
    checks = {}
    try:
        v.cli('bluetooth', 'open')
        v.eventually(lambda: state()['controllers'] == 2, 'Native controller snapshot did not arrive')
        v.click('bluetooth-tab-devices')
        # The native controller list follows the sorted snapshot. IDs are opaque
        # runtime handles; never equate their numeric values with HCI indices.
        controls = [h['id'] for s in v.inspect()['surfaces'] if s['id'] == 'bluetooth'
                    for h in s['hits'] if h['id'].startswith('bluetooth-controller-')]
        assert len(controls) == 2
        v.click(controls[0])
        if property_value(PATHS[0], 'Powered'):
            v.click('bluetooth-power')
            v.eventually(lambda: not property_value(PATHS[0], 'Powered'), 'Native Power off did not reach the selected radio')
            v.eventually(lambda: not state()['busy'], 'Power-off command did not finish')
        if not property_value(PATHS[0], 'Powered'):
            v.click('bluetooth-power')
            v.eventually(lambda: property_value(PATHS[0], 'Powered'), 'Native Power did not reach the selected radio')
            native_power_checked = True
        v.eventually(lambda: not state()['busy'], 'Power command did not finish')
        v.click('bluetooth-tab-radio')
        for identifier, name in [('bluetooth-visibility', 'Discoverable'), ('bluetooth-pairable', 'Pairable')]:
            baseline = property_value(PATHS[0], name)
            for expected in (not baseline, baseline):
                v.click(identifier)
                v.eventually(lambda: property_value(PATHS[0], name) == expected, 'Native ' + name + ' did not change')
                v.eventually(lambda: not state()['busy'], name + ' acknowledgement did not arrive')
                assert not state()['error'], 'Native radio control reported an error'
            checks['native_' + name.lower() + '_toggle'] = True
        # Changes from another bus client must update the native snapshot without
        # a Lucent control call, client polling or a desktop restart.
        for path in PATHS:
            restore_property(path, 'Powered', False)
        v.eventually(lambda: state()['label'] == 'Bluetooth off', 'Foreign property signals left a stale native snapshot')
        for path in PATHS:
            restore_property(path, 'Powered', True)
        v.eventually(lambda: state()['label'] == 'Bluetooth ready', 'Foreign power recovery was not observed')
        assert state()['incoming'] == original_incoming
        checks.update(native_power=native_power_checked, external_property_signals=True, no_restart=True,
                      incoming_agent_authority_unchanged=True, controllers=2, physical_hardware=False)
        v.screenshot('bluetooth-native-radio.png')
    finally:
        for path, flags in original.items():
            # Restore visibility while powered, then the original power state.
            restore_property(path, 'Powered', True)
            for name in ('Pairable', 'Discoverable'):
                restore_property(path, name, flags[name])
            restore_property(path, 'Powered', flags['Powered'])
        v.cli('bluetooth', 'open')
        if original_controller is not None:
            v.click('bluetooth-controller-' + str(original_controller))
        v.click({'available': 'bluetooth-tab-devices', 'paired': 'bluetooth-tab-paired', 'radio': 'bluetooth-tab-radio'}[original_tab])
        v.cli('bluetooth', 'open' if original_open else 'close')
    assert all(property_value(path, name) == expected for path, flags in original.items()
               for name, expected in flags.items())
    assert state()['open'] == original_open and state()['tab'] == original_tab and state()['controller'] == original_controller
    checks.update(original_radio_flags_restored=True, original_panel_state_restored=True)
    (v.REPORT / 'bluetooth-native-radio.json').write_text(json.dumps(checks, indent=2) + '\n')
    print(json.dumps(checks, indent=2))


if __name__ == '__main__':
    main()
