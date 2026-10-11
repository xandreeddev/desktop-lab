#!/usr/bin/env python3
"""Native widget input and persistence checks on an unused workspace.

Requires an unlocked Lucent guest. Restores preferences, workspace and the normal
desktop service, including on failure. Restart resets transient calendar/timer
state. Leaves existing applications and the selected wallpaper alone.
"""
import json
import math
from pathlib import Path
import runpy
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
v = runpy.run_path(str(ROOT / 'vm/test-framework.py'), run_name='harness')
remote, session, cli = v['remote'], v['session'], v['cli']
eventually, click = v['eventually'], v['click']


def client():
    return v['inspect']()['client']


def suite():
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    eventually(lambda: client()['background_ready'] and client()['settings_writable'],
               'Initial desktop not ready', timeout=35)
    initial = client()
    workspace = json.loads(session('hyprctl', '-j', 'activeworkspace'))['id']
    assert workspace > 0
    occupied = {w['id'] for w in json.loads(session('hyprctl', '-j', 'workspaces'))}
    target = next(i for i in range(70, 10000) if i not in occupied)
    state = '/home/omarchy/.local/state/lucent/desktop.json'
    backup = remote('mktemp', '/tmp/lucent-widget-host.XXXXXX').strip()
    remote('cp', state, backup)
    checks = []
    try:
        session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{target}" }})')
        for widget in ('calendar', 'notes', 'timer'):
            cli('widget', widget, 'show')
        cli('widgets', 'open')
        click('reset-layout')
        cli('launcher', 'close')
        eventually(lambda: not client()['positions'], 'Layout reset failed')
        def widget_frames():
            return next(s['frames'] for s in v['inspect']()['surfaces'] if s['id'] == 'widgets')
        for button in ('calendar-previous', 'calendar-next'):
            before = widget_frames()
            click(button)
            eventually(lambda: widget_frames() > before, 'Calendar control did not repaint')
        checks.append('native_calendar_controls_repaint')

        click('notes-input')
        v['text'](' widget test')
        eventually(lambda: client()['notes'].endswith(' widget test'), 'Note edit did not reach child')
        notes = client()['notes']
        eventually(lambda: json.loads(remote('cat', state))['notes'] == notes, 'Note was not persisted')
        checks.append('native_note_edit_and_persistence')

        click('timer-reset')
        start = client()['timer_seconds']
        click('timer-toggle')
        eventually(lambda: client()['timer_seconds'] < start, 'Timer did not count down', timeout=8)
        click('timer-toggle')
        paused = client()['timer_seconds']
        time.sleep(2.2)
        assert client()['timer_seconds'] == paused
        click('timer-reset')
        assert client()['timer_seconds'] == start
        checks.append('native_timer_start_pause_reset')

        hit = v['hit']('widget-calendar')
        x, y = hit['x'] + 100, hit['y'] + 20
        v['move'](x, y)
        v['button'](True)
        for index in range(1, 13):
            v['move'](x + index * 15, y + index * 6)
            time.sleep(.025)
        eventually(lambda: client()['widget_grid'], 'Drag grid did not appear')
        # Saving another preference while the pointer is held must see the
        # committed layout, not the transient preview rendered under the hand.
        assert 'calendar' not in client()['positions']
        cli('launcher', 'size', '800', '720')
        eventually(lambda: client()['settings_save_status'] == 'saved' and
                   json.loads(remote('cat', state))['launcher'] == {'width': 800, 'max_height': 720},
                   'Unrelated preference save not acknowledged')
        assert 'calendar' not in json.loads(remote('cat', state))['positions']
        preview = v['hit']('widget-calendar')
        assert abs(preview['x'] - hit['x'] - 180) < 1
        assert abs(preview['y'] - hit['y'] - 72) < 1
        checks.append('native_drag_preview_excluded_from_unrelated_save')
        v['button'](False)
        eventually(lambda: not client()['widget_grid'] and 'calendar' in client()['positions'], 'Drag did not finish')
        position = client()['positions']['calendar']
        step = v['token_value']('component.widget_layout.grid_step')
        expected = {'x': math.floor((hit['x'] + 180) / step + .5) * step,
                    'y': math.floor((hit['y'] + 72) / step + .5) * step}
        assert position == expected, (position, expected)
        eventually(lambda: json.loads(remote('cat', state))['positions'].get('calendar') == position, 'Position not saved')
        checks.append('native_drag_grid_and_release_snap')

        cli('widgets', 'open')
        v['move'](960, 250)
        # Calendar is the first catalog entry; activate it through the selector.
        v['key'](28)
        eventually(lambda: 'calendar' not in client()['widgets'], 'Keyboard widget toggle failed')
        cli('launcher', 'close')
        cli('widget', 'calendar', 'show')
        eventually(lambda: 'calendar' in json.loads(remote('cat', state))['visible_widgets'], 'Show intent not saved')
        checks.append('native_selector_and_ipc_share_widget_identity')

        session('systemctl', '--user', 'restart', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['positions'].get('calendar') == position,
                   'Widget position did not survive restart', timeout=35)
        assert client()['notes'] == notes
        assert client()['wallpaper'] == initial['wallpaper']
        assert not client()['error']
        checks.append('notes_and_positions_survive_restart')
    finally:
        v['button'](False)
        session('systemctl', '--user', 'stop', 'lucent.service')
        remote('cp', backup, state)
        session('systemctl', '--user', 'start', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['settings_writable'], 'Original desktop not ready', timeout=35)
        for field in ('positions', 'widgets', 'notes', 'wallpaper', 'palette', 'launcher_size'):
            assert client()[field] == initial[field], 'Failed to restore ' + field
        session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{workspace}" }})')
        remote('rm', '-f', backup)
    report = {'checks': checks, 'preferences_and_workspace_restored': True,
              'no_existing_apps_closed': True}
    (v['REPORT'] / 'lucent-widget-host.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)


def main():
    measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
    with measure['stay_awake'](remote):
        suite()


if __name__ == '__main__':
    main()
