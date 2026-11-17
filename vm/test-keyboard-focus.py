#!/usr/bin/env python3
"""Exercise the independent focus client with real pointer and keyboard input.

The existing unlocked 1920x1080/1x Lucent guest must have no application windows
or open shell panels. Only a new temporary example unit is started. Test text
belongs to that example; desktop preferences, clipboard and wallpapers are not
edited. The original unit identity, desktop state and pointer are checked on exit.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import runpy
import shlex
import subprocess
import time
import uuid

from application_intent_policy import CursorOwnership
from owned_application_units import UNIT_IDENTITY
from preservation_fingerprint import fingerprint

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('framework_test', ROOT / 'vm/test-framework.py')
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)

GUEST = 'if not __debug__: raise SystemExit("assertions_required")\n' + UNIT_IDENTITY + r'''
import shutil,socket,struct,subprocess,sys
operation,folder,unit,expected=sys.argv[1:]
stage=Path(folder)
assert stage.parent==Path.home()/'.cache' and re.fullmatch('lucent-focus-test-[0-9a-f]{24}',stage.name)
assert stage.resolve(strict=True)==stage and stage.stat().st_uid==os.getuid()
assert stat.S_IMODE(stage.stat().st_mode)==0o700
binary=stage/'keyboard-focus';program=stage/'guest.py'
argv=['/usr/bin/python3','-I','-B',str(program),'start',folder,unit,expected]
def origin():
 path=stage/'origin.json';assert path.stat().st_uid==os.getuid() and path.stat().st_size<=131072
 return json.loads(path.read_text())
if operation=='start':
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==expected
 captured=capture_unit_origin(unit,argv)
 with (stage/'origin.json').open('x') as out:json.dump(captured,out)
 os.execv(str(binary),[str(binary)])
elif operation=='inspect':
 captured=origin();current=assert_unit_origin(unit_snapshot(unit),unit,argv,captured,hashed=True)
 assert current is not None and current['sha256']==expected
 path=Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-keyboard-focus.sock'
 before=path.lstat();assert stat.S_ISSOCK(before.st_mode) and before.st_uid==os.getuid()
 with socket.socket(socket.AF_UNIX) as connection:
  connection.settimeout(5);connection.connect(str(path))
  pid,uid,_=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
  assert pid==current['pid'] and uid==os.getuid()
  connection.sendall(b'inspect\n');data=bytearray()
  while True:
   chunk=connection.recv(65536)
   if not chunk:break
   data.extend(chunk);assert len(data)<=8*1024*1024
 assert path.lstat()==before
 assert assert_unit_origin(unit_snapshot(unit),unit,argv,captured,hashed=True)==current
 print(data.decode())
elif operation=='stop':
 captured=origin()
 current=assert_unit_origin(unit_snapshot(unit),unit,argv,captured,hashed=True)
 graceful=False
 if current is not None:
  assert current['sha256']==expected
  path=Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-keyboard-focus.sock'
  with socket.socket(socket.AF_UNIX) as connection:
   connection.settimeout(5);connection.connect(str(path))
   pid,uid,_=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
   assert pid==current['pid'] and uid==os.getuid()
   connection.sendall(b'quit\n');reply=bytearray()
   while True:
    chunk=connection.recv(65536)
    if not chunk:break
    reply.extend(chunk);assert len(reply)<=131072
   assert bytes(reply).strip()==b'ok'
  deadline=time.monotonic()+10
  while not _retirement_drained(captured) and time.monotonic()<deadline:time.sleep(.1)
  graceful=_retirement_drained(captured)
 retire_original_unit(unit,argv,captured)
 assert _retirement_drained(captured)
 assert not (Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-keyboard-focus.sock').exists()
 print(json.dumps({'original_unit_drained':True,'graceful_exit_and_socket_cleanup':graceful}))
elif operation=='remove':
 captured=origin();assert _retirement_drained(captured)
 assert set(path.name for path in stage.iterdir())=={'guest.py','keyboard-focus','origin.json'}
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==expected
 shutil.rmtree(stage)
 print(json.dumps({'owned_stage_removed':True}))
else:raise ValueError('unknown_operation')
'''


def wait(predicate, label, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (subprocess.CalledProcessError, KeyError, ValueError):
            pass
        time.sleep(.1)
    raise AssertionError(label)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    assert binary.is_file() and binary.stat().st_size <= 128 * 1024 * 1024
    assert v.session('python3', '-c',
        'import subprocess;r=subprocess.run(["omarchy-hyprland-session-locked"],capture_output=True);'
        'assert r.returncode==1;print("unlocked")').strip() == 'unlocked'
    monitors = json.loads(v.session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1
    monitor = monitors[0]
    assert all(monitor[key] == value for key, value in {
        'width': 1920, 'height': 1080, 'x': 0, 'y': 0, 'scale': 1, 'transform': 0}.items())
    assert json.loads(v.session('hyprctl', '-j', 'clients')) == []
    desktop = v.inspect()
    assert not desktop['client']['launcher'] and not desktop['client']['notification_history_open']
    assert not any(surface['keyboard'] == 'exclusive' for surface in desktop['surfaces'])
    assert v.remote('python3', '-c', '''import errno,os,socket,stat
from pathlib import Path
path=Path('/run/user')/str(os.getuid())/'lucent-keyboard-focus.sock'
try:before=path.lstat()
except FileNotFoundError:before=None
if before is not None:
 assert stat.S_ISSOCK(before.st_mode) and before.st_uid==os.getuid()
 with socket.socket(socket.AF_UNIX) as connection:
  connection.settimeout(1);assert connection.connect_ex(str(path))==errno.ECONNREFUSED
 assert path.lstat()==before
print('available')''').strip() == 'available'
    original = fingerprint(v.session)
    cursor = CursorOwnership(json.loads(v.session('hyprctl', '-j', 'cursorpos')))
    tag = uuid.uuid4().hex[:24]
    unit = 'lucent-focus-test-' + tag + '.service'
    directory = v.remote('python3', '-c',
        'import sys;from pathlib import Path;p=Path.home()/".cache"/sys.argv[1];p.mkdir(mode=0o700);print(p)',
        'lucent-focus-test-' + tag).strip()
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    checks = {}
    started = False
    launcher_opened = False
    completed = False
    cleanup_complete = False
    captures = []
    failure_class = None

    def guest(operation):
        command = ['python3', 'desktop-lab/scripts/in-session.py', '/usr/bin/python3', '-I', '-B',
                   directory + '/guest.py', operation, directory, unit, digest]
        result = subprocess.run(v.lab.ssh_args('lucent') + [shlex.join(command)],
                                capture_output=True, timeout=25, check=True)
        assert len(result.stdout) <= 8 * 1024 * 1024 and not result.stderr
        return json.loads(result.stdout)

    def inspect():
        return guest('inspect')

    def cursor_position():
        return json.loads(v.session('hyprctl', '-j', 'cursorpos'))

    def move(point):
        point = tuple(round(value) for value in point)
        cursor.begin(cursor_position(), point)
        v.move(*point)
        wait(lambda: tuple(cursor_position()[key] for key in ('x', 'y')) == point,
             'native_pointer_not_at_target')
        cursor.finish(cursor_position())

    def click(identifier):
        state = inspect()
        surface = state['surfaces'][0]
        hit = next(hit for hit in surface['hits'] if hit['id'] == identifier)
        layers = json.loads(v.session('hyprctl', '-j', 'layers'))
        candidates = [layer for output in layers.values() for level in output['levels'].values()
                      for layer in level if layer['namespace'] == 'lucent-keyboard-focus-editor']
        assert len(candidates) == 1
        layer = candidates[0]
        left, top = max(hit['x'], hit['clip']['x']), max(hit['y'], hit['clip']['y'])
        right = min(hit['x'] + hit['width'], hit['clip']['x'] + hit['clip']['width'])
        bottom = min(hit['y'] + hit['height'], hit['clip']['y'] + hit['clip']['height'])
        assert right > left and bottom > top
        move((layer['x'] + (left + right) / 2, layer['y'] + (top + bottom) / 2))
        v.button(True)
        v.button(False)

    def focused():
        state = inspect()
        return (state['client']['retained'] and state['surfaces'][0]['actual_native_keyboard_focused']
                and state['surfaces'][0]['committed_keyboard'] == 'on-demand')

    def capture(label):
        result = subprocess.run(v.lab.ssh_args('lucent') + [shlex.join(
            ['python3', 'desktop-lab/scripts/in-session.py', 'grim', '-'])],
            capture_output=True, timeout=20, check=True)
        assert result.stdout.startswith(b'\x89PNG\r\n\x1a\n') and len(result.stdout) <= 16 * 1024 * 1024
        path = v.REPORT / ('keyboard-focus-' + tag + '-' + label + '.png')
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as output:
            output.write(result.stdout)
        captures.append(str(path.relative_to(ROOT)))

    try:
        for filename, data in [('keyboard-focus', binary.read_bytes()), ('guest.py', GUEST.encode())]:
            command = 'cat > ' + shlex.quote(directory + '/' + filename)
            subprocess.run(v.lab.ssh_args('lucent') + [command], input=data, check=True)
        v.remote('chmod', '700', directory + '/keyboard-focus')
        v.session('systemd-run', '--user', '--quiet', '--collect', '--service-type=exec', '--unit', unit,
                  '--property=RuntimeMaxSec=180', '--property=KillMode=control-group',
                  '/usr/bin/python3', '-I', '-B', directory + '/guest.py', 'start', directory, unit, digest)
        started = True
        wait(lambda: len(inspect()['surfaces']) == 1, 'example_did_not_map', 30)
        initial = inspect()
        assert initial['client']['values'] == ['', ''] and not initial['client']['requested']
        assert initial['surfaces'][0]['keyboard'] == 'none'
        assert not initial['surfaces'][0]['actual_native_keyboard_focused']
        checks['starts_passive_on_bottom_layer'] = True
        capture('passive')

        click('first')
        wait(focused, 'first_field_did_not_retain_keyboard')
        v.text('alpha')
        wait(lambda: inspect()['client']['values'] == ['alpha', ''], 'first_field_native_text')
        v.key(15)
        wait(lambda: inspect()['surfaces'][0]['focus'] == 'second', 'tab_did_not_select_second_field')
        v.text('beta')
        wait(lambda: inspect()['client']['values'] == ['alpha', 'beta'], 'second_field_native_text')
        checks['physical_press_text_and_tab'] = True
        capture('focused')

        v.key(1)
        wait(lambda: not inspect()['client']['requested']
             and inspect()['surfaces'][0]['committed_keyboard'] == 'none', 'escape_did_not_release')
        assert inspect()['client']['values'] == ['alpha', 'beta']
        checks['escape_releases_and_retains_text'] = True
        capture('released')
        click('second')
        wait(focused, 'second_field_reacquisition_failed')
        checks['reacquires_after_release'] = True

        v.key(125, 57)
        launcher_opened = True
        wait(lambda: v.inspect()['client']['launcher'], 'native_launcher_shortcut_failed')
        wait(lambda: not inspect()['client']['requested'], 'foreground_did_not_retire_demand')
        v.key(1)
        wait(lambda: not v.inspect()['client']['launcher'], 'launcher_escape_failed')
        launcher_opened = False
        assert not inspect()['client']['requested']
        assert inspect()['client']['values'] == ['alpha', 'beta']
        checks['foreground_focus_loss_does_not_reacquire'] = True
        click('first')
        wait(focused, 'reacquisition_after_foreground_failed')
        click('release')
        wait(lambda: not inspect()['client']['requested'], 'release_button_failed')
        checks['explicit_release_button'] = True
        completed = True
    except BaseException as error:
        failure_class = type(error).__name__
        raise
    finally:
        cleanup_errors = []
        try:
            if launcher_opened:
                v.key(1)
        except BaseException as error:
            cleanup_errors.append(error)
        if started:
            for operation in ('stop', 'remove'):
                try:
                    checks.update(guest(operation))
                except BaseException as error:
                    cleanup_errors.append(error)
        try:
            cursor.observe(cursor_position())
            move(cursor.initial)
            assert fingerprint(v.session) == original
            assert json.loads(v.session('hyprctl', '-j', 'clients')) == []
            checks['desktop_settings_wallpaper_and_cursor_preserved'] = True
            cleanup_complete = not cleanup_errors
        except BaseException as error:
            cleanup_errors.append(error)
        finally:
            report = {'source': 'independent keyboard-focus example', 'binary_sha256': digest,
                      'harness_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                      'example_source_sha256': hashlib.sha256(
                          (ROOT / 'lucent/examples/keyboard-focus/src/main.rs').read_bytes()).hexdigest(),
                      'private_captures': captures,
                      'initial_cursor': list(cursor.initial),
                      'failure_class': failure_class,
                      'cleanup_error_classes': [type(error).__name__ for error in cleanup_errors],
                      'checks': checks, 'passed': completed and cleanup_complete and all(checks.values()),
                      'scope': 'One virtual 1920x1080/1x output, real QMP input; no hardware or pacing claim.'}
            report_path = v.REPORT / ('keyboard-focus-' + tag + '.json')
            with os.fdopen(os.open(report_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as output:
                json.dump(report, output, indent=2)
                output.write('\n')
            print(json.dumps(report))
        if cleanup_errors and failure_class is None:
            raise AssertionError('example_cleanup_failed') from cleanup_errors[0]


if __name__ == '__main__':
    measurement = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
    with measurement['stay_awake'](v.remote):
        main()
