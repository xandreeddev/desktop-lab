#!/usr/bin/env python3
"""Reversible real Chromium ↔ Lucent Notes keyboard clipboard acceptance.

Run only in the idle, unlocked, single-output test VM. The browser has a unique
profile, class, file and time-bounded transient unit. CDP prepares/selects that
one textarea and reads its DOM; actual Ctrl+C/Ctrl+V goes through VM input.
Existing Notes/clipboard/history/settings/media stay in private checkpoints.

Ordinary failure restores them through the existing guarded Notes harness. A
host crash leaves an owned watchdog to request lease retirement after 180 seconds;
failed lease/drain proof stays explicit. It does not automatically restore Notes:
the private recovery checkpoint is retained. No
physical-device, rich-HTML clipboard, other browser or focus-race claim is made.
"""

import argparse
import copy
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import secrets
import signal
import time


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('gui_clipboard_notes', ROOT / 'vm/test-multiline-notes.py')
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)
require, private, CheckFailure = notes.require, notes.private, notes.CheckFailure
input_policy = notes.load('gui_clipboard_input_policy', ROOT / 'vm/application_intent_policy.py')
SOURCE_TEXT = 'browser one\n雪 e\u0301 👩\u200d💻\nlast line\n'
DESTINATION_TEXT = 'notes one\n\nnotes two\n'
HOST_PHASES = frozenset(('harness_init', 'native_suite', 'preflight', 'settings_checkpoint',
    'clipboard_checkpoint', 'peer_preparation', 'browser_start', 'browser_origin', 'browser_admission',
    'browser_window', 'notes_visibility', 'browser_copy', 'browser_to_notes', 'notes_copy',
    'notes_to_browser', 'browser_retirement', 'notes_restoration', 'cursor_restoration',
    'profile_cleanup', 'unexpected_restoration'))
HOST_SOURCES = {str(Path(__file__)): 'test-gui-clipboard.py',
    str(Path(notes.__file__)): 'test-multiline-notes.py',
    str(Path(input_policy.__file__)): 'application_intent_policy.py'}


def failure_details(error, phase):
    """Bounded trusted code locations and literal labels, never exception payloads."""
    known = ('CheckFailure', 'ValueError', 'AssertionError', 'OSError', 'FileNotFoundError',
             'PermissionError', 'TimeoutError', 'TimeoutExpired', 'KeyError', 'LookupError',
             'JSONDecodeError', 'UnicodeDecodeError', 'ConnectionRefusedError')
    result = {'phase': phase if type(phase) is str and phase in HOST_PHASES else 'native_suite',
              'error_class': type(error).__name__ if type(error).__name__ in known else 'OtherError',
              'fixed_reason': 'unclassified_host_failure', 'trace_frames': []}
    reason = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else None
    reason = reason if reason is not None and re.fullmatch('[a-z][a-z0-9_]{0,127}', reason) else None
    trace = error.__traceback__; observed = 0; trusted = 0
    while trace is not None and observed < 64:
        code = trace.tb_frame.f_code; source = HOST_SOURCES.get(code.co_filename)
        if source is not None:
            trusted += 1
            # A syntactically safe user value is not a reason. It must also be
            # a literal in this exact source-pinned host call path.
            if isinstance(error, (CheckFailure, ValueError, AssertionError)) and reason in code.co_consts:
                result['fixed_reason'] = reason or 'unclassified_host_failure'
            if len(result['trace_frames']) < 12:
                result['trace_frames'].append({'source': source,
                    'function': code.co_name if re.fullmatch('[A-Za-z_][A-Za-z0-9_]{0,95}', code.co_name) else 'module',
                    'line': trace.tb_lineno if 0 < trace.tb_lineno <= 1000000 else 0})
        trace = trace.tb_next; observed += 1
    result['trace_truncated'] = trace is not None or trusted > 12
    return result


def record_restoration_failure(h, result, error, phase):
    details = failure_details(error, phase)
    if error is getattr(h, 'last_peer_error', None):
        details['peer'] = copy.deepcopy(h.last_peer_failure)
    result.setdefault('restoration_errors', {})[phase] = details


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def require_safe_bindings(bindings):
    """Reject every actually injected chord/button, including catchall binds."""
    require(isinstance(bindings, list) and len(bindings) <= 4096, 'bounded_compositor_bindings')
    plain = {(symbol, notes.LETTERS[symbol]) for symbol in set(DESTINATION_TEXT) if symbol in notes.LETTERS}
    plain.update((('space', 57), ('return', 28), ('enter', 28), ('mouse:272', 272), ('control_l', 29)))
    chords = [(0, symbol, code) for symbol, code in plain]
    chords += [(notes.CONTROL, symbol, notes.LETTERS[symbol]) for symbol in ('a', 'c', 'v')]
    for binding in bindings:
        require(isinstance(binding, dict) and type(binding.get('modmask', 0)) is int,
                'well_formed_compositor_binding')
        mask = binding.get('modmask', 0) & ~(2 | 16)
        key = str(binding.get('key', '')).lower()
        for expected, symbol, code in chords:
            aliases = (symbol, 'catchall', 'any', f'code:{code}', f'code:{code + 8}')
            if code == 272:
                aliases += ('mouse:left',)
            require(mask != expected or (not binding.get('catch_all', False) and key not in aliases
                    and binding.get('keycode', 0) not in (code, code + 8)),
                    'injected_keys_and_pointer_have_no_compositor_bindings')


def require_desktop_layers(layers, pid):
    """A shell-like namespace is not ownership; bind every layer to its PID."""
    require(type(pid) is int and pid > 1, 'actual_desktop_layer_owner')
    def visit(value):
        if isinstance(value, dict):
            if 'namespace' in value:
                name = value['namespace']
                require(isinstance(name, str) and name.startswith('lucent-')
                        and not name.startswith('lucent-authorization-')
                        and value.get('pid') == pid, 'no_foreign_native_layer_before_input')
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)
    visit(layers)


def notes_pointer_target(surface, hit, layer, desktop_pid):
    """One current fully visible Notes hit and exact native layer lifetime."""
    require(surface.get('frames', 0) > 0 and surface.get('native_frames', 0) > 0
            and surface.get('pointer_input') in ('all', 'content') and hit.get('id') == 'notes-input',
            'submitted_notes_pointer_target')
    require(layer.get('namespace') == 'lucent-' + surface['id'] and layer.get('pid') == desktop_pid
            and isinstance(layer.get('address'), str) and bool(layer['address']), 'exact_notes_native_layer_lifetime')
    clip = hit['clip']
    values = [source.get(key) for source, keys in (
        (surface, ('width', 'height')), (layer, ('x', 'y', 'w', 'h')),
        (hit, ('x', 'y', 'width', 'height')), (clip, ('x', 'y', 'width', 'height')))
        for key in keys]
    require(all(type(value) in (int, float) and math.isfinite(value) for value in values),
            'finite_notes_pointer_geometry')
    require(abs(layer['w'] - surface['width']) <= .5 and abs(layer['h'] - surface['height']) <= .5,
            'current_notes_native_layer_geometry')
    left, top = max(hit['x'], clip['x'], 0), max(hit['y'], clip['y'], 0)
    right = min(hit['x'] + hit['width'], clip['x'] + clip['width'], surface['width'])
    bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'], surface['height'])
    require(right - left > 10 and bottom - top > 10
            and right - left >= hit['width'] - .5 and bottom - top >= hit['height'] - .5,
            'fully_visible_notes_pointer_target')
    point = (round(layer['x'] + (left + right) / 2), round(layer['y'] + (top + bottom) / 2))
    require(0 <= point[0] < 1920 and 0 <= point[1] < 1080, 'notes_pointer_target_on_prepared_output')
    return {'id': surface['id'], 'instance': surface.get('instance'), 'output': surface.get('output'),
            'layer': {key: layer[key] for key in ('address', 'namespace', 'pid', 'x', 'y', 'w', 'h')},
            'hit': {key: copy.deepcopy(hit[key]) for key in ('id', 'x', 'y', 'width', 'height', 'clip')},
            'point': point}


def browser_focus_target(clients, captured):
    """A visible center belongs only to the persistent admitted browser window."""
    require(type(clients) is list and len(clients) == 1 and type(clients[0]) is dict,
            'one_owned_browser_focus_target')
    window = clients[0]
    require(type(window.get('pid')) is int and window['pid'] > 1
            and isinstance(window.get('address'), str) and re.fullmatch(r'0x[0-9a-fA-F]+', window['address'])
            and {key: window.get(key) for key in ('address', 'pid', 'class', 'workspace')} == captured,
            'persistent_owned_browser_focus_target')
    position, size = window.get('at'), window.get('size')
    require(type(position) is list and type(size) is list and len(position) == len(size) == 2
            and all(type(value) in (int, float) and math.isfinite(value) for value in position + size),
            'finite_browser_focus_geometry')
    x, y = position; width, height = size
    require(x >= 0 and y >= 0 and width > 0 and height > 0
            and x + width <= 1920 and y + height <= 1080, 'fully_visible_browser_focus_geometry')
    point = (round(x + width / 2), round(y + height / 2))
    require(0 <= point[0] < 1920 and 0 <= point[1] < 1080, 'browser_focus_center_on_prepared_output')
    return {'window': {key: copy.deepcopy(window[key]) for key in
                      ('address', 'pid', 'class', 'workspace', 'at', 'size')}, 'point': point}


class Harness(notes.Harness):
    def __init__(self, args, directory):
        super().__init__(args, directory)
        self.peer_directory = None
        self.peer_initialized = self.browser_started = False
        self.browser_attempted = self.start_acknowledged = False
        self.browser_retired = False
        self.browser_origin = self.browser_identity = None
        self.browser_window = None
        self.peer = None
        self.target = None
        self.input_deadline = None
        self.original_cursor = self.last_cursor = None
        self.cursor_owner = None
        self.desktop_pid = None
        self.restoring_desktop = False
        self.notes_press_target = None
        self.browser_focus_snapshot = None
        self.last_peer_failure = None
        self.last_peer_error = None
        self.phase = 'harness_init'
        self.notes_workspace = 'lucent-clipboard-notes-' + self.tag
        self.browser_workspace = 'lucent-clipboard-browser-' + self.tag

    def call_peer(self, operation, *arguments, data=None):
        require(operation in ('initialize', 'absent', 'origin', 'admit', 'clients', 'dom', 'clipboard-observe', 'retire', 'cleanup'),
                'fixed_gui_peer_operation')
        require(self.peer_initialized or operation == 'initialize', 'owned_peer_initialized')
        if operation in ('dom', 'clipboard-observe', 'retire', 'cleanup'):
            require(data is None, 'one_fixed_peer_authority_payload')
            data = json.dumps({'origin': self.browser_origin, 'identity': self.browser_identity}).encode()
        try:
            value = json.loads(self.session('/usr/bin/python3', self.peer_directory + '/fixture.py',
                                           operation, self.peer_directory, self.tag,
                                           *arguments, data=data, timeout=25))
            self.last_peer_failure = self.last_peer_error = None
            return value
        except Exception as error:
            self.last_peer_failure = {'operation': operation,
                'error_class': type(error).__name__ if type(error).__name__ in (
                    'CheckFailure', 'ValueError', 'OSError', 'TimeoutError', 'JSONDecodeError') else 'OtherError',
                'fixed_reason': 'gui_peer_operation_failed'}
            self.last_peer_error = error
            raise

    def input_guard(self):
        if self.input_deadline is not None:
            require(time.monotonic() < self.input_deadline, 'bounded_native_input_window')
        require(not self.browser_attempted or self.browser_retired or
                (self.start_acknowledged and self.browser_started and self.browser_identity is not None),
                'browser_admission_or_original_retirement_required')
        require(self.session('omarchy-shell', 'lock', 'isLocked').strip() == b'false', 'unlocked_before_each_input')
        desktop_pid = self.verify_binary()
        require(self.desktop_pid is None or desktop_pid == self.desktop_pid, 'same_desktop_process_before_input')
        require_desktop_layers(json.loads(self.session('hyprctl', '-j', 'layers')), desktop_pid)
        if self.cursor_owner is not None:
            self.cursor_owner.observe(json.loads(self.session('hyprctl', '-j', 'cursorpos')))
        require_safe_bindings(json.loads(self.session('hyprctl', '-j', 'binds')))
        clients = json.loads(self.session('hyprctl', '-j', 'clients'))
        if self.browser_started:
            require(self.browser_identity is not None, 'captured_browser_identity_before_input')
            require(self.call_peer('clients', data=json.dumps({'clients': clients, 'identity': self.browser_identity}).encode())['owned_clients'] == len(clients),
                    'only_exact_owned_browser_clients')
            require(len(clients) == 1 and self.browser_window == {
                key: clients[0].get(key) for key in ('address', 'pid', 'class', 'workspace')},
                    'same_admitted_browser_native_window')
        else:
            require(not clients, 'no_foreign_clients')
        monitors = json.loads(self.session('hyprctl', '-j', 'monitors'))
        require(len(monitors) == 1 and all(monitors[0].get(key) == value
                    for key, value in self.original_monitor.items()), 'unchanged_native_output')
        workspace = json.loads(self.session('hyprctl', '-j', 'activeworkspace'))
        require(workspace['id'] == self.workspace or workspace.get('name') in (
                    self.notes_workspace, self.browser_workspace), 'only_owned_or_original_workspace')
        client = self.inspect()['client']
        require(not client['launcher'] and not client['query'] and not client['widgets_editing']
                and not client.get('notification_history_open') and not client.get('notification_count')
                and not client.get('dock_surface_expanded') and not client.get('widget_grid')
                and not client.get('wallpaper_busy'), 'idle_shell_before_each_input')
        require(all(not client.get(panel, {}).get('open') and not client.get(panel, {}).get('visible')
                    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray',
                                  'settings', 'live_wallpaper', 'application_actions', 'weather', 'displays')),
                'no_foreground_shell_panel')
        require(not client.get('network', {}).get('form')
                and not any(client.get('bluetooth', {}).get(key) for key in ('form', 'requests', 'answering')),
                'no_foreground_credential_request')
        if self.target == 'notes':
            require(workspace.get('name') == self.notes_workspace
                    and self.notes_surface()[0]['focus'] == 'notes-input', 'exact_notes_input_focus')
            active = json.loads(self.session('hyprctl', '-j', 'activewindow'))
            require(not active.get('address') and not active.get('pid'), 'no_managed_window_has_notes_keyboard_focus')
        elif self.target == 'browser':
            active = json.loads(self.session('hyprctl', '-j', 'activewindow'))
            require(len(clients) == 1 and active.get('address') == clients[0].get('address')
                    and active.get('pid') == clients[0].get('pid')
                    and workspace.get('name') == self.browser_workspace, 'exact_browser_native_focus')
            require(self.call_peer('dom', 'read')['focused'], 'owned_browser_document_has_keyboard_focus')
        # qmp() invokes this guard immediately before sending a physical down.
        # Keep target validation last, after all slower session/DOM checks.
        if getattr(self, 'notes_press_target', None) is not None:
            require(self.current_notes_target() == self.notes_press_target,
                    'same_notes_hit_and_native_lifetime_before_press')
        if getattr(self, 'browser_focus_snapshot', None) is not None:
            require(self.current_browser_target() == self.browser_focus_snapshot,
                    'same_browser_window_and_geometry_during_focus')

    def move(self, x, y):
        self.input_guard()
        destination = (round(x), round(y))
        self.cursor_owner.begin(json.loads(self.session('hyprctl', '-j', 'cursorpos')), destination)
        self.session('hyprctl', 'dispatch', input_policy.compositor_move(destination))
        self.eventually(lambda: self.cursor_owner.observe(json.loads(self.session('hyprctl', '-j', 'cursorpos'))) == destination,
                        'owned_cursor_destination_confirmed')
        self.cursor_owner.finish(json.loads(self.session('hyprctl', '-j', 'cursorpos')))
        self.last_cursor = dict(zip(('x', 'y'), self.cursor_owner.expected))

    def key(self, *codes):
        require(self.target in ('notes', 'browser'), 'concrete_keyboard_target_required')
        super().key(*codes)

    def current_notes_target(self):
        surface, hit = self.notes_surface()
        return notes_pointer_target(surface, hit, self.origin(surface), self.desktop_pid)

    def focus(self):
        self.input_guard()
        previous = None
        for _ in range(40):
            try:
                target = self.current_notes_target()
            except (CheckFailure, LookupError):
                time.sleep(.15)
                continue
            if previous == target:
                self.move(*target['point'])
                time.sleep(.12)
                self.input_guard()
                require(self.current_notes_target() == target, 'same_notes_hit_and_native_lifetime_before_press')
                # Balance even when a down event reached QMP but its reply was
                # lost. A stale target never authorizes the down event at all.
                self.notes_press_target = target
                try:
                    self.qmp([{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
                    time.sleep(.05)
                finally:
                    try:
                        self.qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
                    finally:
                        self.notes_press_target = None
                self.eventually(lambda: self.notes_surface()[0]['focus'] == 'notes-input', 'native_notes_focus')
                return
            previous = target
            time.sleep(.15)
        raise CheckFailure('stable_notes_hit_geometry')

    def stop_quiescent(self):
        # A failure during secure/foreign interaction never stops that desktop.
        self.input_guard()
        self.restoring_desktop = True
        return super().stop_quiescent()

    def stable(self, seconds=35):
        super().stable(seconds)
        if self.restoring_desktop:
            self.desktop_pid = self.verify_binary()
            self.restoring_desktop = False

    def workspace_focus(self, name):
        require(name in (self.notes_workspace, self.browser_workspace), 'owned_workspace_destination')
        self.target = None
        self.input_guard()
        self.session('hyprctl', 'dispatch', 'hl.dsp.focus({ workspace = ' + json.dumps('name:' + name) + ' })')
        self.eventually(lambda: json.loads(self.session('hyprctl', '-j', 'activeworkspace')).get('name') == name,
                        'owned_workspace_focused')

    def focus_notes(self):
        self.workspace_focus(self.notes_workspace)
        self.focus()
        self.target = 'notes'
        self.input_guard()

    def focus_browser(self, operation):
        require(operation in ('seed', 'clear'), 'owned_dom_preparation')
        self.target = None
        self.input_guard()
        target = self.current_browser_target()
        self.browser_focus_snapshot = target
        try:
            # Normal compositor focus can center the cursor in this window.
            # Declare and observe that point before requesting focus, without
            # adopting any independent movement or changing cursor policy.
            self.move(*target['point'])
            self.workspace_focus(self.browser_workspace)
            self.input_guard()
            self.session('hyprctl', 'dispatch', 'hl.dsp.focus({ window = '
                         + json.dumps('address:' + target['window']['address']) + ' })')
            self.input_guard()
            self.call_peer('dom', operation)
            self.eventually(lambda: self.call_peer('dom', 'read')['focused'],
                            'browser_document_keyboard_focus_settled')
            self.target = 'browser'
            self.input_guard()
            result = self.call_peer('dom', 'read')
            require(result['focused'] and result['selection_complete'], 'owned_textarea_focused_and_selected')
            return result
        finally:
            self.browser_focus_snapshot = None

    def wait_browser_copy_offer(self, result):
        try:
            self.eventually(lambda: self.clip('read')['sha256'] == sha(SOURCE_TEXT),
                            'real_browser_copy_offer')
        except CheckFailure:
            # Observational only. Never broaden accepted MIME/bytes, replace
            # the original failure, send another key or change restoration.
            observation = {}
            try:
                self.input_guard()
                document = self.call_peer('dom', 'read')
                observation['document'] = {
                    'focused': document['focused'],
                    'fixture_source_selected': document['sha256'] == sha(SOURCE_TEXT)
                        and document['selection_complete'],
                    'events': document['events']}
                observation['clipboard'] = self.call_peer('clipboard-observe')
            except Exception as error:
                observation['diagnostic_failure'] = failure_details(error, self.phase)
            result['browser_copy_diagnostics'] = observation
            raise

    def current_browser_target(self):
        clients = json.loads(self.session('hyprctl', '-j', 'clients'))
        target = browser_focus_target(clients, self.browser_window)
        self.call_peer('clients', data=json.dumps({'clients': clients, 'identity': self.browser_identity}).encode())
        return target

    def restore_workspace(self):
        self.input_guard()
        self.session('hyprctl', 'dispatch', 'hl.dsp.focus({ workspace = ' + json.dumps(str(self.workspace)) + ' })')
        require(json.loads(self.session('hyprctl', '-j', 'activeworkspace'))['id'] == self.workspace,
                'original_workspace_restored')


def suite(h, result):
    h.phase = 'preflight'
    notes.preflight(h)
    require_safe_bindings(json.loads(h.session('hyprctl', '-j', 'binds')))
    require(h.monitor['width'] == 1920 and h.monitor['height'] == 1080 and h.monitor['scale'] == 1,
            'single_1920x1080_scale_one_output')
    h.original_cursor = json.loads(h.session('hyprctl', '-j', 'cursorpos'))
    h.last_cursor = dict(h.original_cursor)
    h.cursor_owner = input_policy.CursorOwnership(h.original_cursor)
    h.desktop_pid = h.verify_binary()
    h.input_guard()
    h.phase = 'settings_checkpoint'
    baseline = h.checkpoint('create')
    h.checkpointed = True
    private(h.directory / 'original-checkpoint.json', baseline)
    private(h.directory / 'recovery.json', {
        'guest_checkpoint_relative': '.local/state/lucent/.native-notes-' + h.tag,
        'source_commit': h.args.source_commit, 'desktop_sha256': h.args.desktop_sha256,
        'browser_unit': 'lucent-gui-clipboard-' + h.tag + '.service',
        'browser_watchdog_unit': 'lucent-gui-clipboard-watchdog-' + h.tag + '.service',
        'host_crash_requires_guarded_notes_checkpoint_restoration': True})
    h.notes.add(baseline['settings']['notes'])
    h.add_layout(baseline['settings'])
    h.checkpoint('awake')
    h.phase = 'clipboard_checkpoint'
    h.clipboard = h.clip('create')
    require(h.clipboard['supported'], 'original_clipboard_supports_exact_text_restoration')
    h.clipboard_hashes.update((h.clip('read')['sha256'], sha(''), sha(SOURCE_TEXT), sha(DESTINATION_TEXT)))
    result['checks']['original_clipboard_checkpoint_and_history_pause'] = True

    # Every path below is newly owned; no existing Chromium profile is opened.
    h.phase = 'peer_preparation'
    h.peer_directory = h.remote('mktemp', '-d', '/tmp/lucent-gui-clipboard.XXXXXXXXXXXX').decode().strip()
    require(re.fullmatch(r'/tmp/lucent-gui-clipboard\.[A-Za-z0-9]{12}', h.peer_directory), 'private_peer_directory')
    for source, name in ((ROOT / 'vm/fixtures/gui-clipboard-peer.py', 'fixture.py'),
                         (ROOT / 'vm/owned_application_units.py', 'owned_application_units.py'),
                         (ROOT / 'vm/owned_chromium_lease.py', 'owned_chromium_lease.py'),
                         (ROOT / 'vm/cdp_peer.py', 'cdp_peer.py')):
        h.remote('python3', '-c',
                 'import os,sys; fd=os.open(sys.argv[1],os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600); '
                 'data=sys.stdin.buffer.read(262145); assert len(data)<=262144; '
                 'f=os.fdopen(fd,"wb"); f.write(data); f.close()',
                 h.peer_directory + '/' + name, data=source.read_bytes())
    h.peer = h.call_peer('initialize')
    h.peer_initialized = True
    require(h.peer['source_sha256'] == sha(SOURCE_TEXT), 'matching_owned_source_fixture')
    private(h.directory / 'peer-recovery.json', {'directory': h.peer_directory, 'run_id': h.tag})

    h.input_deadline = time.monotonic() + 170
    h.workspace_focus(h.browser_workspace)
    require(h.call_peer('absent')['owned_unit_name_absent'], 'typed_browser_unit_name_absent_before_start')
    # Chromium may migrate to its own registered scope. A distinct owned peer
    # watchdog retains the original lease; original-service expiry alone is not
    # evidence that the migrated scope and its descendants have drained.
    private(h.directory / 'browser-attempt.json', {
        'unit': 'lucent-gui-clipboard-' + h.tag + '.service',
        'directory': h.peer_directory, 'run_id': h.tag, 'typed_name_absent': True})
    h.phase = 'browser_start'
    h.browser_attempted = True
    h.session('systemd-run', '--user', '--quiet', '--collect', '--property=Type=exec',
              '--property=RuntimeMaxSec=210s', '--property=TimeoutStopSec=10s',
              '--unit=lucent-gui-clipboard-' + h.tag + '.service',
              '/usr/bin/python3', h.peer_directory + '/fixture.py', 'serve', h.peer_directory, h.tag)
    h.start_acknowledged = True
    private(h.directory / 'browser-start-reply.json', {'unit': 'lucent-gui-clipboard-' + h.tag + '.service',
                                                     'acknowledged': True})
    def capture_origin():
        h.browser_origin = h.call_peer('origin')
        return True
    h.phase = 'browser_origin'
    h.eventually(capture_origin, 'original_browser_wrapper_receipt_available')
    private(h.directory / 'browser-origin.json', h.browser_origin)
    def capture_browser():
        h.browser_identity = h.call_peer('admit')
        return True
    h.phase = 'browser_admission'
    h.eventually(capture_browser, 'original_browser_full_identity_available', timeout=35)
    private(h.directory / 'browser-admission.json', h.browser_identity)
    h.browser_started = True
    h.phase = 'browser_window'
    h.eventually(lambda: len(json.loads(h.session('hyprctl', '-j', 'clients'))) == 1,
                 'owned_real_chromium_window', timeout=35)
    current_window = json.loads(h.session('hyprctl', '-j', 'clients'))
    require(len(current_window) == 1 and re.fullmatch(r'0x[0-9a-fA-F]+', current_window[0].get('address', '')),
            'one_concrete_owned_browser_window')
    h.browser_window = {key: current_window[0].get(key) for key in ('address', 'pid', 'class', 'workspace')}
    private(h.directory / 'browser-window.json', h.browser_window)
    h.input_guard()
    result['checks']['real_owned_wayland_browser_and_unit'] = True

    h.phase = 'notes_visibility'
    settings = copy.deepcopy(baseline['settings'])
    layout = notes.expected_layout(h, settings)
    if 'notes' not in layout['visible_widgets']:
        layout['visible_widgets'].append('notes')
    h.add_layout(settings)
    h.mutated = True
    h.cli('widget', 'notes', 'show')
    h.saved()

    h.phase = 'browser_copy'
    source = h.focus_browser('seed')
    require(source['sha256'] == sha(SOURCE_TEXT), 'owned_browser_source_selected')
    h.key(29, notes.LETTERS['c'])
    h.wait_browser_copy_offer(result)
    h.phase = 'browser_to_notes'
    h.focus_notes()
    h.notes.add(SOURCE_TEXT)
    h.key(29, 30)
    h.key(29, notes.LETTERS['v'])
    h.saved(SOURCE_TEXT)
    result['checks'].update(browser_copy_to_native_notes=True,
                            multiline_combining_emoji_cjk_preserved=True,
                            browser_offer_replaces_notes_selection=True)

    h.phase = 'notes_copy'
    h.replace(DESTINATION_TEXT)
    h.key(29, 30)
    h.key(29, notes.LETTERS['c'])
    h.eventually(lambda: h.clip('read')['sha256'] == sha(DESTINATION_TEXT), 'real_native_notes_copy_offer')
    h.phase = 'notes_to_browser'
    h.focus_browser('clear')
    h.key(29, notes.LETTERS['v'])
    h.eventually(lambda: h.call_peer('dom', 'read')['sha256'] == sha(DESTINATION_TEXT),
                 'real_browser_paste_dom_matches_native_notes')
    result['checks'].update(native_notes_copy_to_browser=True,
                            native_offer_replaces_browser_selection=True,
                            blank_lines_and_trailing_lf_preserved=True,
                            transfers_use_actual_keyboard_not_cdp_clipboard=True)


def restore(h, result):
    h.target = None
    h.input_deadline = None
    if h.browser_attempted:
        h.phase = 'browser_retirement'
        try:
            require(h.start_acknowledged and h.browser_origin is not None,
                    'acknowledged_original_browser_receipt_required_for_retirement')
            h.call_peer('retire')
            h.browser_started = False
            h.browser_retired = True
            h.eventually(lambda: not json.loads(h.session('hyprctl', '-j', 'clients')),
                         'owned_browser_windows_retired', timeout=15)
            result['restoration']['owned_browser_retired'] = True
        except Exception as error:
            record_restoration_failure(h, result, error, h.phase)
            result['restoration']['owned_browser_retired'] = False
    # Do not stop/restart Lucent under foreign/secure UI or uncertain browser
    # ownership. Retain private checkpoints for guarded manual restoration.
    h.phase = 'notes_restoration'
    try:
        require(not h.browser_attempted or result['restoration'].get('owned_browser_retired'),
                'owned_browser_retired_before_notes_restoration')
        h.input_guard()
        notes.restore(h, result)
    except Exception as error:
        record_restoration_failure(h, result, error, h.phase)
        result['restoration']['guarded_notes_restoration'] = False
    if h.original_cursor is not None:
        h.phase = 'cursor_restoration'
        try:
            current = json.loads(h.session('hyprctl', '-j', 'cursorpos'))
            require(current == h.last_cursor, 'cursor_still_at_last_owned_position')
            h.input_guard()
            h.move(h.original_cursor['x'], h.original_cursor['y'])
            require(json.loads(h.session('hyprctl', '-j', 'cursorpos')) == h.original_cursor,
                    'original_cursor_restored')
            result['restoration']['original_cursor'] = True
        except Exception as error:
            record_restoration_failure(h, result, error, h.phase)
            result['restoration']['original_cursor'] = False
    if h.peer_initialized and result.get('completed_checks') and all(result['restoration'].values()):
        h.phase = 'profile_cleanup'
        try:
            h.call_peer('cleanup')
            result['restoration']['owned_private_profile_removed'] = True
        except Exception as error:
            record_restoration_failure(h, result, error, h.phase)
            result['restoration']['owned_private_profile_removed'] = False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--desktop-sha256', required=True)
    args = parser.parse_args()
    require(re.fullmatch(r'[0-9a-f]{40}', args.source_commit) and
            re.fullmatch(r'[0-9a-f]{64}', args.desktop_sha256), 'exact_release_identity_required')
    directory = ROOT / 'reports/local' / ('gui-clipboard-native-' + secrets.token_hex(12))
    directory.mkdir(parents=True, mode=0o700)
    result = {'passed': False, 'source_commit': args.source_commit,
              'binary_sha256': args.desktop_sha256, 'checks': {}, 'restoration': {},
              'harness_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'support_sha256': {name: hashlib.sha256((ROOT / 'vm' / name).read_bytes()).hexdigest()
                                 for name in ('fixtures/gui-clipboard-peer.py', 'owned_application_units.py',
                                              'owned_chromium_lease.py', 'cdp_peer.py',
                                              'application_intent_policy.py', 'test-multiline-notes.py')},
              'scope': 'Real owned Chromium textarea and native Notes, single-output keyboard text transfers',
              'not_covered': ['rich HTML/images', 'other applications', 'delayed-provider and focus-retirement races',
                              'physical hardware', 'IME/shaping pixel correctness', 'automatic Notes restore after host crash'],
              'privacy': {'original_notes_or_clipboard_published': False}}
    h = None
    def interrupted(*_):
        raise CheckFailure('native_clipboard_acceptance_interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        h = Harness(args, directory)
        h.phase = 'native_suite'
        suite(h, result)
        result['completed_checks'] = True
    except Exception as error:
        details = failure_details(error, getattr(h, 'phase', 'harness_init'))
        result['primary_failure'] = details
        result['failed_check'] = details['fixed_reason'] if details['fixed_reason'] != 'unclassified_host_failure' else details['error_class']
        if h is not None and error is getattr(h, 'last_peer_error', None):
            result['primary_peer_failure'] = copy.deepcopy(h.last_peer_failure)
    finally:
        if h is not None:
            # Suite retries are historical once restoration begins; only a
            # failure from a restoration operation belongs to this phase.
            h.last_peer_failure = h.last_peer_error = None
            try:
                restore(h, result)
            except Exception as error:
                record_restoration_failure(h, result, error, 'unexpected_restoration')
                result['restoration']['unexpected_restoration_failure'] = False
            if getattr(h, 'last_peer_failure', None) is not None \
                    and h.last_peer_failure != result.get('primary_peer_failure'):
                result['restoration_peer_failure'] = copy.deepcopy(h.last_peer_failure)
        result['passed'] = bool(result.get('completed_checks') and result['restoration']
                                and all(result['restoration'].values()))
        private(directory / 'report.json', result)
    print(json.dumps(result, indent=2))
    print('Private evidence: ' + str(directory.relative_to(ROOT)))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
