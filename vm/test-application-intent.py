#!/usr/bin/env python3
"""Native SDK intent + real Chromium file opening with guarded caller restoration.

Requires an unlocked idle single-output VM and a release manifest containing
desktop, CLI and the independent application-intent executable. Physical input
uses the compositor's observed hit geometry. Only owned entries/profile/windows
are created; token values never enter the report. No test runs on import.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import runpy
import re
import shlex
import stat
import subprocess
import sys
import time

from native_surfaces import find_surface
from owned_application_units import RETIRE


ROOT = Path(__file__).resolve().parents[1]


def load_helpers():
    """Feature scripts use the canonical checkout's existing private inventory."""
    global lab, a, h, remote, session, cli, inspect, require, eventually, CheckFailure
    result = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute',
                             '--git-common-dir'], check=True, capture_output=True, text=True, timeout=5)
    common = Path(result.stdout.strip()).resolve(strict=True)
    spec = importlib.util.spec_from_file_location('application_intent_lab', common.parent / 'vm/lab.py')
    if spec is None or spec.loader is None:
        raise ValueError('Canonical VM helper is unavailable')
    lab = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(lab)
    previous = sys.modules.get('lab')
    try:
        sys.modules['lab'] = lab
        a = runpy.run_path(str(ROOT / 'vm/test-application-actions.py'))
    finally:
        if previous is None:
            sys.modules.pop('lab', None)
        else:
            sys.modules['lab'] = previous
    h = a['h']
    remote, session, cli, inspect = [h[key] for key in ('remote', 'session', 'cli', 'inspect')]
    require, eventually, CheckFailure = [h[key] for key in ('require', 'eventually', 'CheckFailure')]

SOCKET = r'''
import json,os,socket,stat,sys
from pathlib import Path
path=Path(os.environ['XDG_RUNTIME_DIR'])/'application-intent.sock'
value=path.lstat()
assert stat.S_ISSOCK(value.st_mode) and value.st_uid==os.getuid()
command=sys.argv[1]; assert command in ('inspect','open','quit')
with socket.socket(socket.AF_UNIX) as connection:
 connection.settimeout(4); connection.connect(str(path)); connection.sendall((command+'\n').encode())
 response=bytearray()
 while True:
  data=connection.recv(65536)
  if not data: break
  response.extend(data); assert len(response)<=4*1024*1024
assert not response.startswith(b'error:')
print(response.decode())
'''
BROWSER_TABS = r'''
import json,stat,sys,urllib.request
from pathlib import Path
directory=Path(sys.argv[1]); run_id=sys.argv[2]
assert directory.resolve()==directory and directory.stat().st_uid==__import__('os').getuid()
manifest=json.loads((directory/'manifest.json').read_text()); assert manifest['run_id']==run_id
path=directory/'profile/DevToolsActivePort'; value=path.lstat()
assert stat.S_ISREG(value.st_mode) and value.st_uid==__import__('os').getuid() and value.st_size<4096
port=int(path.read_text().splitlines()[0]); assert 1024<=port<=65535
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open('http://127.0.0.1:'+str(port)+'/json/list',timeout=3) as response:
 data=response.read(1024*1024+1); assert len(data)<=1024*1024
tabs=json.loads(data); assert isinstance(tabs,list) and len(tabs)<=32
print(json.dumps({'loaded':[any(tab.get('type')=='page' and tab.get('title')==item['title']
 and tab.get('url')==item['uri'] for tab in tabs) for item in manifest['files']]}))
'''


def checked_binary(manifest, name, supplied=None):
    item = manifest['binaries'][name]
    path = Path(supplied or item['path']).resolve(strict=True)
    value = path.stat()
    require(stat.S_ISREG(value.st_mode) and value.st_size <= 64 * 1024 * 1024
            and hashlib.sha256(path.read_bytes()).hexdigest() == item['sha256'],
            'exact_release_binary_' + name)
    return path


def suite(args, results):
    manifest = json.loads(Path(args.release_manifest).read_text())
    require(re.fullmatch('[a-f0-9]{40}', manifest['source_commit']), 'identified_release_source')
    example = checked_binary(manifest, 'application-intent', args.example_binary)
    for executable in ('lucent-desktop', 'lucent-cli'):
        expected = manifest['binaries'][executable]['sha256']
        require(remote('sha256sum', '/home/omarchy/.local/bin/' + executable).split()[0] == expected,
                'installed_exact_' + executable)
    expected = manifest['binaries']['lucent-desktop']['sha256']
    actual = remote('python3', '-c',
        'import hashlib,subprocess; from pathlib import Path; '
        'pids=subprocess.check_output(["pgrep","-x","lucent-desktop"]).split(); '
        'assert len(pids)==1; print(hashlib.sha256(Path("/proc/"+pids[0].decode()+"/exe").read_bytes()).hexdigest())')
    require(actual.strip() == expected, 'running_exact_release_desktop')
    require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
    before = inspect()['client']
    require(not before['launcher'] and not before['query']
            and before['settings_save_status'] == 'saved' and not before['wallpaper_busy'],
            'idle_desktop_before_application_acceptance')
    require(json.loads(session('hyprctl', '-j', 'clients')) == [], 'no_foreign_managed_clients')
    require(json.loads(session('python3', '-c',
        'import json,os; from pathlib import Path; '
        'print(json.dumps(not os.path.lexists(Path(os.environ["XDG_RUNTIME_DIR"])/"application-intent.sock")))')),
            'independent_example_not_already_running')
    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper', 'display'):
        require(not before.get(panel, {}).get('open', False)
                and not before.get(panel, {}).get('visible', False), 'other_panels_initially_closed')
    require('lucent-authorization-' not in session('hyprctl', '-j', 'layers'), 'no_authentication_prompt')
    a['keyboard_preflight']()
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1 and monitors[0]['width'] == 1920 and monitors[0]['height'] == 1080
            and monitors[0]['x'] == monitors[0]['y'] == 0 and monitors[0]['scale'] == 1,
            'single_scale_one_output')
    monitor = monitors[0]
    def output_state(values):
        keys = ('id', 'name', 'x', 'y', 'width', 'height', 'scale', 'transform',
                'refreshRate', 'focused', 'dpmsStatus', 'activeWorkspace', 'specialWorkspace')
        return [{key: value.get(key) for key in keys} for value in values]

    def workspace_state():
        value = json.loads(session('hyprctl', '-j', 'activeworkspace'))
        return {key: value.get(key) for key in ('id', 'name', 'monitorID')}

    original_outputs = output_state(monitors)
    original_workspace = workspace_state()
    original_cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
    last_cursor = dict(original_cursor)
    name = lab.inventory()['vms']['lucent']['name']
    preferences = json.loads(remote('python3', '-c', a['PREFERENCES']))
    scratch = remote('mktemp', '-d', '/tmp/lucent-application-intent.XXXXXXXXXXXX').strip()
    run_id = scratch.rsplit('.', 1)[-1]
    fixture, binary = scratch + '/fixture.py', scratch + '/application-intent'
    unit = 'lucent-application-intent-' + run_id + '.service'
    guard = 'lucent-application-intent-watchdog-' + run_id + '.service'
    initialized, example_attempted, guard_attempted = False, False, False
    completed_success, pressed = False, False
    cleanup = results['cleanup'] = {}

    def call(operation, *arguments):
        return session('/usr/bin/python3', fixture, operation, scratch, run_id, *arguments)

    def example_command(command):
        return session('/usr/bin/python3', '-c', SOCKET, command)

    def example_state():
        return json.loads(example_command('inspect'))

    def events():
        return json.loads(call('state'))['events']

    def finished(count):
        state = example_state()['client']
        return state['completed'] == count and not state['busy']

    def loaded():
        return all(json.loads(session('python3', '-c', BROWSER_TABS, scratch, run_id))['loaded'])

    def interaction_guard():
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'current_unlocked_guest')
        require('lucent-authorization-' not in session('hyprctl', '-j', 'layers'), 'current_no_authentication_prompt')
        current = inspect()['client']
        require(not current['launcher'] and not current['query']
                and not current.get('notification_history_open', False)
                and not current.get('widget_grid', False) and not current.get('widgets_editing', False)
                and current['settings_save_status'] == 'saved' and not current['wallpaper_busy'],
                'current_idle_shell_without_competing_controls')
        for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper', 'display', 'weather'):
            require(not current.get(panel, {}).get('open', False)
                    and not current.get(panel, {}).get('visible', False), 'current_panels_closed')
        require(output_state(json.loads(session('hyprctl', '-j', 'monitors'))) == original_outputs,
                'current_original_output_layout')
        require(workspace_state() == original_workspace, 'current_original_active_workspace')
        clients = json.loads(session('hyprctl', '-j', 'clients'))
        if initialized:
            require(json.loads(call('guard', json.dumps(clients)))['owned_clients'] == len(clients),
                    'current_exact_owned_browser_clients')
        else:
            require(clients == [], 'current_no_foreign_managed_clients')
        return clients

    def physical_click(identifier):
        nonlocal last_cursor, pressed
        interaction_guard()
        surface = find_surface(example_state(), 'intent')
        hits = [hit for hit in surface['hits'] if hit['id'] == identifier]
        require(len(hits) == 1, 'independent_example_current_control')
        hit, clip = hits[0], hits[0]['clip']
        left, top = max(hit['x'], clip['x']), max(hit['y'], clip['y'])
        right = min(hit['x'] + hit['width'], clip['x'] + clip['width'])
        bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'])
        require(right - left > 4 and bottom - top > 4
                and right - left >= hit['width'] - .5 and bottom - top >= hit['height'] - .5,
                'independent_example_control_fully_visible')
        origin = h['layer_origin'](surface)
        require(abs(origin['w'] - surface['width']) <= .5 and abs(origin['h'] - surface['height']) <= .5,
                'independent_example_current_native_geometry')
        target = (origin['x'] + (left + right) / 2, origin['y'] + (top + bottom) / 2)
        last_cursor = {'x': target[0], 'y': target[1]}
        h['move'](name, monitor, *target)
        def arrived():
            position = json.loads(session('hyprctl', '-j', 'cursorpos'))
            return abs(position['x'] - target[0]) <= 2 and abs(position['y'] - target[1]) <= 2
        eventually(arrived, 'independent_control_pointer_arrived', timeout=3)
        time.sleep(.15)
        interaction_guard()
        try:
            pressed = True  # Also release when QMP delivered a press but lost its reply.
            a['pointer'](name, True)
            time.sleep(.05)
        finally:
            a['pointer'](name, False)
            pressed = False

    try:
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(fixture)],
                 data=(ROOT / 'vm/fixtures/application-intent-fixture.py').read_bytes(), timeout=20)
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(scratch + '/owned_application_units.py')],
                 data=(ROOT / 'vm/owned_application_units.py').read_bytes(), timeout=20)
        initialized = True
        entry = json.loads(call('initialize'))
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(binary)],
                 data=example.read_bytes(), timeout=30)
        require(remote('sha256sum', binary).split()[0] == manifest['binaries']['application-intent']['sha256'],
                'independent_example_uploaded_digest')
        remote('chmod', '700', binary)
        guard_attempted = True
        session('systemd-run', '--user', '--collect', '--unit=' + guard,
                '--property=RuntimeMaxSec=300', '--property=MemoryMax=64M',
                '/usr/bin/python3', fixture, 'watchdog', scratch, run_id)
        example_attempted = True
        session('systemd-run', '--user', '--collect', '--unit=' + unit,
                '--property=RuntimeMaxSec=240', '--property=MemoryMax=768M',
                '--property=UMask=0077', binary, '--app', entry['app_id'], '--action', 'Probe',
                '--target', scratch + '/' + entry['files'][0]['name'],
                '--target', scratch + '/' + entry['files'][1]['name'])
        eventually(lambda: bool(find_surface(example_state(), 'intent')['hits']), 'independent_native_example_ready')

        # The first physical launch starts a fresh private Chromium profile.
        # We separately observe focus; token presence proves child delivery only,
        # and does not prove Chromium consumed the xdg-activation protocol token.
        physical_click('intent-launch')
        eventually(lambda: finished(1) and len(events()) == 1, 'physical_first_launch_completed')
        event = events()[0]
        require(event['operation'] == 'open' and event['uris'] == []
                and event['fresh_token_present'] and not event['legacy_startup_present']
                and example_state()['client']['fresh_token'], 'first_physical_token_delivered_to_owned_child')
        def focused_owned_browser():
            clients = interaction_guard()
            focused = json.loads(session('hyprctl', '-j', 'activewindow'))
            return bool(focused) and focused.get('class') == entry['class'] and any(
                client.get('address') == focused.get('address') and client.get('pid') == focused.get('pid')
                for client in clients)
        eventually(focused_owned_browser, 'fresh_owned_browser_focus_observed')
        results['checks']['fresh_token_child_delivery_and_fresh_browser_focus_observed'] = True

        for index, (identifier, operation) in enumerate((
            ('intent-open', 'open'), ('intent-action', 'action'),
        ), start=2):
            physical_click(identifier)
            eventually(lambda: finished(index) and len(events()) == index, 'physical_intent_dispatch_completed')
            event = events()[-1]
            require(event['operation'] == operation and event['fresh_token_present']
                    and not event['legacy_startup_present'] and example_state()['client']['fresh_token'],
                    'physical_input_token_delivered_to_selected_child')
            eventually(loaded, 'real_browser_loaded_both_literal_file_uris')
        results['checks']['real_browser_opened_space_percent_unicode_and_shell_literal_filenames'] = True
        results['checks']['physical_launch_open_and_declared_action_deliver_fresh_tokens_to_children'] = True
        interaction_guard()
        example_command('open')
        eventually(lambda: finished(4) and len(events()) == 4, 'later_socket_open_completed')
        require(not events()[-1]['fresh_token_present'] and not events()[-1]['legacy_startup_present']
                and not example_state()['client']['fresh_token'], 'later_socket_request_cannot_reuse_physical_authority')
        interaction_guard()
        cli('applications', 'open', entry['app_id'], scratch + '/' + entry['files'][0]['name'],
            scratch + '/' + entry['files'][1]['name'])
        eventually(lambda: len(events()) == 5, 'desktop_cli_open_reaches_real_application')
        require(not events()[-1]['fresh_token_present'] and not events()[-1]['legacy_startup_present'],
                'desktop_cli_has_no_physical_authority')
        eventually(loaded, 'desktop_opened_both_real_browser_documents')
        interaction_guard()
        results['checks']['desktop_cli_open_and_no_stale_token_reuse'] = True
        completed_success = True
        results['source_commit'] = manifest['source_commit']
    finally:
        failures = []
        if pressed:
            try:
                a['pointer'](name, False)
                pressed = False
            except Exception:
                failures.append('pointer_release')
        if example_attempted:
            try:
                require(json.loads(session('python3', '-c', RETIRE,
                    'lucent-application-intent-', unit, binary, run_id))['retired'],
                    'independent_example_retired')
                cleanup['example_retired'] = True
            except Exception:
                failures.append('example_retirement')
        if initialized:
            try:
                require(json.loads(call('retire'))['owned_browser_units_retired'], 'owned_browser_retired')
                cleanup['browser_retired'] = True
            except Exception:
                failures.append('browser_retirement')
        if guard_attempted and cleanup.get('browser_retired', False) and cleanup.get('example_retired', False):
            try:
                require(json.loads(session('python3', '-c', RETIRE,
                    'lucent-application-intent-watchdog-', guard, fixture, run_id))['retired'],
                    'owned_watchdog_retired')
                cleanup['watchdog_retired'] = True
            except Exception:
                failures.append('watchdog_retirement')
        try:
            require(json.loads(remote('python3', '-c', a['PREFERENCES'])) == preferences,
                    'exact_settings_and_wallpaper_restored')
            require(inspect()['client']['wallpaper'] == before['wallpaper'], 'wallpaper_identity_preserved')
            cleanup['settings_and_wallpaper_preserved'] = True
        except Exception:
            failures.append('caller_preservation')
        try:
            interaction_guard()
            cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
            require(all(abs(cursor[key] - last_cursor[key]) <= 2 for key in ('x', 'y')),
                    'cursor_still_owned_before_restore')
            h['move'](name, monitor, original_cursor['x'], original_cursor['y'])
            def restored():
                cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
                return all(abs(cursor[key] - original_cursor[key]) <= 2 for key in ('x', 'y'))
            eventually(restored, 'original_cursor_restored', timeout=3)
            interaction_guard()
            cleanup['output_workspace_and_cursor_preserved'] = True
        except Exception:
            failures.append('output_workspace_cursor_preservation')
        if completed_success and not failures:
            try:
                call('cleanup')
                cleanup['owned_scratch_and_entry_removed'] = True
            except Exception:
                failures.append('owned_file_cleanup')
        else:
            cleanup['private_artifacts_retained'] = True
        require(not failures, 'application_acceptance_cleanup_and_preservation')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release-manifest', required=True)
    parser.add_argument('--example-binary')
    parser.add_argument('--report')
    args = parser.parse_args()
    load_helpers()
    result = {'passed': False, 'checks': {},
              'scope': 'Fresh token delivery to owned children and observed browser focus; no Chromium protocol-consumption claim'}
    try:
        measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
        with measure['stay_awake'](remote):
            suite(args, result)
        result['passed'] = True
    except CheckFailure as error:
        result['failure'] = str(error)
    except Exception:
        result['failure'] = 'bounded_native_application_acceptance_failure'
    path = Path(args.report or lab.ROOT / 'reports/local/application-intent-native.json')
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result, indent=2) + '\n')
    path.chmod(0o600)
    print(json.dumps(result))
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
