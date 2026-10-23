#!/usr/bin/env python3
"""Owned native notification rich-link/default-handler acceptance plan.

Default is inert. --run needs an exact release manifest, fresh desktop PID and
VM UUID. This never deploys, logs in, creates activation tokens or forces focus.
The existing fixture owns a warm Chromium profile/HTTP listener. Real pointer
and Tab/Enter requests must each deliver fresh authority through xdg-open and
create one exact fresh document. Browser focus is reported independently.
Read-only startup proof precedes the canonical first immutable browser lease;
fixed readiness phases distinguish missing startup state from ownership errors.
Every browser window in the new private test profile is closed by canonical
teardown. All content there, including user-added content, is disposable and
unprotected; private evidence may remain. Existing personal profiles, browser
processes and successors are never adopted.
Notification IDs are observation data only; fixtures use bounded automatic
expiry. A changed or remaining foreign notification is never dismissed.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SOURCE = '26d03adf476af44c97fb7dcdbfb75cbdc2112770'
MANIFEST_SHA = '5b228fa1bbdc71c50a245edcb70e726ef4d5826f476ba261e53042023d322aec'


def require(value, reason):
    if not value:
        raise ValueError(reason)


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def qualify(args):
    require(isinstance(args.vm_uuid, str) and re.fullmatch(
        '[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}', args.vm_uuid), 'exact_vm_uuid_required')
    require(type(args.expected_desktop_pid) is int and args.expected_desktop_pid > 1
            and args.manifest_sha == MANIFEST_SHA and args.release_manifest is not None,
            'reviewed_manifest_and_fresh_desktop_pid_required')
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
    # Reusing the existing acceptance stack must not execute locally edited
    # helpers under a source label belonging to the archived runtime.
    for relative in ('vm/test-application-intent.py', 'vm/test-application-actions.py',
                     'vm/test-live-wallpapers.py', 'vm/native_surfaces.py', 'vm/owned_fixture_scratch.py',
                     'vm/owned_application_units.py', 'vm/application_intent_policy.py',
                     'vm/application_intent_browser.py', 'vm/owned_chromium_lease.py',
                     'vm/wallpaper_checkpoint.py', 'vm/fixtures/application-intent-fixture.py',
                     'vm/multi_output_policy.py', 'vm/preservation_fingerprint.py', 'vm/lab.py'):
        committed = subprocess.check_output(['git', 'show', SOURCE + ':' + relative], cwd=ROOT, timeout=5)
        require((ROOT / relative).read_bytes() == committed, 'canonical_acceptance_source_changed')
    common = subprocess.check_output(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'],
                                     cwd=ROOT, text=True, timeout=5).strip()
    canonical_transport = Path(common).parent / 'vm/lab.py'
    require(canonical_transport.read_bytes() == (ROOT / 'vm/lab.py').read_bytes(),
            'canonical_private_inventory_transport_changed')
    return manifest


def notification_surface(snapshot, layers, desktop_pid):
    # Only one short, unwrapped owned link is used. Never infer a native origin
    # from a toolkit layout or confuse its logical pixels with physical buffers.
    from multi_output_policy import layer_for
    found = [row for row in snapshot['surfaces'] if row.get('role', row['id']) == 'notifications']
    require(len(found) == 1, 'one_native_notification_surface')
    surface = found[0]
    require(surface['keyboard'] == 'exclusive' and surface['pointer_input'] == 'all'
            and surface['frames'] > 0 and surface['native_frames'] > 0
            and surface['output'] == 'Virtual-1' and surface['density'] == 1
            and (surface['width'], surface['height'], surface['buffer_width'], surface['buffer_height'])
            == (1920, 1080, 1920, 1080), 'actual_native_notification_history_geometry')
    return surface, layer_for(layers, 'Virtual-1', surface, desktop_pid)


def notification_hit(surface, layer, hit):
    from multi_output_policy import hit_center
    monitor = {'x': 0, 'y': 0, 'width': 1920, 'height': 1080, 'scale': 1}
    point = hit_center(monitor, surface, layer, hit['id'])
    return {'surface': surface['id'], 'instance': surface['instance'], 'output': surface['output'],
            'hit': hit, 'layer': layer, 'point': point, 'focus': surface.get('focus')}


def target(snapshot, layers, note, uri, desktop_pid):
    surface, layer = notification_surface(snapshot, layers, desktop_pid)
    hits = [hit for hit in surface['hits'] if re.fullmatch(
        'notification-body-' + str(note) + r'-[0-9]+-0/link/' + re.escape(uri), hit['id'])]
    require(len(hits) == 1, 'one_current_selected_notification_link')
    hit = hits[0]
    return notification_hit(surface, layer, hit)


def group_target(snapshot, layers, run_id, desktop_pid):
    require(type(run_id) is str and re.fullmatch('[a-f0-9]{12}', run_id) is not None,
            'owned_notification_group_run_required')
    surface, layer = notification_surface(snapshot, layers, desktop_pid)
    identifier = 'notification-group-name:Lucent owned URI ' + run_id
    hits = [hit for hit in surface['hits'] if hit['id'] == identifier]
    require(len(hits) == 1, 'one_current_owned_notification_group')
    return notification_hit(surface, layer, hits[0])


def keyboard_preflight(bindings):
    from application_intent_policy import unbound_pointer
    unbound_pointer(bindings)
    for row in bindings:
        if row['modmask'] & ~(2 | 16):
            continue
        require(not row.get('catch_all') and str(row.get('key', '')).lower() not in
                ('tab', 'return', 'enter', 'escape', 'esc', 'code:15', 'code:23', 'code:28',
                 'code:36', 'code:1', 'code:9') and row.get('keycode', 0) not in (1, 9, 15, 23, 28, 36),
                'plain_native_notification_keys_must_be_unbound')


def normal_off(snapshot):
    require(snapshot['frame_profiling'] is False
            and snapshot['presentation_feedback']['enabled'] is False
            and all(row['source_frames'] is None for row in snapshot['client']['live_wallpaper']['producers']),
            'normal_off_instrumentation_required')


def admit_warm_browser(call, wait, diagnostics):
    """Persist the first canonical lease only after read-only startup proof."""
    from default_uri_policy import warm_diagnostic
    diagnostics.update(polls=0, phases=[], immutable_admission=False)
    proven = False
    def ready():
        nonlocal proven
        diagnostics['phase'] = 'readonly_probe'
        value = warm_diagnostic(call('warm-ready'))
        diagnostics['polls'] += 1
        require(diagnostics['polls'] <= 256, 'bounded_warm_readiness_observations')
        if not diagnostics['phases'] or diagnostics['phases'][-1] != value:
            require(len(diagnostics['phases']) < 64, 'bounded_warm_readiness_phase_history')
            diagnostics['phases'].append(value)
        require(not value['blocked'], 'warm_readonly_' + value['phase'] + '_refused')
        proven = value['ready']
        return value['ready']
    wait(ready, 'warm_readonly_startup_not_ready', timeout=30)
    require(proven, 'warm_readonly_startup_not_proven')
    diagnostics['phase'] = 'immutable_admission'
    value = call('browser-snapshot', json.dumps({'identity': None}), canonical=True)
    require(value['pages'] == [value['identity']['control']], 'only_original_blank_control')
    diagnostics['immutable_admission'] = True
    return value


class BridgeFailure(ValueError):
    def __init__(self, details):
        super().__init__('owned_default_uri_bridge_failed')
        self.details = dict(details)


def bridge_command(argv, operation):
    """Keep nonzero bridge failures observable without relaying private stderr."""
    import default_uri_policy as p
    require(operation in p.BRIDGE_OPERATIONS, 'known_default_uri_bridge_operation')
    def refused(reason, error_class='OtherError'):
        raise BridgeFailure({'operation': operation, 'phase': 'transport',
                             'error_class': error_class, 'fixed_reason': reason}) from None
    try:
        result = subprocess.run(argv, capture_output=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        name = type(error).__name__
        refused('bounded_bridge_transport_failed', name if name in p.BRIDGE_ERROR_CLASSES else 'OtherError')
    if len(result.stdout) > 8 * 1024 * 1024 or len(result.stderr) > 65536:
        refused('bounded_bridge_output_required')
    if result.returncode != 0:
        try:
            require(result.stdout == b'' and len(result.stderr) <= 2048, 'one_bridge_error_packet_required')
            packet = p.bridge_error_packet(json.loads(result.stderr), operation)
        except (ValueError, TypeError, UnicodeDecodeError):
            refused('invalid_bridge_error_packet')
        raise BridgeFailure(packet)
    try:
        require(result.stderr == b'', 'silent_bridge_success_required')
        value = json.loads(result.stdout)
        require(type(value) is dict, 'bridge_result_object_required')
        if 'failure' in value:
            raise BridgeFailure(p.bridge_error_packet(value, operation))
    except (ValueError, TypeError, UnicodeDecodeError) as error:
        if isinstance(error, BridgeFailure):
            raise
        refused('invalid_bridge_success_packet')
    return value


POST_WARM_PHASES = ('warm_state', 'warm_authority_assert', 'publication', 'published_admission',
                    'published_authority_assert', 'security_snapshot', 'http_identity',
                    'http_identity_assert', 'default_selected_assert')

# Fixed labels from this controller and its pinned pure native policies.
# Never infer a diagnostic reason from a regex match on exception payloads.
FAILURE_REASONS = (
    'actual_keyboard_focus_on_owned_link', 'actual_native_notification_history_geometry',
    'all_owned_browser_windows_retired', 'ambiguous_captured_host_surface_ids',
    'ambiguous_output_names', 'application_cursor_authority_lost',
    'application_cursor_transition_not_confirmed', 'bounded_actual_pointer_bindings',
    'bounded_exact_pointer_destination', 'bounded_native_bindings_required',
    'bounded_native_input_window', 'bounded_native_keyboard_window',
    'bounded_native_layer_tree', 'bounded_subprocess_failed',
    'bounded_warm_readiness_observations', 'bounded_warm_readiness_phase_history',
    'bridge_result_object_required', 'button_state_must_be_boolean',
    'canonical_acceptance_source_changed', 'canonical_browser_and_entry_retirement',
    'canonical_private_inventory_transport_changed', 'canonical_release_manifest_required',
    'captured_desktop_layer_owner_required', 'captured_host_retirement_authority_required',
    'captured_host_surfaces_required', 'captured_monitor_host_instance_required',
    'changed_notification_revision_is_not_adopted', 'complete_actual_pointer_binding',
    'complete_guardian_restoration', 'complete_native_binding_required',
    'concrete_native_layer_namespace', 'concrete_owned_example_native_layer',
    'current_owned_intent_hit_and_layer_lifetime', 'current_owned_native_intent_geometry',
    'declared_cursor_transition_required', 'escape_or_plain_pointer_binding_intercepts_native_test',
    'exact_built_runtime_source_required', 'exact_current_native_rich_link_before_press',
    'exact_current_owned_group_before_press', 'exact_desktop_native_layer_pid',
    'exact_guardian_cursor_destination_required', 'exact_initial_application_cursor',
    'exact_initial_native_cursor_required', 'exact_owned_document_retired',
    'exact_owned_example_native_layer_pid', 'exact_owned_example_socket_peer',
    'exact_reviewed_release_manifest_required', 'exact_vm_uuid_changed',
    'exact_vm_uuid_required', 'finite_owned_intent_geometry',
    'fixed_native_phase_required', 'fixed_post_warm_phase_required',
    'fixed_secure_clients_and_login_configuration_preserved', 'foreign_layout_changed',
    'foreign_native_layer_owner_or_authorization', 'foreign_notification_must_not_be_hidden_between_cases',
    'foreign_or_uncertain_cursor_movement_blocks_warp', 'fractional_original_edge',
    'framework_native_geometry_not_settled', 'fresh_exact_running_desktop',
    'fully_visible_owned_intent_control', 'group',
    'hit_not_safely_visible', 'hit_outside_target_output',
    'identified_desktop_layer_owner', 'idle_native_notification_baseline',
    'incomplete_native_monitor_identity', 'independent_config_guardian_ready',
    'independent_default_config_restore_receipt', 'invalid_owned_output_name',
    'known_default_uri_bridge_operation', 'known_http_uri_field_code_required',
    'known_native_case_required', 'known_native_pointer_control_required',
    'known_original_vm_transport', 'named_native_hosts_required',
    'native_default_handler_and_browser_wrapper_receive_fresh_authority', 'native_layer_monitor_missing',
    'native_layer_owner_changed', 'native_pointer_focus_on_owned_group',
    'native_pointer_focus_on_owned_link', 'no_competing_native_control',
    'no_foreign_native_clients', 'nonfinite_geometry',
    'normal_off_instrumentation_required', 'notification_absence_after_bounded_expiry',
    'one_bridge_error_packet_required', 'one_current_host_surface_required',
    'one_current_monitor_host_required', 'one_current_owned_intent_hit',
    'one_current_owned_notification_group', 'one_current_selected_notification_link',
    'one_current_wayland_global_required', 'one_declared_application_cursor_transition',
    'one_exact_owned_example_layer', 'one_fresh_selected_real_browser_document',
    'one_live_hit_required', 'one_native_layer_on_exact_output_required',
    'one_native_notification_surface', 'one_original_output_required',
    'only_original_blank_control', 'only_original_owned_browser_clients',
    'original_configuration_guardian_retired', 'original_desktop_supervisor_compositor_lifetimes',
    'original_http_guardian_retired', 'original_http_watchdog_ready',
    'original_native_authority_before_cursor_restore', 'original_native_outputs_and_workspaces',
    'original_native_outputs_before_cursor_restore', 'original_notification_state_restored',
    'owned_history_closed_between_cases', 'owned_intent_point_on_prepared_output',
    'owned_notification_group_native_history_ready', 'owned_notification_group_run_required',
    'owned_output_layout_changed', 'owned_output_lifetime_changed',
    'owned_release_manifest_required', 'owned_rich_link_native_history_ready',
    'owned_rich_link_not_reached_by_actual_tab', 'plain_native_notification_keys_must_be_unbound',
    'plain_pointer_binding_intercepts_owned_input', 'pointer_input_disabled',
    'prepared_single_native_output', 'reviewed_manifest_and_fresh_desktop_pid_required',
    'same_authority_after_warm_browser_setup', 'selected_default_handler_received_native_uri',
    'settings_media_clipboard_and_supervision_preserved', 'silent_bridge_success_required',
    'singular_field_code_requires_http_mode', 'submitted_owned_intent_surface_required',
    'unowned_scale_request', 'unsupported_monitor_layout',
    'warm_original_browser_recorded', 'warm_readonly_startup_not_proven',
    'warm_readonly_startup_not_ready', 'warm_setup_has_no_input_authority',
)

NATIVE_CASES = ('pointer', 'keyboard')
NATIVE_PHASES = (
    'case_preflight', 'notification_create', 'notification_history_show',
    'group_target_wait', 'group_target_capture', 'group_cursor_begin', 'group_cursor_dispatch',
    'group_cursor_confirm', 'group_target_revalidate', 'group_target_assert', 'group_deadline_assert',
    'group_button_down', 'group_focus_wait', 'group_button_up', 'notification_link_wait',
    'notification_revision_capture', 'notification_target_validate',
    'pointer_target_capture', 'pointer_cursor_begin', 'pointer_cursor_dispatch', 'pointer_cursor_confirm',
    'pointer_target_revalidate', 'pointer_target_assert', 'pointer_deadline_assert',
    'pointer_button_down', 'pointer_focus_wait', 'pointer_button_up',
    'keyboard_target_capture', 'keyboard_deadline_assert', 'keyboard_tab_dispatch',
    'keyboard_focus_assert', 'keyboard_enter_dispatch', 'default_handler_delivery_wait',
    'default_handler_state_read', 'default_handler_authority_assert', 'browser_document_wait',
    'browser_guard', 'browser_focus_read', 'browser_targets_retire', 'notification_expiry_wait',
    'notification_post_expiry_guard', 'notification_absence_assert', 'notification_history_hide',
    'notification_history_closed_wait', 'case_complete',
)


def begin_native_case(report, case):
    require(type(case) is str and case in NATIVE_CASES, 'known_native_case_required')
    report['native_flow'] = {'current_case': case, 'phase': 'case_preflight',
                             'group_dispatch_attempted': False, 'group_dispatch_returned': False,
                             'pointer_dispatch_attempted': False, 'pointer_dispatch_returned': False}


def native_step(report, phase, action):
    require(phase in NATIVE_PHASES, 'fixed_native_phase_required')
    flow = report.get('native_flow', {})
    case = flow.get('current_case')
    require(type(case) is str and case in NATIVE_CASES
            and (not phase.startswith('pointer_') or case == 'pointer')
            and (not phase.startswith('keyboard_') or case == 'keyboard'), 'known_native_case_required')
    flow['phase'] = phase
    dispatch = {'group_button_down': 'group', 'pointer_button_down': 'pointer'}.get(phase)
    if dispatch:
        flow[dispatch + '_dispatch_attempted'] = True
    try:
        value = action()
    except BaseException as error:
        record_failure(report, error, phase)
        raise
    if dispatch:
        flow[dispatch + '_dispatch_returned'] = True
    return value


def native_pointer_control(report, kind, observe, cursor, read_cursor, move_cursor, button, wait,
                           deadline, set_button_held):
    require(kind in ('group', 'pointer') and (kind != 'pointer'
            or report.get('native_flow', {}).get('current_case') == 'pointer'),
            'known_native_pointer_control_required')
    def step(suffix, action):
        return native_step(report, kind + '_' + suffix, action)
    initial = step('target_capture', observe)
    step('cursor_begin', lambda: cursor.begin(read_cursor(), initial['point']))
    step('cursor_dispatch', lambda: move_cursor(initial['point']))
    step('cursor_confirm', lambda: cursor.finish(read_cursor()))
    after = step('target_revalidate', observe)
    step('target_assert', lambda: require(
        all(after[key] == initial[key] for key in ('surface', 'instance', 'output', 'hit', 'layer', 'point')),
        'exact_current_owned_group_before_press' if kind == 'group' else 'exact_current_native_rich_link_before_press'))
    step('deadline_assert', lambda: require(time.monotonic() < deadline, 'bounded_native_input_window'))
    set_button_held(True)
    try:
        step('button_down', lambda: button(True))
        step('focus_wait', lambda: wait(lambda: observe()['focus'] == initial['hit']['id'],
            'native_pointer_focus_on_owned_group' if kind == 'group' else 'native_pointer_focus_on_owned_link', 3))
    finally:
        step('button_up', lambda: button(False))
        set_button_held(False)


def post_warm_step(diagnostic, phase, action):
    require(phase in POST_WARM_PHASES, 'fixed_post_warm_phase_required')
    diagnostic['phase'] = phase
    value = action()
    diagnostic.setdefault('completed', []).append(phase)
    return value


def record_failure(report, error, phase):
    """The first failure survives cleanup; all recorded labels are fixed."""
    if 'failure' in report:
        return
    import default_uri_policy as p
    classes = p.BRIDGE_ERROR_CLASSES + ('BridgeFailure', 'CheckFailure', 'FixtureFailure', 'KeyboardInterrupt', 'Blocked')
    value = {'error_class': type(error).__name__ if type(error).__name__ in classes else 'OtherError',
             'scope': 'native_default_uri_acceptance',
             'phase': phase if phase in (*POST_WARM_PHASES, *NATIVE_PHASES, 'setup', 'native_acceptance', 'cleanup') else 'setup'}
    flow = report.get('native_flow', {})
    if flow.get('current_case') in NATIVE_CASES:
        value['current_case'] = flow['current_case']
        for key in ('group_dispatch_attempted', 'group_dispatch_returned',
                    'pointer_dispatch_attempted', 'pointer_dispatch_returned'):
            value[key] = flow.get(key) is True
    if isinstance(error, BridgeFailure):
        value['bridge'] = dict(error.details)
    elif len(error.args) == 1 and type(error.args[0]) is str and (
            error.args[0] in FAILURE_REASONS
            or error.args[0] in tuple('warm_readonly_' + item + '_refused' for item in p.WARM_PHASES)):
        value['fixed_reason'] = error.args[0]
    report['failure'] = value


def run(args, manifest):
    sys.path.insert(0, str(ROOT / 'vm'))
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
    keyboard_preflight(json.loads(session('hyprctl', '-j', 'binds')))
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
    from application_intent_browser import fresh_documents
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
        ('default-uri.py', ROOT / 'vm/fixtures/default-uri-fixture.py'),
        ('default_uri_policy.py', ROOT / 'vm/default_uri_policy.py'),
        ('owned_application_units.py', ROOT / 'vm/owned_application_units.py'),
        ('application_intent_policy.py', ROOT / 'vm/application_intent_policy.py'),
        ('application_intent_browser.py', ROOT / 'vm/application_intent_browser.py'),
        ('owned_chromium_lease.py', ROOT / 'vm/owned_chromium_lease.py'))}
    header = {'desktop_pid': args.expected_desktop_pid, 'desktop_sha256': expected,
              'sources': {key: hashlib.sha256(raw).hexdigest() for key, raw in sources.items()}}
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
    report = {'source_commit': SOURCE, 'manifest_sha256': MANIFEST_SHA, 'success': False, 'checks': {},
              'scope': 'Owned warm-browser default-handler transport; no fresh-browser focus, direct Chromium token-consumption, physical-hardware or performance acceptance.',
              'native_input': {}, 'browser_focus': {}, 'cleanup': {}, 'vm_uuid': args.vm_uuid,
              'helper_sources_sha256': header['sources'], 'guest_directory': scratch}
    report['host_harness_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    baseline = security = browser = None
    initialized_attempted = False
    guard_started = old_guard_started = False
    held = False
    deadline = time.monotonic() + 125
    def native_guard():
        admitted = call('admission')
        require(admitted['baseline'] == baseline, 'original_desktop_supervisor_compositor_lifetimes')
        clients = json.loads(session('hyprctl', '-j', 'clients'))
        require(call('guard', json.dumps(clients), canonical=True)['owned_clients'] == len(clients),
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
        session('systemd-run', '--user', '--collect', '--unit=' + old_guard,
            '--property=RuntimeMaxSec=300', '--property=MemoryMax=64M', '--property=UMask=0077',
            '/usr/bin/python3', scratch + '/fixture.py', 'watchdog', scratch, run_id)
        wait(lambda: call('http-info', canonical=True).get('ready'), 'original_http_watchdog_ready')
        prepared = call('prepare')
        unit = 'lucent-default-uri-' + run_id + '.service'
        guard_started = True
        session('systemd-run', '--user', '--collect', '--unit=' + unit,
            '--property=RuntimeMaxSec=240', '--property=TimeoutStopSec=45', '--property=MemoryMax=96M',
            '--property=KillMode=control-group', '--property=UMask=0077', '/usr/bin/python3', '-I',
            scratch + '/default-uri.py', 'watchdog', scratch, run_id)
        wait(lambda: receipt('default-ready.json'), 'independent_config_guardian_ready')
        call('warm')
        def browser_state():
            return call('browser-snapshot', json.dumps({'identity': browser}), canonical=True)
        wait(lambda: len(call('state', canonical=True)['events']) == 1, 'warm_original_browser_recorded')
        report['warm_readiness'] = {}
        browser = admit_warm_browser(call, wait, report['warm_readiness'])['identity']
        post = report['post_warm'] = {}
        warm = post_warm_step(post, 'warm_state', lambda: call('state', canonical=True)['events'][0])
        post_warm_step(post, 'warm_authority_assert', lambda: require(
            not warm['fresh_token_present'] and not warm['legacy_startup_present'], 'warm_setup_has_no_input_authority'))
        post_warm_step(post, 'publication', lambda: call('publish'))
        admitted = post_warm_step(post, 'published_admission', lambda: call('admission'))
        post_warm_step(post, 'published_authority_assert', lambda: require(
            admitted['baseline'] == baseline, 'same_authority_after_warm_browser_setup'))
        security = post_warm_step(post, 'security_snapshot', lambda: call('security'))
        http = post_warm_step(post, 'http_identity', lambda: call('http-info', canonical=True)['identity'])
        uri = post_warm_step(post, 'http_identity_assert', lambda: http['targets'][0]['uri'])
        report['checks']['temporary_real_mime_default_selected'] = post_warm_step(
            post, 'default_selected_assert', lambda: prepared['app_id'].endswith('.desktop'))
        post['phase'] = 'complete'
        def set_button_held(value):
            nonlocal held
            held = value
        for sequence in ('pointer', 'keyboard'):
            begin_native_case(report, sequence)
            native_step(report, 'case_preflight', native_guard)
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
            count = 1 if sequence == 'pointer' else 2
            native_step(report, 'default_handler_delivery_wait', lambda: wait(
                lambda: len(call('state')['bridge']) == count and len(call('state')['events']) == count + 1,
                'selected_default_handler_received_native_uri'))
            state = native_step(report, 'default_handler_state_read', lambda: call('state'))
            bridge, event = state['bridge'][-1], state['events'][-1]
            native_step(report, 'default_handler_authority_assert', lambda: require(
                    bridge == {'selected_uri_matches': True, 'fresh_token_present': True, 'legacy_startup_present': False}
                    and event['uris'] == [uri] and event['fresh_token_present'] and not event['legacy_startup_present'],
                    'native_default_handler_and_browser_wrapper_receive_fresh_authority'))
            fresh = None
            def document_ready():
                nonlocal fresh
                fresh = fresh_documents([browser['control']], browser_state()['pages'], http['targets'], browser['control'])
                return fresh is not None
            native_step(report, 'browser_document_wait', lambda: wait(document_ready, 'one_fresh_selected_real_browser_document'))
            _, clients = native_step(report, 'browser_guard', native_guard)
            focus = native_step(report, 'browser_focus_read', lambda: json.loads(session('hyprctl', '-j', 'activewindow')))
            report['browser_focus'][sequence] = bool(focus) and focus.get('class') == entry['class'] and any(
                row.get('pid') == focus.get('pid') and row.get('address') == focus.get('address') for row in clients)
            native_step(report, 'browser_targets_retire', lambda: require(call('browser-close', json.dumps({
                'identity': browser, 'before': [browser['control']], 'targets': fresh}), canonical=True)
                ['fresh_owned_targets_retired'], 'exact_owned_document_retired'))
            native_step(report, 'notification_expiry_wait', lambda: require(
                call('wait-expiry')['absent_after_expiry_window'], 'notification_absence_after_bounded_expiry'))
            current, _ = native_step(report, 'notification_post_expiry_guard', native_guard)
            native_step(report, 'notification_absence_assert', lambda: require(
                current['client']['notification_count'] == 0, 'foreign_notification_must_not_be_hidden_between_cases'))
            native_step(report, 'notification_history_hide', lambda: cli('notifications', 'hide'))
            native_step(report, 'notification_history_closed_wait', lambda: wait(
                lambda: not inspect()['client']['notification_history_open'], 'owned_history_closed_between_cases'))
            report['checks'][sequence + '_fresh_token_default_route_exact_document'] = True
            report['checks'][sequence + '_notification_absent_after_expiry_window'] = True
            native_step(report, 'case_complete', lambda: None)
        report['success'] = True
    except BaseException as error:
        phase = report.get('native_flow', {}).get('phase', report.get('post_warm', {}).get('phase', 'setup'))
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
                    done = wait(lambda: receipt('default-finished.json'), 'independent_default_config_restore_receipt', 60)
                require(done['success'] is True and all(done['restoration'].values()), 'complete_guardian_restoration')
                report['cleanup'].update(done['restoration'])
                require(call('retire-guardian')['retired'], 'original_configuration_guardian_retired')
                report['cleanup']['configuration_guardian_retired'] = True
            except BaseException:
                report['cleanup']['guardian_restoration'] = False
        if initialized_attempted:
            try:
                retired = call('retire', canonical=True)
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
        path = lab.ROOT / 'reports/local' / ('native-default-uri-' + run_id + '.json')
        old.write_private_report(path, report)
        print(json.dumps({'success': report['success'], 'report': str(path.relative_to(lab.ROOT)),
                          'performance_accepted': False, 'physical_hardware_accepted': False}))
    return 0 if report['success'] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--release-manifest', type=Path)
    parser.add_argument('--manifest-sha')
    parser.add_argument('--expected-desktop-pid', type=int)
    parser.add_argument('--vm-uuid')
    args = parser.parse_args()
    if not args.run:
        print(json.dumps({'source_only_acceptance_plan': True, 'guest_actions': False,
                          'native_or_performance_accepted': False,
                          'test_profile_policy': 'All windows in the new private profile are closed; content there is disposable and not protected. Private evidence may remain. Existing personal browsers and successors are never adopted.',
                          'notification_policy': 'Bounded automatic expiry only; changed or foreign notifications remain untouched.'}))
        return 0
    return run(args, qualify(args))


if __name__ == '__main__':
    raise SystemExit(main())
