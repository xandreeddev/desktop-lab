#!/usr/bin/env python3
"""Native single-click panel switching using ordinary open/close controls.

Uses the existing unlocked single-output VM, QMP input and public inspection.
No radio toggles, volume controls or power actions are selected. Normal panel
discovery behavior remains active and is closed during cleanup. Saved settings and wallpaper are checked after cleanup.
"""
import json
from pathlib import Path
import runpy
import time

import lab
from native_surfaces import active_surfaces, find_surface

ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
v = runpy.run_path(str(ROOT / 'vm/test-framework.py'))
cli, inspect, session = [h[key] for key in ('cli', 'inspect', 'session')]
require, eventually = h['require'], h['eventually']


def opened(data):
    client = data['client']
    panels = {name for name in ('audio', 'network', 'bluetooth', 'power', 'tray')
              if client[name]['open']}
    if client['launcher']:
        panels.add('dock')
    if client['notification_history_open']:
        panels.add('notifications')
    # Live wallpaper's native keyboard ownership is explicit and public.
    panels.update(surface['role'] for surface in active_surfaces(data)
                  if surface['role'] == 'live-wallpaper' and surface['keyboard'] == 'exclusive')
    return panels


def only(panel):
    data = inspect()
    expected = set() if panel is None else {panel}
    if opened(data) != expected:
        return False
    exclusive = {surface['role'] for surface in active_surfaces(data)
                 if surface['keyboard'] == 'exclusive'}
    return exclusive == expected


def click_at(x, y):
    v['move'](x, y)
    time.sleep(.08)
    v['button'](True)
    time.sleep(.04)
    v['button'](False)


def suite():
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
    before = inspect()
    require(not opened(before) and before['client']['bar_visible'], 'initial_panels_closed_bar_visible')
    geometry = v['layer_geometry']('bar')
    require(geometry['w'] == 1920 and geometry['x'] == 0, 'single_output_fixture_geometry')
    digest = h['settings_digest']()
    checks = {}
    try:
        cli('launcher', 'open')
        eventually(lambda: only('dock'), 'initial_launcher')
        for name in ('audio', 'network', 'bluetooth', 'power', 'tray', 'notifications'):
            v['click']('bar-' + name)
            eventually(lambda: only(name), 'one_click_switch_to_' + name)
            current = find_surface(inspect(), name)
            require(current['pointer_input'] == 'inset', 'ordinary_overlay_uses_inset')
            require(current['pointer_insets'] == [v['token_value']('component.panel.bar_height'), 0, 0, 0],
                    'bar_strip_remains_reachable')
        checks['single_click_bar_switches_all_control_panels'] = True
        # Padding inside history is an inert hit, not an outside-click close.
        panel = v['hit']('notification-history-panel')
        click_at(panel['x'] + 8, panel['y'] + 40)
        time.sleep(.2)
        require(only('notifications'), 'history_padding_is_inert')
        v['key'](15)
        eventually(lambda: find_surface(inspect(), 'notifications')['focus'] is not None,
                   'history_has_native_keyboard_focus')
        v['key'](1)
        eventually(lambda: only(None), 'escape_closes_history')
        checks['history_padding_keyboard_and_escape'] = True

        cli('wallpaper', 'live', 'open')
        eventually(lambda: only('live-wallpaper'), 'live_panel_opens')
        v['click']('bar-audio')
        eventually(lambda: only('audio'), 'bar_switches_from_live_panel')
        v['click']('bar-audio')
        eventually(lambda: only(None), 'same_bar_control_toggles_closed')
        checks['live_panel_and_same_button_toggle'] = True

        cli('launcher', 'open')
        eventually(lambda: only('dock'), 'launcher_reopens')
        v['click']('bar-clock')
        require(only('dock'), 'clock_remains_passive')
        click_at(20, 200)
        eventually(lambda: only(None), 'launcher_outside_click_still_closes')
        cli('notifications', 'show')
        eventually(lambda: only('notifications'), 'history_reopens')
        click_at(20, 200)
        eventually(lambda: only(None), 'history_outside_click_closes')
        checks['passive_clock_and_outside_dismissal'] = True

        cli('launcher', 'open')
        cli('bar', 'hide')
        eventually(lambda: find_surface(inspect(), 'dock')['pointer_insets'] == [0, 0, 0, 0],
                   'hidden_bar_releases_reserved_strip')
        click_at(20, 20)
        eventually(lambda: only(None), 'hidden_bar_strip_dismisses_launcher')
        checks['hidden_bar_input_region_updates'] = True
    finally:
        cli('bar', 'show')
        for name in ('launcher', 'audio', 'network', 'bluetooth', 'power', 'tray'):
            cli(name, 'close')
        cli('notifications', 'hide')
        cli('wallpaper', 'live', 'close')
    eventually(lambda: only(None), 'all_test_panels_closed')
    require(h['settings_digest']() == digest, 'saved_settings_preserved')
    require(inspect()['client']['wallpaper'] == before['client']['wallpaper'], 'wallpaper_preserved')
    checks['settings_wallpaper_and_bar_restored'] = True
    report = lab.ROOT / 'reports/local/panel-flow-native.json'
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(checks, indent=2) + '\n')
    print(json.dumps(checks))


if __name__ == '__main__':
    suite()
