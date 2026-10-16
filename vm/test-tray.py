#!/usr/bin/env python3
"""Reversible SNI fixture + real pointer acceptance in the existing Lucent VM.

Requires an unlocked single 1920x1080 output and an empty native tray. Starts one
owned, time-limited fixture, never replaces the watcher, and preserves settings.
This checks protocol integration, not third-party application interoperability.
Only sanitized results are written under ignored reports/local.
"""
import json
from pathlib import Path
import runpy
import shlex
import subprocess
import time

import lab
from native_surfaces import active_surfaces


ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
v = runpy.run_path(str(ROOT / 'vm/test-framework.py'))
remote, session, cli, inspect = [h[k] for k in ('remote', 'session', 'cli', 'inspect')]
require, eventually = h['require'], h['eventually']


def bar_items():
    return [hit['id'] for surface in active_surfaces(inspect())
            for hit in surface['hits'] if hit['id'].startswith('bar-tray-item-')]


def press(identifier, button='left'):
    target = v['hit'](identifier)
    v['move'](target['x'] + target['width'] / 2, target['y'] + target['height'] / 2)
    time.sleep(.08)
    v['send']([{'type': 'btn', 'data': {'down': True, 'button': button}}])
    time.sleep(.04)
    v['send']([{'type': 'btn', 'data': {'down': False, 'button': button}}])


def suite():
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
    before = inspect()['client']
    require(before['tray']['ready'] and before['tray']['items'] == 0, 'empty_native_tray')
    require(not before['tray']['open'] and not before['launcher'], 'test_panels_closed')
    geometry = v['layer_geometry']('bar')
    require(geometry['w'] == 1920 and geometry['x'] == 0, 'single_output_fixture_geometry')
    settings = h['settings_digest']()
    scratch = remote('mktemp', '-d', '/tmp/lucent-tray-test.XXXXXXXX').strip()
    fixture = scratch + '/fixture.py'
    unit = 'lucent-tray-fixture-' + scratch.rsplit('.', 1)[-1]
    results = {}

    def control(*args):
        return session('python3', fixture, *args)

    def state():
        return json.loads(control('state'))

    def count(name):
        return state()['counts'].get(name, 0)

    def configure(**options):
        reads = count('read')
        control('configure', json.dumps(options))
        eventually(lambda: count('read') > reads, 'fixture_change_observed')
        time.sleep(.15)

    def action(identifier, button, name):
        previous = count(name)
        press(identifier, button)
        eventually(lambda: count(name) == previous + 1, 'native_' + name)
        eventually(lambda: not inspect()['client']['tray']['busy'], 'action_acknowledged')

    try:
        with (ROOT / 'vm/fixtures/tray-fixture.py').open('rb') as stream:
            subprocess.run(lab.ssh_args('lucent') + ['cat > ' + shlex.quote(fixture)],
                           stdin=stream, check=True, timeout=20)
        session('systemd-run', '--user', '--unit=' + unit, '--property=RuntimeMaxSec=180',
                '--property=MemoryMax=96M', '/usr/bin/python3', fixture)
        eventually(lambda: inspect()['client']['tray']['items'] == 1 and len(bar_items()) == 1,
                   'fixture_registered_and_visible')
        require(state()['registration_errors'] == 0, 'non_replacing_registration')
        item = bar_items()[0]
        cli('launcher', 'open')
        action(item, 'left', 'activate')
        require(inspect()['client']['launcher'], 'activation_preserves_existing_panel')
        cli('launcher', 'close')
        action(item, 'middle', 'secondary')
        action(item, 'wheel-up', 'scroll')
        action(item, 'wheel-down', 'scroll')
        scrolls = [event[1:] for event in state()['events'] if event[0] == 'scroll']
        require(scrolls == [[-1, 'vertical'], [1, 'vertical']], 'native_wheel_direction')
        results['primary_middle_vertical_wheel'] = True

        press(item, 'right')
        eventually(lambda: inspect()['client']['tray']['menu'], 'right_opens_native_menu')
        invoked = count('event')
        v['click']('tray-menu-1')
        eventually(lambda: count('event') == invoked + 1, 'native_menu_invoked')
        eventually(lambda: not inspect()['client']['tray']['open'], 'menu_closes_after_invocation')
        results['right_click_lucent_menu_and_invocation'] = True

        configure(menu_only=True)
        activations = count('activate')
        press(item)
        eventually(lambda: inspect()['client']['tray']['menu'], 'menu_only_primary')
        require(count('activate') == activations, 'menu_only_does_not_activate')
        cli('tray', 'close')
        configure(has_menu=False)
        action(item, 'left', 'context')
        action(item, 'right', 'context')
        results['menu_only_and_context_fallback'] = True

        configure(status='Passive')
        eventually(lambda: not bar_items(), 'passive_item_hidden')
        configure(status='NeedsAttention', menu_only=False, has_menu=True)
        eventually(lambda: bar_items() == [item], 'attention_item_returns')
        results['status_lifecycle'] = True

        configure(status='Active', delayed_failure=True)
        activations = count('activate')
        press(item)
        eventually(lambda: count('activate') == activations + 1, 'delayed_action_started')
        cli('tray', 'close')
        time.sleep(1.5)
        require(not inspect()['client']['tray']['open'], 'dismissed_error_does_not_reopen')
        configure(delayed_failure=False, items=7)
        eventually(lambda: inspect()['client']['tray']['items'] == 7 and len(bar_items()) == 4,
                   'bounded_icons_with_overflow')
        v['click']('bar-tray')
        eventually(lambda: inspect()['client']['tray']['open'], 'overflow_opens')
        results['late_reply_retirement_and_overflow'] = True
        cli('tray', 'close')
        control('quit')
        eventually(lambda: inspect()['client']['tray']['items'] == 0 and not bar_items(),
                   'owner_exit_removes_icons')
        results['owner_retirement'] = True
    finally:
        # A clean fixture exit can already have collected its transient unit.
        # Still reject a live unit after the bounded stop attempt.
        remote('python3', '-c',
               "import subprocess,sys; unit=sys.argv[1]; "
               "subprocess.run(['systemctl','--user','stop',unit],capture_output=True,timeout=10); "
               "r=subprocess.run(['systemctl','--user','is-active',unit],capture_output=True,text=True,timeout=5); "
               "assert r.stdout.strip() not in ('active','activating','deactivating')", unit)
        cli('tray', 'close')
        cli('launcher', 'close')
        remote('rm', '-rf', '--', scratch)
    require(h['settings_digest']() == settings, 'saved_settings_preserved')
    require(inspect()['client']['wallpaper'] == before['wallpaper'], 'wallpaper_preserved')
    results['settings_and_wallpaper_preserved'] = True
    results['scope'] = 'Owned SNI/DBusMenu fixture and QMP input; no third-party app coverage'
    report = lab.ROOT / 'reports/local/tray-bar-native.json'
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results))


if __name__ == '__main__':
    suite()
