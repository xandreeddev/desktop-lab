#!/usr/bin/env python3
"""Verify domain-value migration, native drag persistence and invalid settings.

Uses an empty workspace and restores preferences/workspace in finally. No private
screen captures are published and no existing application is closed.
"""
import importlib.util
import json
import math
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
spec = importlib.util.spec_from_file_location('v', ROOT / 'vm/test-framework.py')
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def main():
    def client():
        return v.inspect()['client']

    def widget_frames():
        return next(s['frames'] for s in v.inspect()['surfaces'] if s['id'] == 'widgets')

    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    v.eventually(lambda: client()['background_ready'], 'Initial background not ready', timeout=35)
    original = json.loads(v.session('hyprctl', '-j', 'activeworkspace'))['id']
    assert original > 0
    existing = {w['id'] for w in json.loads(v.session('hyprctl', '-j', 'workspaces'))}
    target = next(i for i in range(70, 10000) if i not in existing)
    state = '/home/omarchy/.local/state/lucent/desktop.json'
    backup = '/tmp/lucent-values-original-settings.json'
    v.remote('cp', state, backup)
    report = {}
    try:
        v.session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{target}" }})')
        v.cli('widget', 'calendar', 'show')
        v.cli('widgets', 'open')
        v.click('reset-layout')
        v.cli('launcher', 'close')
        v.eventually(lambda: not client()['positions'], 'Layout reset did not apply')
        time.sleep(0.8)
        before = widget_frames()
        v.click('calendar-previous')
        v.eventually(lambda: widget_frames() > before, 'Previous-month action did not render')
        v.click('calendar-next')
        report['native_calendar_buttons_render'] = True
        hit = v.hit('widget-calendar')
        x, y = hit['x'] + 100, hit['y'] + 20
        v.move(x, y)
        v.button(True)
        for index in range(1, 13):
            v.move(x + index * 15, y + index * 6)
            time.sleep(0.025)
        v.eventually(lambda: client()['widget_grid'], 'Drag grid did not appear')
        v.button(False)
        v.eventually(lambda: not client()['widget_grid'] and 'calendar' in client()['positions'], 'Drag did not finish')
        position = client()['positions']['calendar']
        step = v.token_value('component.widget_layout.grid_step')
        expected = {'x': math.floor((hit['x'] + 180) / step + 0.5) * step,
                    'y': math.floor((hit['y'] + 72) / step + 0.5) * step}
        assert position == expected, (position, expected)
        v.eventually(lambda: json.loads(v.remote('cat', state))['positions'].get('calendar') == position, 'Position was not saved')
        v.session('systemctl', '--user', 'restart', 'lucent.service')
        v.eventually(lambda: client()['background_ready'] and client()['positions'].get('calendar') == position, 'Position did not survive restart', timeout=35)
        report['native_drag_grid_snap_and_restart'] = True
        v.session('systemctl', '--user', 'stop', 'lucent.service')
        v.remote('python3', '-c', 'import json,sys; from pathlib import Path; p=Path(sys.argv[1]); d=json.loads(p.read_text()); d["positions"]["calendar"]={"x":1e39,"y":12}; p.write_text(json.dumps(d))', state)
        invalid = v.remote('cat', state)
        v.session('systemctl', '--user', 'start', 'lucent.service')
        v.eventually(lambda: bool(client()['error']) and client()['background_ready'], 'Invalid saved coordinates did not report an error', timeout=35)
        assert v.remote('cat', state) == invalid, 'Invalid settings were overwritten'
        report['invalid_saved_coordinates_preserved_and_reported'] = True
        print(json.dumps(report, indent=2), flush=True)
    finally:
        v.button(False)
        v.session('systemctl', '--user', 'stop', 'lucent.service')
        v.remote('cp', backup, state)
        v.session('systemctl', '--user', 'start', 'lucent.service')
        v.eventually(lambda: client()['background_ready'] and not client()['error'], 'Original settings did not restore', timeout=35)
        v.eventually(lambda: json.loads(v.remote('cat', '/home/omarchy/.local/state/lucent/theme/palette.json')) == client()['palette'], 'Original palette did not restore', timeout=35)
        v.session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{original}" }})')
        v.remote('rm', '-f', backup)
        print('Original settings, palette and workspace restored', flush=True)
    (ROOT / 'reports/local/lucent-domain-values.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
