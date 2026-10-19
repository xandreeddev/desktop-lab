#!/usr/bin/env python3
"""Guarded real-input acceptance for the native Displays transaction.

Requires a deliberately idle, unlocked single-output QEMU guest at 1920x1080/1x,
with 1280x800 advertised. Uses the already deployed executable: no build/deploy.
A separate SSH-started recovery process holds a private original-inode checkpoint
before the first UI change. Reports remain private. Physical hardware and multiple
outputs are outside this harness. --test-crash explicitly also kills only the
captured desktop UI PID; its independent guardian must restore the preview.
"""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import runpy
import shlex
import subprocess
import time
import uuid

import lab
from native_surfaces import active_surfaces, find_surface
from preservation_fingerprint import fingerprint

ROOT = Path(__file__).resolve().parents[1]
STARTED = None
INPUT_ALLOWED = []
HELPER = runpy.run_path(str(ROOT / 'vm/test-framework.py'))
remote, session, inspect = [HELPER[key] for key in ('remote', 'session', 'inspect')]
raw_key, eventually = [HELPER[key] for key in ('key', 'eventually')]

RECOVERY = r'''
import ctypes,hashlib,json,os,stat,subprocess,sys,time
from pathlib import Path
operation,raw=sys.argv[1:]
directory=Path(raw)
assert directory.name.startswith('lucent-display-transaction-') and directory.is_absolute()
meta=directory.lstat(); assert stat.S_ISDIR(meta.st_mode) and meta.st_uid==os.getuid() and stat.S_IMODE(meta.st_mode)==0o700
manifest=directory/'manifest.json'; backup=directory/'monitors.original'
config=Path.home()/'.config/hypr/monitors.lua'
state=Path.home()/'.local/state/lucent/desktop.json'
def file_identity(meta):return [meta.st_dev,meta.st_ino,meta.st_mtime_ns,meta.st_size,meta.st_mode,meta.st_uid]
def read(path,limit):
    descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(descriptor,'rb') as stream:
        before=os.fstat(stream.fileno()); assert stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_size<=limit
        data=stream.read(limit+1); after=os.fstat(stream.fileno()); path_meta=path.lstat()
        assert len(data)<=limit and file_identity(before)==file_identity(after)==file_identity(path_meta)
        return data,file_identity(before)

def session(*args):
    result=subprocess.run(['python3',str(Path.home()/'desktop-lab/scripts/in-session.py'),*args],capture_output=True,text=True,timeout=10)
    assert result.returncode==0
    return result.stdout
def monitors(): return json.loads(session('hyprctl','-j','monitors'))
def signature(m): return [m['name'],m['width'],m['height'],round(m['refreshRate']*1000),m['x'],m['y'],m['scale'],m['transform']]
def private(path,value):
    descriptor=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
    with os.fdopen(descriptor,'w') as stream: json.dump(value,stream)
def current_manifest():
    value,_=read(manifest,65536); return json.loads(value)
def native_restore(value):
    current=monitors(); assert len(current)==1
    original=value['monitor']; actual=current[0]
    assert actual['name']==original['name'] and actual['transform']==original['transform']
    assert signature(actual) in value['allowed_native'], 'An unrelated native layout is never ours to restore'
    output=json.dumps(original['name'])
    source='hl.monitor({ output = '+output+', mode = "'+str(original['width'])+'x'+str(original['height'])+'@'+str(original['refreshRate'])+'", position = "'+str(original['x'])+'x'+str(original['y'])+'", scale = '+str(original['scale'])+', transform = '+str(original['transform'])+' })'
    session('hyprctl','eval',source)
    for _ in range(40):
        current=monitors()
        if len(current)==1 and signature(current[0])==signature(original): return
        time.sleep(.1)
    raise AssertionError('Native recovery could not verify the captured output')
def expected_keep(value,original):
    marker=b'-- BEGIN LUCENT DISPLAY CONFIGURATION\n'
    prefix=original.split(marker,1)[0]
    native=value['monitor']
    mode=str(native['width'])+'x'+str(native['height'])+'@'+format(native['refreshRate'],'.3f')
    rule='hl.monitor({ output = '+json.dumps(native['name'])+', disabled = false, mode = "'+mode+'", position = "'+str(native['x']+16)+'x'+str(native['y'])+'", scale = '+format(native['scale'],'.3f')+', transform = '+str(native['transform'])+' })\n'
    prefix=prefix+(b'\n' if prefix and not prefix.endswith(b'\n') else b'')
    return prefix+marker+rule.encode()+b'-- END LUCENT DISPLAY CONFIGURATION\n'
def restore(value):
    actual=monitors();assert len(actual)==1 and signature(actual[0]) in value['allowed_native']
    original,original_id=read(backup,256*1024)
    assert hashlib.sha256(original).hexdigest()==value['config_sha256'] and original_id==value['config_identity']
    current,current_id=read(config,256*1024)
    if current!=original or current_id!=original_id:
        expected=expected_keep(value,original)
        kept=value.get('kept')
        assert kept and current==expected and current_id==kept['identity'] and hashlib.sha256(current).hexdigest()==kept['sha256'], 'A concurrent monitor file replacement is never ours to restore'
        replacement=directory/'monitors.restore'
        os.link(backup,replacement)
        # Exchange preserves the outgoing inode. If a replacement raced the
        # validation, put that exact foreign inode back instead of deleting it.
        rename=ctypes.CDLL(None,use_errno=True).renameat2
        rename.argtypes=[ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint]
        rename.restype=ctypes.c_int
        def exchange():
            if rename(-100,os.fsencode(replacement),-100,os.fsencode(config),2)!=0:raise OSError(ctypes.get_errno(),'Monitor checkpoint exchange failed')
        exchange()
        try:
            # Inspect identity before opening: a raced symlink/directory must be
            # exchanged back too, even though read() deliberately refuses it.
            outgoing_meta=replacement.lstat()
            assert file_identity(outgoing_meta)==current_id
            outgoing,outgoing_id=read(replacement,256*1024)
            assert outgoing==current and outgoing_id==current_id
        except BaseException:
            installed,installed_id=read(config,256*1024)
            assert installed==original and installed_id==original_id, 'Concurrent replacement retained beside the recovery checkpoint'
            exchange()
            raise AssertionError('Concurrent monitor replacement preserved') from None
        restored,restored_id=read(config,256*1024)
        assert restored==original and restored_id==original_id
        replacement.unlink()
    native_restore(value)
    actual,actual_id=read(config,256*1024)
    settings,settings_id=read(state,4*1024*1024)
    assert actual==original and actual_id==value['config_identity']
    assert hashlib.sha256(settings).hexdigest()==value['settings_sha256'] and settings_id==value['settings_identity']
    return {'config_bytes_and_inode':True,'native_baseline':True,'settings_bytes_and_inode':True}
if operation=='create':
    config_data,config_id=read(config,256*1024); settings,settings_id=read(state,4*1024*1024)
    current=monitors(); assert len(current)==1
    os.link(config,backup)
    base=current[0]; mode=next(mode for mode in base['availableModes'] if mode.startswith('1280x800@'))
    mode_monitor={**base,'width':1280,'height':800,'refreshRate':float(mode.removesuffix('Hz').split('@')[1])}
    allowed=[signature(base),signature(mode_monitor),signature({**base,'scale':2}),signature({**base,'x':base['x']+16})]
    value={'allowed_native':allowed,'monitor':current[0],'config_sha256':hashlib.sha256(config_data).hexdigest(),'config_identity':config_id,'settings_sha256':hashlib.sha256(settings).hexdigest(),'settings_identity':settings_id}
    private(manifest,value)
    print(json.dumps({'checkpoint':True}))
elif operation=='watch':
    private(directory/'watch.ready',{'pid':os.getpid()})
    deadline=time.monotonic()+240
    while time.monotonic()<deadline:
        if (directory/'watch.stop').exists(): raise SystemExit(0)
        time.sleep(.25)
    value=current_manifest()
    try: result=restore(value)
    except BaseException:
        private(directory/'watch.result.json',{'restoration':False}); raise
    private(directory/'watch.result.json',{'restoration':True,**result})
elif operation=='capture-keep':
    value=current_manifest();original,original_id=read(backup,256*1024)
    assert original_id==value['config_identity'] and hashlib.sha256(original).hexdigest()==value['config_sha256']
    current,current_id=read(config,256*1024);assert current==expected_keep(value,original)
    value['kept']={'sha256':hashlib.sha256(current).hexdigest(),'identity':current_id}
    staged=directory/'manifest.keep';private(staged,value);os.replace(staged,manifest)
    print(json.dumps({'captured':True}))
elif operation=='restore': print(json.dumps(restore(current_manifest())))
elif operation=='stop':
    restore(current_manifest()); private(directory/'watch.stop',{'stop':True}); print(json.dumps({'restoration':True}))
else: raise ValueError('Unknown recovery operation')
'''


def cli(*args):
    return session('/home/omarchy/.local/bin/lucent-cli', *args)


def state():
    return inspect()['client']['displays']


def monitor():
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1
    return monitors[0]


def phase():
    return (state().get('transaction') or {}).get('phase')


def panel():
    return find_surface(inspect(), 'displays')


def signature(native):
    return [native['name'],native['width'],native['height'],round(native['refreshRate']*1000),native['x'],native['y'],native['scale'],native['transform']]


def safe_input(focus=None):
    assert STARTED is not None and time.monotonic()-STARTED<180, 'Stop UI input before the independent recovery deadline'
    assert session('omarchy-shell','lock','isLocked').strip()=='false', 'Never type into a secure prompt'
    assert 'lucent-authorization-' not in session('hyprctl','-j','layers'), 'Never type into an authorization prompt'
    assert not json.loads(session('hyprctl','-j','clients')), 'No foreign application may receive test input'
    assert not json.loads(session('hyprctl','-j','activewindow'))
    snapshot=inspect();client=snapshot['client']
    assert not client['launcher'] and not client['notification_history_open'] and not client['widget_grid']
    for name in ('audio','media','network','bluetooth','power','tray','live_wallpaper','weather'):
        assert not client.get(name,{}).get('open',False) and not client.get(name,{}).get('visible',False)
    current=find_surface(snapshot,'displays')
    assert client['displays']['open'] and current['pointer_input']=='all' and current['keyboard']=='exclusive'
    assert [item['id'] for item in active_surfaces(snapshot) if item['keyboard']=='exclusive']==[current['id']]
    assert current['focus'] is None or any(hit['id']==current['focus'] for hit in current['hits'])
    if focus is not None:assert current['focus']==focus, 'The expected display field lost focus'
    native=monitor();assert signature(native) in INPUT_ALLOWED, 'The expected native display state changed'
    return current,native


def key(*codes,focus=None):
    safe_input(focus)
    raw_key(*codes)


def click(identifier):
    """Guard each physical action and use actual post-modeset dimensions."""
    for _ in range(48):
        current,native=safe_input()
        hit=next((hit for hit in current['hits'] if hit['id']==identifier),None)
        if hit:
            clip=hit['clip']
            if hit['y']>=clip['y'] and hit['y']+hit['height']<=clip['y']+clip['height']+.01:
                layer=HELPER['layer_geometry']('displays')
                x=(layer['x']+hit['x']+hit['width']/2-native['x'])*native['scale']
                y=(layer['y']+hit['y']+hit['height']/2-native['y'])*native['scale']
                checked,actual=safe_input()
                assert checked['id']==current['id'] and signature(actual)==signature(native)
                assert next(h for h in checked['hits'] if h['id']==identifier)==hit
                HELPER['send']([{'type':'abs','data':{'axis':axis,'value':round(value/(size-1)*32767)}} for axis,value,size in [('x',x,native['width']),('y',y,native['height'])]])
                time.sleep(.1)
                try:
                    checked,actual=safe_input()
                    assert checked['id']==current['id'] and signature(actual)==signature(native)
                    assert next(h for h in checked['hits'] if h['id']==identifier)==hit
                    assert HELPER['layer_geometry']('displays')==layer
                    HELPER['button'](True);time.sleep(.05)
                finally:HELPER['button'](False)
                time.sleep(.3)
                return
        key(15)
    raise AssertionError('Display control did not become reachable: '+identifier)


def enter(value):
    focused=panel()['focus'];assert focused in {field(0),field(1),field(2)}
    mapping={}
    for node in ast.walk(ast.parse((ROOT/'vm/run.py').read_text())):
        if isinstance(node,ast.Assign) and len(node.targets)==1 and isinstance(node.targets[0],ast.Name) and node.targets[0].id in ('normal','shifted','codes'):
            mapping[node.targets[0].id]=ast.literal_eval(node.value)
    key(29,30,focus=focused)
    for char in value:
        if char in mapping['normal']:key(mapping['codes'][mapping['normal'].index(char)],focus=focused)
        elif char in mapping['shifted']:key(42,mapping['codes'][mapping['shifted'].index(char)],focus=focused)
        else:raise ValueError('Unsupported test character')


def field(index):
    return next(hit['id'] for hit in panel()['hits'] if hit['id'].startswith(f'display-field-{index}-'))


def changed(native):
    return tuple(native[key] for key in ('width','height','x','y','scale','transform'))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--desktop-sha256',required=True)
    parser.add_argument('--test-crash',action='store_true')
    args=parser.parse_args()
    assert len(args.desktop_sha256)==64 and all(c in '0123456789abcdef' for c in args.desktop_sha256)
    assert remote('systemd-detect-virt','--vm').strip() in ('qemu','kvm')
    assert session('omarchy-shell','lock','isLocked').strip()=='false'
    before=inspect();client=before['client']
    assert not client['launcher'] and not client['notification_history_open'] and not client['widget_grid']
    assert not json.loads(session('hyprctl','-j','activewindow')), 'No foreground app may be used as an accidental input target'
    for name in ('audio','media','network','bluetooth','power','tray','live_wallpaper','displays','weather'):
        assert not client.get(name,{}).get('open',False), 'Close panels before this deliberate acceptance run'
    observed=monitor();assert changed(observed)==(1920,1080,0,0,1,0)
    global INPUT_ALLOWED
    INPUT_ALLOWED=[signature(observed)]
    assert any(mode.startswith('1280x800@') for mode in observed['availableModes'])
    assert remote('sha256sum','/home/omarchy/.local/bin/lucent-desktop').split()[0]==args.desktop_sha256
    before_preservation=fingerprint(session)
    process_script=r"""
import hashlib,json,os,stat,subprocess,sys
from pathlib import Path
properties=dict(line.split('=',1) for line in subprocess.check_output(['systemctl','--user','show','lucent.service','-p','ActiveState','-p','MainPID','-p','ControlGroup','-p','KillMode'],text=True).splitlines())
assert properties['ActiveState']=='active' and properties['KillMode']=='mixed'
assert properties['ControlGroup'].startswith(f'/user.slice/user-{os.getuid()}.slice/user@{os.getuid()}.service/') and properties['ControlGroup'].endswith('/lucent.service')
expected=Path.home()/'.local/bin/lucent-desktop'; wanted=sys.argv[1]; matches=[]
for entry in Path('/proc').iterdir():
    if not entry.name.isdecimal():continue
    try:
        args=(entry/'cmdline').read_bytes().split(bytes([0]))
        if len([a for a in args if a])!=1 or (entry/'exe').resolve()!=expected:continue
        with (entry/'exe').open('rb') as stream:
            meta=os.fstat(stream.fileno());assert stat.S_ISREG(meta.st_mode) and meta.st_size<=128*1024*1024
            value=hashlib.file_digest(stream,'sha256').hexdigest()
        assert value==wanted
        assert (entry/'cgroup').read_text().strip()=='0::'+properties['ControlGroup']
        assert (entry/'stat').read_text().rsplit(')',1)[1].split()[1]==properties['MainPID']
        matches.append({'supervisor_pid':properties['MainPID'],'cgroup':properties['ControlGroup'],'pid':int(entry.name),'start':(entry/'stat').read_text().rsplit(')',1)[1].split()[19],'exe_identity':[meta.st_dev,meta.st_ino]})
    except (FileNotFoundError,ProcessLookupError,PermissionError):pass
assert len(matches)==1
print(json.dumps(matches[0]))
"""
    desktop_process=json.loads(remote('python3','-c',process_script,args.desktop_sha256))
    directory='/home/omarchy/.local/state/lucent/lucent-display-transaction-'+uuid.uuid4().hex
    remote('mkdir','-m','700',directory)
    subprocess.run(lab.ssh_args('lucent')+['cat > '+shlex.quote(directory+'/recovery.py')],input=RECOVERY.encode(),check=True)
    def recovery(operation): return json.loads(remote('python3',directory+'/recovery.py',operation,directory))
    recovery('create')
    global STARTED
    STARTED=time.monotonic()
    launch="import os,subprocess,sys; directory=sys.argv[1]; subprocess.Popen(['python3',directory+'/recovery.py','watch',directory],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True); print('started')"
    remote('python3','-c',launch,directory)
    eventually(lambda:remote('test','-f',directory+'/watch.ready')=='','Independent recovery watchdog did not arm')
    checks=[];restoration=None;preservation_verified=False
    report_path=ROOT/'reports/local'/('display-settings-native-'+uuid.uuid4().hex+'.json')
    report_path.parent.mkdir(parents=True,exist_ok=True)
    def original(): return signature(monitor())==signature(observed)
    def open_panel():
        cli('displays','open');eventually(lambda:state()['open'] and panel()['frames']>0,'Display panel did not submit');time.sleep(.3)
    def apply(expected):
        global INPUT_ALLOWED
        INPUT_ALLOWED=[signature(monitor()),signature(expected)]
        preview_started=time.monotonic()
        click('display-apply');eventually(lambda:phase()=='confirmation','Native preview was not verified',timeout=10)
        actual=monitor()
        assert signature(actual)==signature(expected), json.dumps({'preview_seconds':round(time.monotonic()-preview_started,3),'expected':signature(expected)[1:],'actual':signature(actual)[1:]})
        INPUT_ALLOWED=[signature(expected)]
    try:
        # Existing Omarchy discovery/action remains, only its presentation changes.
        session('omarchy-shell','shell','summon','omarchy.monitor')
        eventually(lambda:state()['open'],'Compatibility display action did not open native child')
        key(1);eventually(lambda:not state()['open'],'Escape did not close the display panel')
        assert original();checks.append('compatibility_open_and_escape')
        open_panel()
        modes=[]
        current=f"{observed['width']}x{observed['height']}@{observed['refreshRate']:.3f}"
        def normalized(value):
            dims,hz=value.removesuffix('Hz').split('@');return dims,round(float(hz)*1000)
        for value in observed['availableModes']:
            parsed=normalized(value)
            if parsed not in modes:modes.append(parsed)
        if normalized(current) not in modes:modes.insert(0,normalized(current))
        index=next(i for i,mode in enumerate(modes) if mode[0]=='1280x800')
        click('display-mode-'+str(index));assert original() and state()['dirty'];checks.append('mode_draft_performs_no_native_change')
        expected_mode={**observed,'width':1280,'height':800,'refreshRate':modes[index][1]/1000}
        apply(expected_mode)
        click('display-revert');eventually(original,'Explicit Revert did not restore full baseline');checks.append('mode_apply_and_explicit_revert');INPUT_ALLOWED=[signature(observed)]
        open_panel();click(field(2));enter('2');apply({**observed,'scale':2})
        eventually(lambda:phase()=='reverted' and original(),'Timeout did not restore scale',timeout=22);checks.append('scale_apply_and_timeout_revert');INPUT_ALLOWED=[signature(observed)]
        open_panel();click(field(0));enter('16');apply({**observed,'x':16})
        click('display-keep');eventually(lambda:phase()=='kept','Keep did not persist')
        assert monitor()['x']==16
        assert recovery('capture-keep')['captured']
        checks.append('position_apply_and_keep')
        cli('displays','close');eventually(lambda:not state()['open'],'Close did not release keyboard')
        restoration=recovery('restore');assert all(restoration.values())
        checks.append('keep_file_and_native_independent_restore');INPUT_ALLOWED=[signature(observed)]
        if args.test_crash:
            open_panel();click(field(0));enter('16');apply({**observed,'x':16})
            # Match only the preflight-verified desktop binary with no arguments;
            # the --display-guard child must remain alive and restore on pipe EOF.
            script=r"""
import hashlib,json,os,signal,subprocess,sys
from pathlib import Path
captured=json.loads(sys.argv[1]); expected_sha=sys.argv[2]
entry=Path('/proc')/str(captured['pid']); descriptor=os.pidfd_open(captured['pid'])
try:
    properties=dict(line.split('=',1) for line in subprocess.check_output(['systemctl','--user','show','lucent.service','-p','ActiveState','-p','MainPID','-p','ControlGroup','-p','KillMode'],text=True).splitlines())
    assert properties['ActiveState']=='active' and properties['MainPID']==captured['supervisor_pid'] and properties['ControlGroup']==captured['cgroup'] and properties['KillMode']=='mixed'
    assert (entry/'cgroup').read_text().strip()=='0::'+captured['cgroup']
    assert (entry/'stat').read_text().rsplit(')',1)[1].split()[19]==captured['start']
    assert len([a for a in (entry/'cmdline').read_bytes().split(bytes([0])) if a])==1
    with (entry/'exe').open('rb') as stream:
        meta=os.fstat(stream.fileno());assert [meta.st_dev,meta.st_ino]==captured['exe_identity']
        assert hashlib.file_digest(stream,'sha256').hexdigest()==expected_sha
    assert (entry/'stat').read_text().rsplit(')',1)[1].split()[19]==captured['start']
    signal.pidfd_send_signal(descriptor,signal.SIGKILL)
finally:os.close(descriptor)
"""
            safe_input()
            remote('python3','-c',script,json.dumps(desktop_process),args.desktop_sha256)
            eventually(original,'Guardian did not restore after desktop crash',timeout=12)
            eventually(lambda:inspect()['client']['settings_writable'],'Supervisor did not recover desktop',timeout=45)
            recovered=json.loads(remote('python3','-c',process_script,args.desktop_sha256))
            assert recovered['pid']!=desktop_process['pid']
            checks.append('desktop_crash_eof_rollback_and_supervisor_recovery')
    finally:
        try:
            try:cli('displays','close');time.sleep(.8)
            except (OSError,subprocess.SubprocessError,AssertionError,KeyError):pass
        finally:
            try:
                restoration=recovery('stop')
                assert fingerprint(session)==before_preservation, 'Preserved settings/media/exports/history/supervisor/binary state changed; no automatic restore'
                preservation_verified=True
            finally:
                report={'desktop_sha256':args.desktop_sha256,'checks':checks,'restoration':restoration,'crash_test_requested':args.test_crash,'preservation_verified':preservation_verified,'scope':'single-output QEMU native panel input and observed layout; no physical/multi-output claims'}
                descriptor=os.open(report_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
                with os.fdopen(descriptor,'w') as stream:json.dump(report,stream,indent=2);stream.write('\n')
    assert restoration and all(restoration.values()) and preservation_verified
    print(json.dumps({'passed':True,'checks':checks,'restoration':restoration,'preservation_verified':preservation_verified,'private_report':str(report_path)}))

if __name__=='__main__': main()
