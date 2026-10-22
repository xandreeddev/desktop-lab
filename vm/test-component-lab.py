#!/usr/bin/env python3
"""Opt-in native component-lab acceptance; import/default invocation is inert.

Requires the prepared, unlocked, idle Lucent guest at 1280x800/1x. Only a new
private example binary/unit and native pointer/keyboard input are used. Normal
shell, compositor, settings and wallpaper must remain unchanged. Observed
events/focus/submission counters do not prove pixel fidelity, values or FPS.
Do not use the guest's pointer or keyboard concurrently with this run.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import shlex
import signal
import stat
import subprocess
import time

from owned_application_units import UNIT_IDENTITY

ROOT = Path(__file__).resolve().parents[1]
STORIES = ('buttons', 'input', 'input-group', 'toggle', 'slider', 'scroll', 'popover', 'menu', 'motion')
ENTRY = r'''import hashlib,os,stat,sys
path=sys.argv[1];fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
with os.fdopen(fd,'rb') as stream:
 before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_size<=131072
 raw=stream.read(131073);assert len(raw)==before.st_size and hashlib.sha256(raw).hexdigest()==sys.argv[2]
 after=os.fstat(stream.fileno());assert (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)==(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns)
sys.argv=[path]+sys.argv[3:];exec(compile(raw,path,'exec'),{'__name__':'__main__','__file__':path})
'''


class Failure(RuntimeError):
    pass


def need(value, label):
    if not value:
        raise Failure(label)


def identity(value):
    return (value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns)


def private(path, value):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as out:
        json.dump(value, out, indent=2)
        out.write('\n')


def visible_point(hit, origin):
    """Use actual scene clip and compositor placement, never guessed centers."""
    x, y = max(hit['x'], hit['clip']['x']), max(hit['y'], hit['clip']['y'])
    right = min(hit['x'] + hit['width'], hit['clip']['x'] + hit['clip']['width'])
    bottom = min(hit['y'] + hit['height'], hit['clip']['y'] + hit['clip']['height'])
    need(all(math.isfinite(v) for v in (x, y, right, bottom)) and right - x > 4 and bottom - y > 4,
         'native_target_is_clipped')
    point = (round(origin['x'] + (x + right) / 2), round(origin['y'] + (y + bottom) / 2))
    need(0 <= point[0] < 1280 and 0 <= point[1] < 800, 'native_target_outside_output')
    return point


# The existing shared unit helper is copied into this private guest program.
# Its origin receipt binds the Python wrapper before exec and the actual Lab
# PID/birth/cgroup afterward; retirement checks the whole original unit.
GUEST = 'if not __debug__: raise SystemExit("assertions_required")\n' + UNIT_IDENTITY + r'''
import socket,struct,sys
stage=Path(sys.argv[2]);unit=sys.argv[3];expected=sys.argv[4]
assert stage.parent==Path.home()/'.cache' and re.fullmatch('lucent-component-lab-[0-9a-f]{24}',stage.name)
assert stage.resolve()==stage and stat.S_ISDIR(stage.lstat().st_mode) and stage.stat().st_uid==os.getuid()
assert stat.S_IMODE(stage.stat().st_mode)==0o700
argv=['/usr/bin/python3','-I','-B',str(stage/'guest.py'),'serve',str(stage),unit,expected]
def run(*args):
 import subprocess
 result=subprocess.run(args,capture_output=True,text=True,timeout=6)
 assert result.returncode==0 and len(result.stdout)<=8*1024*1024,'guest_command_failed'
 return result.stdout.strip()
def hypr(name):return json.loads(run('hyprctl','-j',name))
def file(path):
 path=Path(path);before=path.lstat()
 facts=[before.st_dev,before.st_ino,before.st_mode,before.st_uid,before.st_size,before.st_mtime_ns,before.st_ctime_ns]
 def identity(value):return [value.st_dev,value.st_ino,value.st_mode,value.st_uid,value.st_size,value.st_mtime_ns,value.st_ctime_ns]
 if stat.S_ISLNK(before.st_mode):value={'link':os.readlink(path)}
 else:
  fd=os.open(path,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW)
  with os.fdopen(fd,'rb') as stream:
   assert stat.S_ISREG(before.st_mode) and before.st_size<=128*1024*1024 and before.st_uid==os.getuid()
   assert identity(os.fstat(stream.fileno()))==facts
   value={'sha256':hashlib.file_digest(stream,'sha256').hexdigest()}
   assert identity(os.fstat(stream.fileno()))==facts
 assert identity(path.lstat())==facts
 return {'identity':facts,**value}
def peer(name):
 path=Path(os.environ['XDG_RUNTIME_DIR'])/(name+'.sock');info=path.lstat()
 assert stat.S_ISSOCK(info.st_mode) and info.st_uid==os.getuid()
 with socket.socket(socket.AF_UNIX) as connection:
  connection.settimeout(5);connection.connect(str(path))
  pid,uid,_=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
  assert uid==os.getuid()
  process=process_snapshot(pid,hashed=True)
  connection.sendall(b'inspect\n');data=bytearray()
  while True:
   chunk=connection.recv(65536)
   if not chunk:break
   data.extend(chunk);assert len(data)<=8*1024*1024
 assert path.lstat()==info
 return process,json.loads(data)
def layers():
 result=[]
 def visit(value):
  if isinstance(value,dict):
   if 'namespace' in value:result.append(value)
   for child in value.values():visit(child)
  elif isinstance(value,list):
   for child in value:visit(child)
 visit(hypr('layers'));return result
def baseline(allow_lab=False):
 assert hypr('locked')=={'locked':False} and run('omarchy-shell','lock','isLocked')=='false'
 assert not hypr('clients') and not hypr('activewindow'),'idle_guest_without_app_windows_required'
 shell,scene=peer('lucent');client=scene['client']
 assert not scene['frame_profiling'] and not scene['presentation_feedback']['enabled']
 assert not scene.get('frame_pacing',{}).get('enabled',False)
 assert client['settings_save_status']=='saved' and not client['settings_save_error']
 assert not any(client.get(key) for key in ('launcher','launcher_opening','dock_surface_expanded','notification_history_open','widgets_editing','widget_grid','wallpaper_busy'))
 for name in ('audio','media','network','bluetooth','power','tray','weather','live_wallpaper','displays','application_actions'):
  assert not any(client.get(name,{}).get(key) for key in ('open','visible','busy')),'shell_panel_not_idle'
 assert all(not host.get('editing') and not host.get('grid_visible') for host in client.get('monitor_hosts',[]))
 monitors=hypr('monitors');assert len(monitors)==1
 m=monitors[0];assert (m['width'],m['height'],m['x'],m['y'],m['scale'],m['transform'])==(1280,800,0,0,1,0)
 signature=os.environ['HYPRLAND_INSTANCE_SIGNATURE']
 instances=[entry for entry in hypr('instances') if entry['instance']==signature];assert len(instances)==1
 compositor=process_snapshot(instances[0]['pid'])
 native=layers();lab=[entry for entry in native if entry['namespace']=='lucent-lab-lab']
 if lab:
  assert allow_lab and len(lab)==1
  _,process=owned();assert lab[0]['pid']==process['pid']
 assert all(entry in lab or (entry['namespace'].startswith('lucent-') and entry['pid']==shell['pid']) for entry in native),'foreign_native_layer'
 return {'shell':shell,'compositor':compositor,'monitors':monitors,'workspace':hypr('activeworkspace'),
         'focus':hypr('activewindow'),'settings':file(Path.home()/'.local/state/lucent/desktop.json'),
         'wallpaper':file(Path.home()/'.local/state/lucent/wallpaper')}
def owned():
 origin=json.loads((stage/'origin.json').read_text());current=unit_snapshot(unit)
 process=assert_unit_origin(current,unit,argv,origin,hashed=True)
 assert process is not None and process['sha256']==expected
 info=(stage/'lucent-lab').stat()
 assert process['executable']==[info.st_dev,info.st_ino,info.st_mode,info.st_uid,info.st_size,info.st_mtime_ns,info.st_ctime_ns]
 return origin,process
op=sys.argv[1]
if op=='serve':
 origin=capture_unit_origin(unit,argv)
 fd=os.open(stage/'origin.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'w') as stream:json.dump(origin,stream)
 os.execv(stage/'lucent-lab',[str(stage/'lucent-lab')])
elif op=='before':
 runtime=Path(os.environ['XDG_RUNTIME_DIR'])
 assert not (runtime/'lucent-lab.sock').exists()
 assert not any((p/'comm').read_text().strip()=='lucent-lab' for p in Path('/proc').iterdir() if p.name.isdigit() and (p/'comm').exists())
 assert unit_snapshot(unit)['load']=='not-found'
 print(json.dumps(baseline()))
elif op=='guard':
 print(json.dumps(baseline(allow_lab=True)))
elif op=='probe':
 origin,process=owned();native=layers()
 own=[entry for entry in native if entry['namespace']=='lucent-lab-lab'];assert len(own)==1 and own[0]['pid']==process['pid']
 desktop,_=peer('lucent')
 assert all(entry in own or (entry['namespace'].startswith('lucent-') and entry['pid']==desktop['pid']) for entry in native)
 peer_process,scene=peer('lucent-lab');assert peer_process==process
 assert len(scene['surfaces'])==1 and scene['surfaces'][0]['id']=='lab' and scene['surfaces'][0]['frames']>0
 print(json.dumps({'process':process,'unit':origin['unit'],'scene':scene,'layer':own[0]}))
elif op=='retire':
 origin=json.loads((stage/'origin.json').read_text())
 retire_original_unit(unit,argv,origin);assert _retirement_drained(origin)
 print(json.dumps({'retired':True}))
elif op=='files':
 print(json.dumps({p.name:file(p) for p in stage.iterdir()}))
elif op=='clean':
 expected_files=json.load(sys.stdin)
 current=unit_snapshot(unit)
 if (stage/'origin.json').exists():
  origin=json.loads((stage/'origin.json').read_text());assert _retirement_drained(origin)
  assert current['load']=='not-found' or current['active'] in ('inactive','failed')
 else:assert current['load']=='not-found'
 assert set(p.name for p in stage.iterdir())==set(expected_files)
 for name,expected_file in expected_files.items():assert name in ('guest.py','lucent-lab','origin.json') and file(stage/name)==expected_file
 for name in expected_files:
  assert file(stage/name)==expected_files[name];(stage/name).unlink()
 stage.rmdir();print('true')
else:raise SystemExit('unknown_operation')
'''
class Trial:
    def __init__(self, args, directory, tag):
        import lab
        lab.ROOT, lab.STATE = args.lab_root, args.lab_root / '.local-vms'
        inventory = lab.inventory()
        need(inventory['uri'] == 'qemu:///session' and inventory['user'] == 'omarchy' and inventory['vms']['lucent'] ==
             {'name': 'desktop-lab-lucent', 'port': 2243}, 'prepared_lucent_inventory_required')
        self.args, self.directory, self.tag = args, directory, tag
        self.name, self.unit = 'desktop-lab-lucent', 'lucent-component-lab-' + tag + '.service'
        self.ssh = [value.replace('StrictHostKeyChecking=accept-new', 'StrictHostKeyChecking=yes')
                    for value in lab.ssh_args('lucent')]
        self.stage, self.base, self.pointer, self.last = None, None, None, None
        self.down, self.keys, self.started = False, (), False
        need(self.virsh('domuuid', self.name) == args.vm_uuid and
             self.virsh('domstate', self.name) == 'running', 'exact_running_vm_required')

    def command(self, argv, data=None, timeout=25):
        result = subprocess.run(argv, input=data, capture_output=True, timeout=timeout)
        need(result.returncode == 0 and len(result.stdout) <= 8 * 1024 * 1024, 'bounded_native_command_failed')
        return result.stdout.decode().strip()

    def virsh(self, *args):
        return self.command(['virsh', '-c', 'qemu:///session', *args], timeout=8)

    def session(self, *args, data=None, timeout=25):
        return self.command(self.ssh + [shlex.join(['python3', '-I', '-B', 'desktop-lab/scripts/in-session.py', *args])], data, timeout)

    def guest(self, operation, data=None):
        return json.loads(self.session('/usr/bin/python3', '-I', '-B', '-c', ENTRY,
                                       self.stage + '/guest.py', hashlib.sha256(GUEST.encode()).hexdigest(),
                                       operation, self.stage, self.unit, self.args.binary_sha256, data=data))

    def cursor(self):
        return json.loads(self.session('hyprctl', '-j', 'cursorpos'))

    def guard(self):
        need(self.guest('guard') == self.base, 'normal_desktop_or_settings_changed')
        need(self.cursor() == self.last, 'independent_pointer_motion')

    def probe(self):
        return self.guest('probe')

    def wait(self, predicate, label, timeout=15, startup=False):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                state = self.probe()
            except Failure:
                if not startup:
                    raise
                time.sleep(.12)
                continue
            if predicate(state):
                return state
            time.sleep(.12)
        raise Failure(label)

    def qmp(self, events):
        reply = json.loads(self.virsh('qemu-monitor-command', self.name, json.dumps({
            'execute': 'input-send-event', 'arguments': {'events': events}})))
        need('return' in reply and 'error' not in reply, 'qmp_input_rejected')

    def move(self, point):
        self.guard()
        self.qmp([{'type': 'abs', 'data': {'axis': axis, 'value': round(pos / (extent - 1) * 32767)}}
                  for axis, pos, extent in [('x', point[0], 1280), ('y', point[1], 800)]])
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            current = self.cursor()
            if all(abs(current[k] - value) <= 1 for k, value in zip(('x', 'y'), point)):
                self.last = current
                return
            need(current == self.last, 'independent_pointer_during_motion')
            time.sleep(.05)
        raise Failure('native_pointer_not_observed')

    def click(self, identifier):
        self.guard()
        previous, deadline = None, time.monotonic() + 15
        while time.monotonic() < deadline:
            state = self.probe()
            hits = [hit for hit in state['scene']['surfaces'][0]['hits'] if hit['id'] == identifier]
            if len(hits) == 1:
                point = visible_point(hits[0], state['layer'])
                if point == previous:
                    break
                previous = point
            time.sleep(.12)
        else:
            raise Failure('stable_native_control_missing')
        self.move(point)
        self.probe(); self.guard()
        self.down = True
        try:
            self.qmp([{'type': 'btn', 'data': {'down': True, 'button': 'left'}}])
        finally:
            self.qmp([{'type': 'btn', 'data': {'down': False, 'button': 'left'}}]); self.down = False

    def key(self, *codes):
        self.probe(); self.guard(); self.keys = codes
        self.virsh('send-key', self.name, '--codeset', 'linux', '--holdtime', '50', *map(str, codes))
        self.keys = ()  # send-key has completed its bounded automatic release.

    def event(self, value):
        return self.wait(lambda state: value in state['scene']['client']['events'], 'fixture_event_not_observed')

    def story(self, name):
        self.click('story-' + name)
        return self.wait(lambda state: state['scene']['client']['story'] == name, 'story_not_selected')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--binary-sha256')
    parser.add_argument('--vm-uuid')
    parser.add_argument('--lab-root', type=Path, default=ROOT)
    args = parser.parse_args()
    if not args.run:
        print(json.dumps({'plan_only': True, 'guest_actions': False, 'stories': STORIES,
                          'requires': ['explicit --run', 'absolute binary and SHA256', 'VM UUID', 'idle unlocked 1280x800 guest']}))
        return 0
    need(args.binary is not None and args.binary.is_absolute() and args.binary.resolve() == args.binary,
         'absolute_regular_binary_required')
    need(re.fullmatch('[a-f0-9]{64}', args.binary_sha256 or '') and
         re.fullmatch('[a-f0-9-]{36}', args.vm_uuid or ''), 'exact_binary_digest_and_vm_uuid_required')
    with os.fdopen(os.open(args.binary, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        info = os.fstat(stream.fileno())
        need(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_mode & 0o100
             and 0 < info.st_size <= 128 * 1024 * 1024, 'bounded_owned_executable_required')
        payload = stream.read(128 * 1024 * 1024 + 1)
        need(len(payload) == info.st_size and payload.startswith(b'\x7fELF'), 'bounded_ELF_binary_required')
        need(identity(os.fstat(stream.fileno())) == identity(info) == identity(args.binary.lstat()) and
             hashlib.sha256(payload).hexdigest() == args.binary_sha256, 'binary_digest_changed')
    tag = secrets.token_hex(12)
    output = args.lab_root / 'reports/local' / ('component-lab-native-' + tag)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = {'success': False, 'checks': {}, 'restoration': {}, 'binary_sha256': args.binary_sha256,
              'limits': ['No pixel/texture proof', 'No clipboard/IME interoperability', 'No FPS or performance acceptance',
                         'Fixture events do not prove input text or slider numeric values'], 'failure': None}
    trial = None
    def interrupt(*_):
        raise Failure('controller_interrupted')
    signal.signal(signal.SIGINT, interrupt); signal.signal(signal.SIGTERM, interrupt)
    try:
        trial = Trial(args, output, tag)
        trial.stage = '/home/' + trial.command(trial.ssh + ['id -un']) + '/.cache/lucent-component-lab-' + tag
        bootstrap = 'import os,sys;from pathlib import Path;p=Path(sys.argv[1]);assert p.parent.resolve()==p.parent;p.mkdir(mode=0o700);f=os.open(p/"guest.py",os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600);out=os.fdopen(f,"wb");out.write(sys.stdin.buffer.read());out.close()'
        trial.session('/usr/bin/python3', '-I', '-B', '-c', bootstrap, trial.stage, data=GUEST.encode())
        baseline, cursor = trial.guest('before'), trial.cursor()
        need(all(type(cursor.get(key)) is int for key in ('x', 'y')) and
             0 <= cursor['x'] < 1280 and 0 <= cursor['y'] < 800, 'restorable_integer_cursor_required_before_input')
        trial.base, trial.pointer, trial.last = baseline, cursor, cursor
        private(output / 'baseline.json', {'normal': trial.base, 'cursor': trial.pointer})
        put = 'import hashlib,os,sys;from pathlib import Path;p=Path(sys.argv[1]);f=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o700);data=sys.stdin.buffer.read(134217729);assert len(data)==int(sys.argv[3]) and hashlib.sha256(data).hexdigest()==sys.argv[2];out=os.fdopen(f,"wb");out.write(data);out.close()'
        trial.session('/usr/bin/python3', '-I', '-B', '-c', put, trial.stage + '/lucent-lab', args.binary_sha256, str(len(payload)), data=payload)
        need(trial.guest('files')['lucent-lab']['sha256'] == args.binary_sha256, 'uploaded_binary_digest_mismatch')
        trial.started = True
        trial.session('systemd-run', '--user', '--collect', '--unit=' + trial.unit, '--slice=app.slice',
                      '--service-type=exec', '--property=RuntimeMaxSec=420', '--property=KillMode=control-group',
                      '--property=MemoryHigh=512M', '--property=MemoryMax=768M', '--setenv=LUCENT_PROFILE_FRAMES=0',
                      '--setenv=LUCENT_PROFILE_PRESENTATION=0', '/usr/bin/python3', '-I', '-B', trial.stage + '/guest.py',
                      'serve', trial.stage, trial.unit, args.binary_sha256)
        trial.wait(lambda state: state['scene']['client']['story'] == 'buttons', 'native_lab_not_ready', timeout=30, startup=True)
        private(output / 'origin.json', trial.probe())
        for name in STORIES:
            trial.story(name)
            if name == 'popover':
                trial.key(1); trial.wait(lambda state: state['scene']['surfaces'][0]['focus'] != 'popover-first', 'popover_not_dismissed')
        report['checks']['nine_actual_story_controls'] = True
        state = trial.story('buttons'); before = state['scene']['surfaces'][0]
        hit = next(hit for hit in before['hits'] if hit['id'] == 'sample-primary')
        trial.move(visible_point(hit, state['layer']))
        state = trial.wait(lambda value: value['scene']['surfaces'][0]['frames'] > before['frames'], 'hover_did_not_redraw')
        need(state['scene']['surfaces'][0]['focus'] == before['focus'], 'pointer_hover_stole_focus')
        for _ in range(40):
            if trial.probe()['scene']['surfaces'][0]['focus'] == 'sample-primary': break
            trial.key(15)
        state = trial.probe()['scene']['surfaces'][0]
        need(state['focus'] == 'sample-primary' and state['focus_visible'], 'keyboard_focus_not_visible')
        report['checks']['native_hover_redraw_and_keyboard_focus'] = True
        # The production palette catalog currently identifies Ocean at index 2.
        need(json.loads((args.lab_root / 'configs/lucent/palettes.json').read_text())[2]['id'] == 'ocean', 'palette_catalog_changed')
        trial.click('palette-2'); trial.wait(lambda state: state['scene']['client']['palette'] == 'ocean', 'palette_not_selected')
        trial.click('viewport-narrow'); trial.wait(lambda state: state['scene']['client']['viewport'] == 'narrow', 'narrow_not_selected')
        trial.click('lab-reset'); trial.event('Fixture reset'); trial.click('viewport-regular')
        report['checks']['palette_narrow_and_reset'] = True
        trial.story('input'); trial.click('lab-editor')
        for code in (38, 30, 48): trial.key(code)  # Known fixture text only: lab.
        trial.event('Text edit or selection')
        need(trial.probe()['scene']['surfaces'][0]['focus'] == 'lab-editor', 'input_focus_missing')
        report['checks']['native_typing_delivers_fixture_edit_events'] = True
        trial.story('slider'); trial.click('sample-slider'); trial.key(106); trial.event('Slider changed')
        trial.story('toggle'); trial.click('sample-toggle'); trial.event('Toggle changed')
        report['checks']['slider_and_toggle_events'] = True
        trial.story('popover'); trial.key(1); trial.click('popover-trigger')
        trial.wait(lambda state: state['scene']['surfaces'][0]['focus'] == 'popover-first', 'popover_focus_missing')
        trial.key(1); trial.wait(lambda state: state['scene']['surfaces'][0]['focus'] == 'popover-trigger', 'popover_focus_not_restored')
        report['checks']['popover_escape_restores_opener_focus'] = True
        trial.story('menu'); trial.click('menu-keyboard-start')
        trial.wait(lambda state: state['scene']['client']['menu_keyboard_session'] and state['scene']['surfaces'][0]['focus'] == 'menu-search', 'menu_session_not_focused')
        trial.key(108); trial.key(15); trial.key(1)
        trial.wait(lambda state: not state['scene']['client']['menu_keyboard_session'], 'menu_keyboard_not_released')
        for _ in range(40):
            if trial.probe()['scene']['surfaces'][0]['focus'] == 'lab-reset': break
            trial.key(15)
        need(trial.probe()['scene']['surfaces'][0]['focus'] == 'lab-reset', 'chrome_focus_not_reachable')
        trial.key(28); trial.event('Fixture reset')
        report['checks']['actual_menu_keyboard_and_return_to_chrome'] = True
        state = trial.story('motion'); initial = state['scene']['surfaces'][0]['frames']
        trial.click('motion-replay'); trial.event('Motion replay')
        trial.wait(lambda state: state['scene']['surfaces'][0]['frames'] > initial + 1, 'motion_submissions_not_observed')
        time.sleep(1); settled = trial.probe()['scene']['surfaces'][0]['frames']
        time.sleep(1); need(trial.probe()['scene']['surfaces'][0]['frames'] == settled, 'lab_idle_submissions_continue')
        report['checks']['motion_replay_submission_progress_then_settled'] = True
        trial.guard(); report['success'] = True
    except BaseException as error:
        report['failure'] = str(error) if isinstance(error, Failure) else type(error).__name__
    finally:
        if trial is not None and trial.stage is not None:
            try:
                if trial.down: trial.qmp([{'type': 'btn', 'data': {'down': False, 'button': 'left'}}])
                qcodes = {1: 'esc', 15: 'tab', 28: 'ret', 38: 'l', 30: 'a', 48: 'b', 106: 'right', 108: 'down'}
                if trial.keys: trial.qmp([{'type': 'key', 'data': {'down': False, 'key': {'type': 'qcode', 'data': qcodes[code]}}} for code in reversed(trial.keys)])
                if trial.started: report['restoration']['owned_unit_drained'] = trial.guest('retire')['retired']
            except BaseException as error:
                report['retirement_failure'] = str(error) if isinstance(error, Failure) else type(error).__name__
                report['success'] = False
            try:
                logs = trial.session('journalctl', '--user', '-u', trial.unit, '-n', '200', '--no-pager', '-o', 'cat')
                private(output / 'journal.json', {'text': logs})
            except BaseException as error:
                report['log_capture_failure'] = type(error).__name__
                report['success'] = False
            try:
                need(not trial.started or report['restoration'].get('owned_unit_drained'), 'owned_unit_not_proven_drained')
                if trial.base is not None:
                    trial.guard()
                    point = trial.pointer
                    need(all(type(point[key]) is int for key in ('x', 'y')), 'integer_original_cursor_required')
                    need(trial.session('hyprctl', 'dispatch', f"hl.dsp.cursor.move({{x={point['x']},y={point['y']}}})") == 'ok', 'cursor_restore_rejected')
                    deadline = time.monotonic() + 3
                    while trial.cursor() != point and time.monotonic() < deadline: time.sleep(.05)
                    need(trial.cursor() == point, 'cursor_not_restored_exactly'); trial.last = point
                    trial.guard(); report['restoration']['normal_shell_compositor_settings_wallpaper_outputs_focus_cursor'] = True
                need('log_capture_failure' not in report, 'logs_not_captured_stage_retained')
                facts = trial.guest('files'); private(output / 'staged-files.json', facts)
                need(all(name in ('guest.py', 'lucent-lab', 'origin.json') for name in facts), 'unexpected_staged_file_retained')
                need(facts['guest.py']['sha256'] == hashlib.sha256(GUEST.encode()).hexdigest(), 'guest_helper_changed')
                if 'lucent-lab' in facts: need(facts['lucent-lab']['sha256'] == args.binary_sha256, 'staged_binary_changed')
                private(output / 'unit-origin.json', json.loads(trial.session('/usr/bin/python3', '-I', '-B', '-c', 'import sys;from pathlib import Path;print(Path(sys.argv[1]).read_text())', trial.stage + '/origin.json')) if 'origin.json' in facts else None)
                need(trial.guest('clean', data=json.dumps(facts).encode()), 'owned_stage_not_removed')
                report['restoration']['only_unchanged_owned_stage_removed_after_logs'] = True
            except BaseException as error:
                report['restoration_failure'] = str(error) if isinstance(error, Failure) else type(error).__name__
                report['success'] = False
        private(output / 'report.json', report)
        print(json.dumps({'success': report['success'], 'checks': report['checks'], 'restoration': report['restoration'],
                          'failure': report['failure'], 'report': str(output / 'report.json')}))
    return 0 if report['success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
