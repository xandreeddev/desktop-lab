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
import signal
import stat
import subprocess
import sys
import time

from native_surfaces import find_surface
from application_intent_policy import CursorOwnership, compositor_move, same_target, target as native_target
from application_intent_browser import fresh_documents


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
            and not any(before.get(key) for key in ('notification_count', 'notification_history_open',
                        'widgets_editing', 'widget_grid', 'dock_surface_expanded', 'launcher_opening'))
            and before['settings_save_status'] == 'saved' and not before['wallpaper_busy'],
            'idle_desktop_before_application_acceptance')
    require(json.loads(session('hyprctl', '-j', 'clients')) == [], 'no_foreign_managed_clients')
    require(json.loads(session('python3', '-c',
        'import json,os; from pathlib import Path; '
        'print(json.dumps(not os.path.lexists(Path(os.environ["XDG_RUNTIME_DIR"])/"application-intent.sock")))')),
            'independent_example_not_already_running')
    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper', 'displays', 'weather',
                  'settings', 'application_actions'):
        require(not before.get(panel, {}).get('open', False)
                and not before.get(panel, {}).get('visible', False)
                and not before.get(panel, {}).get('busy', False), 'other_panels_initially_closed')
    require('lucent-authorization-' not in session('hyprctl', '-j', 'layers'), 'no_authentication_prompt')
    a['keyboard_preflight']()
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1 and monitors[0]['width'] == 1920 and monitors[0]['height'] == 1080
            and monitors[0]['x'] == monitors[0]['y'] == 0 and monitors[0]['scale'] == 1
            and monitors[0].get('transform') == 0 and monitors[0].get('dpmsStatus') is True,
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
    cursor_owner = CursorOwnership(original_cursor)
    name = lab.inventory()['vms']['lucent']['name']
    preferences = json.loads(remote('python3', '-c', a['PREFERENCES']))
    scratch = remote('mktemp', '-d', '/tmp/lucent-application-intent.XXXXXXXXXXXX').strip()
    run_id = scratch.rsplit('.', 1)[-1]
    fixture, binary = scratch + '/fixture.py', scratch + '/application-intent'
    unit = 'lucent-application-intent-' + run_id + '.service'
    guard = 'lucent-application-intent-watchdog-' + run_id + '.service'
    initialized, example_attempted, guard_attempted = False, False, False
    completed_success, pressed = False, False
    baseline, example_identity, example_layer = None, None, None
    browser_identity = None
    cleanup = results['cleanup'] = {}

    def call(operation, *arguments):
        return session('/usr/bin/python3', fixture, operation, scratch, run_id, *arguments)

    def example_command(command):
        require(example_identity is not None, 'captured_example_socket_authority')
        return call('example-command', command, json.dumps(example_identity))

    def example_state():
        return json.loads(example_command('inspect'))

    def events():
        return json.loads(call('state'))['events']

    def finished(count):
        state = example_state()['client']
        return state['completed'] == count and not state['busy']

    def browser_snapshot():
        nonlocal browser_identity
        value = json.loads(call('browser-snapshot', json.dumps({'identity': browser_identity})))
        require(browser_identity is None or value['identity'] == browser_identity,
                'frozen_original_browser_lifetime_and_control_target')
        browser_identity = value['identity']
        return value['pages']

    def before_request():
        current = browser_snapshot()
        require(current == [browser_identity['control']], 'only_original_blank_before_each_document_request')
        return current

    def verify_documents_and_retire(before_pages):
        selected = None
        def loaded_fresh():
            nonlocal selected
            selected = fresh_documents(before_pages, browser_snapshot(), entry['files'], browser_identity['control'])
            return selected is not None
        eventually(loaded_fresh, 'request_created_both_new_literal_document_targets')
        require(json.loads(call('browser-close', json.dumps({'identity': browser_identity,
                    'before': before_pages, 'targets': selected})))['fresh_owned_targets_retired'],
                'only_fresh_document_targets_retired')
        require(browser_snapshot() == [browser_identity['control']], 'original_blank_browser_target_preserved')

    def observe_native(with_example=True):
        value = json.loads(call('admission', json.dumps({'desktop_sha256': expected,
            'example_sha256': manifest['binaries']['application-intent']['sha256'] if with_example else None})))
        if baseline is not None:
            require(value['baseline'] == baseline, 'frozen_desktop_supervisor_compositor_authority')
        if with_example and example_identity is not None:
            require(value['example'] == example_identity, 'frozen_example_unit_process_and_socket_authority')
            require(all(value['intent_layer'][key] == example_layer[key]
                        for key in ('address', 'namespace', 'pid')), 'frozen_example_native_layer_lifetime')
        return value

    def interaction_guard(with_example=True):
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'current_unlocked_guest')
        require('lucent-authorization-' not in session('hyprctl', '-j', 'layers'), 'current_no_authentication_prompt')
        current = inspect()['client']
        require(not current['launcher'] and not current['query']
                and not current.get('notification_history_open', False)
                and not current.get('notification_count', 0)
                and not current.get('dock_surface_expanded', False) and not current.get('launcher_opening', False)
                and not current.get('widget_grid', False) and not current.get('widgets_editing', False)
                and current['settings_save_status'] == 'saved' and not current['wallpaper_busy'],
                'current_idle_shell_without_competing_controls')
        for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper', 'displays', 'weather',
                      'settings', 'application_actions'):
            require(not current.get(panel, {}).get('open', False)
                    and not current.get(panel, {}).get('visible', False)
                    and not current.get(panel, {}).get('busy', False), 'current_panels_closed')
        require(not current.get('network', {}).get('form')
                and not any(current.get('bluetooth', {}).get(key) for key in ('form', 'requests', 'answering')),
                'no_current_native_credential_requests')
        require(output_state(json.loads(session('hyprctl', '-j', 'monitors'))) == original_outputs,
                'current_original_output_layout')
        require(workspace_state() == original_workspace, 'current_original_active_workspace')
        clients = json.loads(session('hyprctl', '-j', 'clients'))
        if initialized:
            require(json.loads(call('guard', json.dumps(clients)))['owned_clients'] == len(clients),
                    'current_exact_owned_browser_clients')
        else:
            require(clients == [], 'current_no_foreign_managed_clients')
        current_native = observe_native(with_example)
        cursor_owner.observe(json.loads(session('hyprctl', '-j', 'cursorpos')))
        return clients, current_native

    def physical_click(identifier):
        nonlocal pressed
        def observed_target(observation):
            return native_target(find_surface(observation['example_state'], 'intent'),
                                 observation['intent_layer'], identifier)
        _, admitted = interaction_guard()
        initial = observed_target(admitted)
        point = initial['point']
        cursor_owner.begin(json.loads(session('hyprctl', '-j', 'cursorpos')), point)
        session('hyprctl', 'dispatch', compositor_move(point))
        def arrived():
            position = json.loads(session('hyprctl', '-j', 'cursorpos'))
            return cursor_owner.observe(position) == point
        eventually(arrived, 'independent_control_pointer_arrived', timeout=3)
        cursor_owner.finish(json.loads(session('hyprctl', '-j', 'cursorpos')))
        _, admitted = interaction_guard()
        same_target(initial, observed_target(admitted))
        cursor_owner.observe(json.loads(session('hyprctl', '-j', 'cursorpos')))
        try:
            pressed = True  # Also release when QMP delivered a press but lost its reply.
            try:
                a['pointer'](name, True)
                _, admitted = interaction_guard()
                same_target(initial, observed_target(admitted))
            except BaseException:
                # Retire only this identified example before balancing a stale
                # capture. No cancellation key is sent to foreign/auth UI.
                retire_example()
                raise
        finally:
            a['pointer'](name, False)
            pressed = False

    def retire_example():
        arguments = [json.dumps(example_identity)] if example_identity is not None else []
        require(json.loads(call('retire-example', *arguments))['retired'],
                'original_independent_example_retired')

    try:
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(fixture)],
                 data=(ROOT / 'vm/fixtures/application-intent-fixture.py').read_bytes(), timeout=20)
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(scratch + '/owned_application_units.py')],
                 data=(ROOT / 'vm/owned_application_units.py').read_bytes(), timeout=20)
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(scratch + '/application_intent_policy.py')],
                 data=(ROOT / 'vm/application_intent_policy.py').read_bytes(), timeout=20)
        h['run'](lab.ssh_args('lucent') + ['umask 077; cat > ' + shlex.quote(scratch + '/application_intent_browser.py')],
                 data=(ROOT / 'vm/application_intent_browser.py').read_bytes(), timeout=20)
        initialized = True
        entry = json.loads(call('initialize', manifest['binaries']['application-intent']['sha256']))
        require(json.loads(call('initial-absence'))['absent'], 'owned_units_initially_absent')
        baseline = observe_native(False)['baseline']
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
                '--property=UMask=0077', '/usr/bin/python3', fixture, 'start-example', scratch, run_id)
        admitted = None
        def ready():
            nonlocal admitted
            try:
                value = observe_native()
                if find_surface(value['example_state'], 'intent')['hits']:
                    admitted = value
                    return True
                return False
            except Exception:
                return None
        eventually(ready, 'independent_native_example_ready')
        example_identity, example_layer = admitted['example'], admitted['intent_layer']

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
            clients, _ = interaction_guard()
            focused = json.loads(session('hyprctl', '-j', 'activewindow'))
            return bool(focused) and focused.get('class') == entry['class'] and any(
                client.get('address') == focused.get('address') and client.get('pid') == focused.get('pid')
                for client in clients)
        eventually(focused_owned_browser, 'fresh_owned_browser_focus_observed')
        results['checks']['fresh_token_child_delivery_and_fresh_browser_focus_observed'] = True
        eventually(lambda: bool(browser_snapshot()), 'original_blank_browser_control_target_admitted')

        for index, (identifier, operation) in enumerate((
            ('intent-open', 'open'), ('intent-action', 'action'),
        ), start=2):
            before_pages = before_request()
            physical_click(identifier)
            eventually(lambda: finished(index) and len(events()) == index, 'physical_intent_dispatch_completed')
            event = events()[-1]
            require(event['operation'] == operation and event['fresh_token_present']
                    and not event['legacy_startup_present'] and example_state()['client']['fresh_token'],
                    'physical_input_token_delivered_to_selected_child')
            verify_documents_and_retire(before_pages)
        results['checks']['real_browser_opened_space_percent_unicode_and_shell_literal_filenames'] = True
        results['checks']['physical_launch_open_and_declared_action_deliver_fresh_tokens_to_children'] = True
        interaction_guard()
        before_pages = before_request()
        example_command('open')
        eventually(lambda: finished(4) and len(events()) == 4, 'later_socket_open_completed')
        require(not events()[-1]['fresh_token_present'] and not events()[-1]['legacy_startup_present']
                and not example_state()['client']['fresh_token'], 'later_socket_request_cannot_reuse_physical_authority')
        verify_documents_and_retire(before_pages)
        results['checks']['later_socket_request_created_both_fresh_documents'] = True
        interaction_guard()
        before_pages = before_request()
        cli('applications', 'open', entry['app_id'], scratch + '/' + entry['files'][0]['name'],
            scratch + '/' + entry['files'][1]['name'])
        eventually(lambda: len(events()) == 5, 'desktop_cli_open_reaches_real_application')
        require(not events()[-1]['fresh_token_present'] and not events()[-1]['legacy_startup_present'],
                'desktop_cli_has_no_physical_authority')
        verify_documents_and_retire(before_pages)
        results['checks']['each_open_action_socket_and_cli_created_both_fresh_documents'] = True
        results['checks']['fresh_targets_retired_with_original_browser_control_preserved'] = True
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
                retire_example()
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
                require(json.loads(call('retire-watchdog'))['retired'],
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
            eventually(lambda: bool(interaction_guard(False)), 'owned_example_layers_retired_before_cursor_restore')
            cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
            cursor_owner.begin(cursor, cursor_owner.initial)
            session('hyprctl', 'dispatch', compositor_move(cursor_owner.initial))
            def restored():
                cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
                return cursor_owner.observe(cursor) == cursor_owner.initial
            eventually(restored, 'original_cursor_restored', timeout=3)
            cursor_owner.finish(json.loads(session('hyprctl', '-j', 'cursorpos')))
            interaction_guard(False)
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
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
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
    except KeyboardInterrupt:
        result['failure'] = 'native_application_acceptance_interrupted'
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
