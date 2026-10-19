#!/usr/bin/env python3
"""Owned compositor IME acceptance for the independent international-text client.

Run only on the prepared unlocked idle one-output Lucent VM. Binaries must already
be deployed at the supplied guest paths and match their supplied SHA256 digests.
Starts one owned transient example unit and a non-replacing input-method-v2 peer;
never stops the desktop, edits settings, grabs keys, installs or replaces an IME.
An existing input method causes refusal. Native pointer/key input requires the
same empty ordinary-app/secure-UI guard before every action. Original settings,
wallpaper link, output/workspace and pointer are verified/restored as appropriate.
Private captures are retained only below ignored reports/local. Import is inert.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import secrets
import signal

import international_text_policy as policy
import shlex
import subprocess
import time


ROOT = Path(__file__).resolve().parents[1]


class Failure(Exception):
    pass


def require(condition, label):
    if not condition:
        raise Failure(label)


def run(argv, *, data=None, timeout=20):
    try:
        result = subprocess.run(argv, input=data, capture_output=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        raise Failure('bounded_subprocess_failed') from None
    require(result.returncode == 0, 'bounded_subprocess_failed')
    return result.stdout


def private(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data if isinstance(data, bytes) else (json.dumps(data, indent=2) + '\n').encode())


PROBE = r'''
import hashlib,json,os,stat,sys
from pathlib import Path
result={}
for name,raw,expected in zip(('example','peer','desktop'),sys.argv[1::2],sys.argv[2::2]):
 path=Path(raw);assert path.is_absolute() and path.resolve(strict=True)==path
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
 with os.fdopen(fd,'rb') as stream:
  info=os.fstat(stream.fileno());assert stat.S_ISREG(info.st_mode) and info.st_uid==os.getuid() and info.st_size<=128*1024*1024
  digest=hashlib.sha256()
  for chunk in iter(lambda:stream.read(1024*1024),b''):digest.update(chunk)
  assert digest.hexdigest()==expected and os.fstat(stream.fileno())==info
 result[name]=expected
print(json.dumps(result))
'''
CHECKPOINT = r'''
import hashlib,json,os,stat
from pathlib import Path
base=Path.home()/'.local/state/lucent';settings=base/'desktop.json'
fd=os.open(settings,os.O_RDONLY|os.O_NOFOLLOW)
with os.fdopen(fd,'rb') as stream:
 info=os.fstat(stream.fileno());assert stat.S_ISREG(info.st_mode) and info.st_size<=4*1024*1024
 data=stream.read(4*1024*1024+1);assert len(data)<=4*1024*1024 and os.fstat(stream.fileno())==info
wallpaper=base/'wallpaper';link=wallpaper.lstat();assert stat.S_ISLNK(link.st_mode)
print(json.dumps({'settings_sha256':hashlib.sha256(data).hexdigest(),'settings_inode':[info.st_dev,info.st_ino,info.st_size,info.st_mtime_ns],'wallpaper':[link.st_dev,link.st_ino,link.st_mtime_ns,os.readlink(wallpaper),str(wallpaper.resolve(strict=True))]}))
'''
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--example-bin', required=True)
    parser.add_argument('--peer-bin', required=True)
    parser.add_argument('--example-sha256', required=True)
    parser.add_argument('--peer-sha256', required=True)
    parser.add_argument('--desktop-sha256', required=True)
    args = parser.parse_args()
    require(re.fullmatch('[a-f0-9]{40}', args.source_commit), 'full_source_commit_required')
    for digest in (args.example_sha256, args.peer_sha256, args.desktop_sha256):
        require(re.fullmatch('[a-f0-9]{64}', digest), 'full_binary_digest_required')
    common = Path(run(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute',
                       '--git-common-dir']).decode().strip()).resolve(strict=True)
    spec = importlib.util.spec_from_file_location('international_text_lab', common.parent / 'vm/lab.py')
    require(spec is not None and spec.loader is not None, 'canonical_lab_loader_required')
    lab = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(lab)
    inventory = lab.inventory()
    require(inventory['user'] == 'omarchy' and inventory['uri'] == 'qemu:///session'
            and inventory['vms']['lucent']['port'] == 2243, 'prepared_guest_required')
    name = inventory['vms']['lucent']['name']
    tag = secrets.token_hex(12)
    directory = lab.ROOT / 'reports/local' / ('international-text-native-' + tag)
    directory.mkdir(mode=0o700)
    unit = 'lucent-international-test-' + tag + '.service'
    peer_unit = 'lucent-international-peer-' + tag + '.service'
    peer = peer_log = None
    example_attempted = peer_attempted = example_confirmed = peer_confirmed = pressed = False
    original = original_outputs = original_workspace = cursor = None
    authority = {}
    frozen_layer = None
    focused = None
    input_deadline = None
    sequence = 0
    report = {'passed': False, 'source_commit': args.source_commit, 'checks': {}, 'restoration': {},
              'helper_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'policy_sha256': hashlib.sha256((ROOT / 'vm/international_text_policy.py').read_bytes()).hexdigest(),
              'binaries': {'desktop': args.desktop_sha256, 'example': args.example_sha256, 'peer': args.peer_sha256},
              'scope': 'One prepared native output; independent non-replacing input-method peer, no third-party IME/hardware claim',
              'host_crash_policy': 'Both exact owned transient units expire after210seconds. Private evidence remains; cursor restoration requires a guarded live host.'}

    def remote(*argv, data=None):
        return run(lab.ssh_args('lucent') + [shlex.join(argv)], data=data)

    def session(*argv, data=None):
        return remote('python3', 'desktop-lab/scripts/in-session.py', *argv, data=data)

    def configuration(owned=True):
        expected = authority if owned else {k: v for k, v in authority.items() if k.startswith('desktop')}
        return {'desktop_sha256': args.desktop_sha256, 'expected': expected,
                'example': {'unit': unit, 'argv': [args.example_bin], 'sha256': args.example_sha256}
                           if owned and example_confirmed else None,
                'peer': {'unit': peer_unit, 'argv': [args.peer_bin], 'sha256': args.peer_sha256}
                        if owned and peer_confirmed else None}

    def observe(owned=True):
        value = json.loads(session('python3', '-c', policy.OBSERVE, json.dumps(configuration(owned))))
        for key, identity in value['authority'].items():
            require(key not in authority or authority[key] == identity, 'frozen_owned_runtime_authority')
            authority[key] = identity
        return value

    def inspect():
        return observe()['example']

    def guard(owned=True, restoring=False):
        nonlocal frozen_layer
        if input_deadline is not None and not restoring:
            require(time.monotonic() < input_deadline, 'bounded_native_input_window')
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == b'false', 'unlocked_guest_required')
        value = observe(owned)
        policy.idle_client(value['desktop']['client'])
        require(json.loads(session('hyprctl', '-j', 'clients')) == [], 'no_foreign_managed_clients')
        policy.safe_bindings(json.loads(session('hyprctl', '-j', 'binds')))
        layer = policy.owned_layers(json.loads(session('hyprctl', '-j', 'layers')),
                    value['authority']['desktop']['pid'],
                    value['authority']['example']['process']['pid'] if owned and example_confirmed else None)
        if layer is not None:
            if frozen_layer is None:
                frozen_layer = layer
            require(layer == frozen_layer, 'frozen_owned_native_layer')
        if original_outputs is not None:
            require(policy.output_state(json.loads(session('hyprctl', '-j', 'monitors'))) == original_outputs,
                    'unchanged_native_output')
            require(policy.workspace_state(json.loads(session('hyprctl', '-j', 'activeworkspace'))) == original_workspace,
                    'unchanged_active_workspace')
        if cursor is not None:
            cursor.observe(json.loads(session('hyprctl', '-j', 'cursorpos')))
        return value, layer

    def eventually(predicate, label, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except (Failure, KeyError, ValueError, LookupError):
                pass
            time.sleep(.12)
        raise Failure(label)

    def command(**value):
        nonlocal sequence
        require(peer is not None and peer.poll() is None, 'owned_peer_alive_required')
        sequence += 1
        value['id'] = sequence
        peer.stdin.write((json.dumps(value) + '\n').encode())
        peer.stdin.flush()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if not select.select([peer.stdout], [], [], max(0, deadline - time.monotonic()))[0]:
                break
            line = peer.stdout.readline(16385)
            require(line and len(line) <= 16384, 'bounded_peer_response_required')
            event = json.loads(line)
            require(event.get('event') not in ('unavailable', 'retired'), 'existing_input_method_or_retired_seat_preserved_refusal')
            if event.get('id') == sequence:
                return event
        raise Failure('native_peer_response_deadline')

    def status(expected):
        return command(action='status', expected=expected)

    def focused_guard(expected=None):
        value, _ = guard()
        state = value['example']
        require(focused in ('single', 'body'), 'ordinary_field_target_required')
        current = state['client'][focused]
        require(expected is None or current == expected, 'exact_current_fixture_text_required')
        observation = status(current)
        policy.focused_field(state, focused, observation)
        return observation

    def send(expected, **changes):
        observed = focused_guard(expected)
        response = command(action='batch', expected=expected, cursor=observed['cursor'], anchor=observed['anchor'], **changes)
        require(response.get('posted'), 'native_peer_batch_posted')

    def key(*codes):
        focused_guard()
        run(['virsh', '-c', lab.URI, 'send-key', name, '--codeset', 'linux', '--holdtime', '25', *map(str, codes)])
        time.sleep(.1)

    def qmp(events, *, restoring=False):
        releasing = all(event['type'] == 'btn' and event['data']['down'] is False for event in events)
        if not releasing:
            guard(owned=not restoring, restoring=restoring)
        value = json.loads(run(['virsh', '-c', lab.URI, 'qemu-monitor-command', name,
                               json.dumps({'execute': 'input-send-event', 'arguments': {'events': events}})]))
        require('error' not in value, 'native_pointer_delivery')

    def geometry(field):
        value, layer = guard()
        return policy.field_geometry(value['example'], layer, field)

    def move(point, *, restoring=False):
        cursor.begin(json.loads(session('hyprctl', '-j', 'cursorpos')), point)
        qmp(policy.absolute_move(point), restoring=restoring)
        eventually(lambda: cursor.observe(json.loads(session('hyprctl', '-j', 'cursorpos'))) == point,
                   'exact_owned_pointer_arrival', timeout=3)
        cursor.finish(json.loads(session('hyprctl', '-j', 'cursorpos')))

    def click(field):
        nonlocal pressed, focused
        _, _, target = geometry(field)
        focused = None
        move(target['point'])
        _, _, fresh = geometry(field)
        require(fresh == target, 'same_current_field_and_layer_before_press')
        pressed = True
        qmp([{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
        qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
        pressed = False
        eventually(lambda: geometry(field)[0]['focus'] == field, 'native_field_focus_' + field)
        focused = field

    def capture(field, label):
        _, hit, target = geometry(field)
        layer = target['layer']
        x, y = round(layer['x'] + hit['x']), round(layer['y'] + hit['y'])
        width, height = round(hit['width']), round(hit['height'])
        data = session('grim', '-g', f'{x},{y} {width}x{height}', '-')
        private(directory / (label + '.png'), data)
        return hashlib.sha256(data).hexdigest()

    def clear(field):
        click(field)
        key(29, 30)
        key(14)
        eventually(lambda: inspect()['client'][field] == '' and status('')['matches'], 'cleared_' + field)

    previous_signal = signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(Failure('native_fixture_interrupted')))
    try:
        guard()
        outputs = json.loads(session('hyprctl', '-j', 'monitors'))
        require(len(outputs) == 1 and outputs[0]['width'] == 1920 and outputs[0]['height'] == 1080
                and outputs[0]['scale'] == 1 and outputs[0]['x'] == outputs[0]['y'] == 0
                and outputs[0].get('transform') == 0 and outputs[0].get('dpmsStatus') is True,
                'native_1920x1080_1x_origin_output_required')
        original_outputs = policy.output_state(outputs)
        original_workspace = policy.workspace_state(json.loads(session('hyprctl', '-j', 'activeworkspace')))
        cursor = policy.CursorOwnership(json.loads(session('hyprctl', '-j', 'cursorpos')))
        original = json.loads(session('python3', '-c', CHECKPOINT))
        private(directory / 'original-private.json', {'state': original, 'outputs': original_outputs,
                'workspace': original_workspace, 'cursor': cursor.initial, 'owned_units': [unit, peer_unit]})
        # These explicitly supplied paths are immutable release files/staged copies,
        # never an existing user's input-method executable or running process.
        session('python3', '-c', PROBE, args.example_bin, args.example_sha256, args.peer_bin, args.peer_sha256,
                '/home/omarchy/.local/bin/lucent-desktop', args.desktop_sha256)
        peer_log = (directory / 'peer-private.log').open('xb')
        os.chmod(directory / 'peer-private.log', 0o600)
        input_deadline = time.monotonic() + 170
        require(json.loads(session('python3', '-c', policy.ABSENT, json.dumps([unit, peer_unit])))['initial_units_absent'],
                'initial_owned_unit_names_absent')
        peer_attempted = True
        peer = subprocess.Popen(lab.ssh_args('lucent') + [shlex.join([
            'python3', 'desktop-lab/scripts/in-session.py', 'systemd-run', '--user', '--quiet', '--pipe', '--wait',
            '--property=Type=exec', '--property=RuntimeMaxSec=210s', '--property=TimeoutStopSec=10s',
            '--property=CPUQuota=100%', '--property=MemoryMax=256M', '--unit=' + peer_unit, '--', args.peer_bin])],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=peer_log, bufsize=0)
        require(not command(action='status', expected='').get('unavailable'), 'nonreplacing_peer_available')
        # A reply on this newly attached child stdio confirms its own launch;
        # a refused systemd-run cannot attach us to an existing service's pipes.
        peer_confirmed = True
        observe()
        example_attempted = True
        session('systemd-run', '--user', '--quiet', '--collect', '--property=Type=exec',
                '--property=RuntimeMaxSec=210s', '--property=TimeoutStopSec=10s',
                '--property=CPUQuota=100%', '--property=MemoryMax=768M', '--unit=' + unit, '--', args.example_bin)
        # A failed/lost start reply leaves this false: only captured successful
        # invocations are eligible for cleanup, never a matching preexisting unit.
        example_confirmed = True
        eventually(lambda: len(inspect()['surfaces']) == 1, 'owned_example_ready')
        guard()
        private(directory / 'owned-authority-private.json', authority)
        clear('single')
        baseline = capture('single', 'before-preedit')
        before_rectangle = focused_guard('')['rectangle']
        require(before_rectangle is not None and before_rectangle[2] > 0 and before_rectangle[3] > 0, 'initial_native_candidate_rectangle')
        send('', preedit='にほん', begin=9, end=9)
        def candidate_advanced():
            rectangle = status('')['rectangle']
            return rectangle is not None and rectangle[0] > before_rectangle[0] + before_rectangle[2] and rectangle[3] > 0
        eventually(candidate_advanced, 'native_preedit_updates_candidate_geometry')
        require(inspect()['client']['single'] == '', 'preedit_not_committed')
        report['preedit_capture'] = {'before_sha256': baseline, 'after_sha256': capture('single', 'preedit'),
              'scope': 'Candidate movement plus uncommitted canonical text is observed. Capture differences alone do not isolate preedit ink from caret blinking.'}
        report['checks']['native_preedit_candidate_moved_without_commit'] = True
        send('', preedit='', commit='日本語')
        eventually(lambda: inspect()['client']['single'] == '日本語' and status('日本語')['matches'], 'native_cjk_commit')
        send('日本語', commit='人', delete=[3, 0])
        eventually(lambda: inspect()['client']['single'] == '日本人' and status('日本人')['matches'], 'native_delete_commit_batch')
        key(29, 44)
        eventually(lambda: inspect()['client']['single'] == '日本語', 'native_ime_undo_is_atomic')
        key(29, 42, 44)
        eventually(lambda: inspect()['client']['single'] == '日本人', 'native_ime_redo_is_atomic')
        report['checks']['commit_and_surrounding_delete_and_history'] = True
        send('日本人', preedit='مرحبا')
        click('masked')
        eventually(lambda: not status('')['active'], 'masked_ime_disabled')
        require(inspect()['client']['single'] == '日本人', 'masked_focus_did_not_commit_preedit')
        require(not command(action='batch', expected='日本人', cursor=9, anchor=9, commit='invalid')['ok'], 'masked_peer_denied')
        report['checks']['masked_focus_retires_composition'] = True
        clear('body')
        body = policy.MULTILINE_TEXT
        send('', commit=body)
        eventually(lambda: inspect()['client']['body'] == body and status(body)['matches'], 'native_multiline_commit')
        rectangle = status(body)['rectangle']
        require(rectangle is not None and rectangle[2] > 0 and rectangle[3] > 0, 'native_cursor_rectangle')
        report['checks']['multiline_commit_and_cursor_rectangle'] = True
        clear('single')
        send('', commit='אבג')
        eventually(lambda: inspect()['client']['single'] == 'אבג', 'rtl_fixture_committed')
        key(102)
        eventually(lambda: inspect()['client']['single_caret'] == len('אבג'.encode()), 'rtl_visual_home')
        key(106)
        eventually(lambda: inspect()['client']['single_caret'] == len('אב'.encode()), 'rtl_visual_right')
        report['checks']['native_rtl_visual_navigation'] = True
        report['passed'] = True
    except (Failure, KeyError, ValueError, LookupError, OSError) as error:
        report['passed'] = False
        label = str(error)
        report['failure'] = label if re.fullmatch('[a-z_]+', label) else 'fixture_protocol_geometry_or_ownership_failed'
    finally:
        if pressed:
            try:
                qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
            except Failure:
                report['restoration']['pointer_released'] = False
        try:
            cleanup, uncertain = policy.captured_cleanup(configuration(),
                [name for name, attempted in (('example', example_attempted), ('peer', peer_attempted)) if attempted])
            session('python3', '-c', policy.RETIRE, json.dumps(cleanup))
            report['restoration']['owned_example_and_peer_units_retired'] = not uncertain
            if uncertain:
                report['unconfirmed_start_attempts_not_retired'] = uncertain
                report['unconfirmed_start_policy'] = 'No cleanup authority inferred after a refused/lost launch reply or missing captured lifetime. Any actually created unit remains bounded by its independent210second watchdog.'
        except (Failure, ValueError):
            report['restoration']['owned_example_and_peer_units_retired'] = False
        if peer is not None:
            try:
                peer.stdin.close()
                peer.wait(timeout=15)
                report['restoration']['owned_peer_transport_closed'] = True
            except (OSError, subprocess.TimeoutExpired):
                # This terminates only our local SSH transport; the exact guest
                # transient unit remains independently bounded by RuntimeMaxSec.
                peer.terminate()
                report['restoration']['owned_peer_transport_closed'] = False
        if peer_log is not None:
            peer_log.close()
        if original is not None:
            try:
                require(report['restoration'].get('owned_example_and_peer_units_retired'), 'owned_units_retired_before_restoration')
                guard(owned=False, restoring=True)
                report['restoration']['settings_and_wallpaper_preserved'] = json.loads(session('python3', '-c', CHECKPOINT)) == original
                report['restoration']['output_and_workspace_preserved'] = True  # exact guard above
                current = cursor.observe(json.loads(session('hyprctl', '-j', 'cursorpos')))
                if current != cursor.initial:
                    move(cursor.initial, restoring=True)
                report['restoration']['pointer_restored'] = cursor.observe(json.loads(session('hyprctl', '-j', 'cursorpos'))) == cursor.initial
            except (Failure, ValueError):
                report['restoration']['guarded_state_and_pointer_verification'] = False
        signal.signal(signal.SIGTERM, previous_signal)
        report['passed'] = report['passed'] and all(report['restoration'].values())
        private(directory / 'report.json', report)
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
