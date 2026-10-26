#!/usr/bin/env python3
"""Two separate cold Chromium births through real notification links.

Default is inert; running requires the exact reviewed release and fresh PID.
A successful pointer run must fully retire before a separate keyboard profile
is allocated. No browser is prewarmed, focused, activated, clicked or hidden by
this harness. One actual trusted KeyA must reach each exact owned document.
Only the new disposable profiles can be retired; existing apps are preserved.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import stat
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SOURCE = 'eb3481f4c75da9e0783975ad9c3e920ad8833ce2'
MANIFEST_SHA = 'e48716b0d7b797292032fec62e3c19219f39184b75221e3249b85633e3812183'
REUSE_SOURCE = '927fcd96b66a3435ccb1e4cd8de1d6fcb41a54c9'
sys.path.insert(0, str(ROOT / 'vm'))
import cold_default_uri_policy as c
import default_uri_policy as p


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


warm = module('maintained_warm_default_uri', ROOT / 'vm/test-default-uri-notification.py')
require = warm.require
normal_off = warm.normal_off
group_target = warm.group_target
target = warm.target
keyboard_preflight = warm.keyboard_preflight
key_transport = warm.key_transport
begin_native_case = warm.begin_native_case
native_step = warm.native_step
native_pointer_control = warm.native_pointer_control
handoff_frame = warm.handoff_frame
assert_browser_key_handoff = warm.assert_browser_key_handoff
record_failure = warm.record_failure


CANONICAL = ('vm/test-application-intent.py', 'vm/test-application-actions.py',
             'vm/test-live-wallpapers.py', 'vm/native_surfaces.py', 'vm/owned_fixture_scratch.py',
             'vm/owned_application_units.py', 'vm/application_intent_policy.py',
             'vm/application_intent_browser.py', 'vm/owned_chromium_lease.py',
             'vm/wallpaper_checkpoint.py', 'vm/fixtures/application-intent-fixture.py',
             'vm/multi_output_policy.py', 'vm/preservation_fingerprint.py', 'vm/lab.py')


def qualify(args):
    # Refuse before loading the native transport, VM inventory, or any RPC.
    require(type(MANIFEST_SHA) is str and len(MANIFEST_SHA) == 64,
            'cold_runtime_manifest_unbound')
    require(args.manifest_sha == MANIFEST_SHA and args.release_manifest is not None
            and type(args.expected_desktop_pid) is int and args.expected_desktop_pid > 1
            and args.vm_uuid == 'a4691892-fa44-41d9-98cf-8f0dc4131ee3',
            'exact_cold_runtime_binding_required')
    path = args.release_manifest
    require(path.is_absolute() and path.resolve(strict=True) == path, 'canonical_release_manifest_required')
    value = path.lstat()
    require(stat.S_ISREG(value.st_mode) and value.st_uid == os.getuid() and value.st_size <= 1024 * 1024,
            'owned_release_manifest_required')
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == MANIFEST_SHA, 'exact_reviewed_release_manifest_required')
    manifest = json.loads(raw)
    require(manifest['source_commit'] == SOURCE and manifest['passed'] is True
            and manifest['source_working_tree_changed'] is False, 'exact_built_runtime_source_required')
    for relative in CANONICAL:
        committed = subprocess.check_output(['git', 'show', SOURCE + ':' + relative], cwd=ROOT, timeout=5)
        require((ROOT / relative).read_bytes() == committed, 'canonical_acceptance_source_changed')
    for relative in ('vm/test-default-uri-notification.py', 'vm/fixtures/default-uri-fixture.py',
                     'vm/default_uri_policy.py', 'vm/cdp_peer.py'):
        committed = subprocess.check_output(['git', 'show', REUSE_SOURCE + ':' + relative], cwd=ROOT, timeout=5)
        require((ROOT / relative).read_bytes() == committed, 'maintained_warm_source_changed')
    common = subprocess.check_output(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'],
                                    cwd=ROOT, text=True, timeout=5).strip()
    require((Path(common).parent / 'vm/lab.py').read_bytes() == (ROOT / 'vm/lab.py').read_bytes(),
            'canonical_private_inventory_transport_changed')
    key_transport()
    return manifest


BridgeFailure = warm.BridgeFailure


def bridge_command(argv, operation):
    require(operation in c.OPERATIONS, 'known_cold_bridge_operation')
    def refused(label, error_class='OtherError'):
        raise BridgeFailure({'operation': operation, 'phase': 'transport',
                             'error_class': error_class, 'fixed_reason': label}) from None
    try:
        result = subprocess.run(argv, capture_output=True, timeout=c.bridge_timeout(operation))
    except (OSError, subprocess.TimeoutExpired) as error:
        refused('bounded_bridge_transport_failed', type(error).__name__)
    if len(result.stdout) > 8 * 1024 * 1024 or len(result.stderr) > 65536:
        refused('bounded_bridge_output_required')
    if result.returncode != 0:
        try:
            require(result.stdout == b'' and len(result.stderr) <= 2048, 'one_bridge_error_packet_required')
            packet = c.validate_error(json.loads(result.stderr), operation)
        except (ValueError, TypeError, UnicodeDecodeError):
            refused('invalid_bridge_error_packet')
        raise BridgeFailure(packet)
    try:
        require(result.stderr == b'', 'silent_bridge_success_required')
        value = json.loads(result.stdout)
        require(type(value) is dict and 'failure' not in value, 'bridge_result_object_required')
    except (ValueError, TypeError, UnicodeDecodeError):
        refused('invalid_bridge_success_packet')
    return value


def run_case(args, manifest, sequence):
    require(sequence in p.NOTIFICATION_CASES and args.assert_browser_key_handoff is True,
            'cold_route_and_trusted_key_required')
    sys.path.insert(0, str(ROOT / 'vm'))
    import default_uri_policy as policy
    old = module('canonical_application_intent_acceptance', ROOT / 'vm/test-application-intent.py')
    old.load_helpers()
    lab, session, remote, cli, inspect = old.lab, old.session, old.remote, old.cli, old.inspect
    name = lab.inventory()['vms']['lucent']['name']
    require(lab.virsh('domuuid', name, capture=True).stdout.strip() == args.vm_uuid, 'exact_vm_uuid_changed')
    original_ssh = lab.ssh_args
    def strict_ssh(profile):
        argv = original_ssh(profile)
        require(profile == 'lucent' and argv.count('StrictHostKeyChecking=accept-new') == 1,
                'known_original_vm_transport')
        return ['StrictHostKeyChecking=yes' if value == 'StrictHostKeyChecking=accept-new' else value for value in argv]
    lab.ssh_args = strict_ssh
    old.a['keyboard_preflight']()
    keyboard_preflight(json.loads(session('hyprctl', '-j', 'binds')),
                       getattr(args, 'assert_browser_key_handoff', False))
    expected = manifest['binaries']['lucent-desktop']['sha256']
    actual = json.loads(remote('python3', '-c',
        'import hashlib,json,subprocess;from pathlib import Path;'
        'p=subprocess.check_output(["pgrep","-x","lucent-desktop"]).split();assert len(p)==1;'
        'print(json.dumps({"pid":int(p[0]),"sha256":hashlib.sha256(Path("/proc/"+p[0].decode()+"/exe").read_bytes()).hexdigest()}))'))
    require(actual == {'pid': args.expected_desktop_pid, 'sha256': expected}, 'fresh_exact_running_desktop')
    initial = inspect()
    normal_off(initial)
    require(initial['client']['notifications_ready'] and not initial['client']['notification_history_open']
            and initial['client']['notification_count'] == 0 and not initial['client']['launcher']
            and initial['client']['settings_save_status'] == 'saved', 'idle_native_notification_baseline')
    require(json.loads(session('hyprctl', '-j', 'clients')) == [], 'no_foreign_native_clients')
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1 and all(monitors[0][key] == value for key, value in
            {'name': 'Virtual-1', 'width': 1920, 'height': 1080, 'x': 0, 'y': 0,
             'scale': 1, 'transform': 0, 'dpmsStatus': True}.items()), 'prepared_single_native_output')
    from application_intent_policy import CursorOwnership, compositor_move
    import owned_fixture_scratch
    import preservation_fingerprint
    cursor = CursorOwnership(json.loads(session('hyprctl', '-j', 'cursorpos')))
    before_files = preservation_fingerprint.fingerprint(session)
    before_outputs = [{k: row[k] for k in ('id', 'name', 'width', 'height', 'scale', 'transform', 'x', 'y',
                                        'refreshRate', 'activeWorkspace', 'specialWorkspace')} for row in monitors]
    def scratch_remote(*argv, **kwargs):
        return old.h['run'](lab.ssh_args('lucent') + [shlex.join(argv)], **kwargs)
    scratch = owned_fixture_scratch.allocate(scratch_remote, 'lucent-application-intent')
    run_id = scratch.rsplit('.', 1)[-1]
    sources = {name: path.read_bytes() for name, path in (
        ('fixture.py', ROOT / 'vm/fixtures/application-intent-fixture.py'),
        ('default-uri.py', ROOT / 'vm/fixtures/cold-default-uri-fixture.py'),
        ('default_uri_warm.py', ROOT / 'vm/fixtures/default-uri-fixture.py'),
        ('cold_default_uri_policy.py', ROOT / 'vm/cold_default_uri_policy.py'),
        ('default_uri_policy.py', ROOT / 'vm/default_uri_policy.py'),
        ('owned_application_units.py', ROOT / 'vm/owned_application_units.py'),
        ('application_intent_policy.py', ROOT / 'vm/application_intent_policy.py'),
        ('application_intent_browser.py', ROOT / 'vm/application_intent_browser.py'),
        ('owned_chromium_lease.py', ROOT / 'vm/owned_chromium_lease.py'))}
    header = {'cold_case': sequence, 'desktop_pid': args.expected_desktop_pid, 'desktop_sha256': expected,
              'sources': {key: hashlib.sha256(raw).hexdigest() for key, raw in sources.items()}}
    if getattr(args, 'assert_browser_key_handoff', False):
        sources['cdp_peer.py'] = key_transport()
        header['sources']['cdp_peer.py'] = hashlib.sha256(sources['cdp_peer.py']).hexdigest()
        header['assert_browser_key_handoff'] = True
    sources['default-header.json'] = json.dumps(header).encode()
    for filename, raw in sources.items():
        scratch_remote('/usr/bin/python3', '-I', '-c',
            'import os,sys;from pathlib import Path;p=Path(sys.argv[1]);'
            'fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600);'
            'f=os.fdopen(fd,"wb");f.write(sys.stdin.buffer.read(262145));f.close()',
            scratch + '/' + filename, data=raw, timeout=20)
    def call(operation, *values, canonical=False):
        program = scratch + ('/fixture.py' if canonical else '/default-uri.py')
        argv = ['/usr/bin/python3', *([] if canonical else ['-I']), program, operation, scratch, run_id, *values]
        if canonical:
            return json.loads(session(*argv))
        return bridge_command(lab.ssh_args('lucent') + [shlex.join(
            ['python3', 'desktop-lab/scripts/in-session.py', *argv])], operation)
    def wait(predicate, label, timeout=15):
        limit = time.monotonic() + timeout
        while time.monotonic() < limit:
            value = predicate()
            if value:
                return value
            time.sleep(.2)
        raise ValueError(label)
    def receipt(filename):
        raw = remote('/usr/bin/python3', '-I', '-c',
            'from pathlib import Path;import sys;p=Path(sys.argv[1]);'
            'print(p.read_text() if p.is_file() else "null")', scratch + '/' + filename)
        return json.loads(raw)
    report = {'source_commit': SOURCE, 'manifest_sha256': args.manifest_sha, 'success': False, 'checks': {},
              'scope': 'One genuinely cold owned Chromium birth via a real notification URI and trusted document KeyA; no physical-hardware or performance acceptance.', 'case': sequence,
              'native_input': {}, 'browser_focus': {}, 'cleanup': {}, 'vm_uuid': args.vm_uuid,
              'helper_sources_sha256': header['sources'], 'guest_directory': scratch}
    if getattr(args, 'assert_browser_key_handoff', False):
        report['browser_key_handoff_requested'] = True
    report['host_harness_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    baseline = security = browser = None
    initialized_attempted = False
    guard_started = old_guard_started = False
    held = False
    deadline = time.monotonic() + policy.NATIVE_INPUT_SECONDS
    http_deadline = None
    def native_guard():
        admitted = call('admission')
        require(admitted['baseline'] == baseline, 'original_desktop_supervisor_compositor_lifetimes')
        clients = json.loads(session('hyprctl', '-j', 'clients'))
        if browser is None:
            require(clients == [], 'no_browser_before_owned_cold_dispatch')
        else:
            require(call('cold-guard', json.dumps(clients))['owned_clients'] == len(clients),
                    'only_original_owned_browser_clients')
        current_outputs = [{k: row[k] for k in before_outputs[0]} for row in
                           json.loads(session('hyprctl', '-j', 'monitors'))]
        require(current_outputs == before_outputs, 'original_native_outputs_and_workspaces')
        cursor.observe(json.loads(session('hyprctl', '-j', 'cursorpos')))
        current = inspect()
        normal_off(current)
        require(not current['client']['launcher'] and not current['client']['widget_grid']
                and not current['client']['widgets_editing'] and not current['client']['wallpaper_busy']
                and current['client']['notification_count'] <= 1
                and current['client']['settings_save_status'] == 'saved'
                and not any(current['client'].get(name, {}).get(key) for name in
                    ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'settings', 'displays', 'application_actions')
                    for key in ('open', 'visible', 'busy')), 'no_competing_native_control')
        return current, clients
    try:
        initialized_attempted = True
        entry = call('initialize', manifest['binaries']['application-intent']['sha256'], 'http', '%u', canonical=True)
        baseline = call('admission', json.dumps({'desktop_sha256': expected, 'example_sha256': None}),
                        canonical=True)['baseline']
        call('initial-absence', canonical=True)
        old_guard = 'lucent-application-intent-watchdog-' + run_id + '.service'
        old_guard_started = True
        # This lower bound precedes even unit launch; it never guesses when
        # the canonical 210-second listener actually became ready.
        http_deadline = time.monotonic() + policy.CANONICAL_HTTP_SECONDS
        session('systemd-run', '--user', '--collect', '--unit=' + old_guard,
            '--property=RuntimeMaxSec=300', '--property=MemoryMax=64M', '--property=UMask=0077',
            '/usr/bin/python3', scratch + '/fixture.py', 'watchdog', scratch, run_id)
        wait(lambda: call('http-info', canonical=True).get('ready'), 'original_http_watchdog_ready')
        prepared = call('prepare')
        unit = 'lucent-default-uri-' + run_id + '.service'
        guard_started = True
        session('systemd-run', '--user', '--collect', '--unit=' + unit,
            '--property=RuntimeMaxSec=' + str(policy.GUARDIAN_UNIT_RUNTIME_SECONDS),
            '--property=TimeoutStopSec=45', '--property=MemoryMax=96M',
            '--property=KillMode=control-group', '--property=UMask=0077', '/usr/bin/python3', '-I',
            scratch + '/default-uri.py', 'watchdog', scratch, run_id)
        wait(lambda: receipt('default-ready.json'), 'independent_config_guardian_ready')
        # No browser process, profile or blank control is created in setup.
        post = report['cold_setup'] = {'phase': 'publication'}
        call('publish')
        admitted = call('admission')
        require(admitted['baseline'] == baseline, 'same_authority_before_cold_browser_dispatch')
        security = call('security')
        http = call('http-info', canonical=True)['identity']
        uri = http['targets'][0]['uri']
        report['checks']['temporary_real_mime_default_selected'] = prepared['app_id'].endswith('.desktop')
        post['phase'] = 'complete'
        def set_button_held(value):
            nonlocal held
            held = value
        # A route owns its entire scratch/profile/guard lifetime. The caller
        # cannot start the second route until this one's cleanup succeeds.
        begin_native_case(report, sequence)
        native_step(report, 'case_preflight', native_guard)
        native_step(report, 'notification_budget_admission', lambda: policy.admit_notification_case(
            time.monotonic(), deadline, http_deadline))
        note = native_step(report, 'notification_create', lambda: call('notify', sequence)['id'])
        native_step(report, 'notification_history_show', lambda: cli('notifications', 'show'))
        def selected_group():
            current, _ = native_guard()
            return group_target(current, json.loads(session('hyprctl', '-j', 'layers')),
                                run_id, args.expected_desktop_pid)
        def group_ready():
            try:
                return selected_group()
            except ValueError as error:
                if str(error) in ('one_native_notification_surface', 'one_current_owned_notification_group',
                                  'actual_native_notification_history_geometry'):
                    return False
                raise
        native_step(report, 'group_target_wait', lambda: wait(group_ready, 'owned_notification_group_native_history_ready'))
        # Expanding a group is presentation setup. It must use the actual
        # owned group button; URI link delivery below stays independent.
        native_pointer_control(report, 'group', selected_group, cursor,
            lambda: json.loads(session('hyprctl', '-j', 'cursorpos')),
            lambda point: session('hyprctl', 'dispatch', compositor_move(point)),
            lambda down: old.a['pointer'](name, down), wait, deadline, set_button_held)
        observed_revision = None
        def selected():
            current, _ = native_guard()
            value = target(current, json.loads(session('hyprctl', '-j', 'layers')), note, uri,
                           args.expected_desktop_pid)
            require(observed_revision is None or value['hit']['id'] ==
                    f'notification-body-{note}-{observed_revision}-0/link/{uri}',
                    'changed_notification_revision_is_not_adopted')
            return value
        def link_ready():
            current, _ = native_guard()
            try:
                return target(current, json.loads(session('hyprctl', '-j', 'layers')), note, uri,
                              args.expected_desktop_pid)
            except ValueError as error:
                if str(error) in ('one_native_notification_surface', 'one_current_selected_notification_link',
                                  'actual_native_notification_history_geometry'):
                    return False
                raise
        native_step(report, 'notification_link_wait', lambda: wait(link_ready, 'owned_rich_link_native_history_ready'))
        report.setdefault('history_setup', {})[sequence] = {'group_native_pointer_focus': True}
        observed_revision = native_step(report, 'notification_revision_capture', lambda: call('observe-note', sequence)['revision'])
        native_step(report, 'notification_target_validate', selected)
        if getattr(args, 'assert_browser_key_handoff', False):
            key_before = native_step(report, 'browser_key_frame_capture', lambda: handoff_frame(native_guard()[0]))
        if sequence == 'pointer':
            native_pointer_control(report, 'pointer', selected, cursor,
                lambda: json.loads(session('hyprctl', '-j', 'cursorpos')),
                lambda point: session('hyprctl', 'dispatch', compositor_move(point)),
                lambda down: old.a['pointer'](name, down), wait, deadline, set_button_held)
            report['native_input']['pointer_link_focus'] = True
        else:
            tab_presses = 0
            for _ in range(128):
                value = native_step(report, 'keyboard_target_capture', selected)
                if tab_presses > 0 and value['focus'] == value['hit']['id']:
                    break
                native_step(report, 'keyboard_deadline_assert', lambda: require(
                    time.monotonic() < deadline, 'bounded_native_keyboard_window'))
                native_step(report, 'keyboard_tab_dispatch', lambda: lab.virsh(
                    'send-key', name, '--codeset', 'linux', '--holdtime', '50', '15', capture=True))
                tab_presses += 1
                time.sleep(.08)
            else:
                native_step(report, 'keyboard_focus_assert', lambda: require(False, 'owned_rich_link_not_reached_by_actual_tab'))
            native_step(report, 'keyboard_focus_assert', lambda: require(
                selected()['focus'] == selected()['hit']['id'], 'actual_keyboard_focus_on_owned_link'))
            native_step(report, 'keyboard_enter_dispatch', lambda: lab.virsh(
                'send-key', name, '--codeset', 'linux', '--holdtime', '50', '28', capture=True))
            report['native_input']['tab_enter_link_focus'] = True
            report['native_input']['tab_events_sent'] = tab_presses
        count = 1
        native_step(report, 'default_handler_delivery_wait', lambda: wait(
            lambda: len(call('state')['bridge']) == count and len(call('state')['events']) == count,
            'selected_default_handler_received_native_uri'))
        state = native_step(report, 'default_handler_state_read', lambda: call('state'))
        bridge, event = state['bridge'][-1], state['events'][-1]
        native_step(report, 'default_handler_authority_assert', lambda: require(
                bridge == {'selected_uri_matches': True, 'fresh_token_present': True, 'legacy_startup_present': False}
                and event['uris'] == [uri] and event['fresh_token_present'] and not event['legacy_startup_present'],
                'native_default_handler_and_browser_wrapper_receive_fresh_authority'))
        def cold_ready():
            value = call('cold-ready')
            return value if value.get('ready') is True else False
        observed = native_step(report, 'browser_document_wait', lambda: wait(
            cold_ready, 'one_fresh_selected_real_browser_document', 30))
        browser = observed['identity']
        fresh = [browser['target']]
        report['cold_browser'] = browser
        _, clients = native_step(report, 'browser_guard', native_guard)
        focus = native_step(report, 'browser_focus_read', lambda: json.loads(session('hyprctl', '-j', 'activewindow')))
        report['browser_focus'][sequence] = bool(focus) and focus.get('class') == entry['class'] and any(
            row.get('pid') == focus.get('pid') and row.get('address') == focus.get('address') for row in clients)
        if getattr(args, 'assert_browser_key_handoff', False):
            assert_browser_key_handoff(report, {'identity': browser, 'before': [],
                'targets': fresh, 'case': sequence}, key_before, call, native_guard, wait,
                lambda: (old.a['keyboard_preflight'](),
                         keyboard_preflight(json.loads(session('hyprctl', '-j', 'binds')), browser_key=True)),
                lambda: lab.virsh('send-key', name, '--codeset', 'linux', '--holdtime', '50', '30', capture=True),
                deadline)
        native_step(report, 'notification_expiry_wait', lambda: require(
            call('wait-expiry')['absent_after_expiry_window'], 'notification_absence_after_bounded_expiry'))
        current, _ = native_step(report, 'notification_post_expiry_guard', native_guard)
        native_step(report, 'notification_absence_assert', lambda: require(
            current['client']['notification_count'] == 0, 'foreign_notification_must_not_be_hidden_between_cases'))
        require(not current['client']['notification_history_open'], 'natural_history_close_remains_current')
        report['checks'][sequence + '_fresh_token_default_route_exact_document'] = True
        report['checks'][sequence + '_notification_absent_after_expiry_window'] = True
        native_step(report, 'case_complete', lambda: None)
        report['success'] = True
    except BaseException as error:
        phase = report.get('native_flow', {}).get('phase', report.get('cold_setup', {}).get('phase', 'setup'))
        record_failure(report, error, 'native_acceptance' if phase == 'complete' else phase)
    finally:
        if held:
            try:
                old.a['pointer'](name, False)
            except BaseException:
                report['cleanup']['pointer_release'] = False
        if guard_started:
            try:
                done = receipt('default-finished.json')
                if done is None:
                    call('finish')
                    done = wait(lambda: receipt('default-finished.json'), 'independent_default_config_restore_receipt',
                                policy.FINISH_RECEIPT_SECONDS)
                require(done['success'] is True and all(done['restoration'].values()), 'complete_guardian_restoration')
                report['cleanup'].update(done['restoration'])
                require(call('retire-guardian')['retired'], 'original_configuration_guardian_retired')
                report['cleanup']['configuration_guardian_retired'] = True
            except BaseException:
                report['cleanup']['guardian_restoration'] = False
        if initialized_attempted:
            try:
                retired = call('cold-retire')
                require(retired['owned_browser_units_retired'] and retired['owned_entry_retired'],
                        'canonical_browser_and_entry_retirement')
                report['cleanup']['canonical_browser_and_entry_retired'] = True
            except BaseException:
                report['cleanup']['canonical_browser_and_entry_retired'] = False
        if old_guard_started:
            try:
                require(call('retire-watchdog', canonical=True)['http_server_retired'], 'original_http_guardian_retired')
                report['cleanup']['original_http_watchdog_retired'] = True
            except BaseException:
                report['cleanup']['original_http_watchdog_retired'] = False
        try:
            require(preservation_fingerprint.fingerprint(session) == before_files, 'settings_media_clipboard_and_supervision_preserved')
            require(security is None or call('security') == security, 'fixed_secure_clients_and_login_configuration_preserved')
            require(json.loads(session('hyprctl', '-j', 'clients')) == [], 'all_owned_browser_windows_retired')
            require(baseline is None or call('admission', json.dumps({'desktop_sha256': expected,
                    'example_sha256': None}), canonical=True)['baseline'] == baseline,
                    'original_native_authority_before_cursor_restore')
            restored_outputs = [{k: row[k] for k in before_outputs[0]} for row in
                                json.loads(session('hyprctl', '-j', 'monitors'))]
            require(restored_outputs == before_outputs, 'original_native_outputs_before_cursor_restore')
            cursor.begin(json.loads(session('hyprctl', '-j', 'cursorpos')), cursor.initial)
            session('hyprctl', 'dispatch', compositor_move(cursor.initial))
            cursor.finish(json.loads(session('hyprctl', '-j', 'cursorpos')))
            current = inspect()['client']
            require(current['notification_count'] == 0 and not current['notification_history_open'], 'original_notification_state_restored')
            report['cleanup']['caller_state_and_cursor_preserved'] = True
        except BaseException:
            report['cleanup']['caller_state_and_cursor_preserved'] = False
        report['success'] = report['success'] and bool(report['cleanup']) and all(report['cleanup'].values())
        report['private_stage_retained'] = True
        path = lab.ROOT / 'reports/local' / ('native-cold-default-uri-' + sequence + '-' + run_id + '.json')
        old.write_private_report(path, report)
        print(json.dumps({'success': report['success'], 'report': str(path.relative_to(lab.ROOT)),
                          'performance_accepted': False, 'physical_hardware_accepted': False}))
    return report


def run(args, manifest):
    receipts = []
    for case in p.NOTIFICATION_CASES:
        report = run_case(args, manifest, case)
        receipts.append(report)
        if not report['success']:
            return 1
        c.case_receipts(receipts)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--release-manifest', type=Path)
    parser.add_argument('--manifest-sha')
    parser.add_argument('--expected-desktop-pid', type=int)
    parser.add_argument('--vm-uuid')
    args = parser.parse_args()
    args.assert_browser_key_handoff = True
    if not args.run:
        print(json.dumps({'source_only_acceptance_plan': True, 'guest_actions': False,
                          'runtime_binding': 'UNBOUND' if MANIFEST_SHA is None else 'explicit',
                          'cases': list(p.NOTIFICATION_CASES), 'separate_cold_births': True,
                          'trusted_browser_key_required': True,
                          'native_or_performance_accepted': False,
                          'profile_policy': 'Only new disposable profiles; no existing apps or successors adopted.'}))
        return 0
    return run(args, qualify(args))


if __name__ == '__main__':
    raise SystemExit(main())
