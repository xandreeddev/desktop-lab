#!/usr/bin/env python3
"""Reversible native live-wallpaper acceptance in an already unlocked Lucent VM.

Uses guest ffmpeg only to encode tiny synthetic fixtures. Rust imports/decodes
them; the public inspector, native QMP input and grim captures establish actual
presentation. No deployment, guest startup or hardware claims. The optional
lock check requires a private login file and uses the guarded native input
helper; credentials never enter reports, arguments, environment or stdout.
Private rollback data remains in guest /tmp until restoration succeeds. Reports
and captures stay under ignored reports/local; stdout contains only sanitized
booleans/counts. Requires one display at origin, ffmpeg and grim in the guest.
"""
import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys
import time

import lab
from native_surfaces import find_surface
from wallpaper_checkpoint import GuestWallpaperCheckpoint


PROFILE = 'lucent'


class CheckFailure(Exception):
    """Only a fixed check label may leave this harness."""


def require(condition, label):
    if not condition:
        raise CheckFailure(label)


def run(command, *, data=None, timeout=40):
    try:
        result = subprocess.run(command, input=data, capture_output=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        raise CheckFailure('bounded_subprocess_failed') from None
    require(result.returncode == 0, 'bounded_subprocess_failed')
    return result.stdout


def remote(*command, timeout=30):
    return run(lab.ssh_args(PROFILE) + [shlex.join(command)], timeout=timeout).decode()


def session(*command, timeout=30):
    return remote('python3', 'desktop-lab/scripts/in-session.py', *command, timeout=timeout)


def measure_resources(scratch, label):
    pid = int(remote('pgrep', '-x', 'lucent-desktop').strip())
    result = json.loads(session('python3', 'desktop-lab/scripts/benchmark.py',
                               '--pid', str(pid), '--seconds', '120',
                               '--output', scratch + '/' + label + '.json', timeout=150))
    require(result['duration_seconds'] >= 120, 'resource_sample_duration')
    return result


def cli(*command):
    return session('/home/omarchy/.local/bin/lucent-cli', *command)


def inspect():
    return json.loads(cli('inspect'))


def client():
    return inspect()['client']


def eventually(predicate, label, timeout=35):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (CheckFailure, ValueError, KeyError, LookupError):
            pass
        time.sleep(.2)
    raise CheckFailure(label)


def saved():
    return client()['settings_save_status'] == 'saved'


def read_settings():
    return json.loads(remote('python3', '-c',
        "from pathlib import Path; print((Path.home()/'.local/state/lucent/desktop.json').read_text())"))


def settings_digest():
    return remote('python3', '-c', "import hashlib; from pathlib import Path; "
                  "print(hashlib.sha256((Path.home()/'.local/state/lucent/desktop.json').read_bytes()).hexdigest())").strip()


def background_frames():
    return int(find_surface(inspect(), 'background')['frames'])


def qmp(name, events):
    result = run(['virsh', '-c', lab.URI, 'qemu-monitor-command', name,
                  json.dumps({'execute': 'input-send-event', 'arguments': {'events': events}})], timeout=10)
    require('error' not in json.loads(result), 'qmp_input_rejected')


def key(name, *codes):
    run(['virsh', '-c', lab.URI, 'send-key', name, '--codeset', 'linux',
         '--holdtime', '25', *map(str, codes)], timeout=10)
    time.sleep(.035)


def type_ascii(name, value):
    letters = dict(zip('qwertyuiopasdfghjklzxcvbnm',
                       [16,17,18,19,20,21,22,23,24,25,30,31,32,33,34,35,36,37,38,44,45,46,47,48,49,50]))
    plain = {**letters, **dict(zip('1234567890', range(2, 12))),
             '/': 53, '.': 52, '-': 12, ' ': 57}
    for char in value:
        if char.isupper() and char.lower() in letters:
            key(name, 42, letters[char.lower()])
        elif char == '_':
            key(name, 42, 12)
        else:
            require(char in plain, 'fixture_path_keyboard_mapping')
            key(name, plain[char])


def move(name, monitor, x, y):
    qmp(name, [{'type': 'abs', 'data': {'axis': axis, 'value': round(value / (size - 1) * 32767)}}
               for axis, value, size in [('x', x * monitor['scale'], monitor['width']),
                                         ('y', y * monitor['scale'], monitor['height'])]])


def layer_origin(surface):
    namespace = 'lucent-' + surface['id']
    matches = []
    def visit(value):
        if isinstance(value, dict):
            if value.get('namespace') == namespace:
                matches.append(value)
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)
    visit(json.loads(session('hyprctl', '-j', 'layers')))
    require(len(matches) == 1, 'native_panel_layer_geometry')
    return matches[0]


def click(name, monitor, identifier):
    previous = None
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        surface = find_surface(inspect(), 'live-wallpaper')
        hits = [hit for hit in surface['hits'] if hit['id'] == identifier]
        if hits:
            h = hits[0]
            clip = h['clip']
            left, top = max(h['x'], clip['x']), max(h['y'], clip['y'])
            right = min(h['x'] + h['width'], clip['x'] + clip['width'])
            bottom = min(h['y'] + h['height'], clip['y'] + clip['height'])
            current = (left, top, right, bottom)
            if right - left > 2 and bottom - top > 2 and current == previous:
                origin = layer_origin(surface)
                move(name, monitor, origin['x'] + (left + right) / 2,
                     origin['y'] + (top + bottom) / 2)
                time.sleep(.12)
                qmp(name, [{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
                time.sleep(.05)
                qmp(name, [{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
                return
            previous = current
        time.sleep(.15)
    raise CheckFailure('native_panel_click_target')


def focus(name, identifier):
    # Tab traversal must reveal an offscreen FPS control through the SDK scroll.
    for _ in range(32):
        if find_surface(inspect(), 'live-wallpaper').get('focus') == identifier:
            return
        key(name, 15)
        time.sleep(.06)
    raise CheckFailure('native_panel_keyboard_reveal')


def ppm_samples(data):
    # grim's P6 output avoids a host image-library dependency.
    tokens, offset = [], 0
    while len(tokens) < 4:
        while offset < len(data) and data[offset:offset+1].isspace():
            offset += 1
        if data[offset:offset+1] == b'#':
            offset = data.index(b'\n', offset) + 1
            continue
        end = offset
        while end < len(data) and not data[end:end+1].isspace():
            end += 1
        tokens.append(data[offset:end])
        offset = end
    require(tokens[0] == b'P6' and tokens[3] == b'255', 'native_capture_format')
    width, height = int(tokens[1]), int(tokens[2])
    # Consume the header delimiter only: a binary pixel may itself be whitespace.
    offset += 2 if data[offset:offset+2] == b'\r\n' else 1
    require(len(data) - offset == width * height * 3, 'native_capture_storage')
    samples = []
    for fy in (.28, .48, .68):
        for fx in (.15, .5, .85):
            at = offset + (int(height * fy) * width + int(width * fx)) * 3
            samples.append(tuple(data[at:at+3]))
    return samples


def capture(scratch, report_dir, number):
    destination = scratch + '/capture.ppm'
    session('grim', '-t', 'ppm', destination)
    data = run(lab.ssh_args(PROFILE) + [shlex.join(['cat', destination])])
    (report_dir / f'capture-{number:03}.ppm').write_bytes(data)
    return ppm_samples(data)


def color(pixel):
    ordered = sorted(enumerate(pixel), key=lambda part: part[1], reverse=True)
    return ordered[0][0] if ordered[0][1] >= 160 and ordered[1][1] <= 95 else None


def verify_colors(scratch, report_dir, captures):
    observed = [set() for _ in range(9)]
    for _ in range(12):
        samples = capture(scratch, report_dir, captures[0])
        captures[0] += 1
        for values, pixel in zip(observed, samples):
            category = color(pixel)
            if category is not None:
                values.add(category)
        if any(len(values) >= 2 for values in observed):
            return [index for index, values in enumerate(observed) if len(values) >= 2]
        time.sleep(.3)
    raise CheckFailure('native_background_pixels_did_not_change')


ENCODE = '''import subprocess, sys
from pathlib import Path
root=Path(sys.argv[1])
inputs=[]
for color in ('red','lime','blue'):
    inputs += ['-f','lavfi','-i',f'color=c={color}:s=160x90:r=15:d=1']
common=['ffmpeg','-nostdin','-hide_banner','-loglevel','error','-y',*inputs,
        '-filter_complex_threads','1','-filter_complex',
        '[0:v][1:v][2:v]concat=n=3:v=1:a=0','-an','-threads','1']
for extension, options in [('mp4',['-c:v','libx264','-preset','ultrafast','-pix_fmt','yuv420p']),
                           ('webm',['-c:v','libvpx','-deadline','realtime','-cpu-used','8','-b:v','300k']),
                           ('gif',['-c:v','gif'])]:
    subprocess.run([*common,*options,str(root/('fixture.'+extension))],check=True,
                   stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=30)
(root/'invalid.mp4').write_bytes(b'Not media')
'''


# Backups never enter a local report or stdout. Keep exact settings/link state,
# plus managed export digests to verify the normal exporters restore the palette.
BACKUP = '''import hashlib, json, os, shutil, sys
from pathlib import Path
scratch=Path(sys.argv[1]); state=Path.home()/'.local/state/lucent'
settings=state/'desktop.json'
if not settings.is_file() or settings.is_symlink(): raise ValueError('Existing ordinary settings required')
if (state/'wallpaper').exists() and not (state/'wallpaper').is_symlink(): raise ValueError('Unexpected selection storage')
manifest={'settings_exists':settings.exists(),'links':{},'exports':{}}
if settings.exists(): shutil.copy2(settings,scratch/'desktop.json')
for path in (state/'wallpaper',Path.home()/'.local/state/omarchy/current/background'):
    manifest['links'][str(path)]=os.readlink(path) if path.is_symlink() else None
paths=list((state/'theme').glob('*'))
paths += list((Path('/var/lib/lucent/wallpapers')/str(os.getuid())).glob('*'))
paths += [Path.home()/'.config/omarchy/themes/lucent-tokens-active/colors.toml']
for path in paths:
    if path.is_file(): manifest['exports'][str(path)]=hashlib.sha256(path.read_bytes()).hexdigest()
(scratch/'rollback.json').write_text(json.dumps(manifest))
'''


RESTORE_FILES = '''import json, os, shutil, sys
from pathlib import Path
scratch=Path(sys.argv[1]); manifest=json.loads((scratch/'rollback.json').read_text())
state=Path.home()/'.local/state/lucent'; settings=state/'desktop.json'
if manifest['settings_exists']:
    shutil.copy2(scratch/'desktop.json',state/'desktop.restore.new')
    (state/'desktop.restore.new').replace(settings)
else: settings.unlink(missing_ok=True)
for name,target in manifest['links'].items():
    path=Path(name)
    if target is not None:
        temporary=path.with_name(path.name+'.live-test-restore')
        temporary.unlink(missing_ok=True); temporary.symlink_to(target); temporary.replace(path)
    elif path.is_symlink(): path.unlink()
'''


VERIFY_RESTORED = '''import hashlib, json, sys
from pathlib import Path
scratch=Path(sys.argv[1]); manifest=json.loads((scratch/'rollback.json').read_text())
settings=Path.home()/'.local/state/lucent/desktop.json'
exact=(settings.read_bytes()==(scratch/'desktop.json').read_bytes()) if manifest['settings_exists'] else not settings.exists()
exports=all(Path(path).is_file() and hashlib.sha256(Path(path).read_bytes()).hexdigest()==digest
            for path,digest in manifest['exports'].items())
print(json.dumps({'exact_settings':exact,'theme_and_login_exports':exports}))
'''


CLEANUP = '''import json, shutil, sys
from pathlib import Path
scratch=Path(sys.argv[1]); assets=json.loads(sys.argv[2]); static=json.loads(sys.argv[3])
state=Path.home()/'.local/state/lucent'; settings=json.loads((state/'desktop.json').read_text())
live=settings.get('live_wallpapers',{}); referenced=[]
for value in [live.get('default'),*live.get('monitors',{}).values()]:
    if value: referenced.append(value['asset']['path'])
selected=(state/'wallpaper').resolve()
# A failed/cancelled delivery may leave a valid owned import which never became
# visible to the client. Select it by its exact fixture provenance, not by age.
for directory in (state/'live-wallpapers').glob('media-*'):
    if directory.is_symlink() or not (directory/'asset.json').is_file(): continue
    asset=json.loads((directory/'asset.json').read_text())
    if Path(asset['source']['path']).parent==scratch and asset not in assets: assets.append(asset)
for directory in (state/'wallpaper-assets').glob('asset-*'):
    if directory.is_symlink() or not (directory/'source.json').is_file(): continue
    asset=json.loads((directory/'source.json').read_text())
    if any(asset['source']['path']==media['poster']['path'] for media in assets) and asset not in static: static.append(asset)
for asset in assets:
    path=Path(asset['path']); directory=path.parent
    if directory.parent!=(state/'live-wallpapers').resolve() or directory.is_symlink(): raise ValueError()
    if Path(asset['source']['path']).parent!=scratch or path.is_symlink(): raise ValueError()
    if json.loads((directory/'asset.json').read_text())!=asset or str(path) in referenced: raise ValueError()
    shutil.rmtree(directory)
for asset in static:
    path=Path(asset['path']); directory=path.parent
    if directory.parent!=(state/'wallpaper-assets').resolve() or directory.is_symlink(): raise ValueError()
    if path==selected: raise ValueError()
    if directory.exists():
        if json.loads((directory/'source.json').read_text())!=asset: raise ValueError()
        if not any(asset['source']['path']==media['poster']['path'] for media in assets): raise ValueError()
        shutil.rmtree(directory)
shutil.rmtree(scratch)
'''


def check_playback_policy(name, monitor, checks):
    """Exercise real fullscreen observations and native preference controls.

    Only the test-owned terminal is closed. The caller's outer checkpoint owns
    exact settings, wallpaper, exports and idle-policy restoration.
    """
    def workspace_selector(workspace):
        if workspace.get('address'):
            return workspace['address']
        if workspace.get('id', 0) > 0:
            return str(workspace['id'])
        return 'name:' + workspace['name'] if workspace.get('name') else ''

    original_workspace = workspace_selector(monitor['activeWorkspace'])
    require(bool(original_workspace), 'original_workspace_identity')
    special = monitor.get('specialWorkspace', {})
    require(not special.get('address') and special.get('id', 0) == 0,
            'policy_test_requires_ordinary_workspace')
    focused = json.loads(session('hyprctl', '-j', 'activewindow')).get('address')
    marker = 'lucent-wallpaper-policy-' + str(time.monotonic_ns())
    test_workspace = 'name:' + marker
    terminal_pid, address = None, None

    def dispatch(action, value):
        # Hyprland's Lua session accepts a dispatcher expression. Native names
        # remain quoted values, never source fragments; JSON quoting matches Lua
        # for these control-free strings when Unicode is kept verbatim.
        require(all(ord(c) >= 32 and ord(c) != 127 for c in value), 'policy_selector_controls')
        literal = json.dumps(value, ensure_ascii=False)
        if action == 'workspace':
            expression = 'hl.dsp.focus({ workspace = ' + literal + ' })'
        elif action == 'focuswindow':
            expression = 'hl.dsp.focus({ window = ' + literal + ' })'
        elif action == 'closewindow':
            expression = 'hl.dsp.window.close({ window = ' + literal + ' })'
        elif action == 'fullscreen':
            require(value in ('0', '2'), 'policy_fullscreen_mode')
            operation = 'set' if value == '2' else 'unset'
            expression = 'hl.dsp.window.fullscreen({ mode = "fullscreen", action = "' + operation + '" })'
        else:
            raise CheckFailure('unknown_policy_dispatch')
        try:
            response = session('hyprctl', 'dispatch', expression)
        except CheckFailure:
            raise CheckFailure('native_policy_dispatch_' + action) from None
        require(response.strip() == 'ok', 'native_policy_dispatch_' + action)

    def terminal():
        return next((c for c in json.loads(session('hyprctl', '-j', 'clients'))
                     if c.get('class') == marker and c.get('pid') == terminal_pid), None)

    def pause_reason():
        outputs = client()['live_wallpaper']['outputs']
        return next((o['pause_reason'] for o in outputs if o['monitor'] == monitor['name']), None)

    def playing(*, visible=True):
        before = background_frames()
        eventually(lambda: client()['live_wallpaper']['running'] == 1
                   and pause_reason() is None
                   and (not visible or background_frames() > before + 2),
                   'policy_allows_native_frame_advance')

    def paused(reason):
        eventually(lambda: client()['live_wallpaper']['running'] == 0
                   and pause_reason() == reason, 'native_policy_pause_reason')
        time.sleep(.7)  # Allow an already-submitted frame/crossfade to retire.
        frames = background_frames()
        time.sleep(1.1)
        require(background_frames() == frames, 'automatic_pause_kept_rendering')

    def policy_control(identifier, key_name, enabled):
        cli('wallpaper', 'live', 'open')
        eventually(lambda: client()['live_wallpaper']['open'], 'policy_panel_open')
        if read_settings()['live_wallpapers']['policy'][key_name] != enabled:
            focus(name, identifier)
            key(name, 28)
        eventually(lambda: saved()
                   and read_settings()['live_wallpapers']['policy'][key_name] == enabled,
                   'native_policy_control_saved')
        key(name, 1)
        eventually(lambda: not client()['live_wallpaper']['open'], 'policy_panel_closed')

    try:
        dispatch('workspace', test_workspace)
        terminal_pid = int(session('python3', '-c', '''import subprocess, sys
p=subprocess.Popen(['foot','--app-id='+sys.argv[1],'--title=Lucent wallpaper policy test',
                    'sleep','300'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL,start_new_session=True)
print(p.pid)
''', marker).strip())
        eventually(lambda: terminal() is not None and terminal()['mapped'], 'policy_terminal_mapped')
        address = terminal()['address']
        dispatch('focuswindow', 'address:' + address)
        playing()
        preferences = read_settings()['live_wallpapers']
        require(terminal()['fullscreen'] == 0, 'test_terminal_initially_windowed')
        dispatch('fullscreen', '2')
        eventually(lambda: terminal()['fullscreen'] == 2, 'test_terminal_fullscreen')
        paused('fullscreen')
        require(read_settings()['live_wallpapers'] == preferences,
                'fullscreen_observation_changed_preferences')
        checks['visible_fullscreen_stops_decoder_and_redraws'] = True

        dispatch('workspace', original_workspace)
        playing()
        require(terminal()['fullscreen'] == 2, 'hidden_terminal_stays_fullscreen')
        checks['fullscreen_on_hidden_workspace_does_not_pause'] = True
        dispatch('workspace', test_workspace)
        paused('fullscreen')

        cli('wallpaper', 'live', 'pause')
        eventually(lambda: saved(), 'manual_pause_saved')
        dispatch('focuswindow', 'address:' + address)
        dispatch('fullscreen', '0')
        eventually(lambda: terminal()['fullscreen'] == 0, 'test_terminal_windowed')
        paused('profile')
        require(read_settings()['live_wallpapers']['default']['paused'], 'manual_pause_retained')
        checks['leaving_fullscreen_preserves_manual_pause'] = True
        cli('wallpaper', 'live', 'resume')
        playing()

        dispatch('fullscreen', '2')
        paused('fullscreen')
        policy_control('live-policy-fullscreen', 'pause_when_fullscreen', False)
        # A compositor may withhold callbacks for covered background surfaces
        # even when the user explicitly keeps the decoder running.
        playing(visible=False)
        policy_control('live-policy-battery', 'pause_on_battery', False)
        preferences = read_settings()['live_wallpapers']
        session('systemctl', '--user', 'restart', 'lucent.service')
        eventually(lambda: client()['background_ready'] and saved()
                   and read_settings()['live_wallpapers'] == preferences,
                   'policy_preferences_survive_restart', timeout=45)
        require(terminal()['fullscreen'] == 2, 'fullscreen_preserved_across_shell_restart')
        playing(visible=False)
        checks['native_policy_controls_and_restart_preserve_opt_outs'] = True
        policy_control('live-policy-fullscreen', 'pause_when_fullscreen', True)
        paused('fullscreen')
        dispatch('focuswindow', 'address:' + address)
        dispatch('fullscreen', '0')
        playing()
        checks['leaving_fullscreen_resumes_unpaused_profile'] = True
    finally:
        try:
            owned = terminal() if terminal_pid is not None else None
            if owned is not None:
                dispatch('closewindow', 'address:' + owned['address'])
        finally:
            dispatch('workspace', original_workspace)
            eventually(lambda: workspace_selector(json.loads(session('hyprctl', '-j', 'activeworkspace')))
                       == original_workspace, 'policy_original_workspace_restored')
            if focused and any(c['address'] == focused
                               for c in json.loads(session('hyprctl', '-j', 'clients'))):
                dispatch('focuswindow', 'address:' + focused)
                eventually(lambda: json.loads(session('hyprctl', '-j', 'activewindow')).get('address')
                           == focused, 'policy_original_window_focus_restored')
            eventually(lambda: terminal() is None, 'policy_test_window_closed')
            checks['workspace_focus_and_test_window_restored'] = True


def check_session_lock(login_file, checks):
    def locked():
        return session('omarchy-shell', 'lock', 'isLocked').strip() == 'true'

    def native_locker():
        return bool(remote('pgrep', '-x', 'lucent-lock').strip())

    def pause_reason():
        return next((o['pause_reason'] for o in client()['live_wallpaper']['outputs']), None)

    def unlock():
        require(locked() and native_locker(), 'native_secure_locker_before_password')
        run([sys.executable, str(lab.ROOT / 'vm/run.py'), '--type-password',
             '--login-file', str(login_file), PROFILE], timeout=40)
        eventually(lambda: not locked(), 'native_authenticated_unlock', timeout=45)

    original = read_settings()['live_wallpapers']
    started_lock = False
    try:
        for manual in (False, True):
            if manual:
                cli('wallpaper', 'live', 'pause')
                eventually(lambda: saved() and client()['live_wallpaper']['running'] == 0,
                           'manual_pause_before_lock')
            preferences = read_settings()['live_wallpapers']
            started_lock = True
            session('omarchy-shell', 'lock', 'lock')
            eventually(lambda: locked() and native_locker(), 'native_lock_acknowledged')
            expected = 'profile' if manual else 'session_lock'
            eventually(lambda: client()['live_wallpaper']['running'] == 0
                       and pause_reason() == expected, 'lock_stops_native_decoder')
            time.sleep(.7)
            frames = background_frames()
            time.sleep(1.1)
            require(background_frames() == frames, 'locked_wallpaper_kept_redrawing')
            require(read_settings()['live_wallpapers'] == preferences,
                    'lock_observation_changed_preferences')
            if not manual:
                checks['secure_lock_stops_decoder_and_background_redraws'] = True
                # Restart the desktop while the independent secure locker stays
                # active, proving initial observation rather than a lock action flag.
                session('systemctl', '--user', 'restart', 'lucent.service')
                eventually(lambda: client()['background_ready']
                           and client()['live_wallpaper']['running'] == 0
                           and pause_reason() == 'session_lock',
                           'restart_observes_existing_secure_lock', timeout=45)
                require(locked() and native_locker(), 'desktop_restart_preserves_secure_locker')
                require(read_settings()['live_wallpapers'] == preferences,
                        'locked_restart_changed_preferences')
                checks['desktop_restart_observes_existing_lock'] = True
            unlock()
            started_lock = False
            if manual:
                time.sleep(1.5)  # Allow the advisory adapter to observe unlock.
                require(client()['live_wallpaper']['running'] == 0 and pause_reason() == 'profile',
                        'authenticated_unlock_cleared_manual_pause')
                checks['authenticated_unlock_preserves_manual_pause'] = True
            else:
                frames = background_frames()
                eventually(lambda: client()['live_wallpaper']['running'] == 1
                           and pause_reason() is None and background_frames() > frames + 2,
                           'authenticated_unlock_resumes_playback')
                checks['authenticated_unlock_resumes_unpaused_profile'] = True
            require(read_settings()['live_wallpapers'] == preferences,
                    'unlock_observation_changed_preferences')
        cli('wallpaper', 'live', 'resume')
        eventually(lambda: saved() and read_settings()['live_wallpapers'] == original,
                   'lock_check_restores_manual_preferences')
    finally:
        if started_lock and locked():
            unlock()
        checks['secure_session_left_unlocked'] = not locked()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--policy-only', action='store_true',
                       help='Test fullscreen/manual pause and native policy controls with one MP4; skip resource benchmarks.')
    modes.add_argument('--lock-only', action='store_true',
                       help='Test native secure lock, restart and authenticated unlock with one MP4; skip resource benchmarks.')
    parser.add_argument('--login-file', type=Path,
                        help='Private test-account login file, required only for --lock-only.')
    args = parser.parse_args()
    if args.lock_only:
        if args.login_file is None or not args.login_file.is_file():
            parser.error('--lock-only requires an existing private --login-file')
        if args.login_file.stat().st_mode & 0o077:
            parser.error('--login-file must not allow group or other access')
    focused_check = args.policy_only or args.lock_only
    report_dir = lab.ROOT / 'reports/local' / ('live-wallpapers-' + time.strftime('%Y%m%d-%H%M%S'))
    report_dir.mkdir(parents=True, exist_ok=False)
    report = {'checks': {}, 'restoration': {}, 'captures': 0, 'passed': False}
    scratch, checkpoint, original, awake = None, None, None, None
    backup_ready = False
    media_assets, static_assets, captures = [], [], [0]
    phase = 'preflight'
    try:
        name = lab.inventory()['vms'][PROFILE]['name']
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_must_be_unlocked')
        eventually(lambda: client()['background_ready'] and saved() and not client()['wallpaper_busy'], 'initial_desktop_ready')
        initial = inspect()
        require('Vulkan' in initial['adapter'], 'native_vulkan_required')
        require(client()['settings_writable'], 'writable_preferences_required')
        require(bool(client()['wallpaper']), 'selected_wallpaper_required')
        monitors = json.loads(session('hyprctl', '-j', 'monitors'))
        require(len(monitors) == 1 and monitors[0]['x'] == 0 and monitors[0]['y'] == 0,
                'single_display_at_origin_required')
        monitor, original = monitors[0], client()['wallpaper']
        awake = json.loads(session('omarchy-toggle-idle', 'status'))['enabled']
        scratch = remote('mktemp', '-d', '/tmp/lucent-live-wallpaper.XXXXXX').strip()
        remote('python3', '-c', BACKUP, scratch)
        backup_ready = True
        checkpoint = GuestWallpaperCheckpoint(remote, original)
        session('omarchy-toggle-idle', 'stay-awake')
        cli('launcher', 'close')
        # Use the public wallpaper API before stopping: checkpoint protects its
        # old selected asset from later poster snapshot pruning.
        phase = 'encode_synthetic_fixtures'
        remote('python3', '-c', ENCODE, scratch, timeout=100)
        report['checks']['synthetic_mp4_webm_gif_encoded'] = True
        # Isolate existing per-output overrides without altering other settings.
        session('systemctl', '--user', 'stop', 'lucent.service')
        remote('python3', '-c', '''import json, sys
from pathlib import Path
p=Path.home()/'.local/state/lucent/desktop.json'
d=json.loads(p.read_text()); d['live_wallpapers']={'default':None,'monitors':{},'paused':False}
t=p.with_name('desktop.live-test.new'); t.write_text(json.dumps(d)); t.replace(p)
''', scratch)
        session('systemctl', '--user', 'start', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['live_wallpaper']['players'] == 0,
                   'isolated_desktop_ready')
        if not focused_check:
            print('Measuring the static baseline for 120 seconds.', flush=True)
            report['resources'] = {'static': measure_resources(scratch, 'static-resources')}
        for extension in (('mp4',) if focused_check else ('mp4', 'webm', 'gif')):
            phase = 'native_' + extension + '_playback'
            fixture = scratch + '/fixture.' + extension
            if extension == 'mp4':
                cli('wallpaper', 'live', 'open')
                eventually(lambda: client()['live_wallpaper']['open'], 'native_live_panel_open')
                eventually(lambda: find_surface(inspect(), 'live-wallpaper').get('focus') == 'live-library-refresh',
                           'native_live_library_autofocus')
                click(name, monitor, 'live-path')
                key(name, 29, 30)
                type_ascii(name, fixture)
                key(name, 107)  # End breaks typing coalescing before this new edit.
                type_ascii(name, '-missing')
                key(name, 29, 44)  # Undo must remove the invalid suffix.
                key(name, 28)
            else:
                cli('wallpaper', 'live', 'play', fixture)
            eventually(lambda: not client()['live_wallpaper']['importing'] and saved()
                       and (read_settings()['live_wallpapers']['default'] or {})
                       .get('asset', {}).get('source', {}).get('path') == fixture,
                       'native_media_import_and_save', timeout=45)
            if extension == 'mp4':
                report['checks']['native_ordinary_field_undo_and_autofocus'] = True
            if extension == 'webm':
                eventually(lambda: saved() and read_settings()['live_wallpapers']['paused']
                           and client()['live_wallpaper']['running'] == 0,
                           'new_import_preserves_pause_all')
                report['checks']['import_preserves_explicit_pause_all'] = True
                cli('wallpaper', 'live', 'resume-all')
            asset = read_settings()['live_wallpapers']['default']['asset']
            media_assets.append(asset)
            eventually(lambda: not client()['wallpaper_busy'] and client()['background_ready']
                       and client()['wallpaper']['source']['path'] == asset['poster']['path'],
                       'poster_wallpaper_commit', timeout=45)
            static_assets.append(client()['wallpaper'])
            require(not client()['wallpaper_error'] and not client()['theme_error'], 'poster_export_error')
            if extension == 'mp4':
                focus(name, 'live-fps-15')
                key(name, 28)
                eventually(lambda: saved() and read_settings()['live_wallpapers']['default']['fps'] == 15,
                           'native_fps_control_and_save')
                report['checks']['native_path_edit_submit_and_fps'] = True
            key(name, 1)
            eventually(lambda: not client()['live_wallpaper']['open'], 'native_live_panel_escape')
            move(name, monitor, 8, 80)
            eventually(lambda: client()['live_wallpaper']['running'] == 1
                       and client()['live_wallpaper']['retained_frame_bytes'] > 0,
                       'native_media_frame_delivery')
            before = background_frames()
            eventually(lambda: background_frames() > before + 2, 'rendered_background_frame_advance')
            changing_samples = verify_colors(scratch, report_dir, captures)
            report['checks'][extension + '_native_import_and_changing_pixels'] = True
            if args.policy_only:
                phase = 'native_playback_policy'
                check_playback_policy(name, monitor, report['checks'])
                continue
            if args.lock_only:
                phase = 'native_session_lock'
                check_session_lock(args.login_file, report['checks'])
                continue
            if extension == 'mp4':
                print('Measuring native 15 FPS MP4 playback for 120 seconds.', flush=True)
                report['resources']['mp4_15_fps'] = measure_resources(scratch, 'live-resources')
                phase = 'pause_resume_failure_and_restart'
                cli('wallpaper', 'live', 'pause')
                eventually(lambda: saved() and client()['live_wallpaper']['running'] == 0,
                           'pause_retires_decoder')
                time.sleep(.7)
                paused_frames = background_frames()
                frozen = capture(scratch, report_dir, captures[0]); captures[0] += 1
                time.sleep(1.2)
                require(background_frames() == paused_frames, 'paused_background_kept_rendering')
                still = capture(scratch, report_dir, captures[0])
                # Only compare positions established as the changing background;
                # clocks/system widgets may independently change other samples.
                require(all(still[index] == frozen[index] for index in changing_samples),
                        'paused_background_pixels_changed')
                captures[0] += 1
                report['checks']['pause_freezes_native_frames_and_pixels'] = True
                cli('wallpaper', 'live', 'resume')
                eventually(lambda: saved() and client()['live_wallpaper']['running'] == 1
                           and background_frames() > paused_frames + 2, 'resume_native_playback')
                report['checks']['resume_restarts_native_playback'] = True
                committed, selected = read_settings(), client()['wallpaper']
                committed_digest = settings_digest()
                cli('wallpaper', 'live', 'open')
                eventually(lambda: client()['live_wallpaper']['open'], 'native_invalid_import_panel')
                click(name, monitor, 'live-path')
                eventually(lambda: find_surface(inspect(), 'live-wallpaper').get('focus') == 'live-path',
                           'native_invalid_import_path_focus')
                key(name, 29, 30)
                type_ascii(name, scratch + '/invalid.mp4')
                key(name, 29, 44)  # Undo the latest typing group.
                key(name, 29, 42, 44)  # Redo must restore the invalid path.
                key(name, 28)
                eventually(lambda: not client()['live_wallpaper']['importing']
                           and 'could not be decoded' in client()['live_wallpaper']['error'],
                           'invalid_media_reports_failure', timeout=45)
                require(settings_digest() == committed_digest and client()['wallpaper'] == selected,
                        'invalid_import_replaced_configuration')
                report['checks']['invalid_import_preserves_configuration'] = True
                report['checks']['native_ordinary_field_redo'] = True
                key(name, 1)
                eventually(lambda: not client()['live_wallpaper']['open'], 'invalid_panel_escape')
                session('systemctl', '--user', 'restart', 'lucent.service')
                eventually(lambda: client()['background_ready'] and client()['live_wallpaper']['running'] == 1
                           and read_settings()['live_wallpapers'] == committed['live_wallpapers'],
                           'live_configuration_survives_restart', timeout=45)
                before = background_frames()
                eventually(lambda: background_frames() > before + 2, 'restart_native_frame_advance')
                verify_colors(scratch, report_dir, captures)
                report['checks']['restart_restores_selection_fps_and_playback'] = True
                cli('wallpaper', 'live', 'pause-all')
                eventually(lambda: saved() and read_settings()['live_wallpapers']['paused']
                           and client()['live_wallpaper']['running'] == 0,
                           'explicit_pause_all_before_next_import')
        report['checks']['native_panel_escape'] = True
        report['passed'] = True
    except CheckFailure as failure:
        report['failed_check'] = str(failure)
        report['failed_phase'] = phase
    except BaseException:
        # Captured subprocess data and exception text may contain private paths.
        report['failed_check'] = phase
    finally:
        if backup_ready:
            # Each stage is independent. An unavailable command socket must not
            # skip restoration of the saved files or leave the service stopped.
            if checkpoint is not None:
                try:
                    checkpoint.restore()
                    report['restoration']['selected_snapshot_checkpoint'] = True
                except BaseException:
                    report['restoration']['selected_snapshot_checkpoint'] = False
                    report['passed'] = False
            try:
                # Reselect through the existing transaction before exact settings
                # restoration. This republishes the original still wallpaper.
                cli('wallpaper', 'set', original['path'])
                eventually(lambda: not client()['wallpaper_busy'] and client()['background_ready']
                           and client()['wallpaper']['source'] == original['source'],
                           'original_wallpaper_transaction', timeout=45)
                report['restoration']['public_wallpaper_transaction'] = True
            except BaseException:
                report['restoration']['public_wallpaper_transaction'] = False
            try:
                session('systemctl', '--user', 'stop', 'lucent.service')
                remote('python3', '-c', RESTORE_FILES, scratch)
                report['restoration']['exact_files_replaced_while_stopped'] = True
            except BaseException:
                report['restoration']['exact_files_replaced_while_stopped'] = False
                report['passed'] = False
            try:
                session('systemctl', '--user', 'start', 'lucent.service')
                eventually(lambda: client()['background_ready'] and client()['wallpaper'] == original,
                           'original_desktop_restart', timeout=45)
                eventually(lambda: all(json.loads(remote('python3', '-c', VERIFY_RESTORED, scratch)).values()),
                           'exact_settings_and_exports_restored', timeout=45)
                report['restoration']['exact_settings_wallpaper_theme_and_login_exports'] = True
            except BaseException:
                report['restoration']['exact_settings_wallpaper_theme_and_login_exports'] = False
                report['passed'] = False
            # Independent cleanup of the idle override runs even after rollback
            # failure. Retain private backups/assets if restoration did not pass.
            try:
                if awake is False:
                    session('omarchy-toggle-idle', 'allow-idle')
                require(json.loads(session('omarchy-toggle-idle', 'status'))['enabled'] == awake,
                        'original_idle_policy_not_restored')
                report['restoration']['original_idle_policy'] = True
            except BaseException:
                report['restoration']['original_idle_policy'] = False
                report['passed'] = False
            if report['restoration'].get('exact_settings_wallpaper_theme_and_login_exports'):
                try:
                    remote('python3', '-c', CLEANUP, scratch, json.dumps(media_assets), json.dumps(static_assets))
                    if checkpoint is not None:
                        checkpoint.discard()
                    report['restoration']['test_owned_assets_removed'] = True
                except BaseException:
                    report['restoration']['test_owned_assets_removed'] = False
                    report['passed'] = False
        elif scratch is not None:
            # Backup failed before mutation; remove only the allocated test dir.
            try:
                remote('python3', '-c', 'import shutil, sys; shutil.rmtree(sys.argv[1])', scratch)
            except BaseException:
                pass
        report['captures'] = captures[0]
        (report_dir / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        (lab.ROOT / 'reports/local/lucent-live-wallpapers.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
