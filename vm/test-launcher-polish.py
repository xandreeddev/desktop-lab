"""Real-input launcher and owned picker acceptance for an installed build bundle.

Requires an unlocked, idle, single-output FHD Lucent VM. The helper edits only
transient launcher state and a private 180-second picker fixture. It verifies
binary/process identity and retains original preferences, wallpaper, clipboard,
media, desktop lifetime and cursor. Screenshots and detailed receipts stay private.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import runpy
import signal
import shlex
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'vm'), str(ROOT / 'scripts')]
from lucent_dev.model import load_bundle
from application_intent_policy import CursorOwnership
from owned_application_units import UNIT_IDENTITY
from preservation_fingerprint import fingerprint
from native_surfaces import find_surface

spec = importlib.util.spec_from_file_location('native_launcher_framework', ROOT / 'vm/test-framework.py')
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


GUEST = 'if not __debug__:raise SystemExit("assertions_required")\n' + UNIT_IDENTITY + r'''
import shutil,socket,struct,subprocess,sys
operation,folder,unit,expected=sys.argv[1:]
stage=Path(folder)
assert stage.parent==Path.home()/'.cache' and re.fullmatch('lucent-launcher-test-[0-9a-f]{24}',stage.name)
assert stage.resolve(strict=True)==stage and stage.stat().st_uid==os.getuid()
assert stat.S_IMODE(stage.stat().st_mode)==0o700
binary=Path.home()/'.local/bin/lucent-menu';program=stage/'guest.py'
argv=['/usr/bin/python3','-I','-B',str(program),'start',folder,unit,expected]
def origin():
 path=stage/'origin.json';assert path.stat().st_uid==os.getuid() and path.stat().st_size<=131072
 return json.loads(path.read_text())
if operation=='start':
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==expected
 captured=capture_unit_origin(unit,argv)
 with (stage/'origin.json').open('x') as out:json.dump(captured,out)
 request=stage/'request.json'
 assert request.stat().st_uid==os.getuid() and request.stat().st_size<=2*1024*1024
 for number,path,flags in [(0,request,os.O_RDONLY),(1,stage/'out.txt',os.O_WRONLY|os.O_CREAT|os.O_EXCL),(2,stage/'err.txt',os.O_WRONLY|os.O_CREAT|os.O_EXCL)]:
  fd=os.open(path,flags|os.O_NOFOLLOW,0o600);os.dup2(fd,number);os.close(fd) if fd!=number else None
 os.execv(str(binary),[str(binary)])
elif operation=='inspect':
 captured=origin();current=assert_unit_origin(unit_snapshot(unit),unit,argv,captured,hashed=True)
 assert current is not None and current['sha256']==expected
 path=Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-menu.sock'
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
 reply=json.loads(data);reply['_owned_menu_pid']=current['pid'];print(json.dumps(reply))
elif operation=='stop':
 captured=origin()
 current=None if _retirement_drained(captured) else assert_unit_origin(unit_snapshot(unit),unit,argv,captured,hashed=True)
 graceful=_retirement_drained(captured)
 if current is not None:
  assert current['sha256']==expected
  path=Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-menu.sock'
  with socket.socket(socket.AF_UNIX) as connection:
   connection.settimeout(5);connection.connect(str(path))
   pid,uid,_=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
   assert pid==current['pid'] and uid==os.getuid()
   connection.sendall(b'close\n');reply=bytearray()
   while True:
    chunk=connection.recv(65536)
    if not chunk:break
    reply.extend(chunk);assert len(reply)<=131072
   assert bytes(reply).strip()==b'ok'
  deadline=time.monotonic()+10
  while not _retirement_drained(captured) and time.monotonic()<deadline:time.sleep(.1)
  graceful=_retirement_drained(captured)
 if not _retirement_drained(captured):retire_original_unit(unit,argv,captured)
 assert _retirement_drained(captured)
 assert not (Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-menu.sock').exists()
 print(json.dumps({'original_unit_drained':True,'graceful_exit_and_socket_cleanup':graceful}))
elif operation=='remove':
 captured=origin();assert _retirement_drained(captured)
 assert set(path.name for path in stage.iterdir())=={'guest.py','request.json','origin.json','out.txt','err.txt'}
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==expected
 shutil.rmtree(stage)
 print(json.dumps({'owned_stage_removed':True}))
elif operation=='result':
 captured=origin();assert _retirement_drained(captured)
 assert (stage/'out.txt').stat().st_size<=131072 and (stage/'err.txt').stat().st_size<=131072
 print(json.dumps({'output':(stage/'out.txt').read_text(),'stderr_empty':not (stage/'err.txt').read_bytes()}))
else:raise ValueError('unknown_operation')
'''

def wait(operation, label, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = operation()
            if value:
                return value
        except (subprocess.CalledProcessError, KeyError, ValueError, StopIteration):
            pass
        time.sleep(.1)
    raise AssertionError(label)


def private(path, data):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as out:
        json.dump(data, out, indent=2)
        out.write('\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    args = parser.parse_args()
    manifest = load_bundle(args.bundle.resolve(strict=True))
    assert manifest['source_dirty'] is False
    expected = {name:entry['sha256'] for name,entry in manifest['binaries'].items()}
    assert v.session('python3', '-c', 'import subprocess;assert subprocess.run(["omarchy-hyprland-session-locked"],capture_output=True).returncode==1;print("unlocked")').strip() == 'unlocked'
    monitors = json.loads(v.session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1 and all(monitors[0][key] == value for key, value in {'width':1920,'height':1080,'scale':1,'x':0,'y':0,'transform':0}.items())
    assert json.loads(v.session('hyprctl', '-j', 'clients')) == []
    baseline = fingerprint(v.session)
    desktop_pid = v.session('python3','-c','import os,socket,struct;from pathlib import Path\nwith socket.socket(socket.AF_UNIX) as s:\n s.settimeout(5);s.connect(str(Path(os.environ["XDG_RUNTIME_DIR"])/"lucent.sock"));pid,uid,_=struct.unpack("3i",s.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert uid==os.getuid();s.sendall(b"inspect\\n");data=bytearray()\n while chunk:=s.recv(65536):\n  data.extend(chunk);assert len(data)<=8*1024*1024\n print(pid)').strip()
    assert desktop_pid.isdecimal() and int(desktop_pid)>1
    started = False
    identity = None
    def guard():
        live = json.loads(v.session('python3', '-c', 'import hashlib,json,os,sys;from pathlib import Path;p=Path("/proc")/sys.argv[1];assert p.stat().st_uid==os.getuid();print(json.dumps({"sha":hashlib.sha256((p/"exe").read_bytes()).hexdigest(),"birth":(p/"stat").read_text().rsplit(")",1)[1].split()[19]}))', desktop_pid))
        assert live['sha'] == expected['lucent-desktop']
        if identity is not None:
            assert live == identity
        assert json.loads(v.session('hyprctl','-j','clients')) == []
        allowed = {int(desktop_pid)}
        if started:
            allowed.add(menu()['_owned_menu_pid'])
        layers=json.loads(v.session('hyprctl','-j','layers'))
        def owned(value):
            if isinstance(value,dict):
                if 'namespace' in value:assert value.get('pid') in allowed
                for child in value.values():owned(child)
            elif isinstance(value,list):
                for child in value:owned(child)
        owned(layers)
        assert v.session('python3', '-c', 'import subprocess;assert subprocess.run(["omarchy-hyprland-session-locked"],capture_output=True).returncode==1;print("ok")').strip() == 'ok'
        return live
    identity = guard()
    assert not v.inspect()['client']['launcher']
    cursor = CursorOwnership(json.loads(v.session('hyprctl', '-j', 'cursorpos')))
    tag = uuid.uuid4().hex[:24]
    output = ROOT / 'reports/local' / ('launcher-polish-' + tag)
    output.mkdir(mode=0o700)
    result = {'source_commit':manifest['source_commit'],'desktop_sha256':expected['lucent-desktop'],'menu_sha256':expected['lucent-menu'],'passed':False,'checks':{},'restoration':{},'visual_review_required':True}
    private(output / 'original-cursor.json', {'point':cursor.initial,'desktop':identity})
    folder = None
    opened = False
    button_down = False

    def durable_cursor():
        data={'original':cursor.initial,'expected':cursor.expected,'pending':cursor.pending,'lost':cursor.lost,'button_down_declared':button_down,'desktop':identity}
        path=output/'.cursor-state.next'
        with os.fdopen(os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600),'w') as stream:
            json.dump(data,stream);stream.write('\n');stream.flush();os.fsync(stream.fileno())
        os.replace(path,output/'cursor-state.json')
        descriptor=os.open(output,os.O_RDONLY|os.O_DIRECTORY)
        try:os.fsync(descriptor)
        finally:os.close(descriptor)

    def position():
        return json.loads(v.session('hyprctl', '-j', 'cursorpos'))
    def move(point):
        guard()
        point = tuple(round(value) for value in point)
        if cursor.observe(position()) == point:
            return
        cursor.begin(position(), point)
        durable_cursor()
        v.move(*point)
        wait(lambda: tuple(position()[key] for key in ('x','y')) == point, 'physical_cursor_did_not_reach_target')
        cursor.finish(position())
        durable_cursor()
    def button(down):
        nonlocal button_down
        if down:button_down=True;durable_cursor()
        v.button(down)
        if not down:button_down=False;durable_cursor()
    def key(*codes):
        guard()
        if started:
            assert menu()['surfaces'][0]['actual_native_keyboard_focused']
        elif v.inspect()['client']['launcher']:
            assert surface()['actual_native_keyboard_focused']
        else:
            assert codes==(125,57)
        v.key(*codes)
    def desktop():
        return v.inspect()['client']
    def query(text):
        key(29, 30)
        key(14)
        for character in text:
            guard()
            v.text(character)
        wait(lambda: desktop()['query'] == text, 'native_search_query_mismatch')
    def surface():
        return find_surface(v.inspect(), 'dock')
    def capture(label, native=None):
        target = native or surface()
        panel_id = 'menu-panel' if native is not None else 'dock-panel'
        hit = next(hit for hit in target['hits'] if hit['id'] == panel_id)
        layer = v.layer_geometry(target['id']) if native is None else menu_layer()
        geometry = f"{round(layer['x']+hit['x'])},{round(layer['y']+hit['y'])} {round(hit['width'])}x{round(hit['height'])}"
        reply = subprocess.run(v.lab.ssh_args('lucent') + [shlex.join(['python3','desktop-lab/scripts/in-session.py','grim','-g',geometry,'-'])], capture_output=True, timeout=20, check=True)
        assert reply.stdout.startswith(b'\x89PNG\r\n\x1a\n') and len(reply.stdout) <= 16*1024*1024
        path = output / (label + '.png')
        with os.fdopen(os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'wb') as stream:stream.write(reply.stdout)
        result.setdefault('private_captures',{})[label] = hashlib.sha256(reply.stdout).hexdigest()
    def point(hit, layer):
        clip = hit['clip']
        left, top = max(hit['x'],clip['x']), max(hit['y'],clip['y'])
        right = min(hit['x']+hit['width'],clip['x']+clip['width'])
        bottom = min(hit['y']+hit['height'],clip['y']+clip['height'])
        assert right > left and bottom > top
        return (layer['x']+(left+right)/2,layer['y']+(top+bottom)/2)
    def thumb_drag(native, layer, prefix, count, pitch, minimum):
        hit = next(hit for hit in native['hits'] if hit['id'].startswith(prefix))
        height, width = hit['height'], v.token_value('component.scroll.thumb_width')
        content_height = count*pitch - (v.token_value('space.xs') if prefix.startswith('menu') else 0)
        thumb = max(height*height/content_height, minimum)
        assert thumb < height
        x = layer['x']+hit['x']+hit['width']-width/2
        move((x,layer['y']+hit['y']+thumb/2))
        button(True)
        try:
            move((x,layer['y']+hit['y']+height-thumb/2))
        finally:
            button(False)
    def guest(operation):
        reply = subprocess.run(v.lab.ssh_args('lucent') + [shlex.join(['python3','desktop-lab/scripts/in-session.py','/usr/bin/python3','-I','-B',folder+'/guest.py',operation,folder,unit,expected['lucent-menu']])],capture_output=True,text=True,timeout=25,check=True)
        assert not reply.stderr and len(reply.stdout)<=8*1024*1024
        return json.loads(reply.stdout)
    def menu():
        return guest('inspect')
    def menu_layer():
        layers = json.loads(v.session('hyprctl','-j','layers'))
        matches=[]
        def visit(value):
            if isinstance(value,dict):
                if value.get('namespace')=='lucent-menu-menu':matches.append(value)
                for child in value.values():visit(child)
            elif isinstance(value,list):
                for child in value:visit(child)
        visit(layers)
        assert len(matches)==1
        return matches[0]
    def interrupted(*_):
        signal.signal(signal.SIGTERM,signal.SIG_IGN)
        signal.signal(signal.SIGINT,signal.SIG_IGN)
        raise RuntimeError('native_controller_interrupted')
    signal.signal(signal.SIGTERM,interrupted)
    signal.signal(signal.SIGINT,interrupted)
    durable_cursor()
    try:
        with runpy.run_path(str(ROOT/'vm/measure-framework.py'))['stay_awake'](v.remote):
            assert v.session('python3','-c','import os;from pathlib import Path;assert not os.path.lexists(Path(os.environ["XDG_RUNTIME_DIR"])/"lucent-menu.sock");print("available")').strip()=='available'
            keyboards=json.loads(v.session('hyprctl','-j','devices'))['keyboards']
            primary=[keyboard for keyboard in keyboards if keyboard.get('main') is True]
            assert len(primary)==1 and primary[0].get('layout')=='us' and primary[0].get('capsLock') is False
            bindings=json.loads(v.session('hyprctl','-j','binds'))
            tested={0:set('abcdefghijklmnopqrstuvwxyz')|{'space','return','enter','escape','esc','tab','home','end','pageup','prior','pagedown','next','backspace','mouse:272','mouse:left'},4:{'a'}}
            plain_codes={1,14,15,28,30,102,104,107,109,272}
            plain_codes.update([16,17,18,19,20,21,22,23,24,25,30,31,32,33,34,35,36,37,38,44,45,46,47,48,49,50])
            for binding in bindings:
                mask=binding.get('modmask',0)&~(2|16)
                if mask not in tested:continue
                symbol=str(binding.get('key','')).lower()
                assert not binding.get('catch_all') and symbol not in tested[mask] and symbol not in ('any','catchall')
                if mask==0:assert binding.get('keycode',0) not in plain_codes|{code+8 for code in plain_codes}
                if symbol.startswith('code:'):assert int(symbol[5:]) not in (plain_codes|{code+8 for code in plain_codes} if mask==0 else {30,38})
            result['phase']='open_launcher'
            move((40,100));move((80,140))
            key(125,57);opened=True
            wait(lambda:desktop()['launcher'] and not desktop()['launcher_opening'] and surface()['focus']=='launcher-search','native_launcher_not_ready')
            query('')
            count=desktop()['result_count'];rows=[hit for hit in surface()['hits'] if hit['id'].startswith('result-')]
            page=len(rows);assert count>page>0
            result['phase']='launcher_page'
            key(109)
            wait(lambda:desktop()['selected']==min(page,count-1),'launcher_page_down_failed')
            assert any(hit['id']=='result-'+desktop()['selected_application'] for hit in surface()['hits'])
            selected=desktop()['selected_application']
            result['phase']='section_context'
            query('a')
            filtered_count=desktop()['result_count']
            filtered_page=len([hit for hit in surface()['hits'] if hit['id'].startswith('result-')])
            assert filtered_count>filtered_page
            key(109);wait(lambda:desktop()['selected']==filtered_page,'filtered_result_page_failed')
            retained=desktop()['selected_application'];retained_query=desktop()['query']
            for mode in ['Commands','Wallpapers','Themes','Widgets','Apps']:
                key(15);wait(lambda:desktop()['mode']==mode,'section_switch_failed')
            assert desktop()['query']==retained_query and desktop()['selected_application']==retained
            assert desktop()['selected']==filtered_page
            assert any(hit['id']=='result-'+retained for hit in surface()['hits'])
            result['checks']['page_navigation_and_section_context']=True
            result['phase']='editor_home'
            query('terminal');key(102);key(30)
            wait(lambda:desktop()['query']=='aterminal','home_key_did_not_edit_search_caret')
            result['checks']['search_home_remains_editor_navigation']=True
            result['phase']='empty_result'
            query('zzzzzzzzzzzzzz');wait(lambda:desktop()['result_count']==0,'empty_query_failed')
            key(28);assert desktop()['launcher']
            result['checks']['empty_result_enter_is_safe']=True
            query('')
            result['phase']='wheel_scroll'
            native=surface();viewport=next(hit for hit in native['hits'] if hit['id'].startswith('launcher-results-'))
            move(point(viewport,v.layer_geometry(native['id'])))
            first_before=next(hit['id'] for hit in surface()['hits'] if hit['id'].startswith('result-'))
            v.send([{'type':'btn','data':{'button':'wheel-down','down':down}} for down in (True,False)])
            wait(lambda:next(hit['id'] for hit in surface()['hits'] if hit['id'].startswith('result-'))!=first_before,'native_wheel_did_not_scroll')
            result['checks']['native_wheel_scroll']=True
            query('')
            result['phase']='launcher_thumb'
            native=surface();thumb_drag(native,v.layer_geometry(native['id']),'launcher-results-',count,v.token_value('component.launcher.row_height'),v.token_value('component.scroll.minimum_thumb'))
            wait(lambda:desktop()['selected']>=count-page,'native_launcher_thumb_did_not_reach_end')
            assert surface()['focus']=='launcher-search'
            capture('launcher-last-page')
            result['checks']['native_thumb_drag_retains_search_focus']=True
            result['phase']='fresh_and_outside'
            query('retained')
            key(1);wait(lambda:not desktop()['launcher'],'escape_did_not_dismiss');opened=False
            key(125,57);opened=True
            wait(lambda:desktop()['launcher'] and desktop()['query']=='','fresh_open_did_not_clear_query')
            move((40,180));button(True);button(False)
            wait(lambda:not desktop()['launcher'],'outside_click_did_not_dismiss');opened=False
            result['checks']['escape_outside_and_fresh_open']=True
            result['phase']='picker_stage'
            unit='lucent-launcher-test-'+tag+'.service'
            folder=v.remote('python3','-c','import sys;from pathlib import Path;p=Path.home()/".cache"/sys.argv[1];p.mkdir(mode=0o700);print(p)','lucent-launcher-test-'+tag).strip()
            disabled=set(range(1,17))
            request={'prompt':'Lucent paging fixture','entries':[{'label':f'Entry {i:02}','detail':'Owned paging fixture','value':f'fixture-{i}','disabled':i in disabled} for i in range(64)],'max_height':500}
            program=GUEST
            for name,data in [('guest.py',program.encode()),('request.json',json.dumps(request).encode())]:
                subprocess.run(v.lab.ssh_args('lucent')+[shlex.join(['python3','-c','import os,sys;fd=os.open(sys.argv[1],os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600);f=os.fdopen(fd,"wb");f.write(sys.stdin.buffer.read(262145));f.close()',folder+'/'+name])],input=data,check=True,capture_output=True,timeout=15)
            v.session('systemd-run','--user','--quiet','--collect','--service-type=exec','--unit='+unit,'--property=RuntimeMaxSec=180','--property=TimeoutStopSec=10','--property=UMask=0077','/usr/bin/python3','-I','-B',folder+'/guest.py','start',folder,unit,expected['lucent-menu']);started=True
            wait(lambda:menu()['surfaces'][0]['native_frames']>0,'owned_menu_not_ready',30)
            result['phase']='picker_page'
            current=menu();page=len([hit for hit in current['surfaces'][0]['hits'] if hit['id'].startswith('menu-row-')])
            key(109);desired=page
            while desired in disabled:desired+=1
            wait(lambda:menu()['client']['selected']==desired,'independent_menu_page_failed')
            assert menu()['surfaces'][0]['focus']=='menu-search'
            previous=max(0,desired-page)
            while previous in disabled:previous-=1
            key(104);wait(lambda:menu()['client']['selected']==previous,'independent_menu_page_up_failed')
            # Move to the start before measuring and dragging its actual top thumb.
            for _ in range(4):key(104)
            wait(lambda:menu()['client']['start']==0,'independent_menu_start_failed')
            result['phase']='picker_thumb'
            current=menu();thumb_drag(current['surfaces'][0],menu_layer(),'menu-results-',64,v.token_value('component.menu.row_height')+v.token_value('space.xs'),v.token_value('component.scroll.minimum_thumb'))
            wait(lambda:menu()['client']['start']>=64-page,'independent_menu_thumb_failed')
            current=menu();chosen=current['client']['selected'];capture('picker-last-page',current['surfaces'][0])
            result['phase']='picker_accept'
            key(28)
            outcome=wait(lambda:guest('result'),'independent_menu_did_not_exit')
            assert outcome['output']==f'fixture-{chosen}\n'
            result['checks']['independent_picker_pages_disabled_rows_scroll_and_original_value']=True
            result['restoration'].update(guest('stop'));started=False
            result['restoration'].update(guest('remove'));folder=None
    except BaseException as error:
        result['failure_class']=type(error).__name__
        if isinstance(error,subprocess.CalledProcessError) and error.stderr:
            data=error.stderr.encode() if isinstance(error.stderr,str) else error.stderr
            with os.fdopen(os.open(output/'private-stderr.log',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'wb') as stream:stream.write(data[:16384])
        if isinstance(error,AssertionError):result['failed_check']=str(error)[:160]
    finally:
        if button_down:
            try:button(False)
            except BaseException:result['restoration']['owned_pointer_released']=False
        if started:
            try:result['restoration'].update(guest('stop'));started=False
            except BaseException as error:result['restoration']['owned_menu_retired']=False
        if folder is not None and not started:
            try:result['restoration'].update(guest('remove'));folder=None
            except BaseException:result['restoration']['owned_stage_removed']=False
        if opened:
            try:
                guard();v.cli('launcher','close');wait(lambda:not desktop()['launcher'],'launcher_cleanup_failed')
            except BaseException:result['restoration']['launcher_closed']=False
        try:
            move(cursor.initial)
            result['restoration']['original_cursor']=tuple(position()[key] for key in ('x','y'))==cursor.initial
        except BaseException:result['restoration']['original_cursor']=False
        try:result['restoration']['desktop_preferences_wallpaper_exports_and_process']=fingerprint(v.session)==baseline
        except BaseException:result['restoration']['desktop_preferences_wallpaper_exports_and_process']=False
        result['passed']=len(result['checks'])==7 and all(result['checks'].values()) and bool(result['restoration']) and all(result['restoration'].values())
        private(output/'report.json',result)
    print(json.dumps({'passed':result['passed'],'private_report':str((output/'report.json').relative_to(ROOT)),'checks':result['checks'],'restoration':result['restoration'],'failed_check':result.get('failed_check'),'failure_class':result.get('failure_class'),'phase':result.get('phase')}))
    return 0 if result['passed'] else 1

if __name__=='__main__':raise SystemExit(main())
