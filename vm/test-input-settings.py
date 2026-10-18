#!/usr/bin/env python3
"""Reversible ordinary launcher keyboard acceptance in the existing Lucent VM.

Uses the existing bounded native helpers, QMP keys and public inspector. Requires
an unlocked single-output guest, Apps mode, no other foreground panel, a reported
main-keyboard US layout/Caps Lock state, and no compositor bindings for the tested chords.
With --skip-bound-chords, conflicting Alt/Super probes are reported as skipped;
baseline/editing chords must still be unbound. Existing bindings are never changed.
With --skip-remapped-caps, Caps-dependent cases are explicitly skipped when
keyboard options remap Caps Lock. Shift and ordinary editing are still tested.
An already-open query is restored through bounded undo, including arbitrary
Unicode. A closed launcher must have an empty query: opening clears it and there
is no public command that could restore arbitrary text without the clipboard.

Does not restart services, write preferences, change wallpaper, use the clipboard,
read credentials or interact with secure clients. Query/open state and Caps Lock
are restored on failure; existing selection/undo history is not preserved. Only
fixed check labels and booleans enter the local report, never query/settings text.

Unsupported-schema/oversized-notes native load acceptance remains pending. A
failed load also applies the default palette to theme/login exports; replacing
desktop.json alone would not be an exact rollback of that wider state.
"""

import argparse
import json
from pathlib import Path
import runpy
import sys
import time

import lab
from native_surfaces import find_surface


ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
remote, session, cli, inspect = [h[key] for key in ('remote', 'session', 'cli', 'inspect')]
require, eventually, CheckFailure = [h[key] for key in ('require', 'eventually', 'CheckFailure')]

# Linux input codes, XKB modifier masks. Lock/Num Lock do not alter binding
# matching here. Guard exact A/U chords before sending them, including code binds.
SHIFT, CONTROL, ALT, LOGO = 1, 4, 8, 64
LETTERS = {'a': 30, 'u': 22, 'q': 16, 'x': 45, 'z': 44}
IGNORED = [
    ('control_alt_a', (29, 56, 30), CONTROL | ALT, 'a', 30),
    ('control_alt_u', (29, 56, 22), CONTROL | ALT, 'u', 22),
    ('control_logo_a', (29, 125, 30), CONTROL | LOGO, 'a', 30),
    ('control_logo_u', (29, 125, 22), CONTROL | LOGO, 'u', 22),
    ('control_shift_alt_a', (29, 42, 56, 30), CONTROL | SHIFT | ALT, 'a', 30),
    ('control_shift_alt_u', (29, 42, 56, 22), CONTROL | SHIFT | ALT, 'u', 22),
    ('control_shift_logo_a', (29, 42, 125, 30), CONTROL | SHIFT | LOGO, 'a', 30),
    ('control_shift_logo_u', (29, 42, 125, 22), CONTROL | SHIFT | LOGO, 'u', 22),
    ('alt_a', (56, 30), ALT, 'a', 30),
    ('alt_u', (56, 22), ALT, 'u', 22),
]
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


def client():
    return inspect()['client']


def main_keyboard():
    keyboards = json.loads(session('hyprctl', '-j', 'devices'))['keyboards']
    main = [keyboard for keyboard in keyboards if keyboard.get('main') is True]
    require(len(main) == 1 and isinstance(main[0].get('capsLock'), bool),
            'main_keyboard_caps_state_available')
    return main[0]


def caps_lock():
    return main_keyboard()['capsLock']


def set_caps(name, enabled):
    if caps_lock() != enabled:
        h['key'](name, 58)
    eventually(lambda: caps_lock() == enabled, 'caps_state_acknowledged', timeout=5)


def bound(bindings, mask, symbol, code):
    for binding in bindings:
        if binding.get('modmask', 0) & ~(2 | 16) != mask:
            continue
        key = str(binding.get('key', '')).lower()
        if key in (symbol, f'code:{code}', f'code:{code + 8}'):
            return True
        if binding.get('keycode', 0) in (code, code + 8):
            return True
    return False


def click_query(name, monitor):
    previous = None
    stable_since = time.monotonic()
    deadline = stable_since + 15
    while time.monotonic() < deadline:
        snapshot = inspect()
        state = snapshot['client']
        surface = find_surface(snapshot, 'dock')
        if (not state['launcher'] or not state['dock_surface_expanded']
                or state['launcher_opening'] or surface['keyboard'] != 'exclusive'
                or surface['pointer_input'] != 'all'
                or surface['height'] != state['desktop_viewport'][1]):
            previous = None
            stable_since = time.monotonic()
            time.sleep(.15)
            continue
        hits = [hit for hit in surface['hits'] if hit['id'] == 'launcher-search']
        if len(hits) == 1:
            hit = hits[0]
            clip = hit['clip']
            left, top = max(hit['x'], clip['x']), max(hit['y'], clip['y'])
            right = min(hit['x'] + hit['width'], clip['x'] + clip['width'])
            bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'])
            origin = h['layer_origin'](surface)
            rect = (left, top, right, bottom, origin['x'], origin['y'],
                    origin['w'], origin['h'])
            if rect != previous:
                stable_since = time.monotonic()
            if (rect == previous and time.monotonic() - stable_since >= .5
                    and right - left >= hit['width'] - .5
                    and bottom - top >= hit['height'] - .5):
                x, y = origin['x'] + (left + right) / 2, origin['y'] + (top + bottom) / 2
                h['move'](name, monitor, x, y)
                def pointer_arrived():
                    cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
                    return abs(cursor['x'] - x) <= 2 and abs(cursor['y'] - y) <= 2
                eventually(pointer_arrived, 'ordinary_launcher_pointer_arrived', timeout=3)
                time.sleep(.1)
                try:
                    h['qmp'](name, [{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
                    time.sleep(.05)
                finally:
                    h['qmp'](name, [{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
                eventually(lambda: find_surface(inspect(), 'dock').get('focus') == 'launcher-search',
                           'ordinary_launcher_field_focused', timeout=5)
                return
            previous = rect
        time.sleep(.15)
    raise CheckFailure('ordinary_launcher_field_visible')


def query_is(value, label):
    eventually(lambda: client()['query'] == value, label, timeout=5)


def unchanged_query(value, label):
    # An immediate inspector read could precede native key delivery. Require the
    # content to remain stable over several reads before probing the selection.
    deadline = time.monotonic() + .4
    while time.monotonic() < deadline:
        require(client()['query'] == value, label)
        time.sleep(.08)


def baseline(name):
    h['key'](name, 107)  # End also breaks a preceding typing group.
    h['key'](name, 29, 30)
    h['key'](name, 16)
    h['key'](name, 16)
    query_is('qq', 'baseline_plain_control_a_and_layout')
    h['key'](name, 107)


def restore_query(name, original):
    set_caps(name, False)
    require(client()['launcher'], 'launcher_present_for_query_rollback')
    for _ in range(128):
        if client()['query'] == original:
            return
        h['key'](name, 29, 44)
        time.sleep(.08)
    raise CheckFailure('original_query_restored_within_history_bound')


def suite(report, *, skip_bound_chords=False, skip_remapped_caps=False):
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
    eventually(lambda: client()['settings_save_status'] == 'saved'
               and not client()['wallpaper_busy'], 'initial_settings_settled')
    initial = client()
    require(initial['mode'] == 'Apps', 'initial_launcher_apps_mode')
    require(initial['launcher'] or initial['query'] == '', 'closed_launcher_has_no_retained_query')
    require(not initial['notification_history_open']
            and not any(initial.get(panel, {}).get('open', False)
                        for panel in ('audio', 'network', 'bluetooth', 'power', 'tray',
                                      'media', 'live_wallpaper')), 'other_foreground_panels_closed')
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1 and monitors[0]['x'] == 0 and monitors[0]['y'] == 0
            and monitors[0]['scale'] == 1, 'single_origin_output_at_1x')
    monitor = monitors[0]
    name = lab.inventory()['vms']['lucent']['name']
    keyboard = main_keyboard()
    require(keyboard.get('layout') == 'us' and keyboard.get('variant', '') == '',
            'plain_us_keyboard_layout')
    caps_remapped = any('caps' in option and option != 'shift:both_capslock_cancel'
                        for option in keyboard.get('options', '').split(','))
    require(not caps_remapped or skip_remapped_caps, 'caps_lock_key_available')
    if caps_remapped:
        report['skipped_caps_lock_cases'] = [
            'control_a_caps', 'control_a_caps_shift',
            'control_u_caps', 'control_u_caps_shift',
        ]
    original_caps = caps_lock()
    require(not caps_remapped or not original_caps, 'remapped_caps_must_start_off')
    preferences = json.loads(remote('python3', '-c', PREFERENCES))
    bindings = json.loads(session('hyprctl', '-j', 'binds'))
    tested_ignored = []
    for probe in IGNORED:
        label, _, mask, symbol, code = probe
        if bound(bindings, mask, symbol, code):
            require(skip_bound_chords, 'tested_chords_have_no_compositor_bindings')
            report['skipped_compositor_bound_chords'].append(label)
        else:
            tested_ignored.append(probe)
    for mask in (CONTROL, CONTROL | SHIFT):
        for symbol, code in (('a', 30), ('u', 22), ('z', 44)):
            require(not bound(bindings, mask, symbol, code), 'editing_chords_have_no_compositor_bindings')
    for symbol, code in (('caps_lock', 58), ('end', 107), ('q', 16), ('x', 45)):
        require(not bound(bindings, 0, symbol, code), 'plain_keys_have_no_compositor_bindings')
    # Super-letter bindings are common. Pick one unbound printable letter and
    # report only whether the native non-insertion check passed, never bindings.
    printable = next(((symbol, code) for symbol, code in LETTERS.items()
                      if not bound(bindings, LOGO, symbol, code)), None)
    require(printable is not None, 'unbound_logo_printable_available')
    _, logo_code = printable
    mutated = False
    try:
        # Opening may succeed even when its acknowledgement is lost.
        mutated = True
        if not initial['launcher']:
            cli('launcher', 'open')
            eventually(lambda: client()['launcher'], 'launcher_opened')
        click_query(name, monitor)
        set_caps(name, False)
        for caps, shift in ((False, False), (False, True), (True, False), (True, True)):
            if caps and caps_remapped:
                continue
            set_caps(name, False)
            baseline(name)
            set_caps(name, caps)
            h['key'](name, 29, *((42,) if shift else ()), 30)
            h['key'](name, 45)
            query_is('X' if caps else 'x', 'control_a_replaces_whole_query')
        report['checks']['control_a_with_shift' if caps_remapped
                         else 'control_a_with_shift_caps_and_both'] = True

        for caps, shift in ((False, True), (True, False), (True, True)):
            if caps and caps_remapped:
                continue
            set_caps(name, False)
            baseline(name)
            set_caps(name, caps)
            h['key'](name, 29, *((42,) if shift else ()), 22)
            query_is('', 'control_u_clears_whole_query')
        report['checks']['control_u_with_shift' if caps_remapped
                         else 'control_u_with_shift_caps_and_both'] = True

        set_caps(name, False)
        for label, codes, _, _, _ in [*tested_ignored, ('logo_printable', (125, logo_code), 0, '', 0)]:
            baseline(name)
            h['key'](name, *codes)
            unchanged_query('qq', 'modified_chord_does_not_clear_or_type_' + label)
            h['key'](name, 45)
            query_is('qqx', 'modified_chord_does_not_select_' + label)
        report['checks']['tested_alt_logo_chords_preserve_content_and_selection'] = True
    finally:
        failures = []
        if mutated:
            try:
                h['qmp'](name, [{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
            except Exception:
                failures.append('pointer_release')
            try:
                current = client()
                if current['launcher']:
                    restore_query(name, initial['query'])
                else:
                    require(not initial['launcher'] and current['query'] == initial['query'],
                            'closed_launcher_query_unchanged')
                if not initial['launcher']:
                    cli('launcher', 'close')
                eventually(lambda: client()['launcher'] == initial['launcher']
                           and client()['query'] == initial['query'], 'original_launcher_state_restored')
                report['restoration']['launcher_open_and_query'] = True
            except Exception:
                failures.append('launcher_open_and_query')
                report['restoration']['launcher_open_and_query'] = False
        try:
            set_caps(name, original_caps)
            report['restoration']['caps_lock'] = True
        except Exception:
            failures.append('caps_lock')
            report['restoration']['caps_lock'] = False
        try:
            require(json.loads(remote('python3', '-c', PREFERENCES)) == preferences,
                    'exact_preferences_and_wallpaper_link_preserved')
            require(client()['wallpaper'] == initial['wallpaper'], 'selected_wallpaper_preserved')
            report['restoration']['settings_and_wallpaper'] = True
        except Exception:
            failures.append('settings_and_wallpaper')
            report['restoration']['settings_and_wallpaper'] = False
        require(not failures, 'native_input_state_restoration')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-bound-chords', action='store_true')
    parser.add_argument('--skip-remapped-caps', action='store_true')
    args = parser.parse_args()
    report = {'checks': {}, 'restoration': {}, 'passed': False,
              'skipped_compositor_bound_chords': [],
              'skipped_caps_lock_cases': [],
              'notes_load_error_acceptance': 'pending_theme_and_login_export_checkpoint'}
    try:
        measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
        with measure['stay_awake'](remote):
            suite(report, skip_bound_chords=args.skip_bound_chords,
                  skip_remapped_caps=args.skip_remapped_caps)
        report['passed'] = bool(report['checks']) and all(report['checks'].values())
    except CheckFailure as error:
        report['failed_check'] = str(error)
    except Exception:
        report['failed_check'] = 'native_input_acceptance_failed'
    path = ROOT / 'reports/local/input-settings-native.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
