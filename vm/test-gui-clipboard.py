#!/usr/bin/env python3
"""Reversible real Chromium ↔ Lucent Notes keyboard clipboard acceptance.

Run only in the idle, unlocked, single-output test VM. The browser has a unique
profile, class, file and time-bounded transient unit. CDP prepares/selects that
one textarea and reads its DOM; actual Ctrl+C/Ctrl+V goes through VM input.
Existing Notes/clipboard/history/settings/media stay in private checkpoints.

Ordinary failure restores them through the existing guarded Notes harness. A
host crash independently retires the browser after 210 seconds, but does not
automatically restore Notes: the private recovery checkpoint is retained. No
physical-device, rich-HTML clipboard, other browser or focus-race claim is made.
"""

import argparse
import copy
import hashlib
import importlib.util
import json
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
SOURCE_TEXT = 'browser one\n雪 e\u0301 👩\u200d💻\nlast line\n'
DESTINATION_TEXT = 'notes one\n\nnotes two\n'


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def require_safe_bindings(bindings):
    """Reject every actually injected chord/button, including catchall binds."""
    require(isinstance(bindings, list) and len(bindings) <= 4096, 'bounded_compositor_bindings')
    plain = {(symbol, notes.LETTERS[symbol]) for symbol in set(DESTINATION_TEXT) if symbol in notes.LETTERS}
    plain.update((('space', 57), ('return', 28), ('enter', 28), ('mouse:272', 272)))
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


class Harness(notes.Harness):
    def __init__(self, args, directory):
        super().__init__(args, directory)
        self.peer_directory = None
        self.peer_initialized = self.browser_started = False
        self.peer = None
        self.target = None
        self.input_deadline = None
        self.original_cursor = self.last_cursor = None
        self.notes_workspace = 'lucent-clipboard-notes-' + self.tag
        self.browser_workspace = 'lucent-clipboard-browser-' + self.tag

    def call_peer(self, operation, *arguments, data=None):
        require(self.peer_initialized or operation == 'initialize', 'owned_peer_initialized')
        return json.loads(self.session('/usr/bin/python3', self.peer_directory + '/fixture.py',
                                       operation, self.peer_directory, self.tag,
                                       *arguments, data=data, timeout=25))

    def input_guard(self):
        if self.input_deadline is not None:
            require(time.monotonic() < self.input_deadline, 'bounded_native_input_window')
        require(self.session('omarchy-shell', 'lock', 'isLocked').strip() == b'false', 'unlocked_before_each_input')
        require_desktop_layers(json.loads(self.session('hyprctl', '-j', 'layers')), self.verify_binary())
        require_safe_bindings(json.loads(self.session('hyprctl', '-j', 'binds')))
        clients = json.loads(self.session('hyprctl', '-j', 'clients'))
        if self.browser_started:
            require(self.call_peer('clients', data=json.dumps(clients).encode())['owned_clients'] == len(clients),
                    'only_exact_owned_browser_clients')
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
                and not client.get('dock_surface_expanded'), 'idle_shell_before_each_input')
        require(all(not client.get(panel, {}).get('open') and not client.get(panel, {}).get('visible')
                    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray',
                                  'settings', 'live_wallpaper', 'application_actions', 'weather', 'displays')),
                'no_foreground_shell_panel')
        if self.target == 'notes':
            require(workspace.get('name') == self.notes_workspace
                    and self.notes_surface()[0]['focus'] == 'notes-input', 'exact_notes_input_focus')
        elif self.target == 'browser':
            active = json.loads(self.session('hyprctl', '-j', 'activewindow'))
            require(len(clients) == 1 and active.get('address') == clients[0].get('address')
                    and active.get('pid') == clients[0].get('pid')
                    and workspace.get('name') == self.browser_workspace, 'exact_browser_native_focus')
            require(self.call_peer('dom', 'read')['focused'], 'owned_browser_document_has_keyboard_focus')

    def move(self, x, y):
        super().move(x, y)
        self.last_cursor = json.loads(self.session('hyprctl', '-j', 'cursorpos'))

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
        self.workspace_focus(self.browser_workspace)
        clients = json.loads(self.session('hyprctl', '-j', 'clients'))
        require(len(clients) == 1 and re.fullmatch(r'0x[0-9a-fA-F]+', clients[0]['address']),
                'one_owned_browser_window')
        self.call_peer('clients', data=json.dumps(clients).encode())
        self.session('hyprctl', 'dispatch', 'hl.dsp.focus({ window = '
                     + json.dumps('address:' + clients[0]['address']) + ' })')
        result = self.call_peer('dom', operation)
        self.eventually(lambda: self.call_peer('dom', 'read')['focused'],
                        'browser_document_keyboard_focus_settled')
        self.target = 'browser'
        self.input_guard()
        result = self.call_peer('dom', 'read')
        require(result['focused'] and result['selection_complete'], 'owned_textarea_focused_and_selected')
        return result

    def restore_workspace(self):
        self.input_guard()
        self.session('hyprctl', 'dispatch', 'hl.dsp.focus({ workspace = ' + json.dumps(str(self.workspace)) + ' })')
        require(json.loads(self.session('hyprctl', '-j', 'activeworkspace'))['id'] == self.workspace,
                'original_workspace_restored')


def suite(h, result):
    notes.preflight(h)
    require_safe_bindings(json.loads(h.session('hyprctl', '-j', 'binds')))
    require(h.monitor['width'] == 1920 and h.monitor['height'] == 1080 and h.monitor['scale'] == 1,
            'single_1920x1080_scale_one_output')
    h.original_cursor = json.loads(h.session('hyprctl', '-j', 'cursorpos'))
    h.last_cursor = dict(h.original_cursor)
    baseline = h.checkpoint('create')
    h.checkpointed = True
    private(h.directory / 'original-checkpoint.json', baseline)
    private(h.directory / 'recovery.json', {
        'guest_checkpoint_relative': '.local/state/lucent/.native-notes-' + h.tag,
        'source_commit': h.args.source_commit, 'desktop_sha256': h.args.desktop_sha256,
        'browser_unit': 'lucent-gui-clipboard-' + h.tag + '.service',
        'host_crash_requires_guarded_notes_checkpoint_restoration': True})
    h.notes.add(baseline['settings']['notes'])
    h.add_layout(baseline['settings'])
    h.checkpoint('awake')
    h.clipboard = h.clip('create')
    require(h.clipboard['supported'], 'original_clipboard_supports_exact_text_restoration')
    h.clipboard_hashes.update((h.clip('read')['sha256'], sha(''), sha(SOURCE_TEXT), sha(DESTINATION_TEXT)))
    result['checks']['original_clipboard_checkpoint_and_history_pause'] = True

    # Every path below is newly owned; no existing Chromium profile is opened.
    h.peer_directory = h.remote('mktemp', '-d', '/tmp/lucent-gui-clipboard.XXXXXXXXXXXX').decode().strip()
    require(re.fullmatch(r'/tmp/lucent-gui-clipboard\.[A-Za-z0-9]{12}', h.peer_directory), 'private_peer_directory')
    for source, name in ((ROOT / 'vm/fixtures/gui-clipboard-peer.py', 'fixture.py'),
                         (ROOT / 'vm/owned_application_units.py', 'owned_application_units.py'),
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
    # Default KillMode=control-group and RuntimeMaxSec retire all this profile's
    # children independently. The exact structured ExecStart is checked later.
    h.session('systemd-run', '--user', '--quiet', '--collect', '--property=Type=exec',
              '--property=RuntimeMaxSec=210s', '--property=TimeoutStopSec=10s',
              '--unit=lucent-gui-clipboard-' + h.tag + '.service',
              '/usr/bin/python3', h.peer_directory + '/fixture.py', 'serve', h.peer_directory, h.tag)
    h.browser_started = True
    h.eventually(lambda: len(json.loads(h.session('hyprctl', '-j', 'clients'))) == 1,
                 'owned_real_chromium_window', timeout=35)
    h.input_guard()
    result['checks']['real_owned_wayland_browser_and_unit'] = True

    settings = copy.deepcopy(baseline['settings'])
    layout = notes.expected_layout(h, settings)
    if 'notes' not in layout['visible_widgets']:
        layout['visible_widgets'].append('notes')
    h.add_layout(settings)
    h.mutated = True
    h.cli('widget', 'notes', 'show')
    h.saved()

    source = h.focus_browser('seed')
    require(source['sha256'] == sha(SOURCE_TEXT), 'owned_browser_source_selected')
    h.key(29, notes.LETTERS['c'])
    h.eventually(lambda: h.clip('read')['sha256'] == sha(SOURCE_TEXT), 'real_browser_copy_offer')
    h.focus_notes()
    h.notes.add(SOURCE_TEXT)
    h.key(29, 30)
    h.key(29, notes.LETTERS['v'])
    h.saved(SOURCE_TEXT)
    result['checks'].update(browser_copy_to_native_notes=True,
                            multiline_combining_emoji_cjk_preserved=True,
                            browser_offer_replaces_notes_selection=True)

    h.replace(DESTINATION_TEXT)
    h.key(29, 30)
    h.key(29, notes.LETTERS['c'])
    h.eventually(lambda: h.clip('read')['sha256'] == sha(DESTINATION_TEXT), 'real_native_notes_copy_offer')
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
    if h.peer_initialized:
        try:
            h.call_peer('retire')
            h.browser_started = False
            h.eventually(lambda: not json.loads(h.session('hyprctl', '-j', 'clients')),
                         'owned_browser_windows_retired', timeout=15)
            result['restoration']['owned_browser_retired'] = True
        except Exception:
            result['restoration']['owned_browser_retired'] = False
    # Restoration is independent from success of browser cleanup; the original
    # Notes helper still refuses foreign settings, services or clipboard data.
    notes.restore(h, result)
    if h.original_cursor is not None:
        try:
            current = json.loads(h.session('hyprctl', '-j', 'cursorpos'))
            require(current == h.last_cursor, 'cursor_still_at_last_owned_position')
            h.input_guard()
            h.move(h.original_cursor['x'], h.original_cursor['y'])
            require(json.loads(h.session('hyprctl', '-j', 'cursorpos')) == h.original_cursor,
                    'original_cursor_restored')
            result['restoration']['original_cursor'] = True
        except Exception:
            result['restoration']['original_cursor'] = False
    if h.peer_initialized and result.get('completed_checks') and all(result['restoration'].values()):
        try:
            h.call_peer('cleanup')
            result['restoration']['owned_private_profile_removed'] = True
        except Exception:
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
        suite(h, result)
        result['completed_checks'] = True
    except Exception as error:
        result['failed_check'] = str(error) if isinstance(error, CheckFailure) else type(error).__name__
    finally:
        if h is not None:
            try:
                restore(h, result)
            except Exception:
                result['restoration']['unexpected_restoration_failure'] = False
        result['passed'] = bool(result.get('completed_checks') and result['restoration']
                                and all(result['restoration'].values()))
        private(directory / 'report.json', result)
    print(json.dumps(result, indent=2))
    print('Private evidence: ' + str(directory.relative_to(ROOT)))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
