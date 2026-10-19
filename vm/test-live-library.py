#!/usr/bin/env python3
"""Reversible native acceptance for the installed pixel live-wallpaper library.

Requires an unlocked single-output Lucent VM, closed panels, writable settings,
and the bundled Moonlit/Amber samples already in ~/Videos/Wallpapers. Installs
nothing. QMP opens Wallpapers -> Live, selects each discovered sample card
without saving, then explicitly clicks Apply. Native background submissions
establish playback after the poster transition settles.
Both imports must default to their source frame rate. Native Tab-revealed FPS
controls and the rate CLI change only the default profile's frame policy; an
explicit source policy and advancing frames must survive one service restart.
No screen recording, benchmark, credentials, external provider or lock action.

The original wallpaper snapshot is checkpointed before temporary imports. Exact
preferences/links, theme/login exports, workspace/focus, bar and idle policy are
restored. Cleanup removes only newly created owned imports whose exact source is
one of the samples, never preexisting assets or source videos. Failed restoration
retains rollback material. Reports contain fixed check labels, counts and the
bundled sample names rather than paths, wallpaper content or user preferences.
"""

import argparse
import json
from pathlib import Path
import runpy
import sys
import time

import lab
from native_surfaces import find_surface
from wallpaper_checkpoint import GuestWallpaperCheckpoint


ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
remote, session, cli, inspect, client = [h[key] for key in
                                       ('remote', 'session', 'cli', 'inspect', 'client')]
require, eventually = h['require'], h['eventually']


# Match NativeVideoWallpapers::library: nonrecursive, first 1024 directory
# entries, regular non-symlink files, accepted extension/size/UTF-8, transformed
# name then path ordering, first 32 choices. Do not assume only our samples exist.
DISCOVER = '''import hashlib, itertools, json, os, sys, unicodedata
from pathlib import Path
expected=json.loads(sys.argv[1]); root=Path.home()/'Videos/Wallpapers'; sources=[]
with os.scandir(root) as entries:
    for entry in itertools.islice(entries,1024):
        if not entry.is_file(follow_symlinks=False): continue
        path=Path(entry.path)
        if path.suffix.lower() not in ('.mp4','.webm','.gif'): continue
        if not 0<entry.stat().st_size<=512*1024*1024: continue
        try: str(path).encode('utf-8')
        except UnicodeEncodeError: continue
        name=path.stem.replace('_',' ').replace('-',' ')
        name=''.join(char for char in name if unicodedata.category(char)!='Cc')[:64]
        sources.append((name,str(path)))
sources=sorted(sources)[:32]; samples=[]
for filename,digest in expected.items():
    path=root/filename
    if path.is_symlink() or not path.is_file(): raise ValueError('installed_sample_required')
    with path.open('rb') as source: actual=hashlib.file_digest(source,'sha256').hexdigest()
    if actual!=digest: raise ValueError('installed_sample_checksum')
    indices=[index for index,(_,source) in enumerate(sources) if source==str(path)]
    if len(indices)!=1: raise ValueError('sample_not_in_bounded_library')
    samples.append({'filename':filename,'path':str(path),'index':indices[0]})
print(json.dumps({'count':len(sources),'samples':samples}))
'''


ASSET_BASELINE = '''import json, sys
from pathlib import Path
scratch=Path(sys.argv[1]); state=Path.home()/'.local/state/lucent'
before={}
for store,prefix in [('live-wallpapers','media-'),('wallpaper-assets','asset-')]:
    root=state/store
    before[store]=[path.name for path in root.glob(prefix+'*')]
(scratch/'asset-baseline.json').write_text(json.dumps({
    'before':before,'sources':json.loads(sys.argv[2])}))
'''


ISOLATE = '''import json
from pathlib import Path
path=Path.home()/'.local/state/lucent/desktop.json'; value=json.loads(path.read_text())
live=value.setdefault('live_wallpapers',{})
live.update(default=None,monitors={},paused=False)
temporary=path.with_name('desktop.live-library-test.new')
temporary.write_text(json.dumps(value)); temporary.replace(path)
'''


CLEANUP = '''import json, shutil, sys
from pathlib import Path
scratch=Path(sys.argv[1]); baseline=json.loads((scratch/'asset-baseline.json').read_text())
state=Path.home()/'.local/state/lucent'; settings=json.loads((state/'desktop.json').read_text())
live=settings.get('live_wallpapers',{}); referenced=set()
for profile in [live.get('default'),*live.get('monitors',{}).values()]:
    if profile: referenced.add(profile['asset']['path'])
selected=(state/'wallpaper').resolve(); media=[]; posters=set(); static=[]
store=(state/'live-wallpapers').resolve()
for directory in store.glob('media-*'):
    if directory.name in baseline['before']['live-wallpapers']: continue
    if directory.is_symlink() or not directory.is_dir(): continue
    metadata=directory/'asset.json'
    if metadata.is_symlink() or not metadata.is_file(): continue
    asset=json.loads(metadata.read_text())
    if asset['source']['path'] not in baseline['sources']: continue
    path=Path(asset['path']); poster=Path(asset['poster']['path'])
    if (directory.parent!=store or path.parent!=directory or path.is_symlink()
            or path.resolve()!=path or not path.is_file()
            or poster!=directory/'poster.png' or poster.is_symlink()
            or not poster.is_file() or str(path) in referenced or poster==selected):
        raise ValueError('sample_media_cleanup_guard')
    media.append(directory); posters.add(str(poster))
store=(state/'wallpaper-assets').resolve()
for directory in store.glob('asset-*'):
    if directory.name in baseline['before']['wallpaper-assets']: continue
    if directory.is_symlink() or not directory.is_dir(): continue
    metadata=directory/'source.json'
    if metadata.is_symlink() or not metadata.is_file(): continue
    asset=json.loads(metadata.read_text())
    if asset['source']['path'] not in posters: continue
    path=Path(asset['path'])
    if (directory.parent!=store or path.parent!=directory or path.is_symlink()
            or path.resolve()!=path or not path.is_file() or path==selected):
        raise ValueError('sample_poster_cleanup_guard')
    static.append(directory)
# All guards pass before removing either store. Original source videos remain.
for directory in static+media: shutil.rmtree(directory)
print(json.dumps({'new_sample_media_removed':len(media),'new_sample_posters_removed':len(static)}))
shutil.rmtree(scratch)
'''


def dispatch(kind, selector):
    require(bool(selector) and all(ord(char) >= 32 and ord(char) != 127 for char in selector),
            'bounded_compositor_selector')
    expression = 'hl.dsp.focus({ ' + kind + ' = ' + json.dumps(selector, ensure_ascii=False) + ' })'
    require(session('hyprctl', 'dispatch', expression).strip() == 'ok', 'native_focus_dispatch')


def click(name, monitor, logical_role, identifier):
    """Click the stable visible part of an actual native hit rectangle."""
    previous = None
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        surface = find_surface(inspect(), logical_role)
        hits = [hit for hit in surface['hits'] if hit['id'] == identifier]
        if len(hits) == 1:
            hit = hits[0]
            clip = hit['clip']
            left, top = max(hit['x'], clip['x']), max(hit['y'], clip['y'])
            right = min(hit['x'] + hit['width'], clip['x'] + clip['width'])
            bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'])
            current = (left, top, right, bottom)
            if right - left > 2 and bottom - top > 2 and current == previous:
                origin = h['layer_origin'](surface)
                h['move'](name, monitor, origin['x'] + (left + right) / 2,
                          origin['y'] + (top + bottom) / 2)
                time.sleep(.12)
                h['qmp'](name, [{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
                time.sleep(.05)
                h['qmp'](name, [{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
                return
            previous = current
        time.sleep(.15)
    raise h['CheckFailure']('native_library_click_target')


def reveal(name, identifier):
    # Tab only reveals a possibly offscreen library row through SDK scrolling;
    # the choice is then made by an actual pointer click, not an IPC import.
    for _ in range(96):
        if find_surface(inspect(), 'live-wallpaper').get('focus') == identifier:
            return
        h['key'](name, 15)
        time.sleep(.04)
    raise h['CheckFailure']('native_library_keyboard_reveal')


def select_library_card(name, monitor, index):
    # The carousel mounts only nearby cards and Open retains its old selection.
    # Reach the discovered sample with actual controls before clicking its card.
    for _ in range(33):
        current = client()['live_wallpaper']['selection']
        if current == index:
            identifier = 'live-library-' + str(index)
            reveal(name, identifier)
            click(name, monitor, 'live-wallpaper', identifier)
            return
        identifier = 'live-next' if current < index else 'live-previous'
        reveal(name, identifier)
        click(name, monitor, 'live-wallpaper', identifier)
        expected = current + (1 if current < index else -1)
        eventually(lambda: client()['live_wallpaper']['selection'] == expected,
                   'native_library_browse_step')
    raise h['CheckFailure']('native_library_sample_not_reachable')


def open_library(name, monitor, count):
    cli('launcher', 'open')
    eventually(lambda: client()['launcher'] and find_surface(inspect(), 'dock')['frames'] > 0,
               'launcher_native_presented')
    time.sleep(.85)
    click(name, monitor, 'dock', 'mode-wallpapers')
    eventually(lambda: client()['launcher'] and client()['mode'] == 'Wallpapers',
               'native_wallpapers_mode')
    click(name, monitor, 'dock', 'source-live')
    eventually(lambda: client()['live_wallpaper']['open'] and not client()['launcher']
               and not client()['live_wallpaper']['awaiting_first_frame']
               and not client()['live_wallpaper']['library_loading']
               and client()['live_wallpaper']['library_count'] == count
               and count >= 2 and not client()['live_wallpaper']['library_error']
               and find_surface(inspect(), 'live-wallpaper')['frames'] > 0,
               'native_live_library_presented', timeout=45)
    time.sleep(.85)  # Native first submission precedes the 850 ms diagonal reveal.


def without_default_rate(settings):
    """Compare all saved preferences while permitting only this profile's FPS."""
    value = dict(settings)
    live = dict(value['live_wallpapers'])
    profile = dict(live['default'])
    profile.pop('fps', None)
    live['default'] = profile
    value['live_wallpapers'] = live
    return value


def source_rate_acceptance(name, monitor, count, report):
    committed = h['read_settings']()
    wallpaper = client()['wallpaper']
    require(committed['live_wallpapers']['default']['fps'] == 'source',
            'first_sample_defaults_to_source_rate')

    def check_rate(rate, label):
        eventually(lambda: h['saved']()
                   and h['read_settings']()['live_wallpapers']['default']['fps'] == rate,
                   label + '_persisted')
        observed = h['read_settings']()
        # This includes the asset, enabled/paused/power policies, monitor
        # profiles, static wallpaper and every unrelated desktop preference.
        require(without_default_rate(observed) == without_default_rate(committed)
                and client()['wallpaper'] == wallpaper,
                label + '_preserves_asset_policy_pause_and_other_preferences')

    open_library(name, monitor, count)
    reveal(name, 'live-fps-15')
    click(name, monitor, 'live-wallpaper', 'live-fps-15')
    check_rate(15, 'native_15_fps_cap')
    reveal(name, 'live-fps-source')
    click(name, monitor, 'live-wallpaper', 'live-fps-source')
    check_rate('source', 'native_source_frame_rate')
    h['key'](name, 1)
    eventually(lambda: not client()['live_wallpaper']['open'], 'native_rate_panel_escape')
    time.sleep(.85)
    report['checks']['native_tab_reveal_and_click_15_then_source'] = True

    cli('wallpaper', 'live', 'rate', '60')
    check_rate(60, 'cli_60_fps_cap')
    cli('wallpaper', 'live', 'rate', 'source')
    check_rate('source', 'cli_source_frame_rate')
    require(h['read_settings']() == committed, 'source_rate_returns_exact_saved_preferences')
    report['checks']['cli_60_then_source_preserves_other_preferences'] = True

    session('systemctl', '--user', 'restart', 'lucent.service')
    eventually(lambda: client()['background_ready'] and h['saved']()
               and client()['live_wallpaper']['players'] == 1
               and client()['live_wallpaper']['running'] == 1
               and client()['live_wallpaper']['retained_frame_bytes'] > 0
               and all(item['pause_reason'] is None
                       for item in client()['live_wallpaper']['outputs'])
               and h['read_settings']() == committed
               and client()['wallpaper'] == wallpaper,
               'explicit_source_rate_and_selection_survive_restart', timeout=45)
    first = h['background_frames']()
    eventually(lambda: h['background_frames']() > first + 3,
               'source_rate_restart_native_frames_advance')
    report['source_restart_background_frames_advanced'] = h['background_frames']() - first
    report['checks']['explicit_source_rate_survives_restart_with_advancing_frames'] = True
    report['checks']['rate_changes_preserve_asset_pause_policy_and_unrelated_preferences'] = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-report', type=Path,
                        help='Optional private release report used to verify the running desktop hash.')
    args = parser.parse_args()
    directory = lab.ROOT / 'reports/local' / ('live-library-' + time.strftime('%Y%m%d-%H%M%S'))
    directory.mkdir(parents=True, exist_ok=False)
    report = {'checks': {}, 'restoration': {}, 'samples': [], 'passed': False}
    scratch = checkpoint = original = original_idle = original_bar = workspace = focus = None
    backup_ready = False
    phase = 'preflight'
    try:
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
        eventually(lambda: client()['background_ready'] and h['saved']() and not client()['wallpaper_busy'],
                   'initial_desktop_ready')
        before = inspect()
        require('Vulkan' in before['adapter'], 'native_vulkan')
        require(before['client']['settings_writable'] and bool(before['client']['wallpaper']),
                'writable_preferences_and_original_wallpaper')
        require(not before['client']['launcher'] and not before['client']['live_wallpaper']['open']
                and not before['client']['notification_history_open']
                and not any(before['client'][name]['open'] for name in ('audio','network','bluetooth','power','tray')),
                'initial_panels_closed')
        monitors = json.loads(session('hyprctl', '-j', 'monitors'))
        require(len(monitors) == 1 and monitors[0]['scale'] == 1
                and monitors[0]['x'] == 0 and monitors[0]['y'] == 0, 'single_origin_output_at_1x')
        monitor = monitors[0]
        require(not monitor.get('specialWorkspace', {}).get('id', 0), 'ordinary_initial_workspace')
        active = monitor['activeWorkspace']
        workspace = active.get('address') or (str(active['id']) if active.get('id', 0) > 0
                                               else 'name:' + active['name'])
        focus = json.loads(session('hyprctl', '-j', 'activewindow')).get('address')
        if args.build_report:
            build = json.loads(args.build_report.read_text())
            pid = remote('pgrep', '-x', 'lucent-desktop').strip()
            expected = build['binaries']['lucent-desktop']['sha256']
            require(remote('sha256sum', f'/proc/{pid}/exe').split()[0] == expected, 'running_binary_identity')
            report['tested_implementation_commit'] = build['source_commit']
            report['desktop_sha256'] = expected
        manifest = json.loads((ROOT / 'assets/live-wallpapers/manifest.json').read_text())
        expected = {item['filename']: item['sha256'] for item in manifest['samples']}
        require(set(expected) == {'moonlit-lake.mp4', 'amber-lake.mp4'}, 'bundled_sample_manifest')
        library = json.loads(remote('python3', '-c', DISCOVER, json.dumps(expected)))
        require(library['count'] >= 2 and len(library['samples']) == 2, 'installed_samples_discovered')
        report['library_count'] = library['count']
        original = before['client']['wallpaper']
        original_bar = before['client']['bar_visible']
        original_idle = json.loads(session('omarchy-toggle-idle', 'status'))['enabled']
        scratch = remote('mktemp', '-d', '/tmp/lucent-live-library.XXXXXX').strip()
        remote('python3', '-c', h['BACKUP'], scratch)
        backup_ready = True
        remote('python3', '-c', ASSET_BASELINE, scratch,
               json.dumps([sample['path'] for sample in library['samples']]))
        checkpoint = GuestWallpaperCheckpoint(remote, original)
        session('omarchy-toggle-idle', 'stay-awake')
        dispatch('workspace', 'name:lucent-live-library-' + str(time.monotonic_ns()))
        session('systemctl', '--user', 'stop', 'lucent.service')
        remote('python3', '-c', ISOLATE)
        session('systemctl', '--user', 'start', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['live_wallpaper']['players'] == 0,
                   'isolated_desktop_ready')
        name = lab.inventory()['vms']['lucent']['name']
        previous_asset = None
        for index, sample in enumerate(library['samples']):
            phase = 'native_' + sample['filename'].removesuffix('.mp4')
            open_library(name, monitor, library['count'])
            before_selection = h['settings_digest']()
            before_wallpaper = client()['wallpaper']
            select_library_card(name, monitor, sample['index'])
            eventually(lambda: client()['live_wallpaper']['selection'] == sample['index']
                       and client()['live_wallpaper']['preview']['frame_size'] is not None
                       and not client()['live_wallpaper']['preview']['loading']
                       and not client()['live_wallpaper']['preview']['failed']
                       and not client()['live_wallpaper']['importing'],
                       'native_sample_read_only_preview', timeout=45)
            require(h['saved']() and h['settings_digest']() == before_selection
                    and client()['wallpaper'] == before_wallpaper,
                    'native_card_selection_preserves_preferences_and_wallpaper')
            reveal(name, 'live-apply')
            click(name, monitor, 'live-wallpaper', 'live-apply')
            eventually(lambda: not client()['live_wallpaper']['importing'] and h['saved']()
                       and (h['read_settings']()['live_wallpapers']['default'] or {})
                       .get('asset', {}).get('source', {}).get('path') == sample['path'],
                       'native_sample_import_and_save', timeout=45)
            profile = h['read_settings']()['live_wallpapers']['default']
            require(profile['fps'] == 'source', 'new_sample_profile_defaults_to_source_rate')
            asset = profile['asset']
            require(asset['path'] != previous_asset, 'new_sample_producer_asset')
            previous_asset = asset['path']
            eventually(lambda: not client()['wallpaper_busy'] and client()['background_ready']
                       and client()['wallpaper']['source']['path'] == asset['poster']['path']
                       and all(not item['presentation']['transition_pending']
                               for item in client()['background_transitions']),
                       'sample_poster_commit_and_transition_settle', timeout=45)
            require(not client()['wallpaper_error'] and not client()['theme_error'], 'sample_export_error')
            h['key'](name, 1)
            eventually(lambda: not client()['live_wallpaper']['open'], 'native_library_escape')
            time.sleep(.85)
            eventually(lambda: client()['live_wallpaper']['players'] == 1
                       and client()['live_wallpaper']['running'] == 1
                       and client()['live_wallpaper']['retained_frame_bytes'] > 0
                       and all(item['pause_reason'] is None for item in client()['live_wallpaper']['outputs']),
                       'sample_native_producer_running')
            first = h['background_frames']()
            eventually(lambda: h['background_frames']() > first + 3, 'sample_native_background_frames_advance')
            last = h['background_frames']()
            require(h['saved']() and h['read_settings']()['live_wallpapers']['default']['asset'] == asset,
                    'sample_saved_profile_stays_selected')
            report['samples'].append({'filename': sample['filename'], 'library_index': sample['index'],
                                      'selection_preserved_preferences_and_wallpaper': True,
                                      'explicit_apply_saved': True,
                                      'saved_source_matches': True, 'producer_running': True,
                                      'saved_frame_rate': 'source',
                                      'background_frames_advanced': last - first})
            if index == 0:
                phase = 'native_and_cli_source_frame_rate'
                source_rate_acceptance(name, monitor, library['count'], report)
        report['checks']['native_wallpapers_live_route_and_library'] = True
        report['checks']['native_first_submission_then_diagonal_cut_settle'] = True
        report['checks']['both_sample_card_selections_preserve_preferences_and_wallpaper'] = True
        report['checks']['both_discovered_samples_explicitly_apply_and_save'] = True
        report['checks']['both_new_sample_profiles_default_to_source_frame_rate'] = True
        report['checks']['both_sample_producers_submit_advancing_background_frames'] = True
        report['passed'] = len(report['samples']) == 2 and all(report['checks'].values())
    except BaseException as error:
        report['failed_phase'] = phase
        report['failed_check'] = str(error) if isinstance(error, h['CheckFailure']) else type(error).__name__
    finally:
        def restore(label, operation):
            try:
                operation()
                report['restoration'][label] = True
            except BaseException:
                report['restoration'][label] = False
                report['passed'] = False
        if backup_ready:
            if checkpoint is not None:
                restore('selected_snapshot_checkpoint', checkpoint.restore)
            def public_restore():
                cli('launcher', 'close')
                cli('wallpaper', 'live', 'close')
                cli('wallpaper', 'set', original['path'])
                eventually(lambda: client()['background_ready'] and not client()['wallpaper_busy']
                           and client()['wallpaper']['source'] == original['source'],
                           'original_wallpaper_transaction', timeout=45)
            restore('original_wallpaper_transaction', public_restore)
            def replace_files():
                session('systemctl', '--user', 'stop', 'lucent.service')
                remote('python3', '-c', h['RESTORE_FILES'], scratch)
            restore('exact_files_replaced_while_stopped', replace_files)
            def restart():
                session('systemctl', '--user', 'start', 'lucent.service')
                eventually(lambda: client()['background_ready'] and client()['wallpaper'] == original,
                           'original_desktop_ready', timeout=45)
                eventually(lambda: all(json.loads(remote('python3', '-c', h['VERIFY_RESTORED'], scratch)).values()),
                           'exact_settings_theme_login_exports', timeout=45)
            restore('exact_settings_wallpaper_theme_login_exports', restart)
            def restore_bar():
                cli('bar', 'show' if original_bar else 'hide')
                eventually(lambda: client()['bar_visible'] == original_bar, 'original_bar_visibility')
            restore('bar_visibility', restore_bar)
            def restore_idle():
                if original_idle is False:
                    session('omarchy-toggle-idle', 'allow-idle')
                require(json.loads(session('omarchy-toggle-idle', 'status'))['enabled'] == original_idle,
                        'original_idle_policy')
            restore('idle_policy', restore_idle)
            def restore_workspace():
                dispatch('workspace', workspace)
                if focus:
                    dispatch('window', focus)
            restore('workspace_and_focus', restore_workspace)
            if report['restoration'].get('exact_settings_wallpaper_theme_login_exports'):
                def cleanup():
                    report['cleanup'] = json.loads(remote('python3', '-c', CLEANUP, scratch))
                    if checkpoint is not None:
                        checkpoint.discard()
                restore('new_test_owned_assets_removed', cleanup)
        (directory / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        (lab.ROOT / 'reports/local/live-library-native.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2), flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
