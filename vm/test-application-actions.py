#!/usr/bin/env python3
"""Reversible desktop-action acceptance using only two owned application entries.

Requires an unlocked, idle, single-output Lucent VM, a plain US keyboard with
Caps Lock off, no compositor bindings for the tested keys/buttons, and guest
python-dbus/GLib. The existing reversible stay-awake marker guards the run.
No real application is launched, no preferences or clipboard are changed, and
the native fixture never replaces a bus owner. Native QMP input exercises the
launcher. Private guest rollback data is retained if identity or preservation
checks fail. Only fixed labels, booleans and counts enter the local report.
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
REPORT = lab.ROOT / 'reports/local/application-actions-native.json'
PREFERENCES = '''import hashlib,json,os,stat
from pathlib import Path
root=Path.home()/'.local/state/lucent'; settings=root/'desktop.json'
s=settings.lstat()
if not stat.S_ISREG(s.st_mode) or s.st_size>4*1024*1024: raise ValueError('settings_checkpoint')
wallpaper=root/'wallpaper'; w=wallpaper.lstat()
if not stat.S_ISLNK(w.st_mode): raise ValueError('wallpaper_checkpoint')
print(json.dumps({'settings_sha256':hashlib.sha256(settings.read_bytes()).hexdigest(),
 'settings_identity':[s.st_dev,s.st_ino,s.st_mode,s.st_size,s.st_mtime_ns],
 'wallpaper_link':os.readlink(wallpaper),
 'wallpaper_identity':[w.st_dev,w.st_ino,w.st_mode,w.st_mtime_ns]}))
'''


def keyboard_preflight():
    keyboards = json.loads(session('hyprctl', '-j', 'devices'))['keyboards']
    main = [keyboard for keyboard in keyboards if keyboard.get('main') is True]
    require(len(main) == 1 and main[0].get('layout') == 'us'
            and main[0].get('variant', '') == '' and main[0].get('capsLock') is False,
            'plain_us_main_keyboard_with_caps_off')
    bindings = json.loads(session('hyprctl', '-j', 'binds'))

    def bound(mask, symbols, code):
        for binding in bindings:
            if binding.get('modmask', 0) & ~(2 | 16) != mask:
                continue
            key = str(binding.get('key', '')).lower()
            if key in (*symbols, 'catchall', f'code:{code}', f'code:{code + 8}'):
                return True
            if binding.get('keycode', 0) in (code, code + 8):
                return True
        return False

    # The random owned query suffix may contain any lowercase letter/digit.
    letters = dict(zip('qwertyuiopasdfghjklzxcvbnm',
                       [16,17,18,19,20,21,22,23,24,25,30,31,32,33,34,35,36,37,38,44,45,46,47,48,49,50]))
    plain = {**letters, **dict(zip('1234567890', range(2, 12))), 'space': 57}
    for symbol, code in plain.items():
        require(not bound(0, (symbol,), code), 'typed_query_keys_have_no_compositor_bindings')
    for mask, symbols, code in [
        (1, ('l',), 38), (1, ('f10',), 68),
        (0, ('return', 'enter'), 28), (0, ('escape', 'esc'), 1),
        (0, ('up',), 103), (0, ('down',), 108),
        (0, ('mouse:272',), 272), (0, ('mouse:273',), 273),
    ]:
        require(not bound(mask, symbols, code), 'action_keys_and_buttons_have_no_compositor_bindings')


def client():
    return inspect()['client']


def menu():
    return client()['application_actions']


def pointer(name, down, button='left'):
    h['qmp'](name, [{'type': 'btn', 'data': {'button': button, 'down': down}}])


def target(identifier):
    snapshot = inspect()
    state = snapshot['client']
    surface = find_surface(snapshot, 'dock')
    require(state['launcher'] and state['dock_surface_expanded']
            and not state['launcher_opening'] and surface['keyboard'] == 'exclusive'
            and surface['pointer_input'] == 'all'
            and surface['height'] == state['desktop_viewport'][1],
            'launcher_native_input_admitted')
    hits = [hit for hit in surface['hits'] if hit['id'] == identifier]
    require(len(hits) == 1, 'owned_launcher_hit_exists')
    value, clip = hits[0], hits[0]['clip']
    left, top = max(value['x'], clip['x']), max(value['y'], clip['y'])
    right = min(value['x'] + value['width'], clip['x'] + clip['width'])
    bottom = min(value['y'] + value['height'], clip['y'] + clip['height'])
    require(right - left > 4 and bottom - top > 4
            and right - left >= value['width'] - .5
            and bottom - top >= value['height'] - .5, 'owned_launcher_hit_visible')
    origin = h['layer_origin'](surface)
    require(abs(origin['w'] - surface['width']) <= .5
            and abs(origin['h'] - surface['height']) <= .5,
            'launcher_compositor_buffer_matches_inspector')
    # Stay clear of a result row's separate pin control at its right edge.
    return (origin['x'] + left + (right - left) * .3,
            origin['y'] + (top + bottom) / 2, right - left, bottom - top,
            origin['x'], origin['y'], origin['w'], origin['h'])


def point(name, monitor, identifier):
    previous = None
    stable_since = time.monotonic()
    deadline = stable_since + 15
    while time.monotonic() < deadline:
        try:
            current = target(identifier)
        except (CheckFailure, KeyError, LookupError):
            previous = None
            stable_since = time.monotonic()
            time.sleep(.1)
            continue
        if current != previous:
            stable_since = time.monotonic()
        if current == previous and time.monotonic() - stable_since >= .5:
            h['move'](name, monitor, current[0], current[1])
            def arrived():
                cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
                return abs(cursor['x'] - current[0]) <= 2 and abs(cursor['y'] - current[1]) <= 2
            eventually(arrived, 'owned_launcher_pointer_arrived', timeout=3)
            time.sleep(.1)
            return
        previous = current
        time.sleep(.1)
    raise CheckFailure('owned_launcher_hit_stable')


def click(name, monitor, identifier, button='left'):
    point(name, monitor, identifier)
    try:
        pointer(name, True, button)
        time.sleep(.06)
    finally:
        # Release even if the press was delivered but its QMP reply was lost.
        pointer(name, False, button)


def reject_closed_action(action):
    try:
        result = subprocess.run(lab.ssh_args('lucent') + [shlex.join([
            'python3', 'desktop-lab/scripts/in-session.py',
            '/home/omarchy/.local/bin/lucent-cli', 'launcher', 'action', action,
        ])], capture_output=True, timeout=12)
    except (OSError, subprocess.TimeoutExpired):
        raise CheckFailure('bounded_stale_action_request') from None
    require(result.returncode == 1 and result.stderr.startswith(b'lucent: error:'),
            'closed_action_authority_rejected')


UNIT_RETIRE = r'''
import json, subprocess, sys
unit, script, run_id = sys.argv[1:]
assert unit == 'lucent-application-actions-' + run_id + '.service'
def show():
    result = subprocess.run(['systemctl','--user','show',unit,
        '--property=LoadState,ActiveState,Transient,ExecStart,ControlGroup'],
        capture_output=True,text=True,timeout=5)
    assert result.returncode in (0,1)
    return dict(line.split('=',1) for line in result.stdout.splitlines() if '=' in line)
value = show()
if value.get('LoadState') != 'not-found':
    assert value.get('Transient') == 'yes'
    assert script in value.get('ExecStart','') and run_id in value.get('ExecStart','')
    group = value.get('ControlGroup','')
    assert not group or group.endswith('/' + unit)
    result = subprocess.run(['systemctl','--user','stop',unit],capture_output=True,timeout=10)
    value = show()
    assert result.returncode == 0 or value.get('LoadState') == 'not-found'
assert value.get('LoadState') == 'not-found' or value.get('ActiveState') in ('inactive','failed')
print(json.dumps({'retired':True}))
'''

SCRATCH_REMOVE = r'''
import hashlib,json,os,shutil,stat,sys
from pathlib import Path
directory=Path(sys.argv[1]); run_id=sys.argv[2]
value=directory.lstat()
assert directory.is_absolute() and directory.resolve()==directory
assert stat.S_ISDIR(value.st_mode) and value.st_uid==os.getuid() and stat.S_IMODE(value.st_mode)==0o700
manifest=json.loads((directory/'manifest.json').read_text())
assert manifest['run_id']==run_id
assert hashlib.sha256((directory/'fixture.py').read_bytes()).hexdigest()==manifest['script']
assert hashlib.sha256(Path(manifest['settings_path']).read_bytes()).hexdigest()==manifest['settings']
assert set(p.name for p in directory.iterdir()) <= {'fixture.py','manifest.json','settings.backup','events.json','events.lock'}
assert all(p.is_file() and not p.is_symlink() for p in directory.iterdir())
shutil.rmtree(directory)
'''


def suite(results):
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
    before = client()
    require(not before['launcher'] and not before['query'] and before['mode'] == 'Apps',
            'launcher_initially_closed_with_empty_query')
    require(not before['notification_history_open'], 'notification_history_initially_closed')
    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper'):
        require(not before.get(panel, {}).get('open', False)
                and not before.get(panel, {}).get('visible', False), 'other_panels_initially_closed')
    require(not before.get('widget_grid', False) and before['settings_save_status'] == 'saved'
            and before['background_ready'] and not before['wallpaper_busy'], 'desktop_initially_ready')
    layers = session('hyprctl', '-j', 'layers')
    require('lucent-authorization-' not in layers, 'no_authorization_surface')
    require(not json.loads(session('hyprctl', '-j', 'activewindow')),
            'no_unrelated_foreground_application')
    # Import and bus reachability are prerequisites; do not install packages.
    session('python3', '-c', 'import dbus,gi; from gi.repository import GLib; dbus.SessionBus(private=True).close()')
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1, 'single_output_fixture_geometry')
    monitor = monitors[0]
    require(monitor['width'] == 1920 and monitor['height'] == 1080 and monitor['x'] == 0
            and monitor['y'] == 0 and monitor['scale'] == 1, 'scale_one_fixture_geometry')
    keyboard_preflight()
    name = lab.inventory()['vms']['lucent']['name']
    settings = h['settings_digest']()
    preferences = json.loads(remote('python3', '-c', PREFERENCES))
    scratch = remote('mktemp', '-d', '/tmp/lucent-application-actions.XXXXXXXXXXXX').strip()
    run_id = scratch.rsplit('.', 1)[-1]
    fixture = scratch + '/fixture.py'
    unit = 'lucent-application-actions-' + run_id + '.service'
    initialized, unit_attempted = False, False
    cleanup = results['cleanup'] = {}

    def call(operation, *arguments):
        return session('/usr/bin/python3', fixture, operation, scratch, run_id, *arguments)

    def events():
        state = json.loads(call('state'))
        require(state['run_id'] == run_id, 'owned_event_log_identity')
        return state['events']

    def count(kind, action=None):
        return sum(event['kind'] == kind and (action is None or event.get('action') == action)
                   for event in events())

    def no_primary():
        require(all(event['kind'] not in ('primary-exec', 'activate', 'open') for event in events()),
                'action_never_uses_primary_launch_or_open')

    def open_query(kind, stationary=False):
        previous_cursor = json.loads(session('hyprctl', '-j', 'cursorpos')) if stationary else None
        cli('launcher', 'open')
        eventually(lambda: client()['launcher'] and not client()['query'], 'launcher_open_empty')
        click(name, monitor, 'launcher-search')
        eventually(lambda: find_surface(inspect(), 'dock').get('focus') == 'launcher-search',
                   'owned_launcher_query_focused', timeout=5)
        if stationary:
            require(json.loads(session('hyprctl', '-j', 'cursorpos')) == previous_cursor,
                    'stationary_search_click_uses_unchanged_physical_cursor')
            require(client()['launcher'], 'stationary_search_click_does_not_dismiss_launcher')
        h['type_ascii'](name, entries[kind]['name'])
        eventually(lambda: client()['query'] == entries[kind]['name']
                   and client()['result_count'] == 1, 'owned_query_has_one_result')
        require(client()['selected'] == 0, 'owned_query_result_selected')

    def chooser(kind, how='keyboard', stationary=False):
        open_query(kind, stationary)
        if how == 'keyboard':
            h['key'](name, 42, 68)  # Shift+F10: the native ContextMenu key route.
        else:
            click(name, monitor, 'result-0', 'right')
        eventually(lambda: menu()['open'] and menu()['application'] == entries[kind]['id'],
                   'owned_action_chooser_open')
        require([action['id'] for action in menu()['actions']] ==
                (['Literal', 'Stale'] if kind == 'command' else ['Native', 'Denied']),
                'owned_action_order_and_identity')

    def action_finished():
        return not menu()['busy'] and menu()['pending'] is None

    try:
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(fixture)],
                 data=(ROOT / 'vm/fixtures/application-actions-fixture.py').read_bytes(), timeout=20)
        # Set before initialization so partial exclusive entry creation is also
        # covered by cleanup whenever its manifest reached disk.
        initialized = True
        entries = json.loads(call('initialize'))
        require(h['settings_digest']() == settings, 'settings_unchanged_during_fixture_install')
        unit_attempted = True
        session('systemd-run', '--user', '--collect', '--unit=' + unit,
                '--slice=app.slice', '--property=RuntimeMaxSec=300',
                '--property=MemoryMax=96M', '--property=UMask=0077',
                '/usr/bin/python3', fixture, 'serve', scratch, run_id)
        eventually(lambda: json.loads(call('native-state'))['ready'], 'non_replacing_native_fixture_ready')

        # Do not invoke an unknown catalog result while waiting for discovery.
        for kind in ('command', 'native'):
            def observed():
                cli('launcher', 'actions', entries[kind]['id'])
                return menu()['open'] and menu()['application'] == entries[kind]['id']
            eventually(observed, 'owned_application_catalog_observed')
            h['key'](name, 1)
            cli('launcher', 'close')
        results['checks']['two_owned_entries_and_non_replacing_native_owner'] = True

        chooser('command')
        query, selected = client()['query'], client()['selected']
        click(name, monitor, 'application-actions-back')
        eventually(lambda: not menu()['open'] and client()['launcher'], 'back_returns_to_results')
        require(client()['query'] == query and client()['selected'] == selected, 'back_preserves_query_selection')
        h['key'](name, 42, 68)
        eventually(lambda: menu()['open'], 'keyboard_reopens_action_chooser')
        h['key'](name, 1)
        eventually(lambda: not menu()['open'] and client()['launcher'], 'first_escape_returns_to_results')
        require(client()['query'] == query and client()['selected'] == selected,
                'escape_preserves_query_selection')
        h['key'](name, 1)
        eventually(lambda: not client()['launcher'], 'second_escape_closes_launcher')
        no_primary()
        results['checks']['back_escape_and_query_selection_preserved'] = True

        chooser('command')
        require(menu()['selected'] == 'Literal', 'first_owned_action_selected')
        h['key'](name, 28)
        eventually(lambda: count('action-exec', 'Literal') == 1 and action_finished(),
                   'keyboard_invokes_literal_action_once')
        event = next(event for event in events() if event['kind'] == 'action-exec')
        require(event['action'] == 'Literal' and event['version'] == 'v1'
                and event['argv'] == ['two words', '$HOME', '$(unused)', 'semi;colon', '%', ''],
                'literal_action_argv_preserved_without_shell_expansion')
        no_primary()
        results['checks']['keyboard_action_and_literal_argv'] = True

        chooser('command', 'pointer', stationary=True)
        results['checks']['stationary_cursor_reopen_keeps_search_focus'] = True
        h['key'](name, 108)  # Down moves inside the chooser, not the app results.
        eventually(lambda: menu()['selected'] == 'Stale', 'chooser_down_selects_second_action')
        h['key'](name, 103)
        eventually(lambda: menu()['selected'] == 'Literal', 'chooser_up_selects_first_action')
        click(name, monitor, 'application-action-Stale')
        eventually(lambda: count('action-exec', 'Stale') == 1 and action_finished(),
                   'secondary_click_chooser_pointer_action_once')
        no_primary()
        results['checks']['secondary_click_pointer_action_and_up_down'] = True

        # Capture a real row press, retire its full descriptor, then release at
        # the same coordinates. Neither stale nor replacement Exec may run.
        for variant in ('changed', 'removed-action', 'removed-entry'):
            chooser('command')
            previous = count('action-exec', 'Stale')
            point(name, monitor, 'application-action-Stale')
            try:
                pointer(name, True)
                call('mutate', variant)
                eventually(lambda: not menu()['open'], 'catalog_change_retires_captured_action')
            finally:
                pointer(name, False)
            reject_closed_action('Stale')
            time.sleep(.4)
            require(count('action-exec', 'Stale') == previous, 'retired_press_never_invokes_old_or_new_action')
            no_primary()
            call('mutate', 'original')
            # A fresh chooser must recover the exact original descriptor.
            def restored():
                cli('launcher', 'actions', entries['command']['id'])
                return (menu()['open'] and [action['name'] for action in menu()['actions']]
                        == ['Literal fixture action', 'Revocable fixture action'])
            eventually(restored, 'owned_original_descriptor_observed_again')
            h['key'](name, 1)
            cli('launcher', 'close')
        results['checks']['changed_removed_action_and_removed_entry_retire_press_and_ipc'] = True

        chooser('native')
        h['key'](name, 28)
        eventually(lambda: count('activate-action', 'Native') == 1 and action_finished(),
                   'native_activate_action_acknowledged')
        native_event = next(event for event in events()
                            if event['kind'] == 'activate-action' and event['action'] == 'Native')
        require(native_event['empty_parameters'] and native_event['empty_platform'],
                'native_action_wire_uses_empty_parameters_and_platform')
        require(count('action-fallback') == 0, 'successful_native_action_has_no_exec_fallback')
        no_primary()
        results['checks']['native_activate_action_exact_name_and_empty_wire_arguments'] = True

        chooser('native', 'pointer')
        click(name, monitor, 'application-action-Denied')
        eventually(lambda: count('activate-action', 'Denied') == 1 and action_finished()
                   and bool(menu()['error']), 'native_denial_is_visible_action_error')
        time.sleep(.4)
        require(count('action-fallback') == 0, 'native_denial_never_falls_back_to_exec')
        no_primary()
        results['checks']['native_denial_has_error_without_action_or_primary_fallback'] = True
        results['counts'] = {'literal_exec': count('action-exec', 'Literal'),
                             'other_exec': count('action-exec', 'Stale'),
                             'native_action': count('activate-action', 'Native'),
                             'native_denied': count('activate-action', 'Denied'),
                             'fallback_exec': count('action-fallback'),
                             'primary_launch': count('primary-exec') + count('activate')}
    finally:
        failures = []
        for button in ('left', 'right'):
            try:
                pointer(name, False, button)
            except Exception:
                failures.append('pointer_release')
        try:
            # Opening clears only our query; the preflight required it empty.
            cli('launcher', 'open')
            cli('launcher', 'close')
            eventually(lambda: not client()['launcher'] and not client()['query']
                       and not menu()['open'] and not menu()['busy'], 'launcher_restored_closed_empty')
            cleanup['launcher_closed_empty'] = True
        except Exception:
            failures.append('launcher_restore')
        retired = not unit_attempted
        if unit_attempted:
            try:
                require(json.loads(session('python3', '-c', UNIT_RETIRE, unit, fixture, run_id))['retired'],
                        'owned_fixture_unit_retired')
                require(json.loads(session('python3', '-c',
                    'import dbus,json,sys; b=dbus.SessionBus(private=True); '
                    'print(json.dumps(not b.name_has_owner(sys.argv[1]))); b.close()',
                    entries['native']['id'].removesuffix('.desktop'))),
                    'owned_native_bus_name_retired')
                retired = True
            except Exception:
                failures.append('owned_unit_retirement')
        cleanup['owned_unit_retired'] = retired
        removed, preserved = False, False
        if initialized and retired:
            try:
                value = json.loads(call('cleanup'))
                removed, preserved = value['entries_removed'], value['settings_preserved']
                eventually(lambda: client()['applications'] == before['applications'],
                           'owned_catalog_entries_retired_after_cleanup')
            except Exception:
                failures.append('owned_entry_cleanup')
        cleanup['owned_entries_removed'] = removed
        try:
            require(h['settings_digest']() == settings and preserved, 'exact_saved_settings_preserved')
            require(json.loads(remote('python3', '-c', PREFERENCES)) == preferences,
                    'exact_settings_and_wallpaper_link_preserved')
            keyboard_preflight()
            require(client()['wallpaper'] == before['wallpaper'], 'exact_wallpaper_identity_preserved')
            cleanup['settings_and_wallpaper_preserved'] = True
        except Exception:
            failures.append('caller_state_preservation')
            cleanup['settings_and_wallpaper_preserved'] = False
        if retired and removed and preserved and not failures:
            try:
                remote('python3', '-c', SCRATCH_REMOVE, scratch, run_id)
                cleanup['owned_scratch_removed'] = True
            except Exception:
                failures.append('owned_scratch_cleanup')
        require(not failures, 'owned_cleanup_and_state_preservation')


def main():
    results = {'passed': False, 'checks': {},
               'scope': 'Two owned desktop entries, non-replacing session-bus fixture and native QMP input'}
    failure = None
    try:
        measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
        with measure['stay_awake'](remote):
            suite(results)
        results['passed'] = True
    except CheckFailure as error:
        failure = str(error)
    except Exception:
        failure = 'unexpected_bounded_acceptance_failure'
    if failure:
        results['failure'] = failure
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results))
    if failure:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
