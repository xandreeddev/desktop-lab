#!/usr/bin/env python3
"""Reversible display-fit acceptance for the prepared 4K60 Source workload.

Uses only read-only inspection and the same runtime hl.monitor configuration
path as test-wallpaper-resolution.py. Requires an unlocked, idle, closed-panel
single-output 1920x1080/1x guest already playing owned H.264 3840x2160 at 60/1.
Never changes wallpaper, manual pause, theme, keys, foreground apps or settings.
Output mode/scale and the existing stay-awake marker are restored on failure.
Exact settings bytes and source/wallpaper identities are checkpointed privately;
concurrent changes fail preservation checks and are never overwritten.

All captures/checkpoints remain private under ignored reports/local or guest
/tmp. Stdout and the summary contain booleans only. Build, deployment, resource
measurement, alpha pixels and multi-output/hardware acceptance are separate.
"""
import json
import math
import os
from pathlib import Path
import re
import runpy
import shlex
import time
import uuid

import lab
from native_surfaces import find_surface


ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
remote, session, inspect = [h[key] for key in ('remote', 'session', 'inspect')]
require, eventually, CheckFailure = [h[key] for key in ('require', 'eventually', 'CheckFailure')]
measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))


CHECKPOINT = r'''
import hashlib,json,os,stat,subprocess,sys
from pathlib import Path
operation,directory,monitor=sys.argv[1:]
directory=Path(directory)
info=directory.lstat()
assert directory.is_absolute() and directory.resolve()==directory
assert directory.name.startswith('lucent-live-fit-')
assert stat.S_ISDIR(info.st_mode) and info.st_uid==os.getuid() and stat.S_IMODE(info.st_mode)==0o700
def digest(data): return hashlib.sha256(data).hexdigest()
def identity(value):
    return [value.st_dev,value.st_ino,value.st_mode,value.st_size,value.st_mtime_ns]
def read(path,limit):
    descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
    with os.fdopen(descriptor,'rb') as stream:
        before=os.fstat(stream.fileno())
        assert stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_size<=limit
        data=stream.read(limit+1)
        assert len(data)<=limit and identity(before)==identity(os.fstat(stream.fileno()))
        return data,identity(before)
def hash_file(path,limit):
    descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
    with os.fdopen(descriptor,'rb') as stream:
        before=os.fstat(stream.fileno())
        assert stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_size<=limit
        hashed=hashlib.sha256(); count=0
        for block in iter(lambda:stream.read(1024*1024),b''):
            count+=len(block); assert count<=limit; hashed.update(block)
        assert count==before.st_size and identity(before)==identity(os.fstat(stream.fileno()))
        return hashed.hexdigest(),identity(before)
def private(path,data):
    descriptor=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
    with os.fdopen(descriptor,'wb') as stream:
        os.fchmod(stream.fileno(),0o600); stream.write(data)
base=Path.home()/'.local/state/lucent'
settings=base/'desktop.json'; wallpaper=base/'wallpaper'
if operation=='create':
    content,settings_id=read(settings,4*1024*1024)
    saved=json.loads(content); live=saved['live_wallpapers']
    assert live.get('paused',False) is False
    profile=live.get('monitors',{}).get(monitor,live.get('default'))
    assert profile and profile.get('enabled',True) is True and profile.get('paused',False) is False
    assert profile.get('fps','source')=='source'
    source=Path(profile['asset']['path'])
    assert source.is_absolute() and source.resolve(strict=True)==source
    store=(base/'live-wallpapers').resolve(strict=True)
    assert source.parent.parent==store and source.parent.name.startswith('media-')
    source_hash,source_id=hash_file(source,512*1024*1024)
    result=subprocess.run(['ffprobe','-v','error','-protocol_whitelist','file',
        '-select_streams','v:0','-show_entries',
        'stream=codec_name,width,height,avg_frame_rate,r_frame_rate,pix_fmt',
        '-of','json',str(source)],capture_output=True,timeout=15)
    assert result.returncode==0 and len(result.stdout)<=16384
    streams=json.loads(result.stdout)['streams']; assert len(streams)==1
    video=streams[0]
    assert video['codec_name']=='h264' and (video['width'],video['height'])==(3840,2160)
    assert video['avg_frame_rate']=='60/1' and video['r_frame_rate']=='60/1'
    assert video['pix_fmt']=='yuv420p'
    link_info=wallpaper.lstat(); assert stat.S_ISLNK(link_info.st_mode)
    resolved=wallpaper.resolve(strict=True)
    still_hash,still_id=hash_file(resolved,128*1024*1024)
    value={'monitor':monitor,'settings_sha256':digest(content),'settings_identity':settings_id,
        'source':str(source),'source_sha256':source_hash,'source_identity':source_id,
        'wallpaper_link':os.readlink(wallpaper),'wallpaper_identity':identity(link_info),
        'wallpaper_target':str(resolved),'wallpaper_sha256':still_hash,'wallpaper_target_identity':still_id,
        'probe':video}
    private(directory/'settings.backup',content)
    private(directory/'manifest.json',json.dumps(value).encode())
    print(json.dumps(value))
else:
    manifest_data,_=read(directory/'manifest.json',65536); value=json.loads(manifest_data)
    assert value['monitor']==monitor
    content,settings_id=read(settings,4*1024*1024)
    backup,_=read(directory/'settings.backup',4*1024*1024)
    source_hash,source_id=hash_file(Path(value['source']),512*1024*1024)
    link_info=wallpaper.lstat()
    still_hash,still_id=hash_file(Path(value['wallpaper_target']),128*1024*1024)
    preserved={'settings':content==backup and digest(content)==value['settings_sha256']
        and settings_id==value['settings_identity'],
        'source':source_hash==value['source_sha256'] and source_id==value['source_identity'],
        'wallpaper':stat.S_ISLNK(link_info.st_mode) and os.readlink(wallpaper)==value['wallpaper_link']
        and identity(link_info)==value['wallpaper_identity']
        and str(wallpaper.resolve(strict=True))==value['wallpaper_target']
        and still_hash==value['wallpaper_sha256'] and still_id==value['wallpaper_target_identity']}
    assert operation in ('verify','discard')
    if operation=='discard':
        assert all(preserved.values())
        assert {path.name for path in directory.iterdir()}=={'manifest.json','settings.backup'}
        assert all(path.is_file() and not path.is_symlink() for path in directory.iterdir())
        (directory/'settings.backup').unlink(); (directory/'manifest.json').unlink(); directory.rmdir()
    print(json.dumps(preserved))
'''


def private_file(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data)


def capture(directory, label, value):
    private_file(directory / (label + '.json'), (json.dumps(value, indent=2) + '\n').encode())


def monitors():
    value = json.loads(session('hyprctl', '-j', 'monitors'))
    require(isinstance(value, list) and len(value) == 1, 'one_native_output')
    return value[0]


def idle(state):
    client = state['client']
    require(not client['launcher'] and not client['query'] and not client['launcher_opening']
            and not client['dock_surface_expanded'] and client['notification_count'] == 0
            and not client['notification_history_open'] and not client['widgets_editing']
            and not client['widget_grid'], 'closed_idle_client')
    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper', 'settings'):
        value = client.get(panel, {})
        require(not value.get('open', False) and not value.get('visible', False)
                and not value.get('busy', False), 'other_panels_closed_and_idle')
    require(not client.get('application_actions', {}).get('open', False)
            and not client.get('application_actions', {}).get('busy', False), 'no_application_action')
    require(client['settings_writable'] and client['settings_save_status'] == 'saved'
            and not client['settings_save_error'] and not client['wallpaper_busy']
            and client['background_ready'] and not client['error'], 'saved_ready_desktop')
    require(all(not entry['presentation']['transition_pending']
                for entry in client['background_transitions']), 'settled_wallpaper_transition')
    require(not json.loads(session('hyprctl', '-j', 'activewindow')), 'no_foreground_application')
    require('lucent-authorization-' not in session('hyprctl', '-j', 'layers'), 'no_authorization_surface')
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'session_stays_unlocked')


def cover_size(width, height):
    if width >= 3840 or height >= 2160:
        return [3840, 2160]
    if width * 2160 >= height * 3840:
        return [width, (2160 * width + 3839) // 3840]
    return [(3840 * height + 2159) // 2160, height]


def current(monitor, width, height, scale):
    native = monitors()
    require(native['name'] == monitor['name'] and native['x'] == monitor['x']
            and native['y'] == monitor['y'] and native['transform'] == monitor['transform'],
            'same_native_output_identity_and_position')
    require((native['width'], native['height']) == (width, height)
            and math.isclose(native['scale'], scale, abs_tol=.0001)
            and math.isclose(native['refreshRate'], monitor['refreshRate'], abs_tol=.001),
            'native_monitor_mode_and_scale')
    state = inspect()
    background = find_surface(state, 'background')
    require([background['buffer_width'], background['buffer_height']] == [width, height]
            and math.isclose(background['density'], scale, abs_tol=.0001)
            and math.isclose(background['width'], width / scale, abs_tol=.001)
            and math.isclose(background['height'], height / scale, abs_tol=.001),
            'native_buffer_matches_physical_demand')
    if not scale.is_integer():
        require(background['buffer_scale'] == 1, 'fractional_viewport_buffer_scale')
    live = state['client']['live_wallpaper']
    require(live['players'] == 1 and live['running'] == 1 and len(live['producers']) == 1,
            'one_running_shared_producer')
    require(len(live['outputs']) == 1 and live['outputs'][0]['pause_reason'] is None,
            'unpaused_live_output')
    producer = live['producers'][0]
    require(set(producer) == {'revision', 'running', 'target', 'frame_size', 'source_aspect'},
            'path_free_producer_inspection')
    require(producer['running'] and producer['target'] == [width, height]
            and producer['frame_size'] == cover_size(width, height)
            and producer['source_aspect'] == [3840, 2160], 'fitted_frame_and_original_aspect')
    require(live['retained_frame_bytes'] == math.prod(producer['frame_size']) * 4,
            'single_fitted_immutable_frame_payload')
    return {'monitor': native, 'background': background, 'producer': producer}


def settled(monitor, width, height, scale):
    previous, since, result = None, time.monotonic(), None

    def ready():
        nonlocal previous, since, result
        try:
            value = current(monitor, width, height, float(scale))
        except (CheckFailure, KeyError, LookupError):
            previous, since = None, time.monotonic()
            return False
        fingerprint = (value['producer']['revision'], value['producer']['target'],
                       value['producer']['frame_size'], value['background']['width'],
                       value['background']['height'], value['background']['density'])
        if fingerprint != previous:
            previous, since = fingerprint, time.monotonic()
        result = value
        return time.monotonic() - since >= 1

    eventually(ready, 'fitted_generation_and_geometry_settle', timeout=40)
    return result


def advancing(monitor, width, height, scale):
    before = settled(monitor, width, height, scale)
    revision, frames = before['producer']['revision'], before['background']['frames']

    def progressed():
        value = current(monitor, width, height, float(scale))
        require(value['producer']['revision'] == revision, 'stable_current_producer_revision')
        return value['background']['frames'] >= frames + 3

    eventually(progressed, 'source_cadence_continues_after_retarget', timeout=8)
    return current(monitor, width, height, float(scale))


def mode_for(monitor, width, height):
    choices = []
    for mode in monitor['availableModes']:
        match = re.fullmatch(r'(\d+)x(\d+)@([0-9]+(?:\.[0-9]+)?)(?:Hz)?', mode)
        if (match and (int(match[1]), int(match[2])) == (width, height)
                and math.isclose(float(match[3]), monitor['refreshRate'], abs_tol=.01)):
            choices.append(f'{width}x{height}@{match[3]}')
    require(len(set(choices)) == 1, 'advertised_exact_refresh_mode_available')
    return choices[0]


def configure(monitor, mode, scale):
    # Exactly the established test-wallpaper-resolution.py Lua mutation path.
    code = 'hl.monitor({output=%s, mode=%s, position=%s, scale=%s})' % (
        json.dumps(monitor['name']), json.dumps(mode),
        json.dumps(f'{monitor["x"]}x{monitor["y"]}'), scale)
    require(session('hyprctl', 'eval', code).strip().lower() == 'ok', 'native_monitor_configure_acknowledged')


def suite(results, directory):
    initial = inspect()
    idle(initial)
    monitor = monitors()
    require(re.fullmatch(r'[A-Za-z0-9_.-]+', monitor['name']), 'literal_output_identity')
    require((monitor['width'], monitor['height'], monitor['scale'], monitor['x'], monitor['y'],
             monitor['transform']) == (1920, 1080, 1, 0, 0, 0), 'initial_1080p_scale_one_workload')
    original_mode = mode_for(monitor, 1920, 1080)
    small_mode = mode_for(monitor, 1024, 768)
    # This expression is read-only. Unsupported Lua/API/reply syntax fails before
    # any monitor or stay-awake mutation; no alternate command is invented.
    response = session('hyprctl', 'eval',
        'assert(type(hl) == "table" and type(hl.monitor) == "function", "lucent_display_fit_unavailable")')
    require(response.strip().lower() == 'ok', 'documented_monitor_api_available')
    require('producers' in initial['client']['live_wallpaper'], 'display_fit_inspector_available')
    pid = remote('pgrep', '-x', 'lucent-desktop').strip()
    require(pid.isdecimal(), 'one_current_desktop_process')
    scratch = remote('mktemp', '-d', '/tmp/lucent-live-fit-XXXXXXXXXXXX').strip()
    checkpointed, attempted, restored, successful = False, False, False, False
    original_wallpaper = initial['client']['wallpaper']

    def checkpoint(operation):
        return json.loads(session('python3', '-c', CHECKPOINT, operation, scratch, monitor['name']))

    def preserved():
        require(all(checkpoint('verify').values()), 'exact_settings_source_and_wallpaper_preserved')
        require(inspect()['client']['wallpaper'] == original_wallpaper, 'selected_wallpaper_identity_preserved')
        require(remote('pgrep', '-x', 'lucent-desktop').strip() == pid, 'desktop_process_not_restarted')

    try:
        manifest = checkpoint('create')
        checkpointed = True
        capture(directory, 'private-checkpoint', manifest)
        private_file(directory / 'settings.backup', h['run'](lab.ssh_args('lucent') + [
            shlex.join(['cat', scratch + '/settings.backup'])]))
        capture(directory, 'original-wallpaper', original_wallpaper)
        baseline = advancing(monitor, 1920, 1080, 1)
        capture(directory, 'initial-native', baseline)
        results['initial_owned_h264_4k60_source_workload'] = True
        with measure['stay_awake'](remote):
            try:
                previous = baseline
                for label, mode, width, height, scale in [
                    ('smaller_cover', small_mode, 1024, 768, 1),
                    ('integer_density', original_mode, 1920, 1080, 2),
                    ('fractional_density', original_mode, 1920, 1080, 1.25),
                ]:
                    idle(inspect())
                    attempted = True  # Covers a delivered command with a lost reply.
                    configure(monitor, mode, scale)
                    value = advancing(monitor, width, height, scale)
                    if previous['producer']['target'] != value['producer']['target']:
                        require(previous['producer']['revision'] != value['producer']['revision'],
                                'physical_target_change_retires_producer_generation')
                    capture(directory, label, value)
                    preserved()
                    results[label] = True
                    previous = value
                # Bounded rapid demand replacement must finish on the latest
                # geometry, with a stable current revision and advancing frames.
                for mode, scale in [(small_mode, 1), (original_mode, 1.25),
                                    (small_mode, 1), (original_mode, 1)]:
                    configure(monitor, mode, scale)
                value = advancing(monitor, 1920, 1080, 1)
                capture(directory, 'latest-after-burst', value)
                preserved()
                results['latest_target_after_burst'] = True
                results['producer_generation_retires_on_physical_change'] = True
                results['frames_advance_with_stable_fitted_geometry'] = True
            finally:
                if attempted:
                    configure(monitor, original_mode, monitor['scale'])
                    advancing(monitor, monitor['width'], monitor['height'], monitor['scale'])
                    native = monitors()
                    require(all(native[key] == monitor[key] for key in
                                ('name', 'width', 'height', 'refreshRate', 'scale', 'x', 'y', 'transform')),
                            'exact_original_output_restored')
                restored = True
                results['original_output_restored'] = True
        results['idle_marker_restored'] = True
        idle(inspect())
        results['closed_idle_state_preserved'] = True
        successful = True
    finally:
        if checkpointed:
            preserved()
            results['exact_settings_wallpaper_and_source_preserved'] = True
            if restored and successful:
                checkpoint('discard')
                results['owned_guest_checkpoint_removed'] = True
        # Never restore settings bytes over an independent change. Failure
        # retains the guest checkpoint and any completed private host backup.


def main():
    reports = ROOT / 'reports/local'
    reports.mkdir(parents=True, exist_ok=True)
    directory = reports / ('live-display-fit-' + uuid.uuid4().hex)
    directory.mkdir(mode=0o700)
    results = {'passed': False}
    failure = None
    try:
        suite(results, directory)
        results['passed'] = True
    except CheckFailure as error:
        failure = str(error)
    except Exception:
        failure = 'unexpected_bounded_display_fit_failure'
    if failure:
        capture(directory, 'failure', {'check': failure})
    private_file(reports / ('live-display-fit-summary-' + directory.name.rsplit('-', 1)[-1] + '.json'),
                 (json.dumps(results, indent=2) + '\n').encode())
    print(json.dumps(results))
    if failure:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
