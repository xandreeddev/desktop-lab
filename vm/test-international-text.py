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
for name,raw,expected in zip(('example','peer'),sys.argv[1::2],sys.argv[2::2]):
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
INSPECT = r'''
import json,os,socket
from pathlib import Path
path=Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-international-text.sock'
s=socket.socket(socket.AF_UNIX);s.settimeout(3);s.connect(str(path));s.sendall(b'inspect\n');data=bytearray()
while True:
 chunk=s.recv(65536)
 if not chunk:break
 data+=chunk;assert len(data)<=4*1024*1024
print(data.decode())
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--example-bin', required=True)
    parser.add_argument('--peer-bin', required=True)
    parser.add_argument('--example-sha256', required=True)
    parser.add_argument('--peer-sha256', required=True)
    args = parser.parse_args()
    require(re.fullmatch('[a-f0-9]{40}', args.source_commit), 'full_source_commit_required')
    for digest in (args.example_sha256, args.peer_sha256):
        require(re.fullmatch('[a-f0-9]{64}', digest), 'full_binary_digest_required')
    # Worktrees share the prepared VM inventory with their canonical checkout.
    common = Path(run(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute',
                       '--git-common-dir']).decode().strip()).resolve(strict=True)
    lab_path = common.parent / 'vm/lab.py'
    require(lab_path.is_file(), 'canonical_lab_helper_required')
    spec = importlib.util.spec_from_file_location('international_text_lab', lab_path)
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
    peer = None
    unit_started = False
    pressed = False
    original_cursor = last_cursor = original = monitor = None
    report = {'passed': False, 'source_commit': args.source_commit, 'checks': {}, 'restoration': {},
              'helper_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'binaries': {'example': args.example_sha256, 'peer': args.peer_sha256},
              'scope': 'One prepared native output; independent controlled input-method peer, no third-party IME/hardware claim'}

    def remote(*argv, data=None):
        return run(lab.ssh_args('lucent') + [shlex.join(argv)], data=data)

    def session(*argv, data=None):
        return remote('python3', 'desktop-lab/scripts/in-session.py', *argv, data=data)

    def guard():
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == b'false', 'unlocked_guest_required')
        require(json.loads(session('hyprctl', '-j', 'clients')) == [], 'idle_guest_without_ordinary_apps_required')
        require(b'lucent-authorization-' not in session('hyprctl', '-j', 'layers'), 'secure_surface_absent_required')

    def inspect():
        return json.loads(session('python3', '-c', INSPECT))

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

    sequence = 0

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
            require(event.get('event') != 'unavailable', 'existing_input_method_preserved_refusal')
            if event.get('id') == sequence:
                return event
        raise Failure('native_peer_response_deadline')

    def status(expected):
        return command(action='status', expected=expected)

    def send(expected, **changes):
        guard()
        observed = status(expected)
        require(observed['active'] and observed['matches'], 'owned_current_fixture_text_required')
        response = command(action='batch', expected=expected, cursor=observed['cursor'], anchor=observed['anchor'], **changes)
        require(response.get('posted'), 'native_peer_batch_posted')

    def key(*codes):
        guard()
        run(['virsh', '-c', lab.URI, 'send-key', name, '--codeset', 'linux', '--holdtime', '25', *map(str, codes)])
        time.sleep(.1)

    def qmp(events):
        releasing = all(event['type'] == 'btn' and event['data']['down'] is False for event in events)
        if not releasing:
            guard()
        value = json.loads(run(['virsh', '-c', lab.URI, 'qemu-monitor-command', name,
                               json.dumps({'execute': 'input-send-event', 'arguments': {'events': events}})]))
        require('error' not in value, 'native_pointer_delivery')

    def geometry(field):
        state = inspect()
        surface = next(s for s in state['surfaces'] if s.get('role') == 'international')
        hit = next(h for h in surface['hits'] if h['id'] == field)
        layers = json.loads(session('hyprctl', '-j', 'layers'))
        found = []
        def visit(value):
            if isinstance(value, dict):
                if value.get('namespace') == 'lucent-international':
                    found.append(value)
                for child in value.values():
                    visit(child)
            elif isinstance(value, list):
                for child in value:
                    visit(child)
        visit(layers)
        require(len(found) == 1, 'one_owned_native_layer')
        return surface, hit, found[0]

    def click(field):
        nonlocal pressed, last_cursor
        _, hit, layer = geometry(field)
        x = layer['x'] + hit['x'] + hit['width'] / 2
        y = layer['y'] + hit['y'] + hit['height'] / 2
        qmp([{'type': 'abs', 'data': {'axis': axis, 'value': round(position / (extent - 1) * 32767)}}
             for axis, position, extent in [('x', x, monitor['width']), ('y', y, monitor['height'])]])
        time.sleep(.2)
        last_cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
        pressed = True
        qmp([{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
        qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
        pressed = False
        eventually(lambda: geometry(field)[0]['focus'] == field, 'native_field_focus_' + field)

    def capture(field, label):
        _, hit, layer = geometry(field)
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

    try:
        guard()
        outputs = json.loads(session('hyprctl', '-j', 'monitors'))
        require(len(outputs) == 1, 'one_output_required')
        monitor = outputs[0]
        require(monitor['scale'] == 1 and monitor['x'] == monitor['y'] == 0, 'native_1x_origin_output_required')
        original_cursor = json.loads(session('hyprctl', '-j', 'cursorpos'))
        original = json.loads(session('python3', '-c', CHECKPOINT))
        private(directory / 'original-private.json', {'state': original, 'monitor': monitor, 'cursor': original_cursor})
        session('python3', '-c', PROBE, args.example_bin, args.example_sha256, args.peer_bin, args.peer_sha256)
        peer = subprocess.Popen(lab.ssh_args('lucent') + [shlex.join(['python3', 'desktop-lab/scripts/in-session.py', args.peer_bin])], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        session('systemd-run', '--user', '--unit', unit, '--collect', '--no-block', '--property', 'CPUQuota=100%', '--property', 'MemoryMax=768M', '--', args.example_bin)
        unit_started = True
        eventually(lambda: len(inspect()['surfaces']) == 1, 'owned_example_ready')
        clear('single')
        baseline = capture('single', 'before-preedit')
        send('', preedit='にほん', begin=9, end=9)
        time.sleep(.25)
        require(inspect()['client']['single'] == '', 'preedit_not_committed')
        require(capture('single', 'preedit') != baseline, 'native_preedit_pixels_changed')
        report['checks']['preedit_transient_and_painted'] = True
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
        body = 'سلام\nक्षि\n日本語'
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
        report['failure'] = str(error) if isinstance(error, Failure) else 'fixture_protocol_or_geometry_failed'
    finally:
        if pressed:
            try:
                qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
            except Failure:
                report['restoration']['pointer_released'] = False
        if peer is not None:
            try:
                if peer.poll() is None:
                    command(action='quit')
                peer.wait(timeout=5)
                report['restoration']['owned_peer_exited'] = peer.returncode == 0
            except (Failure, subprocess.TimeoutExpired):
                peer.terminate()
                report['restoration']['owned_peer_exited'] = False
        if unit_started:
            try:
                # The random owned unit is never reused and only this binary was launched.
                observed = session('systemctl', '--user', 'show', unit, '-p', 'ExecStart', '--value').decode()
                require(args.example_bin in observed, 'owned_unit_exec_identity_required')
                session('systemctl', '--user', 'stop', unit)
                report['restoration']['owned_example_stopped'] = True
            except Failure:
                report['restoration']['owned_example_stopped'] = False
        if original is not None:
            try:
                report['restoration']['settings_and_wallpaper_preserved'] = json.loads(session('python3', '-c', CHECKPOINT)) == original
                after = json.loads(session('hyprctl', '-j', 'monitors'))
                report['restoration']['output_and_workspace_preserved'] = len(after) == 1 and all(after[0].get(k) == monitor.get(k) for k in ('name', 'width', 'height', 'scale', 'x', 'y', 'activeWorkspace', 'specialWorkspace'))
                if original_cursor is not None:
                    current = json.loads(session('hyprctl', '-j', 'cursorpos'))
                    require(last_cursor is None or current == last_cursor, 'concurrent_pointer_change_preserved')
                    session('hyprctl', 'dispatch', 'movecursor', str(original_cursor['x']), str(original_cursor['y']))
                    report['restoration']['pointer_restored'] = json.loads(session('hyprctl', '-j', 'cursorpos')) == original_cursor
            except Failure:
                report['restoration']['state_verified'] = False
        report['passed'] = report['passed'] and all(report['restoration'].values())
        private(directory / 'report.json', report)
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
