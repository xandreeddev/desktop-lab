#!/usr/bin/env python3
"""Guarded genuine nm-applet SNI/menu acceptance in an already running guest.

No work is performed on import. Requires an identified release, empty tray,
idle/unlocked 1920x1080 single output and wired-only guest. Never installs or
enables anything, reads secrets, changes connections or uses a fixture SNI.
Only the exact native Connection Information leaf may be invoked. See
docs/third-party-tray.md for preconditions, cleanup and proof limits.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import signal
import subprocess
import time

from native_surfaces import active_surfaces
from third_party_tray_policy import (Blocked, Unsupported, CursorOwnership, button_event,
                                     current_menu, require, unique, visible_point)

ROOT = Path(__file__).resolve().parents[1]


def private(path, value):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def eventually(function, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = function()
        if value:
            return value
        time.sleep(.2)
    raise Blocked(label)


class VM:
    def __init__(self, lab_root):
        import lab
        self.lab = lab
        lab.ROOT, lab.STATE = lab_root, lab_root / '.local-vms'
        inventory = lab.inventory()
        require(inventory.get('uri') == 'qemu:///session' and inventory.get('user') == 'omarchy'
                and inventory['vms']['lucent'] == {'name': 'desktop-lab-lucent', 'port': 2243},
                'prepared_session_only_lucent_inventory_required')
        self.name = inventory['vms']['lucent']['name']
        require(self.virsh('domstate', self.name).strip() == 'running', 'already_running_guest_required')

    def command(self, args, data=None, timeout=35):
        try:
            result = subprocess.run(args, input=data, text=True, capture_output=True, timeout=timeout)
        except (OSError, subprocess.TimeoutExpired):
            raise Blocked('bounded_native_transport_failed') from None
        require(result.returncode == 0 and len(result.stdout) <= 8 * 1024 * 1024,
                'bounded_native_transport_failed')
        return result.stdout.strip()

    def virsh(self, *args):
        return self.command(['virsh', '-c', 'qemu:///session', *args], timeout=8)

    def remote(self, *args, data=None):
        return self.command(self.lab.ssh_args('lucent') + [shlex.join(args)], data=data)

    def session(self, *args, **kwargs):
        return self.remote('python3', 'desktop-lab/scripts/in-session.py', *args, **kwargs)

    def hypr(self, name):
        return json.loads(self.session('hyprctl', '-j', name))

    def inspect(self):
        return json.loads(self.session('/home/omarchy/.local/bin/lucent-cli', 'inspect'))

    def qmp(self, events):
        reply = json.loads(self.virsh('qemu-monitor-command', self.name, json.dumps({
            'execute': 'input-send-event', 'arguments': {'events': events}})))
        require('return' in reply and 'error' not in reply, 'actual_qmp_pointer_delivery_required')


def stage(vm):
    guest = '/tmp/lucent-third-party-tray-' + secrets.token_hex(12)
    payload = {name: base64.b64encode((ROOT / 'vm' / name).read_bytes()).decode()
               for name in ('third_party_tray_guest.py', 'third_party_tray_policy.py')}
    code = '''import base64,json,os,sys
from pathlib import Path
p=Path(sys.argv[1]);assert p.parent==Path('/tmp') and not p.exists()
p.mkdir(mode=0o700)
for name,data in json.load(sys.stdin).items():
 assert name in ('third_party_tray_guest.py','third_party_tray_policy.py')
 fd=os.open(p/name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'wb') as f:f.write(base64.b64decode(data,validate=True))
print('staged')
'''
    require(vm.remote('python3', '-c', code, guest, data=json.dumps(payload)) == 'staged',
            'private_guest_tools_not_staged')
    return guest


class Run:
    def __init__(self, vm, guest, directory):
        self.vm, self.guest, self.directory = vm, guest, directory
        self.cursor, self.pid = None, None
        self.held = None
        self.release_uncertain = False
        self.started = False
        self.checks = []
        self.sequence = 0

    def probe(self, operation, *, optional=False, **arguments):
        argv = ['python3', self.guest + '/third_party_tray_guest.py', operation, self.guest]
        for name, value in arguments.items():
            argv += ['--' + name.replace('_', '-'), str(value)]
        result = json.loads(self.vm.session(*argv))
        require(result.get('status') in ('ok', 'blocked', 'unsupported'), 'recognized_guest_probe_reply_required')
        if result['status'] != 'ok':
            if optional:
                return None
            error = Unsupported if result['status'] == 'unsupported' else Blocked
            raise error(result.get('reason', 'native_guest_probe_blocked'))
        return result['value']

    def record(self, name, value=None):
        self.checks.append(name)
        if value is not None:
            self.sequence += 1
            private(self.directory / f'{self.sequence:02d}-{name}.json', value)
        print(json.dumps({'check': name, 'passed': True}), flush=True)

    def point(self, prefix=None, identifier=None):
        state = self.vm.inspect()
        pairs = [(surface, hit) for surface in active_surfaces(state) for hit in surface.get('hits', [])
                 if (hit['id'] == identifier if identifier else hit['id'].startswith(prefix))]
        surface, hit = unique(pairs, lambda _: True, 'one_current_native_tray_control_required')
        require(surface['frames'] > 0 and surface['native_frames'] > 0, 'native_tray_surface_submission_required')
        layers = []
        def visit(value):
            if isinstance(value, dict):
                if 'namespace' in value:
                    layers.append(value)
                for child in value.values():
                    visit(child)
            elif isinstance(value, list):
                for child in value:
                    visit(child)
        visit(self.vm.hypr('layers'))
        layer = unique(layers, lambda value: value.get('namespace') == 'lucent-' + surface['id']
                       and value.get('pid') == self.pid, 'one_current_pid_bound_native_tray_layer_required')
        point = visible_point(surface, layer, hit['id'])
        require(0 <= point[0] < 1920 and 0 <= point[1] < 1080, 'single_output_native_tray_point_required')
        # Address, object and geometry must all survive the press/release.
        identity = {'id': surface['id'], 'instance': surface.get('instance'), 'output': surface.get('output'),
                    'address': layer.get('address'), 'point': point, 'hit': hit['id']}
        require(identity['address'], 'concrete_native_tray_layer_identity_required')
        return identity

    def guard(self):
        self.probe('input')
        self.cursor.observe(self.vm.hypr('cursorpos'))

    def move(self, destination):
        self.guard()
        self.cursor.begin(self.vm.hypr('cursorpos'), destination)
        events = [{'type': 'abs', 'data': {'axis': axis, 'value': round(value / extent * 32767)}}
                  for axis, value, extent in (('x', destination[0], 1919), ('y', destination[1], 1079))]
        self.vm.qmp(events)
        eventually(lambda: self.vm.hypr('cursorpos') == dict(zip(('x', 'y'), destination)),
                   'absolute_pointer_backend_did_not_reach_declared_point', timeout=2)
        self.cursor.finish(self.vm.hypr('cursorpos'))

    def click(self, button, *, prefix=None, identifier=None, menu=None):
        initial = self.point(prefix, identifier)
        self.move(initial['point'])
        self.guard()
        require(self.point(prefix, identifier) == initial, 'native_tray_target_changed_before_press')
        if menu is not None:
            current_menu(menu, self.probe('menu'))
        self.guard()  # Fresh OS/PID/binding admission after potentially slow D-Bus observations.
        require(self.point(prefix, identifier) == initial, 'fresh_native_tray_target_retired_before_press')
        self.cursor.observe(self.vm.hypr('cursorpos'))
        self.held = button  # A lost QMP reply still requires a best-effort balanced release.
        self.vm.qmp([button_event(button, True)])
        try:
            if menu is not None:
                current_menu(menu, self.probe('menu'))
            self.guard()
            require(self.point(prefix, identifier) == initial, 'native_tray_target_changed_before_release')
            self.cursor.observe(self.vm.hypr('cursorpos'))
        except BaseException:
            # Cancel the Lucent capture through a fresh, unbound Escape before
            # release. Never send this key after auth/foreign-layer admission fails.
            try:
                self.guard()
                self.vm.virsh('send-key', self.vm.name, '--codeset', 'linux', '--holdtime', '30', '1')
                eventually(lambda: self.vm.inspect()['client']['tray']['open'] is False,
                           'stale_tray_capture_cancellation_not_observed', timeout=2)
            finally:
                self.release()
            raise
        self.release()

    def release(self):
        if self.held is not None:
            button, self.held = self.held, None
            try:
                self.vm.qmp([button_event(button, False)])
            except BaseException:
                self.release_uncertain = True
                raise

    def execute(self, sha):
        prepared = self.probe('prepare', desktop_sha256=sha)
        # Keep private checkpoint identities/cursor out of public report output.
        code = 'import json,sys;v=json.load(open(sys.argv[1]));print(json.dumps({"pid":v["desktop"]["process"]["pid"],"cursor":v["cursor"]}))'
        saved = json.loads(self.vm.session('python3', '-c', code, self.guest + '/checkpoint.json'))
        self.pid, self.cursor = saved['pid'], CursorOwnership(saved['cursor'])
        self.record('wired_only_idle_identified_release', prepared)
        unit = self.guest.rsplit('/', 1)[-1] + '.service'
        self.started = True  # Before dispatch: a lost start reply does not imply no process.
        self.vm.session('systemd-run', '--user', '--collect', '--quiet', '--slice=app.slice', '--unit=' + unit,
                        '--property=RuntimeMaxSec=300', '--property=TimeoutStopSec=10',
                        '--property=MemoryMax=192M', '--property=TasksMax=64', '--property=UMask=0077',
                        '--property=StandardOutput=append:' + self.guest + '/stdout.log',
                        '--property=StandardError=append:' + self.guest + '/stderr.log',
                        'python3', 'desktop-lab/scripts/in-session.py',
                        'python3', self.guest + '/third_party_tray_guest.py', 'launch', self.guest)
        descriptor = eventually(lambda: self.probe('describe', optional=True), 'genuine_nm_applet_never_registered')
        require(descriptor['icon_provided'], 'genuine_sni_icon_metadata_required')
        self.record('real_applet_exact_pid_cgroup_bus_owner', descriptor)
        eventually(lambda: len([hit for surface in active_surfaces(self.vm.inspect())
                                for hit in surface.get('hits', []) if hit['id'].startswith('bar-tray-item-')]) == 1,
                   'genuine_item_missing_native_bar_control')
        self.point(prefix='bar-tray-item-')
        self.record('real_item_native_bar_control')
        self.click('right', prefix='bar-tray-item-')
        menu = eventually(lambda: self.probe('menu', optional=True), 'genuine_safe_information_menu_unavailable')
        identifier = 'tray-menu-' + str(menu['information_id'])
        eventually(lambda: any(hit['id'] == identifier for surface in active_surfaces(self.vm.inspect())
                                for hit in surface.get('hits', [])), 'genuine_menu_not_rendered_natively')
        self.point(identifier=identifier)
        self.record('genuine_menu_visible_and_exact_safe_leaf', menu)
        self.click('left', identifier=identifier, menu=menu)
        eventually(lambda: (self.probe('dialog', optional=True) or {}).get('owned_dialogs') == 1,
                   'native_information_action_did_not_open_genuine_dialog')
        self.record('native_real_application_information_action')

    def cleanup(self):
        failures = []
        try:
            self.release()  # Balance an owned button even after guard loss.
        except BaseException:
            failures.append('physical_button_release_uncertain')
        result = {'owned_applet_quiescent': not self.started, 'state_preserved': False,
                  'cursor_restored': self.cursor is None, 'physical_button_released': not self.release_uncertain,
                  'failures': failures}
        try:
            if self.started:
                self.probe('stop')
                result['owned_applet_quiescent'] = True
                eventually(lambda: self.probe('retired', optional=True), 'real_applet_retirement_or_preservation_failed')
                result['state_preserved'] = True
            elif self.cursor is not None:
                self.probe('verify')
                result['state_preserved'] = True
        except BaseException as error:
            failures.append(str(error) if isinstance(error, Blocked) else 'owned_applet_cleanup_failed')
        if self.cursor is not None and result['owned_applet_quiescent'] and not self.release_uncertain:
            # State preservation and exact-unit cleanup still run when cursor
            # authority was lost. Never overwrite independent user movement.
            try:
                self.cursor.observe(self.vm.hypr('cursorpos'))
                if self.cursor.expected != self.cursor.initial:
                    # App retirement replaces input guard's owned-process check.
                    self.probe('restore-admission')
                    self.cursor.begin(self.vm.hypr('cursorpos'), self.cursor.initial)
                    self.vm.qmp([{'type': 'abs', 'data': {'axis': axis, 'value': round(value / extent * 32767)}}
                                 for axis, value, extent in (('x', self.cursor.initial[0], 1919),
                                                             ('y', self.cursor.initial[1], 1079))])
                    eventually(lambda: self.vm.hypr('cursorpos') == dict(zip(('x', 'y'), self.cursor.initial)),
                               'original_cursor_restore_not_confirmed', timeout=2)
                    self.cursor.finish(self.vm.hypr('cursorpos'))
                result['cursor_restored'] = True
            except BaseException as error:
                failures.append(str(error) if isinstance(error, Blocked) else 'cursor_restore_not_proven')
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true', help='Explicitly enable guarded native input and owned app lifecycle.')
    parser.add_argument('--lab-root', type=Path, required=True)
    parser.add_argument('--build-report', type=Path, required=True)
    args = parser.parse_args()
    if not args.run:
        parser.error('--run is required; there is no automatic VM setup or install')
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    build = json.loads(args.build_report.read_text())
    sha = build['binaries']['lucent-desktop']['sha256']
    require(re.fullmatch('[a-f0-9]{64}', sha) and re.fullmatch('[a-f0-9]{40}', build['source_commit']),
            'source_specific_release_build_receipt_required')
    lab_root = args.lab_root.resolve()
    parent = lab_root / 'reports/local'
    parent.mkdir(parents=True, exist_ok=True)
    directory = parent / ('third-party-tray-' + secrets.token_hex(12))
    directory.mkdir(mode=0o700)
    report = {'status': 'blocked', 'source_commit': build['source_commit'], 'desktop_sha256': sha,
              'native_execution_attempted': False, 'native_mutations_started': False, 'checks': [],
              'proof_scope': 'One genuine nm-applet SNI item and safe native menu action in a wired-only '
              'single-output VM. No Wi-Fi scans, secret-agent/pairing, physical-device, nested-menu, '
              'pixel-quality or presentation-FPS acceptance.',
              'tooling_sha256': {name: hashlib.sha256((ROOT / 'vm' / name).read_bytes()).hexdigest()
                                  for name in ('test-third-party-tray.py', 'third_party_tray_guest.py',
                                               'third_party_tray_policy.py')}}
    run = None
    try:
        vm = VM(lab_root)
        run = Run(vm, stage(vm), directory)
        report['native_execution_attempted'] = True
        report['private_guest_directory'] = run.guest
        run.execute(sha)
        report['status'] = 'passed'
    except Unsupported as error:
        report.update(status='unsupported', reason=str(error))
    except Blocked as error:
        report.update(status='blocked', reason=str(error))
    except BaseException as error:
        report.update(status='blocked', reason='interrupted' if isinstance(error, KeyboardInterrupt)
                      else 'unexpected_native_probe_failure')
    finally:
        if run is not None:
            report.update(checks=run.checks, native_mutations_started=run.started)
            try:
                report['cleanup'] = run.cleanup()
                if run.started and (report['cleanup']['failures'] or not all(report['cleanup'][key] for key in
                        ('owned_applet_quiescent', 'state_preserved', 'cursor_restored', 'physical_button_released'))):
                    report['status'] = 'blocked'
            except BaseException as error:
                report.update(status='blocked', cleanup={'completed': False, 'reason': str(error)
                              if isinstance(error, Blocked) else 'bounded_cleanup_failed'})
        private(directory / 'result.json', report)
        print(json.dumps({'status': report['status'], 'report': str(directory / 'result.json')}))
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
