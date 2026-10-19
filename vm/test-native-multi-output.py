#!/usr/bin/env python3
"""Guarded real-Hyprland simultaneous-output, scale and host-retirement checks.

Requires an explicitly idle/unlocked guest, no ordinary windows, an existing
stay-awake marker, and an identified release. Creates only one UUID-named native
headless output. A separate guest user unit owns its fixed 240-second lease and
cleanup. Never changes the original mode, configuration, settings or wallpaper.

Actual QMP left-button presses follow independently verified global cursor
warps. Unsupported headless input is BLOCKED, never replaced by CLI actions.
Submission counts do not prove presentation FPS, physical cables or GPU output.
No work is performed when imported. See docs/native-multi-output-acceptance.md.
"""
import argparse
import base64
import hashlib
import json
import math
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import time
import uuid

from multi_output_policy import (Blocked, button_event, cursor_matches, hit_center, host, host_surface_ids,
                                 layer_for, monitor_signature, output, require, retired,
                                 surface, unique)


ROOT = Path(__file__).resolve().parents[1]


def private_json(path, value):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def eventually(predicate, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.25)
    raise Blocked(label)


class VM:
    def __init__(self, lab_root):
        # Match the established session-only lab inventory/transport. Explicit
        # lab-root supports review worktrees without copying a private SSH key.
        import lab
        self.lab = lab
        lab.ROOT, lab.STATE = lab_root, lab_root / '.local-vms'
        inventory = lab.inventory()
        require(inventory.get('uri') == 'qemu:///session', 'session_vm_inventory_required')
        require(inventory.get('user') == 'omarchy', 'prepared_test_account_required')
        self.name = inventory['vms']['lucent']['name']
        require(self.name == 'desktop-lab-lucent', 'prepared_lucent_vm_required')
        require(self.virsh('domstate', self.name).strip() == 'running', 'already_running_guest_required')

    def virsh(self, *args):
        value = subprocess.run(['virsh', '-c', 'qemu:///session', *args], check=False,
                               capture_output=True, text=True, timeout=8)
        require(value.returncode == 0 and len(value.stdout) <= 8 * 1024 * 1024,
                'bounded_libvirt_command_failed')
        return value.stdout

    def remote(self, *args, data=None, timeout=45):
        value = subprocess.run(self.lab.ssh_args('lucent') + [shlex.join(args)], input=data,
                               capture_output=True, text=True, timeout=timeout, check=False)
        require(value.returncode == 0 and len(value.stdout) <= 8 * 1024 * 1024,
                'bounded_guest_transport_failed')
        return value.stdout.strip()

    def session(self, *args, **kwargs):
        return self.remote('python3', 'desktop-lab/scripts/in-session.py', *args, **kwargs)

    def hypr(self, *args):
        return self.session('hyprctl', *args)

    def inspect(self):
        return json.loads(self.session('/home/omarchy/.local/bin/lucent-cli', 'inspect'))

    def monitors(self):
        return json.loads(self.hypr('-j', 'monitors'))

    def qmp(self, events):
        result = json.loads(self.virsh('qemu-monitor-command', self.name, json.dumps({
            'execute': 'input-send-event', 'arguments': {'events': events}})))
        require('error' not in result and 'return' in result, 'native_qmp_input_unavailable')

    def button(self, down):
        self.qmp([button_event(down)])

    def escape(self):
        self.virsh('send-key', self.name, '--codeset', 'linux', '--holdtime', '50', '1')


class Run:
    def __init__(self, vm, directory, guest, pid):
        self.vm, self.directory, self.guest, self.pid = vm, directory, guest, pid
        self.checks = []
        self.held = False
        self.sequence = 0

    def record(self, label, value=None):
        self.checks.append(label)
        if value is not None:
            self.sequence += 1
            private_json(self.directory / f'{self.sequence:02d}-{label}.json', value)
        print(json.dumps({'check': label, 'passed': True}), flush=True)

    def lease(self, operation, scale=None, **hit):
        args = ['python3', self.guest + '/multi_output_guard.py', 'request', self.guest, operation]
        if scale is not None:
            args += ['--scale', str(scale)]
        for key in ('monitor', 'identifier', 'generation', 'global_id'):
            if key in hit:
                args += ['--' + key.replace('_', '-'), str(hit[key])]
        value = json.loads(self.vm.session(*args))
        require(value.get('ok') is True, 'lease_' + str(value.get('blocked', 'request_failed')))
        return value['value']

    def observed_host(self, name, expected=None):
        state = self.vm.inspect()
        monitor = unique(self.vm.monitors(), lambda m: m['name'] == name, 'native_output_missing')
        if expected is not None:
            require(monitor_signature(monitor) == expected, 'native_case_changed')
        current = host(state, name)
        background, bar = surface(state, name, 'background'), surface(state, name, 'bar')
        layers = json.loads(self.vm.hypr('-j', 'layers'))
        native = {s['id']: layer_for(layers, name, s, self.pid) for s in (background, bar)}
        width, height = monitor['width'] / monitor['scale'], monitor['height'] / monitor['scale']
        require(all(math.isclose(a, b, abs_tol=.01) for a, b in zip(current['viewport'], (width, height))),
                'host_viewport_not_current')
        require(math.isclose(current['density'], monitor['scale'], abs_tol=.001)
                and math.isclose(background['density'], monitor['scale'], abs_tol=.001),
                'host_fractional_density_not_current')
        require([background['buffer_width'], background['buffer_height']] ==
                [monitor['width'], monitor['height']], 'background_physical_buffer_mismatch')
        require(math.isclose(bar['width'], width, abs_tol=.01) and 0 < bar['height'] < height
                and all(s['native_frames'] > 0 and s['frames'] > 0 for s in (background, bar)),
                'both_host_surfaces_must_have_real_native_submissions')
        require(any(h['id'] == 'bar-audio' for h in bar['hits']), 'host_action_hit_missing')
        widget_roles = {entry['id'] for entry in current['widget_surfaces']}
        widgets = [s for s in state['surfaces'] if s.get('instance') == current['instance']
                   and s.get('output') == name and s.get('role', s['id']) in widget_roles]
        require(len(widgets) == len(widget_roles), 'per_host_widget_surfaces_missing')
        for widget in widgets:
            require(widget['native_frames'] > 0, 'visible_widget_not_submitted')
            native[widget['id']] = layer_for(layers, name, widget, self.pid)
        return {'monitor': monitor_signature(monitor), 'host': current,
                'output': output(state, name), 'background': background, 'bar': bar,
                'widgets': widgets, 'layers': native,
                'live': state['client'].get('live_wallpaper', {})}

    def settled(self, name, expected=None):
        previous, value = None, None

        def ready():
            nonlocal previous, value
            try:
                value = self.observed_host(name, expected)
            except (Blocked, KeyError):
                previous = None
                return False
            fingerprint = (value['host']['generation'], value['output']['global'],
                           value['background']['buffer_width'], value['background']['buffer_height'],
                           value['background']['density'], value['bar']['hits'], value['layers'])
            same, previous = previous == fingerprint, fingerprint
            return value if same else False

        return eventually(ready, 'native_host_geometry_did_not_settle')

    def warp(self, name, identifier, observation, point):
        result = self.lease('warp', monitor=name, identifier=identifier,
                            generation=observation['host']['generation'],
                            global_id=observation['output']['global'])
        require(tuple(result['point']) == point and result['surface'] == observation['bar']['id'],
                'guardian_observed_hit_differs_from_host_geometry')
        eventually(lambda: cursor_matches(json.loads(self.vm.hypr('-j', 'cursorpos')), point),
                   'headless_cursor_warp_did_not_reach_target', timeout=3)

    def current_cursor(self, point):
        require(cursor_matches(json.loads(self.vm.hypr('-j', 'cursorpos')), point),
                'native_button_warped_cursor_away_from_headless_target')

    def press_action(self, name, identifier, fresh_focus=False):
        self.lease('status')  # guard authority immediately before actual input
        value = self.settled(name)
        if fresh_focus:
            require(value['bar'].get('focus') != identifier, 'fresh_native_press_focus_required')
        point = hit_center(value['monitor'], value['bar'], value['layers'][value['bar']['id']], identifier)
        self.warp(name, identifier, value, point)
        # Read the same source/native host again after entering it. A source or
        # geometry change invalidates this press instead of reusing stale hits.
        after = self.settled(name)
        require(after['host']['generation'] == value['host']['generation']
                and after['output']['global'] == value['output']['global']
                and hit_center(after['monitor'], after['bar'], after['layers'][after['bar']['id']],
                               identifier) == point, 'hit_retired_before_native_press')
        self.current_cursor(point)
        # Geometry settling may be slow on a software-rendered guest. Recheck
        # OS/credential/layer/binding/cursor authority after it, then bind a fresh
        # SDK host/global/hit observation immediately before the physical down.
        self.lease('status')
        admitted = self.vm.inspect()
        admitted_source = surface(admitted, name, 'bar')
        require(host(admitted, name)['generation'] == after['host']['generation']
                and output(admitted, name)['global'] == after['output']['global']
                and admitted_source['id'] == after['bar']['id']
                and hit_center(after['monitor'], admitted_source,
                               after['layers'][after['bar']['id']], identifier) == point,
                'host_or_hit_retired_at_final_input_admission')
        self.current_cursor(point)
        self.held = True  # An uncertain QMP acknowledgement still needs one release.
        self.vm.button(True)
        self.current_cursor(point)
        if fresh_focus:
            def received():
                state = self.vm.inspect()
                source = surface(state, name, 'bar')
                require(host(state, name)['generation'] == value['host']['generation']
                        and output(state, name)['global'] == value['output']['global'],
                        'pressed_host_retired_before_capture_observation')
                return source.get('focus') == identifier and source.get('focus_visible') is False
            eventually(received, 'actual_down_event_not_observed_on_old_surface', timeout=5)
        return point

    def press_audio(self, name):
        return self.press_action(name, 'bar-audio')

    def panels_stay_closed(self, seconds=1):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            client = self.vm.inspect()['client']
            require(not client['launcher'] and not client['notification_history_open']
                    and all(not any(client.get(name, {}).get(key) for key in ('open', 'visible'))
                            for name in ('audio', 'network', 'bluetooth', 'power', 'tray',
                                         'weather', 'media', 'live_wallpaper', 'settings', 'displays')),
                    'retired_press_or_release_opened_a_foreground_panel')
            time.sleep(.1)

    def release(self):
        if self.held:
            self.vm.button(False)
            self.held = False

    def open_close_audio(self, name):
        point = self.press_audio(name)
        try:
            self.release()
            self.current_cursor(point)

            def opened():
                state = self.vm.inspect()
                panels = [s for s in state['surfaces'] if s.get('role', s['id']) == 'audio']
                return (state['client']['audio']['open'] and state['client']['active_monitor'] == name
                        and len(panels) == 1 and panels[0]['output'] == name
                        and panels[0]['native_frames'] > 0)

            eventually(opened, 'real_headless_qmp_press_did_not_open_panel_on_exact_host', timeout=12)
            self.lease('status')
            self.vm.escape()
            eventually(lambda: not self.vm.inspect()['client']['audio']['open']
                       and not any(s.get('role', s['id']) == 'audio'
                                   for s in self.vm.inspect()['surfaces']), 'native_panel_did_not_close')
        finally:
            self.release()
        self.record('actual_qmp_audio_open_close_' + ('original' if not name.startswith('LUCENT-TEST-') else 'owned'))


def stage(vm):
    guest = vm.remote('mktemp', '-d', '/tmp/lucent-native-outputs-XXXXXXXXXXXX')
    bundle = {name: base64.b64encode((ROOT / 'vm' / name).read_bytes()).decode()
              for name in ('multi_output_guard.py', 'multi_output_policy.py')}
    script = ('import base64,json,os,pathlib,sys; p=pathlib.Path(sys.argv[1]); '
              'assert p.resolve()==p and p.name.startswith("lucent-native-outputs-") '
              'and p.stat().st_uid==os.getuid() and (p.stat().st_mode&511)==448; '
              'items=json.load(sys.stdin); '
              'assert set(items)=={"multi_output_guard.py","multi_output_policy.py"}; '
              '[(lambda fd,data: (os.write(fd,data),os.close(fd)))('
              'os.open(p/name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,384),'
              'base64.b64decode(data,validate=True)) for name,data in items.items()]')
    vm.remote('python3', '-c', script, guest, data=json.dumps(bundle))
    return guest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true', help='Explicitly admit this bounded VM-only test.')
    parser.add_argument('--build-report', type=Path, required=True,
                        help='Private release identity report with source_commit and binary SHA-256.')
    parser.add_argument('--lab-root', type=Path, default=ROOT)
    args = parser.parse_args()
    require(args.run, 'explicit_run_flag_required')
    # A normal stop follows the same balanced-input/finally path as Ctrl+C.
    # SIGKILL/host loss is covered for output ownership by the guest lease;
    # physical button release after a killed host requires operator recovery.
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    build = json.loads(args.build_report.read_text())
    sha = build['binaries']['lucent-desktop']['sha256']
    require(isinstance(sha, str) and len(sha) == 64 and all(c in '0123456789abcdef' for c in sha),
            'valid_release_digest_required')
    vm = VM(args.lab_root.resolve())
    reports = args.lab_root.resolve() / 'reports/local'
    reports.mkdir(parents=True, exist_ok=True)
    directory = reports / ('native-multi-output-' + uuid.uuid4().hex)
    directory.mkdir(mode=0o700)
    report = {'status': 'blocked', 'tested_implementation_commit': build['source_commit'],
              'desktop_sha256': sha, 'checks': [], 'native_execution_attempted': True,
              'native_mutations_started': False,
              'proof_scope': 'Two simultaneous native compositor outputs, one owned headless; '
              'observed geometry, host lifetime and actual QMP button actions. Submission counts '
              'are not presentation FPS. No physical cable, GPU, suspend or real-app proof.',
              'tooling_sha256': {name: hashlib.sha256((ROOT / 'vm' / name).read_bytes()).hexdigest()
                                  for name in ('test-native-multi-output.py', 'multi_output_guard.py',
                                               'multi_output_policy.py')}}
    guest, run, finish, started = None, None, None, False
    try:
        guest = stage(vm)
        name = 'LUCENT-TEST-' + uuid.uuid4().hex[:24]
        unit = 'lucent-native-outputs-' + uuid.uuid4().hex
        # The owner remains alive after host/SSH loss. Its internal deadline is
        # 240s, with a separate unit hard bound allowing cleanup to complete.
        vm.session('systemd-run', '--user', '--collect', '--quiet', '--unit=' + unit,
                   '--property=RuntimeMaxSec=300', '--property=TimeoutStopSec=20',
                   '--property=MemoryMax=128M', '--property=TasksMax=16',
                   'python3', str(Path('desktop-lab/scripts/in-session.py')),
                   'python3', guest + '/multi_output_guard.py', 'serve', guest, name, '--sha256', sha)
        started = True

        def ready():
            text = vm.remote('python3', '-c',
                             'from pathlib import Path; import sys; p=Path(sys.argv[1]); '
                             'failed=p/"startup-blocked.json"; ready=p/"ready.json"; '
                             'print(failed.read_text() if failed.exists() else '
                             '(ready.read_text() if ready.exists() else "null"))', guest)
            value = json.loads(text)
            require(not isinstance(value, dict) or not value.get('blocked'),
                    'guard_startup_' + str(value.get('blocked')) if isinstance(value, dict)
                    else 'guard_startup_failed')
            return value

        receipt = eventually(ready, 'independent_guard_preconditions_not_satisfied', timeout=35)
        run = Run(vm, directory, guest, receipt['desktop_pid'])
        original_name = receipt['baseline'][0]['name']
        initial = run.settled(original_name, receipt['baseline'][0])
        run.record('release_supervisor_config_media_and_idle_guards', receipt)
        run.lease('create')
        report['native_mutations_started'] = True
        for scale in (1, 2, 1.25, 1):
            lease = run.lease('configure', scale)
            owned = run.settled(name, lease['owned'])
            original = run.settled(original_name, receipt['baseline'][0])
            require(original['host']['generation'] == initial['host']['generation']
                    and original['output']['global'] == initial['output']['global']
                    and original['layers'] == initial['layers'], 'unrelated_original_host_recreated')
            state = vm.inspect()
            require({o.get('name') for o in state['outputs']} == {original_name, name}
                    and len(state['client']['monitor_hosts']) == 2, 'two_native_hosts_required')
            run.record('simultaneous_native_scale_' + str(scale).replace('.', '_'),
                       {'original': original, 'owned': owned})
            run.open_close_audio(name)
            run.open_close_audio(original_name)
        # Retire an actual press without releasing it onto a replacement host.
        run.press_action(name, 'bar-network', fresh_focus=True)
        run.panels_stay_closed(.25)
        before = run.settled(name)
        old_instance = before['host']['instance']
        old_ids = host_surface_ids(vm.inspect(), name, old_instance)
        run.lease('remove')
        eventually(lambda: retired(vm.inspect(), name, old_instance, old_ids), 'removed_host_still_has_live_surfaces')
        run.release()
        run.panels_stay_closed()
        surviving = run.settled(original_name, receipt['baseline'][0])
        require(surviving['host']['generation'] == initial['host']['generation']
                and surviving['output']['global'] == initial['output']['global'],
                'original_host_changed_during_hotplug')
        run.record('native_output_removal_retires_pressed_surface_and_host')
        recreated = run.lease('create')
        after = run.settled(name, recreated['owned'])
        require(after['host']['generation'] > before['host']['generation']
                and after['output']['global'] != before['output']['global'],
                'same_name_recreation_reused_retired_authority')
        run.panels_stay_closed()
        run.open_close_audio(name)
        run.record('same_name_new_wayland_global_and_client_generation',
                   {'before': before, 'after': after})
        run.lease('finish')
    except BaseException as error:
        report['blocked'] = ('interrupted' if isinstance(error, KeyboardInterrupt) else
                             str(error) if isinstance(error, Blocked) else type(error).__name__)
    finally:
        if run:
            try:
                run.release()
            except Exception:
                pass
            try:
                run.lease('finish')
            except Exception:
                pass
        if guest and started:
            def finished():
                text = vm.remote('python3', '-c',
                                 'from pathlib import Path; import sys; p=Path(sys.argv[1]); '
                                 'print(p.read_text() if p.exists() else "null")', guest + '/finished.json')
                return json.loads(text)
            try:
                # If the host lost a request reply the independent owner still
                # expires. Leave its directory/unit intact for that cleanup.
                finish = eventually(finished, 'independent_cleanup_pending', timeout=35)
                private_json(directory / 'cleanup.json', finish)
                require(finish.get('preserved') and finish.get('removed_only_owned_output')
                        and finish.get('cursor_restored'),
                        'independent_cleanup_or_preservation_blocked')
                if 'blocked' not in report:
                    require(not vm.inspect()['client']['audio']['open'], 'final_owned_panel_still_open')
                    report['status'] = 'passed'
            except Exception as error:
                report['cleanup_pending_or_blocked'] = str(error) if isinstance(error, Blocked) else type(error).__name__
        report['checks'] = run.checks if run else []
        report['guest_private_receipt'] = guest
        private_json(directory / 'report.json', report)
        print(json.dumps({'status': report['status'], 'checks': len(report['checks']),
                          'cleanup_preserved': bool(finish and finish.get('preserved'))}), flush=True)
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())
