#!/usr/bin/env python3
"""Check real launcher app activation on the idle, unlocked FHD Lucent VM.

Stages two bounded foot processes through the production application adapter.
Real QMP keys must reach their PTYs after keyboard launch, Escape and pointer
launch. No compositor command forces focus. Requires a qualified release bundle,
plain US input and no existing application clients. Private receipts include
exact source/binary identity and restoration of owned artifacts and user state.
"""
import argparse,hashlib,importlib.util,json,os,runpy,shlex,signal,subprocess,sys,time,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'vm'),str(ROOT/'scripts')]
from application_intent_policy import CursorOwnership
from preservation_fingerprint import fingerprint
from lucent_dev.model import load_bundle
spec=importlib.util.spec_from_file_location('launcher_app_framework',ROOT/'vm/test-framework.py')
v=importlib.util.module_from_spec(spec);spec.loader.exec_module(v)
def wait(fn,label,seconds=20):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   value=fn()
   if value:return value
  except (KeyError,ValueError,subprocess.CalledProcessError):pass
  time.sleep(.1)
 raise AssertionError(label)
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--bundle',type=Path,required=True);args=p.parse_args()
 manifest=load_bundle(args.bundle.resolve(strict=True));expected=manifest['binaries']['lucent-desktop']['sha256']
 assert json.loads(v.session('hyprctl','-j','clients'))==[]
 original=v.inspect()['client'];assert not original['launcher'] and not original['query']
 peer_code='''import hashlib,json,os,socket,struct
from pathlib import Path
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(5);s.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'lucent.sock'));pid,uid,_=struct.unpack('3i',s.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert uid==os.getuid();s.sendall(b'inspect\\n');data=bytearray()
 while chunk:=s.recv(65536):data.extend(chunk);assert len(data)<=8*1024*1024
p=Path('/proc')/str(pid)
print(json.dumps({'pid':pid,'birth':(p/'stat').read_text().rsplit(')',1)[1].split()[19],'sha':hashlib.sha256((p/'exe').read_bytes()).hexdigest()}))
'''
 identity=json.loads(v.session('python3','-c',peer_code));assert identity['sha']==expected
 monitors=json.loads(v.session('hyprctl','-j','monitors'));assert len(monitors)==1
 assert all(monitors[0][key]==value for key,value in {'width':1920,'height':1080,'scale':1,'x':0,'y':0,'transform':0}.items())
 before=fingerprint(v.session);workspace=json.loads(v.session('hyprctl','-j','activeworkspace'))
 cursor=CursorOwnership(json.loads(v.session('hyprctl','-j','cursorpos')))
 tag=uuid.uuid4().hex[:24];output=ROOT/'reports/local'/('launcher-application-'+tag);output.mkdir(mode=0o700)
 result={'source_commit':manifest['source_commit'],'desktop_sha256':expected,'passed':False,'checks':{},'restoration':{},'controller_forced_focus':False}
 folder=v.remote('python3','-c','import sys;from pathlib import Path;p=Path.home()/".cache"/sys.argv[1];p.mkdir(mode=0o700);print(p)','lucent-launcher-application-'+tag).strip()
 initialized=False;opened=False
 def durable_cursor():
  path=output/'.cursor.next'
  with os.fdopen(os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'w') as stream:
   json.dump({'original':cursor.initial,'expected':cursor.expected,'pending':cursor.pending,'lost':cursor.lost,'desktop':identity,'guest_stage':folder,'tag':tag},stream);stream.write('\n');stream.flush();os.fsync(stream.fileno())
  os.replace(path,output/'cursor.json')
  descriptor=os.open(output,os.O_RDONLY|os.O_DIRECTORY)
  try:os.fsync(descriptor)
  finally:os.close(descriptor)
 def operation(op):
  return json.loads(v.session('/usr/bin/python3','-I','-B',folder+'/fixture.py',op,folder,tag))
 def state():return operation('state')
 def clients():return json.loads(v.session('hyprctl','-j','clients'))
 def capture_launcher(label):
  snapshot=v.inspect();dock=next(s for s in snapshot['surfaces'] if s.get('role',s['id'])=='dock')
  hit=next(h for h in dock['hits'] if h['id']=='dock-panel');layer=v.layer_geometry(dock['id'])
  geometry=f"{round(layer['x']+hit['x'])},{round(layer['y']+hit['y'])} {round(hit['width'])}x{round(hit['height'])}"
  reply=subprocess.run(v.lab.ssh_args('lucent')+[shlex.join(['python3','desktop-lab/scripts/in-session.py','grim','-g',geometry,'-'])],capture_output=True,timeout=20,check=True)
  assert reply.stdout.startswith(b'\x89PNG\r\n\x1a\n') and len(reply.stdout)<=16*1024*1024
  with os.fdopen(os.open(output/(label+'.png'),os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'wb') as stream:stream.write(reply.stdout)
  result.setdefault('private_captures',{})[label]=hashlib.sha256(reply.stdout).hexdigest()
 def guard(allow_foreground=True):
  assert v.session('python3','-c','import subprocess;assert subprocess.run(["omarchy-hyprland-session-locked"],capture_output=True).returncode==1;print("ok")').strip()=='ok'
  assert json.loads(v.session('python3','-c',peer_code))==identity
  current=state()['events'] if initialized else []
  allowed={event['pid'] for event in current if not event['drained']}
  current_clients=clients();assert all(client['pid'] in allowed and client['class']==entry['class'] for client in current_clients)
  def own_layers(value):
   if isinstance(value,dict):
    if 'namespace' in value:assert value.get('pid')==identity['pid']
    for child in value.values():own_layers(child)
   elif isinstance(value,list):
    for child in value:own_layers(child)
  own_layers(json.loads(v.session('hyprctl','-j','layers')))
  if not allow_foreground:assert current_clients==[]
  return current_clients
 def move(point):
  guard();point=tuple(round(x) for x in point)
  current=json.loads(v.session('hyprctl','-j','cursorpos'))
  if cursor.observe(current)==point:return
  cursor.begin(current,point);durable_cursor();v.move(*point)
  wait(lambda:[json.loads(v.session('hyprctl','-j','cursorpos'))[key] for key in ('x','y')]==list(point),'owned_cursor_move_failed')
  cursor.finish(json.loads(v.session('hyprctl','-j','cursorpos')))
  durable_cursor()
 def key(*codes):
  guard()
  snapshot=v.inspect()
  if opened:
   dock=next(surface for surface in snapshot['surfaces'] if surface.get('role',surface['id'])=='dock')
   assert snapshot['client']['launcher'] and dock['actual_native_keyboard_focused']
  else:assert all(surface['committed_keyboard']=='none' for surface in snapshot['surfaces'])
  v.key(*codes)
 def open_query():
  nonlocal opened
  key(125,57);opened=True
  def ready():
   snapshot=v.inspect();dock=next(surface for surface in snapshot['surfaces'] if surface.get('role',surface['id'])=='dock')
   return snapshot['client']['launcher'] and not snapshot['client']['launcher_opening'] and dock['actual_native_keyboard_focused'] and dock['focus']=='launcher-search'
  wait(ready,'launcher_not_open')
  key(29,30);key(14)
  for char in entry['name'].lower():guard();v.text(char)
  wait(lambda:v.inspect()['client']['result_count']==1 and v.inspect()['client']['selected_application']==entry['app_id'],'owned_application_not_selected')
 def owned_foreground(index):
  observed=state()['events'];assert len(observed)<=index+1
  if len(observed)!=index+1 or observed[index]['drained']:return False
  current=guard();matches=[client for client in current if client['pid']==observed[index]['pid']]
  if len(matches)!=1:return False
  active=json.loads(v.session('hyprctl','-j','activewindow'))
  return (active.get('pid')==observed[index]['pid'] and active.get('address')==matches[0]['address'] and active.get('class')==entry['class'])
 def typed_a(index,count):
  assert owned_foreground(index)
  assert not v.inspect()['client']['launcher']
  assert all(surface['committed_keyboard']=='none' for surface in v.inspect()['surfaces'])
  key(30);key(28)
  wait(lambda:state()['events'][index]['lines']==count,'foreground_application_did_not_receive_key_a')
 def interrupted(*_):
  signal.signal(signal.SIGTERM,signal.SIG_IGN);signal.signal(signal.SIGINT,signal.SIG_IGN)
  raise RuntimeError('owned_application_controller_interrupted')
 signal.signal(signal.SIGTERM,interrupted);signal.signal(signal.SIGINT,interrupted)
 durable_cursor()
 try:
  with runpy.run_path(str(ROOT/'vm/measure-framework.py'))['stay_awake'](v.remote):
   devices=json.loads(v.session('hyprctl','-j','devices'))['keyboards'];primary=[item for item in devices if item.get('main')]
   assert len(primary)==1 and primary[0].get('layout')=='us' and primary[0].get('capsLock') is False
   bindings=json.loads(v.session('hyprctl','-j','binds'))
   plain=set('abcdefghijklmnopqrstuvwxyz')|{'space','return','enter','escape','esc','backspace','mouse:272','mouse:left'}
   plain_codes={1,14,28,30,57,272}
   plain_codes.update([16,17,18,19,20,21,22,23,24,25,30,31,32,33,34,35,36,37,38,44,45,46,47,48,49,50])
   for binding in bindings:
    mask=binding.get('modmask',0)&~(2|16);symbol=str(binding.get('key','')).lower()
    if mask not in (0,4):continue
    assert not binding.get('catch_all') and symbol not in ('any','catchall') and symbol not in (plain if mask==0 else {'a'})
    codes=plain_codes|{code+8 for code in plain_codes} if mask==0 else {30,38}
    assert binding.get('keycode',0) not in codes
    if symbol.startswith('code:'):assert int(symbol[5:]) not in codes
   for source,name in [(ROOT/'vm/fixtures/launcher-application-fixture.py','fixture.py'),(ROOT/'vm/owned_application_units.py','owned_application_units.py')]:
    subprocess.run(v.lab.ssh_args('lucent')+[shlex.join(['python3','-c','import os,sys;fd=os.open(sys.argv[1],os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600);f=os.fdopen(fd,"wb");f.write(sys.stdin.buffer.read(262145));f.close()',folder+'/'+name])],input=source.read_bytes(),capture_output=True,check=True,timeout=20)
   entry=operation('initialize');initialized=True
   for index,method in enumerate(('keyboard','pointer')):
    result['phase']=method+'_launch'
    guard(False);open_query()
    wait(lambda:any(h['id']=='result-'+entry['app_id'] for s in v.inspect()['surfaces'] for h in s['hits']),'owned_result_not_prepared')
    capture_launcher(method+'-launcher')
    if method=='keyboard':key(28)
    else:
     target=v.hit('result-'+entry['app_id']);move((target['x']+target['width']/2,target['y']+target['height']/2));v.button(True)
     try:v.button(False)
     finally:v.button(False)
    wait(lambda:not v.inspect()['client']['launcher'],'launcher_did_not_release_after_launch');opened=False
    wait(lambda:owned_foreground(index),'launched_owned_app_not_foreground',30)
    typed_a(index,1)
    result['checks'][method+'_launch_foreground_and_physical_key_delivery']=True
    if index==0:
     birth=state()['events'][0]['birth'];open_query();key(1);wait(lambda:not v.inspect()['client']['launcher'],'escape_did_not_release_launcher');opened=False
     wait(lambda:owned_foreground(0),'escape_did_not_return_to_existing_app')
     assert len(state()['events'])==1 and state()['events'][0]['birth']==birth
     typed_a(0,2);result['checks']['escape_returns_input_to_same_application']=True
    result['restoration'].update(operation('retire'));wait(lambda:clients()==[],'owned_application_windows_not_retired')
   result['restoration'].update(operation('cleanup'));initialized=False
 except BaseException as error:
  result['failure_class']=type(error).__name__
  if isinstance(error,AssertionError):result['failed_check']=str(error)[:160]
  if isinstance(error,subprocess.CalledProcessError) and error.stderr:
   raw=error.stderr.encode() if isinstance(error.stderr,str) else error.stderr
   with os.fdopen(os.open(output/'private-stderr.log',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'wb') as stream:stream.write(raw[:16384])
 finally:
  if initialized:
   try:result['restoration'].update(operation('retire'));result['restoration'].update(operation('cleanup'));initialized=False
   except BaseException:result['restoration']['owned_application_cleanup']=False
  if opened:
   try:guard();v.cli('launcher','close');wait(lambda:not v.inspect()['client']['launcher'],'close_failed')
   except BaseException:result['restoration']['launcher_closed']=False
  try:move(cursor.initial);result['restoration']['original_cursor']=True
  except BaseException:result['restoration']['original_cursor']=False
  try:result['restoration']['desktop_preferences_wallpaper_exports_and_process']=fingerprint(v.session)==before
  except BaseException:result['restoration']['desktop_preferences_wallpaper_exports_and_process']=False
  current=json.loads(v.session('hyprctl','-j','activeworkspace'));result['restoration']['original_workspace']=all(current[key]==workspace[key] for key in ('id','name','monitor'))
  result['passed']=len(result['checks'])==3 and all(result['checks'].values()) and bool(result['restoration']) and all(result['restoration'].values())
  with os.fdopen(os.open(output/'report.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'w') as stream:json.dump(result,stream,indent=2);stream.write('\n')
 print(json.dumps({'passed':result['passed'],'checks':result['checks'],'restoration':result['restoration'],'phase':result.get('phase'),'failure_class':result.get('failure_class'),'failed_check':result.get('failed_check'),'private_report':str((output/'report.json').relative_to(ROOT))}))
 return 0 if result['passed'] else 1
if __name__=='__main__':raise SystemExit(main())
