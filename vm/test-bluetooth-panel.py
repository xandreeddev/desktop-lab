#!/usr/bin/env python3
"""Native Bluetooth surface/input check in an already unlocked Lucent guest.

Does not change radio flags, pair devices or start a daemon. Test captures and
inspection data remain private. Ready with zero controllers is separate from
unavailable; neither state proves controller/pairing/profile acceptance.
"""
import importlib.util
import json
from pathlib import Path
from native_surfaces import active_surfaces, find_surface, role

spec = importlib.util.spec_from_file_location('bluetooth_input', Path(__file__).with_name('test-framework.py'))
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def state():
    return v.inspect()['client']['bluetooth']


def panel():
    return find_surface(v.inspect(), 'bluetooth')


def main():
    before = v.inspect()
    if any(role(s) != 'bluetooth' and s['pointer_input'] in ('all', 'inset') for s in active_surfaces(before)):
        raise SystemExit('Close other modal panels before the native input test')
    original_open = before['client']['bluetooth']['open']
    checks = {}
    try:
        v.cli('bluetooth', 'close')
        v.eventually(lambda: not state()['visible'], 'Initial close did not release the surface')
        v.click('bar-bluetooth')
        v.eventually(lambda: state()['open'] and panel()['frames'] > 0, 'Bar did not open native Bluetooth')
        assert panel()['pointer_input'] in ('all', 'inset')
        assert v.layer_geometry('bluetooth')['namespace'] == 'lucent-' + panel()['id']
        checks.update(bar_pointer_open=True, actual_native_layer=True, pointer_region=True,
                      observed_state=state()['status'], controllers=state()['controllers'])
        v.screenshot('bluetooth-native-panel.png')
        v.key(15)
        v.eventually(lambda: panel()['focus'] is not None, 'Tab did not route to the native panel')
        checks['keyboard_focus'] = True
        v.key(1)
        v.eventually(lambda: not state()['visible'], 'Escape did not release the native surface')
        checks['escape_releases_surface'] = True
        v.cli('bluetooth', 'open')
        v.eventually(lambda: state()['open'], 'IPC did not open native Bluetooth')
        v.cli('bluetooth', 'close')
        v.eventually(lambda: not state()['visible'], 'IPC close did not release the surface')
        checks.update(ipc_open_close=True, controls_sent=0, controller_pairing_verified=False)
    finally:
        v.cli('bluetooth', 'open' if original_open else 'close')
        v.eventually(lambda: state()['open'] == original_open, 'Original panel state was not restored')
    checks['original_panel_state_restored'] = True
    (v.REPORT / 'bluetooth-native-panel.json').write_text(json.dumps(checks, indent=2) + '\n')
    print(json.dumps(checks, indent=2))


if __name__ == '__main__':
    main()
