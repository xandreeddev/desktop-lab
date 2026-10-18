#!/usr/bin/env python3
"""Reversible owned MPRIS and real input acceptance in the existing Lucent VM.

Requires an unlocked, single 1920x1080 scale-1 output, python-dbus/gobject and
no existing MPRIS players. Starts only two fixed fixture names in bounded user
units. All media commands target fixture identities; no real player is changed.
Settings and current wallpaper must remain byte-for-byte/identity unchanged.
Only sanitized assertions are published under ignored reports/local.
"""
import json
from pathlib import Path
import runpy
import shlex
import subprocess
import time

import lab
from native_surfaces import find_surface


ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
remote, session, cli, inspect = [h[key] for key in ('remote', 'session', 'cli', 'inspect')]
require, eventually, CheckFailure = [h[key] for key in ('require', 'eventually', 'CheckFailure')]
IDENTITIES = {'a': 'Lucent fixture A', 'b': 'Lucent fixture B'}


def media():
    return inspect()['client']['media']


def current_player(key):
    players = [player for player in media()['players'] if player['name'] == IDENTITIES[key]]
    require(len(players) == 1 and players[0]['available'], 'fixture_player_available_' + key)
    return players[0]


def owned_selection(key):
    state = media()
    require(all(player['name'] in IDENTITIES.values() for player in state['players']),
            'only_owned_players_before_native_action')
    player = current_player(key)
    require(state['selected'] == player['id'], 'selected_player_is_owned_' + key)
    return player


def focus(name, identifier):
    for _ in range(64):
        if find_surface(inspect(), 'media-controls').get('focus') == identifier:
            return
        h['key'](name, 15)
    raise CheckFailure('native_media_keyboard_reveal')


def slider_identifier():
    state = media()
    player = next(player for player in state['players'] if player['id'] == state['selected'])
    prefix = f'media-seek-{player["id"]}-{player["track"]}-'
    hits = [hit['id'] for hit in find_surface(inspect(), 'media-controls')['hits']
            if hit['id'].startswith(prefix)]
    require(len(hits) == 1, 'native_media_slider_identity')
    return hits[0]


def hit(identifier):
    surface = find_surface(inspect(), 'media-controls')
    matches = [value for value in surface['hits'] if value['id'] == identifier]
    require(len(matches) == 1, 'native_media_hit_exists')
    value = dict(matches[0])
    clip = value['clip']
    left, top = max(value['x'], clip['x']), max(value['y'], clip['y'])
    right = min(value['x'] + value['width'], clip['x'] + clip['width'])
    bottom = min(value['y'] + value['height'], clip['y'] + clip['height'])
    require(right - left > 2 and bottom - top > 2, 'native_media_hit_visible')
    origin = h['layer_origin'](surface)
    return {'x': left + origin['x'], 'y': top + origin['y'],
            'width': right - left, 'height': bottom - top}


def stable_hit(identifier):
    previous = None
    for _ in range(40):
        try:
            current = hit(identifier)
        except (CheckFailure, LookupError, KeyError):
            previous = None
            time.sleep(.15)
            continue
        if current == previous:
            return current
        previous = current
        time.sleep(.15)
    raise CheckFailure('native_media_hit_stable')


def point(name, monitor, target, fraction=.5):
    h['move'](name, monitor, target['x'] + target['width'] * fraction,
              target['y'] + target['height'] / 2)
    time.sleep(.1)


def button(name, down):
    h['qmp'](name, [{'type': 'btn', 'data': {'button': 'left', 'down': down}}])


def click(name, monitor, identifier):
    focus(name, identifier)  # Real traversal reveals controls inside the scroll.
    point(name, monitor, stable_hit(identifier))
    button(name, True)
    time.sleep(.05)
    button(name, False)


def stale_command_rejected(*arguments):
    result = subprocess.run(lab.ssh_args('lucent') + [shlex.join([
        'python3', 'desktop-lab/scripts/in-session.py',
        '/home/omarchy/.local/bin/lucent-cli', 'media', *arguments,
    ])], capture_output=True, timeout=15)
    require(result.returncode != 0, 'retired_media_cli_target_rejected')


def stop_owned_unit(unit):
    remote('python3', '-c',
           "import subprocess,sys; unit=sys.argv[1]; "
           "subprocess.run(['systemctl','--user','stop',unit],capture_output=True,timeout=10); "
           "r=subprocess.run(['systemctl','--user','is-active',unit],capture_output=True,text=True,timeout=5); "
           "assert r.stdout.strip() not in ('active','activating','deactivating')", unit)


def suite():
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
    # Import failures and existing real players are explicit prerequisite failures.
    names = json.loads(session('python3', '-c',
        "import json,dbus; import gi; from gi.repository import GLib; "
        "print(json.dumps([str(n) for n in dbus.SessionBus().list_names() "
        "if str(n).startswith('org.mpris.MediaPlayer2.')]))"))
    require(not names, 'no_existing_mpris_players')
    before = inspect()['client']
    require(before['media']['status'] == 'ready' and not before['media']['players'],
            'empty_native_media_catalog')
    require(not before['media']['open'] and not before['launcher'], 'test_panels_closed')
    for panel in ('audio', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper'):
        require(not before.get(panel, {}).get('open', False), 'other_test_panels_closed')
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1, 'single_output_fixture_geometry')
    monitor = monitors[0]
    require(monitor['width'] == 1920 and monitor['height'] == 1080 and
            monitor['x'] == 0 and monitor['y'] == 0 and monitor['scale'] == 1,
            'scale_one_fixture_geometry')
    name = lab.inventory()['vms']['lucent']['name']
    settings = h['settings_digest']()
    scratch = remote('mktemp', '-d', '/tmp/lucent-media-test.XXXXXXXX').strip()
    fixture = scratch + '/fixture.py'
    run_id = scratch.rsplit('.', 1)[-1]
    units, results = [], {}
    pointer_down = False

    def control(key, action, *arguments):
        return session('python3', fixture, action, key, *arguments)

    def state(key):
        value = json.loads(control(key, 'state'))
        require(value['run_id'] == run_id and value['key'] == key, 'owned_fixture_control')
        return value

    def count(key, action):
        return state(key)['counts'].get(action, 0)

    def start(key):
        unit = f'lucent-media-fixture-{run_id}-{key}-{len(units)}'
        units.append(unit)
        session('systemd-run', '--user', '--collect', '--unit=' + unit,
                '--property=RuntimeMaxSec=300', '--property=MemoryMax=96M',
                '--property=UMask=0077', '/usr/bin/python3', fixture,
                'serve', key, scratch, run_id)
        eventually(lambda: state(key)['identity'] == IDENTITIES[key], 'fixture_owner_started_' + key)

    def acknowledged():
        return not media()['busy'] and not media()['control_error']

    try:
        h['run'](lab.ssh_args('lucent') + ['cat > ' + shlex.quote(fixture)],
                 data=(ROOT / 'vm/fixtures/media-fixture.py').read_bytes(), timeout=20)
        start('a')
        start('b')
        eventually(lambda: len(media()['players']) == 2 and
                   all(player['available'] for player in media()['players']) and
                   {player['name'] for player in media()['players']} == set(IDENTITIES.values()),
                   'both_owned_players_observed')
        a, b = current_player('a'), current_player('b')
        owners = {key: state(key)['owner'] for key in IDENTITIES}
        require(owners['a'] != owners['b'], 'independent_native_owners')
        results['two_non_replacing_independent_owners'] = True

        cli('media', 'open')
        eventually(lambda: media()['open'], 'native_media_panel_open')
        focus(name, f'media-player-{a["id"]}')
        h['key'](name, 28)
        eventually(lambda: media()['selected'] == a['id'], 'keyboard_selects_fixture_a')
        owned_selection('a')
        eventually(lambda: media()['artwork_ready'] and not media()['artwork_busy'],
                   'owned_fixture_artwork_prepared')
        focus(name, 'media-play')
        a_plays, b_plays = count('a', 'play'), count('b', 'play')
        owned_selection('a')
        h['key'](name, 28)
        eventually(lambda: count('a', 'play') == a_plays + 1 and acknowledged(),
                   'keyboard_play_targets_selected_fixture')
        require(count('b', 'play') == b_plays, 'other_player_not_played')
        eventually(lambda: current_player('a')['status'] == 'playing', 'native_play_observed')

        click(name, monitor, f'media-player-{b["id"]}')
        eventually(lambda: media()['selected'] == b['id'], 'pointer_selects_fixture_b')
        a_pauses, b_pauses = count('a', 'pause'), count('b', 'pause')
        owned_selection('b')
        click(name, monitor, 'media-pause')
        eventually(lambda: count('b', 'pause') == b_pauses + 1 and acknowledged(),
                   'pointer_pause_targets_selected_fixture')
        require(count('a', 'pause') == a_pauses, 'other_player_not_paused')
        eventually(lambda: current_player('b')['status'] == 'paused', 'native_pause_observed')
        results['keyboard_pointer_selection_and_targeted_play_pause'] = True
        results['owned_local_raster_artwork_prepared'] = True

        # The retained distribution media shortcut must use the same selected
        # player, rather than a global player's alphabetical/default ordering.
        owned_selection('b')
        a_plays, b_plays = count('a', 'play'), count('b', 'play')
        session('omarchy-shell', 'media', 'playPause')
        eventually(lambda: count('b', 'play') == b_plays + 1 and acknowledged(),
                   'compatibility_media_key_targets_selected_player')
        require(count('a', 'play') == a_plays, 'compatibility_does_not_retarget_other_player')
        eventually(lambda: current_player('b')['status'] == 'playing', 'compatibility_play_observed')
        b_pauses = count('b', 'pause')
        session('omarchy-shell', 'media', 'playPause')
        eventually(lambda: count('b', 'pause') == b_pauses + 1 and acknowledged(),
                   'compatibility_pause_acknowledged')
        eventually(lambda: current_player('b')['status'] == 'paused', 'compatibility_pause_observed')
        results['compatibility_media_key_uses_shared_selection'] = True

        owned_selection('b')
        slider = slider_identifier()
        focus(name, slider)
        target = stable_hit(slider)
        seeks = count('b', 'set_position')
        point(name, monitor, target, .25)
        button(name, True)
        pointer_down = True
        eventually(lambda: media()['seek_active'], 'native_slider_capture')
        point(name, monitor, target, .75)
        time.sleep(.25)
        require(count('b', 'set_position') == seeks, 'seek_preview_does_not_send')
        button(name, False)
        pointer_down = False
        eventually(lambda: count('b', 'set_position') == seeks + 1 and acknowledged(),
                   'seek_release_sends_one_native_call')
        event = [event for event in state('b')['events'] if event[0] == 'set_position'][-1]
        require(event[1] == state('b')['track'] and 84_000_000 <= event[2] <= 96_000_000,
                'seek_addresses_current_fixture_track')
        require(count('a', 'set_position') == 0, 'seek_does_not_retarget_other_player')
        results['release_only_seek_with_exact_track'] = True

        old_track = current_player('b')['track']
        slider = slider_identifier()
        focus(name, slider)
        target = stable_hit(slider)
        point(name, monitor, target, .2)
        button(name, True)
        pointer_down = True
        eventually(lambda: media()['seek_active'], 'native_retiring_slider_capture')
        control('b', 'configure', json.dumps({'new_track': True}))
        eventually(lambda: current_player('b')['track'] != old_track and not media()['seek_active'],
                   'track_change_retires_native_seek')
        point(name, monitor, target, .8)
        button(name, False)
        pointer_down = False
        time.sleep(.35)
        require(count('b', 'set_position') == seeks + 1 and count('b', 'stale_seek') == 0,
                'retired_slider_release_sends_no_native_call')
        stale_command_rejected('seek', str(b['id']), str(old_track), '1000000')
        eventually(lambda: media()['artwork_ready'] and not media()['artwork_busy'],
                   'same_uri_new_track_artwork_prepared')
        results['track_retirement_and_same_uri_artwork'] = True

        # Retire just A. B's separate owner and transient identity must survive.
        cli('media', 'close')
        control('a', 'quit')
        eventually(lambda: len(media()['players']) == 1 and current_player('b')['id'] == b['id'],
                   'one_owner_retirement_preserves_other')
        start('a')
        eventually(lambda: len(media()['players']) == 2 and current_player('a')['id'] != a['id'],
                   'replacement_owner_gets_new_identity')
        require(state('a')['owner'] != owners['a'] and state('b')['owner'] == owners['b'],
                'native_owner_replacement_is_isolated')
        stale_command_rejected('play', str(a['id']))
        require(count('a', 'play') == 0, 'retired_player_command_not_retargeted')
        results['owner_retirement_and_stale_player_rejection'] = True

        cli('media', 'open')
        eventually(lambda: media()['open'], 'media_panel_reopens')
        h['key'](name, 1)
        eventually(lambda: not media()['open'] and not media()['visible'], 'escape_closes_native_panel')
        results['native_escape_dismissal'] = True
    finally:
        failures = []
        if pointer_down:
            try:
                button(name, False)
            except Exception:
                failures.append('pointer_release')
        try:
            cli('media', 'close')
        except Exception:
            failures.append('panel_close')
        stopped = True
        for unit in reversed(units):
            try:
                stop_owned_unit(unit)
            except Exception:
                stopped = False
                failures.append('owned_unit_stop')
        if stopped:
            try:
                remote('rm', '-rf', '--', scratch)
            except Exception:
                failures.append('owned_scratch_cleanup')
        try:
            require(h['settings_digest']() == settings, 'saved_settings_preserved')
            require(inspect()['client']['wallpaper'] == before['wallpaper'], 'wallpaper_preserved')
        except Exception:
            failures.append('caller_state_preservation')
        require(not failures, 'owned_fixture_cleanup_and_state_preservation')
    eventually(lambda: not media()['players'], 'owned_catalog_empty_after_cleanup')
    results['settings_and_wallpaper_preserved'] = True
    results['owned_units_and_files_removed'] = True
    results['scope'] = 'Owned MPRIS/session-bus fixture and native QMP input; no real-player or hardware acceptance'
    report = lab.ROOT / 'reports/local/media-native.json'
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results))


if __name__ == '__main__':
    try:
        suite()
    except CheckFailure as error:
        raise SystemExit('Native media acceptance failed: ' + str(error)) from None
