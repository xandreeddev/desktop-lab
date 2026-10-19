#!/usr/bin/env python3
"""Reversible native Notes acceptance; importing this file performs no actions.

Run after deploying the supplied release identity to the existing, unlocked,
single-output Lucent VM. Requires an idle desktop, no ordinary application
windows on any workspace and an English US main keyboard with Caps Lock off.
Uses actual pointer/keyboard delivery, public IPC and ordinary text selection.
Original Notes, clipboard, settings and captures stay in private checkpoints.

The clipboard probe supports empty selection or bounded UTF-8 text aliases with
identical payloads. Other MIME/provider states are reported as unsupported and
left alone. An active history watcher is paused for the fixture; its actual file
and referenced images are checked and its original state restored. Text clipboard
payload/MIME interoperability is restored, not the original provider process.

Finally stops only lucent.service before guarded exact settings restoration,
then starts the normal service and requires 35 seconds of stable readiness.
Independent settings, clipboard or history changes block their restoration and
retain checkpoints for review. No VM reboot, force stop, secure UI or wallpaper
selection is involved. Transient Notes history/selection/scroll are not restored.
"""
import argparse
import base64
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import signal
import subprocess
import time


ROOT = Path(__file__).resolve().parents[1]
SHIFT, CONTROL = 1, 4
LAYOUT_KEYS = ('visible_widgets', 'positions', 'widget_sizes')
LETTERS = dict(zip('qwertyuiopasdfghjklzxcvbnm',
                   (16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32,
                    33, 34, 35, 36, 37, 38, 44, 45, 46, 47, 48, 49, 50)))


class CheckFailure(Exception):
    """Fixed labels only; original text must never enter an exception."""


def require(condition, label):
    if not condition:
        raise CheckFailure(label)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    require(spec is not None and spec.loader is not None, 'helper_unavailable')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def private(path, value):
    data = value if isinstance(value, bytes) else (json.dumps(value, indent=2) + '\n').encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)


SERVICE_QUIESCENCE = r'''
import json,os,stat,subprocess
from pathlib import Path
def _service_properties():
 argv=['systemctl','--user','show','lucent.service']
 for name in ('ActiveState','SubState','MainPID','ControlPID','ControlGroup','Job'):argv += ['-p',name]
 result=subprocess.run(argv,capture_output=True,text=True,timeout=6);assert result.returncode==0,'service_properties_unavailable'
 return dict(line.split('=',1) for line in result.stdout.splitlines() if '=' in line)
def _service_cgroup(group):
 uid=os.getuid();prefix=f'/user.slice/user-{uid}.slice/user@{uid}.service/'
 assert isinstance(group,str) and group.startswith(prefix) and group.endswith('/lucent.service'),'exact_user_unit_cgroup_required'
 parts=group.split('/')[1:];assert all(part and part not in ('.','..') for part in parts),'canonical_cgroup_components_required'
 root=Path('/sys/fs/cgroup');path=root.joinpath(*parts)
 assert root.resolve(strict=True)==root and path.resolve(strict=False)==path,'canonical_cgroup_path_required'
 return path
def capture_service_scope():
 value=_service_properties();assert value.get('ActiveState')=='active' and value.get('SubState')=='running','active_unit_required_before_stop'
 pid=int(value['MainPID']);assert pid>0,'active_main_pid_required'
 group=value['ControlGroup'];path=_service_cgroup(group)
 assert stat.S_ISDIR(path.lstat().st_mode),'active_unit_cgroup_required'
 assert str(pid) in (path/'cgroup.procs').read_text().split(),'main_pid_belongs_to_exact_cgroup'
 return {'unit':'lucent.service','uid':os.getuid(),'control_group':group}
def require_service_quiescent(scope):
 assert scope.get('unit')=='lucent.service' and scope.get('uid')==os.getuid(),'owned_service_scope_required'
 path=_service_cgroup(scope['control_group']);value=_service_properties()
 assert value.get('ActiveState') in ('inactive','failed') and value.get('SubState') in ('dead','failed'),'service_not_stopped'
 assert value.get('MainPID')=='0' and value.get('ControlPID')=='0','service_process_or_control_process_still_present'
 assert value.get('ControlGroup','') in ('',scope['control_group']),'service_cgroup_changed'
 assert value.get('Job','0') in ('','0'),'service_job_still_pending'
 processes=subprocess.run(['pgrep','-x','lucent-desktop'],capture_output=True,timeout=5)
 assert processes.returncode==1 and not processes.stdout,'desktop_process_still_present'
 try:info=path.lstat()
 except FileNotFoundError:return {'stopped_service':True,'no_desktop_process':True,'exact_cgroup_absent_or_empty':True}
 assert stat.S_ISDIR(info.st_mode),'exact_cgroup_directory_required'
 pending=[path];count=0
 while pending:
  current=pending.pop();count+=1;assert count<=128,'bounded_cgroup_tree_required'
  assert current.resolve(strict=True)==current,'canonical_recursive_cgroup_required'
  assert not (current/'cgroup.procs').read_text().strip(),'cgroup_processes_remain'
  assert not (current/'cgroup.threads').read_text().strip(),'cgroup_threads_remain'
  events=dict(line.split() for line in (current/'cgroup.events').read_text().splitlines())
  assert events.get('populated')=='0','cgroup_descendants_populated'
  with os.scandir(current) as entries:
   for entry in entries:
    assert not entry.is_symlink(),'cgroup_symlink_not_allowed'
    if entry.is_dir(follow_symlinks=False):pending.append(Path(entry.path))
 return {'stopped_service':True,'no_desktop_process':True,'exact_cgroup_absent_or_empty':True}
'''


def capture_desktop_scope(session):
    """Read active unit identity before stop; keep the returned scope private."""
    return json.loads(session('python3', '-c', SERVICE_QUIESCENCE + '\nprint(json.dumps(capture_service_scope()))'))


def verify_desktop_quiescence(session, scope):
    """Read-only proof of process/cgroup quiescence, accepting a stopped failure."""
    code = SERVICE_QUIESCENCE + '\nprint(json.dumps(require_service_quiescent(json.load(__import__("sys").stdin))))'
    return json.loads(session('python3', '-c', code, data=json.dumps(scope).encode()))


CHECKPOINT = r'''
import base64,hashlib,json,os,shutil,stat,sys
from pathlib import Path
operation,tag,monitor=sys.argv[1:]
assert tag.isalnum() and len(tag)==24
base=Path.home()/'.local/state/lucent'; directory=base/('.native-notes-'+tag)
settings=base/'desktop.json'; marker=Path.home()/'.local/state/omarchy/indicators/stay-awake'
def digest(data):return hashlib.sha256(data).hexdigest()
def identity(v):return [v.st_dev,v.st_ino,v.st_mode,v.st_size,v.st_mtime_ns,v.st_uid]
def read(path,limit=4*1024*1024):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
 with os.fdopen(fd,'rb') as stream:
  before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_size<=limit
  data=stream.read(limit+1);assert len(data)<=limit and identity(before)==identity(os.fstat(stream.fileno()))
  return data,identity(before)
def hashed(path,limit):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
 with os.fdopen(fd,'rb') as stream:
  before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_size<=limit
  value=hashlib.sha256();count=0
  for block in iter(lambda:stream.read(1048576),b''):count+=len(block);assert count<=limit;value.update(block)
  assert count==before.st_size and identity(before)==identity(os.fstat(stream.fileno()))
  return {'sha256':value.hexdigest(),'identity':identity(before)}
def write(path,data):
 fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'wb') as stream:os.fchmod(stream.fileno(),0o600);stream.write(data)
def external(saved):
 links={}
 for path in (base/'wallpaper',Path.home()/'.local/state/omarchy/current/background'):
  if path.is_symlink():links[str(path)]={'target':os.readlink(path),'identity':identity(path.lstat())}
  else:assert not path.exists();links[str(path)]=None
 wallpaper=base/'wallpaper';target=wallpaper.resolve(strict=True)
 source=None;live=saved.get('live_wallpapers',{});profile=live.get('monitors',{}).get(monitor,live.get('default'))
 if profile:source=hashed(Path(profile['asset']['path']),512*1024*1024)
 exports={};paths=list((base/'theme').glob('*'))+list((Path('/var/lib/lucent/wallpapers')/str(os.getuid())).glob('*'))
 paths += [Path.home()/'.config/omarchy/themes/lucent-tokens-active/colors.toml']
 for path in paths:
  if path.is_file() and not path.is_symlink():exports[str(path)]=hashed(path,128*1024*1024)['sha256']
 return {'links':links,'wallpaper':hashed(target,128*1024*1024),'source':source,'exports':exports}
if operation=='create':
 content,original_id=read(settings);assert original_id[-1]==os.getuid();saved=json.loads(content)
 directory.mkdir(mode=0o700)
 try:
  os.link(settings,directory/'original.link',follow_symlinks=False);write(directory/'original.bytes',content)
  value={'settings':saved,'settings_identity':original_id,'settings_sha256':digest(content),'external':external(saved),
         'marker_exists':marker.exists(),'marker_identity':identity(marker.lstat()) if marker.exists() else None,'owned_marker':None}
  write(directory/'manifest.json',json.dumps(value).encode());print(json.dumps(value))
 except BaseException:
  # No shared preference/idle/clipboard mutation precedes this completed
  # checkpoint. Remove only this invocation's newly created private directory.
  shutil.rmtree(directory);raise
else:
 info=directory.lstat();assert stat.S_ISDIR(info.st_mode) and info.st_uid==os.getuid() and stat.S_IMODE(info.st_mode)==0o700
 value=json.loads((directory/'manifest.json').read_text())
 if operation=='read':
  data,current_id=read(settings);print(json.dumps({'settings':json.loads(data),'identity':current_id,'sha256':digest(data)}))
 elif operation=='awake':
  if not value['marker_exists']:
   marker.parent.mkdir(parents=True,exist_ok=True);fd=os.open(marker,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600);os.close(fd)
   value['owned_marker']=identity(marker.lstat())
   try:(directory/'manifest.json').write_text(json.dumps(value));os.chmod(directory/'manifest.json',0o600)
   except BaseException:
    assert identity(marker.lstat())==value['owned_marker'];marker.unlink();raise
 elif operation=='restore':
  allowed=json.load(sys.stdin);content,current_id=read(settings);current=json.loads(content)
  assert any({k:v for k,v in current.items() if k!='notes'}==layout for layout in allowed['layouts'])
  assert current.get('notes') in allowed['notes'];assert external(current)==value['external']
  original,original_id=read(directory/'original.link');assert digest(original)==value['settings_sha256'] and original_id==value['settings_identity']
  assert (directory/'original.bytes').read_bytes()==original
  require_service_quiescent(allowed['service_scope'])
  temporary=base/('.native-notes-restore-'+tag);os.link(directory/'original.link',temporary,follow_symlinks=False)
  assert identity(settings.lstat())==current_id;os.replace(temporary,settings)
  print(json.dumps({'settings_restored':settings.read_bytes()==original,'identity_restored':identity(settings.lstat())==original_id}))
 elif operation=='verify':
  content,current_id=read(settings);assert digest(content)==value['settings_sha256'] and current_id==value['settings_identity']
  assert content==(directory/'original.bytes').read_bytes() and external(json.loads(content))==value['external']
  print(json.dumps({'exact_settings_bytes_and_identity':True,'wallpaper_source_exports_unchanged':True}))
 elif operation=='idle-restore':
  if value['owned_marker'] is not None:
   assert identity(marker.lstat())==value['owned_marker'];marker.unlink()
  if value['marker_exists']:assert identity(marker.lstat())==value['marker_identity']
  else:assert not marker.exists()
 elif operation=='discard':
  assert settings.read_bytes()==(directory/'original.bytes').read_bytes();shutil.rmtree(directory)
 else:raise ValueError('checkpoint_operation')
'''


CLIPBOARD = r'''
import base64,hashlib,json,os,selectors,stat,subprocess,sys,time
from pathlib import Path
operation,tag=sys.argv[1:];assert tag.isalnum() and len(tag)==24
directory=Path.home()/'.local/state/lucent'/('.native-notes-'+tag);info=directory.lstat()
assert stat.S_ISDIR(info.st_mode) and info.st_uid==os.getuid() and stat.S_IMODE(info.st_mode)==0o700
manifest=directory/'clipboard.json';history=Path.home()/'.local/state/lucent/clipboard/history.json'
aliases={'text/plain;charset=utf-8','text/plain;charset=UTF-8','text/plain','UTF8_STRING','STRING','TEXT'}
def run(argv,**kwargs):return subprocess.run(argv,capture_output=True,timeout=5,**kwargs)
def bounded_offer(argv,limit):
 child=subprocess.Popen(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 selector=selectors.DefaultSelector();selector.register(child.stdout,selectors.EVENT_READ,0);selector.register(child.stderr,selectors.EVENT_READ,1)
 output=[bytearray(),bytearray()];deadline=time.monotonic()+3
 try:
  for stream in (child.stdout,child.stderr):os.set_blocking(stream.fileno(),False)
  while selector.get_map():
   assert time.monotonic()<deadline
   for key,mask in selector.select(.05):
    data=os.read(key.fd,4096)
    if not data:selector.unregister(key.fileobj);continue
    output[key.data].extend(data);assert len(output[key.data])<=(limit if key.data==0 else 4096)
  return child.wait(timeout=1),bytes(output[0]),bytes(output[1])
 finally:
  selector.close()
  if child.poll() is None:child.kill();child.wait(timeout=1)
  child.stdout.close();child.stderr.close()
def read_offer():
 code,stdout,stderr=bounded_offer(['wl-paste','--list-types'],16384)
 if code==1 and b'Nothing is copied' in stderr:return {'empty':True,'types':[],'payload':b''}
 assert code==0
 types=stdout.decode().splitlines()
 if not types or not set(types)<=aliases:return {'unsupported':'nontext_or_sensitive_mime'}
 data=[]
 for mime in types:
  code,stdout,stderr=bounded_offer(['wl-paste','--no-newline','--type',mime],65536);assert code==0
  try:stdout.decode('utf-8')
  except UnicodeDecodeError:return {'unsupported':'non_utf8_clipboard'}
  data.append(stdout)
 if any(value!=data[0] for value in data):return {'unsupported':'different_mime_payloads'}
 return {'empty':False,'types':types,'payload':data[0]}
def sha(data):return hashlib.sha256(data).hexdigest()
def bounded_file(path,limit):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
 with os.fdopen(fd,'rb') as stream:
  info=os.fstat(stream.fileno());assert stat.S_ISREG(info.st_mode) and info.st_uid==os.getuid() and info.st_size<=limit
  data=stream.read(limit+1);after=os.fstat(stream.fileno())
  assert len(data)<=limit and (info.st_dev,info.st_ino,info.st_size,info.st_mtime_ns)==(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns)
  return data
def history_bytes():
 try:return bounded_file(history,2*1024*1024)
 except FileNotFoundError:return None
def write(path,data):
 fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'wb') as stream:os.fchmod(stream.fileno(),0o600);stream.write(data)
def hist_images():
 paths=list(history.parent.glob('*.png'));assert len(paths)<=128
 result={}
 for p in paths:
  data=bounded_file(p,2*1024*1024);info=p.lstat()
  result[str(p)]={'sha256':sha(data),'identity':[info.st_dev,info.st_ino,info.st_mode,info.st_size,info.st_mtime_ns]}
 return result
def save(value):manifest.write_text(json.dumps(value));os.chmod(manifest,0o600)
if operation=='create':
 try:offer=read_offer()
 except (OSError,AssertionError,subprocess.TimeoutExpired):offer={'unsupported':'clipboard_tools_capacity_or_offer_unavailable'}
 if 'unsupported' in offer:print(json.dumps({'supported':False,'reason':offer['unsupported']}));raise SystemExit
 active=run(['systemctl','--user','is-active','lucent-clipboard.service']).stdout.strip()==b'active'
 before=history_bytes()
 items=json.loads(before) if before is not None else [];assert isinstance(items,list)
 original_id=hashlib.sha256(b'text\0'+offer['payload']).hexdigest()
 if active and not offer['empty'] and not any(item.get('id')==original_id and item.get('text')==offer['payload'].decode() for item in items):
  print(json.dumps({'supported':False,'reason':'unrecorded_text_cannot_preserve_active_history'}));raise SystemExit
 value={'empty':offer['empty'],'types':offer['types'],'original_sha256':sha(offer['payload']),'history_active':active,
        'history_exists':before is not None,'history_items':items,'history_images':hist_images(),'units':[],'supported':True}
 write(directory/'clipboard.original',offer['payload']);save(value)
 try:
  if active:assert run(['systemctl','--user','stop','lucent-clipboard.service']).returncode==0
  assert history_bytes()==before
  if before is not None:os.link(history,directory/'history.original',follow_symlinks=False);write(directory/'history.bytes',before)
 except BaseException:
  if active:run(['systemctl','--user','start','lucent-clipboard.service'])
  raise
 print(json.dumps({'supported':True,'empty':offer['empty'],'history_paused':active}))
else:
 value=json.loads(manifest.read_text())
 if operation=='read':
  offer=read_offer();assert 'unsupported' not in offer;print(json.dumps({'sha256':sha(offer['payload']),'empty':offer['empty']}))
 elif operation=='owned':
  allowed=json.load(sys.stdin);offer=read_offer();assert 'unsupported' not in offer and sha(offer['payload']) in allowed
  print(json.dumps({'owned_fixture_selection_current':True}))
 elif operation=='set':
  request=json.load(sys.stdin);data=base64.b64decode(request['content_base64'],validate=True);assert len(data)<=65536;data.decode('utf-8')
  offer=read_offer();assert 'unsupported' not in offer and sha(offer['payload']) in set(request['allowed'])|{value['original_sha256']}
  for unit in value['units']:run(['systemctl','--user','stop',unit])
  filename=directory/('clipboard.fixture-'+str(len(value['units'])));write(filename,data)
  unit='lucent-notes-clipboard-'+tag+'-'+str(len(value['units']))+'.service';value['units'].append(unit);save(value)
  code='import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY|os.O_NOFOLLOW); os.dup2(fd,0); os.close(fd); os.execvp("wl-copy",["wl-copy","--foreground","--type","text/plain;charset=utf-8"])'
  assert run(['systemd-run','--user','--quiet','--collect','--property=Type=exec','--unit',unit,'python3','-c',code,str(filename)]).returncode==0
 elif operation=='restore':
  allowed=json.load(sys.stdin);offer=read_offer();assert 'unsupported' not in offer
  assert sha(offer['payload']) in set(allowed)|{value['original_sha256']}
  for unit in value['units']:run(['systemctl','--user','stop',unit])
  if value['empty']:assert run(['wl-copy','--clear']).returncode==0
  else:
   with (directory/'clipboard.original').open('rb') as stream:assert run(['wl-copy','--type','text/plain;charset=utf-8'],stdin=stream).returncode==0
  offer=read_offer();assert 'unsupported' not in offer and offer['empty']==value['empty'] and sha(offer['payload'])==value['original_sha256']
  assert set(value['types'])<=set(offer['types'])
  if value['history_exists']:assert history_bytes()==(directory/'history.bytes').read_bytes()
  else:assert not history.exists()
  assert hist_images()==value['history_images']
  if value['history_active']:
   began=int(time.time()*1000);assert run(['systemctl','--user','start','lucent-clipboard.service']).returncode==0
   if not value['empty']:
    original=next(item for item in value['history_items'] if item['id']==hashlib.sha256(b'text\0'+(directory/'clipboard.original').read_bytes()).hexdigest())
    deadline=time.monotonic()+5
    while time.monotonic()<deadline:
     observed=history_bytes();items=json.loads(observed)
     if items and items[0].get('id')==original['id'] and items[0].get('capturedAt',0)>=began-20:break
     time.sleep(.1)
    else:raise ValueError('history_initial_offer_not_observed')
    first=dict(items[0]);first['capturedAt']=original['capturedAt']
    assert [first]+items[1:]==[original]+[item for item in value['history_items'] if item['id']!=original['id']]
    assert hist_images()==value['history_images'] and history_bytes()==observed
    temporary=directory/'history.restore';os.link(directory/'history.original',temporary,follow_symlinks=False);os.replace(temporary,history)
   time.sleep(.4)
  assert history_bytes()==((directory/'history.bytes').read_bytes() if value['history_exists'] else None)
  assert hist_images()==value['history_images']
  offer=read_offer();assert sha(offer['payload'])==value['original_sha256'] and offer['empty']==value['empty']
  assert (run(['systemctl','--user','is-active','lucent-clipboard.service']).stdout.strip()==b'active')==value['history_active']
  print(json.dumps({'clipboard_payload_and_text_mimes_restored':True,'history_file_and_images_restored':True,'history_service_restored':True}))
 elif operation=='verify':
  offer=read_offer();assert 'unsupported' not in offer and offer['empty']==value['empty'] and sha(offer['payload'])==value['original_sha256']
  assert set(value['types'])<=set(offer['types'])
  assert history_bytes()==((directory/'history.bytes').read_bytes() if value['history_exists'] else None)
  assert hist_images()==value['history_images']
  assert (run(['systemctl','--user','is-active','lucent-clipboard.service']).stdout.strip()==b'active')==value['history_active']
  print(json.dumps({'clipboard_history_and_helper_preserved_after_restart':True}))
 else:raise ValueError('clipboard_operation')
'''


class Harness:
    def __init__(self, args, directory):
        self.args, self.directory = args, directory
        self.tag = secrets.token_hex(12)
        common = self.run(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute', '--git-common-dir']).decode().strip()
        self.lab = load('notes_root_vm_lab', Path(common).parent / 'vm/lab.py')
        inventory = self.lab.inventory()
        require(inventory['uri'] == 'qemu:///session'
                and inventory['vms']['lucent']['name'] == 'desktop-lab-lucent'
                and inventory['vms']['lucent']['port'] == 2243
                and inventory['user'] == 'omarchy', 'existing_lucent_inventory_required')
        self.name = inventory['vms']['lucent']['name']
        self.native = load('notes_native_surfaces', ROOT / 'vm/native_surfaces.py')
        self.ready = load('notes_readiness', ROOT / 'scripts/lucent_readiness.py')
        self.layouts, self.notes, self.clipboard_hashes = [], set(), set()
        self.clipboard = {'supported': False, 'reason': 'not_attempted'}
        self.checkpointed = self.service_stopped = self.mutated = self.clipboard_restored = False
        self.service_scope = None

    def progress(self, phase, state='entered', check=None):
        """Only fixed phase/check labels; never Notes, clipboard or raw errors."""
        event = {'phase': phase, 'state': state, 'monotonic_seconds': time.monotonic()}
        if check is not None:
            event['check'] = check
        private(self.directory / ('progress-' + str(time.monotonic_ns()) + '.json'), event)

    def stop_quiescent(self):
        """Capture exact active cgroup, stop normally, then prove no writer lives.

        Root recovery may call this on an imported Harness, then checkpoint
        restore with its independently validated exact owned layout/fixture.
        """
        self.progress('stop_quiescent_capture')
        self.service_scope = capture_desktop_scope(self.session)
        private(self.directory / ('service-scope-' + secrets.token_hex(6) + '.json'), self.service_scope)
        self.service_stopped = True
        self.progress('stop_quiescent_stop')
        self.session('systemctl', '--user', 'stop', 'lucent.service')
        self.progress('stop_quiescent_verify')
        result = verify_desktop_quiescence(self.session, self.service_scope)
        require(all(result.values()), 'desktop_service_not_quiescent')
        self.progress('stop_quiescent_verify', 'passed')
        return result

    def run(self, argv, *, data=None, timeout=20, okay=False):
        try:
            value = subprocess.run(argv, input=data, capture_output=True, timeout=timeout)
        except (OSError, subprocess.TimeoutExpired) as error:
            private(self.directory / ('command-failure-' + secrets.token_hex(6) + '.log'),
                    (getattr(error, 'stdout', None) or b'') + b'\nSTDERR\n' + (getattr(error, 'stderr', None) or b''))
            raise CheckFailure('bounded_command_failed') from None
        if value.returncode and not okay:
            private(self.directory / ('command-failure-' + secrets.token_hex(6) + '.log'),
                    value.stdout + b'\nSTDERR\n' + value.stderr)
        require(okay or value.returncode == 0, 'bounded_command_failed')
        return value if okay else value.stdout

    def remote(self, *argv, data=None, timeout=25):
        return self.run(self.lab.ssh_args('lucent') + [shlex.join(argv)], data=data, timeout=timeout)

    def session(self, *argv, data=None, timeout=25):
        return self.remote('python3', 'desktop-lab/scripts/in-session.py', *argv, data=data, timeout=timeout)

    def cli(self, *args):
        return self.session('/home/omarchy/.local/bin/lucent-cli', *args)

    def input_guard(self):
        code = ('import json,subprocess; '
                'defrun=lambda argv:subprocess.check_output(argv,timeout=4); '
                'assert defrun(["omarchy-shell","lock","isLocked"]).strip()==b"false"; '
                'assert not json.loads(defrun(["hyprctl","-j","clients"])); '
                'assert b"lucent-authorization-" not in defrun(["hyprctl","-j","layers"]); print("safe")')
        require(self.session('python3', '-c', code, timeout=15).strip() == b'safe',
                'native_input_still_unlocked_without_clients_or_authorization')

    def inspect(self):
        return json.loads(self.cli('inspect'))

    def eventually(self, predicate, label, timeout=25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except (CheckFailure, KeyError, ValueError, LookupError):
                pass
            time.sleep(.15)
        raise CheckFailure(label)

    def checkpoint(self, operation, data=None):
        if operation == 'restore':
            require(self.service_scope is not None, 'captured_service_scope_required_for_restore')
            guard = json.loads(data)
            guard['service_scope'] = self.service_scope
            data = json.dumps(guard).encode()
        raw = self.session('python3', '-c', SERVICE_QUIESCENCE + CHECKPOINT, operation, self.tag,
                           self.monitor['name'], data=data, timeout=45)
        return json.loads(raw) if raw.strip() else None

    def clip(self, operation, data=None):
        if operation in ('create', 'set', 'restore'):
            self.input_guard()
        if operation == 'set':
            data = json.dumps({'content_base64': base64.b64encode(data).decode(),
                               'allowed': sorted(self.clipboard_hashes)}).encode()
        raw = self.session('python3', '-c', CLIPBOARD, operation, self.tag, data=data, timeout=35)
        return json.loads(raw) if raw.strip() else None

    def current_file(self):
        return self.checkpoint('read')

    def add_layout(self, value):
        layout = {key: item for key, item in value.items() if key != 'notes'}
        if layout not in self.layouts:
            self.layouts.append(copy.deepcopy(layout))

    def saved(self, expected_notes=None):
        def accepted():
            state = self.inspect()['client']
            return (state['settings_save_status'] == 'saved' and not state['settings_save_error']
                    and (expected_notes is None or state['notes'] == expected_notes))
        self.eventually(accepted, 'accepted_content_saved')
        value = self.current_file()['settings']
        require(any({key: item for key, item in value.items() if key != 'notes'} == layout
                    for layout in self.layouts), 'unrelated_settings_changed')
        require(expected_notes is None or value['notes'] == expected_notes,
                'saved_notes_match_accepted_content')
        self.notes.add(value['notes'])
        return value

    def qmp(self, events):
        # Always permit release of our already-held pointer button; suppress
        # new pointer actions if a secure surface or an application appears.
        if not all(event['type'] == 'btn' and event['data'].get('down') is False for event in events):
            self.input_guard()
        result = self.run(['virsh', '-c', self.lab.URI, 'qemu-monitor-command', self.name,
                           json.dumps({'execute': 'input-send-event', 'arguments': {'events': events}})])
        require('error' not in json.loads(result), 'native_pointer_delivery')

    def key(self, *codes):
        self.input_guard()
        if self.clipboard.get('supported') and codes in ((29, LETTERS['c']), (29, LETTERS['x']), (29, LETTERS['v'])):
            self.clip('owned', json.dumps(sorted(self.clipboard_hashes)).encode())
        self.run(['virsh', '-c', self.lab.URI, 'send-key', self.name, '--codeset', 'linux',
                  '--holdtime', '25', *map(str, codes)])
        time.sleep(.045)

    def move(self, x, y):
        self.qmp([{'type': 'abs', 'data': {'axis': axis, 'value': round(value / (extent - 1) * 32767)}}
                  for axis, value, extent in [('x', x * self.monitor['scale'], self.monitor['width']),
                                               ('y', y * self.monitor['scale'], self.monitor['height'])]])

    def notes_surface(self):
        state = self.inspect()
        matches = [(surface, hit) for surface in self.native.active_surfaces(state)
                   for hit in surface['hits'] if hit['id'] == 'notes-input']
        require(len(matches) == 1, 'one_active_notes_field')
        return matches[0]

    def origin(self, surface):
        namespace = 'lucent-' + surface['id']
        result = []
        def visit(value):
            if isinstance(value, dict):
                if value.get('namespace') == namespace:
                    result.append(value)
                for child in value.values():
                    visit(child)
            elif isinstance(value, list):
                for child in value:
                    visit(child)
        visit(json.loads(self.session('hyprctl', '-j', 'layers')))
        require(len(result) == 1, 'exact_native_notes_layer')
        return result[0]

    def focus(self):
        self.input_guard()
        previous = None
        for _ in range(40):
            try:
                surface, hit = self.notes_surface()
            except (CheckFailure, LookupError):
                time.sleep(.15)
                continue
            origin = self.origin(surface)
            clip = hit['clip']
            rect = (max(hit['x'], clip['x']), max(hit['y'], clip['y']),
                    min(hit['x'] + hit['width'], clip['x'] + clip['width']),
                    min(hit['y'] + hit['height'], clip['y'] + clip['height']))
            current = (surface['id'], surface.get('instance'), rect, origin['x'], origin['y'])
            if current == previous and rect[2] - rect[0] > 10 and rect[3] - rect[1] > 10:
                self.input_guard()
                self.move(origin['x'] + (rect[0] + rect[2]) / 2,
                          origin['y'] + (rect[1] + rect[3]) / 2)
                time.sleep(.12)
                try:
                    self.qmp([{'type': 'btn', 'data': {'button': 'left', 'down': True}}])
                    time.sleep(.05)
                finally:
                    self.qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}])
                self.eventually(lambda: self.notes_surface()[0]['focus'] == 'notes-input', 'native_notes_focus')
                return
            previous = current
            time.sleep(.15)
        raise CheckFailure('stable_notes_hit_geometry')

    def typed(self, value):
        self.input_guard()
        for character in value:
            self.key(28 if character == '\n' else 57 if character == ' ' else LETTERS[character])

    def replace(self, value):
        require(set(value) <= set(LETTERS) | {' ', '\n'}, 'owned_ascii_fixture')
        self.notes.update(value[:index] for index in range(len(value) + 1))
        self.key(29, 30)
        if value:
            self.typed(value)
        else:
            self.key(14)
        self.saved(value)

    def no_save(self, operation):
        self.saved()
        before = self.current_file()
        operation()
        time.sleep(.4)
        require(self.current_file() == before, 'selection_navigation_scroll_do_not_save')

    def probe(self, expected, operation):
        before = self.inspect()['client']['notes']
        self.no_save(operation)
        self.notes.add(expected)
        self.key(LETTERS['q'])
        self.saved(expected)
        self.key(29, LETTERS['z'])
        self.saved(before)

    def verify_binary(self):
        code = ('import hashlib,json,subprocess; from pathlib import Path; '
                'p=subprocess.check_output(["pgrep","-x","lucent-desktop"],text=True).strip(); assert p.isdecimal(); '
                'print(json.dumps({"pid":int(p),"installed":hashlib.sha256((Path.home()/".local/bin/lucent-desktop").read_bytes()).hexdigest(),'
                '"running":hashlib.sha256((Path("/proc")/p/"exe").read_bytes()).hexdigest()}))')
        value = json.loads(self.remote('python3', '-c', code))
        require(value['installed'] == value['running'] == self.args.desktop_sha256, 'supplied_release_binary_identity')
        return value['pid']

    def stable(self, seconds=35):
        self.eventually(lambda: self.ready.desktop_rendered(self.inspect())
                        and self.inspect()['client']['background_ready'], 'desktop_rendered_readiness', timeout=45)
        pid = self.verify_binary()
        first = self.inspect()
        frames = sum(s['frames'] for s in first['surfaces'] if self.native.role(s) == 'background')
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            state = self.inspect()
            require(self.ready.desktop_rendered(state) and state['client']['background_ready']
                    and not state['client']['error'], 'stable_rendered_desktop')
            require(self.remote('pgrep', '-x', 'lucent-desktop').decode().strip() == str(pid), 'stable_desktop_process')
            time.sleep(min(2, max(0, deadline - time.monotonic())))
        require(self.verify_binary() == pid, 'stable_binary_identity')
        final = self.inspect()
        if final['client']['live_wallpaper']['running']:
            require(sum(s['frames'] for s in final['surfaces'] if self.native.role(s) == 'background') > frames,
                    'live_background_frames_advance')

    def restart(self):
        old = self.verify_binary()
        self.session('systemctl', '--user', 'restart', 'lucent.service')
        self.stable()
        require(self.verify_binary() != old, 'normal_service_restart_observed')


def preflight(h):
    h.run(['git', '-C', str(ROOT), 'cat-file', '-e', h.args.source_commit + '^{commit}'])
    require(h.session('omarchy-shell', 'lock', 'isLocked').strip() == b'false', 'unlocked_session_required')
    require(h.session('systemctl', '--user', 'show', 'lucent-clipboard.service', '-p', 'ActiveState', '--value').strip() == b'active',
            'normal_clipboard_history_helper_active_for_restart')
    monitors = json.loads(h.session('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1, 'single_output_required')
    h.monitor = monitors[0]
    require(h.monitor['x'] == h.monitor['y'] == h.monitor['transform'] == 0
            and h.monitor['scale'] > 0, 'unrotated_single_output_origin')
    require(not json.loads(h.session('hyprctl', '-j', 'clients')), 'no_application_clients_on_any_workspace')
    require(b'lucent-authorization-' not in h.session('hyprctl', '-j', 'layers'), 'no_secure_authorization_surface')
    state = h.inspect(); client = state['client']
    require(h.ready.desktop_rendered(state) and 'Vulkan' in state['adapter'] and client['background_ready']
            and client['settings_writable'] and client['settings_save_status'] == 'saved'
            and not client['error'] and not client['launcher'] and not client['query']
            and not client['dock_surface_expanded'] and not client['widgets_editing']
            and not client['widget_grid'] and not client['notification_history_open']
            and not client['notification_count'], 'closed_idle_desktop_required')
    require(all(not client.get(panel, {}).get('open') for panel in
                ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'settings', 'live_wallpaper', 'application_actions')),
            'no_foreground_panel')
    keyboards = [keyboard for keyboard in json.loads(h.session('hyprctl', '-j', 'devices'))['keyboards'] if keyboard.get('main')]
    require(len(keyboards) == 1 and keyboards[0].get('capsLock') is False
            and keyboards[0].get('active_keymap', '').startswith('English (US)'), 'us_keyboard_caps_off')
    bindings = json.loads(h.session('hyprctl', '-j', 'binds'))
    chords = [(0, 'Return', 28), (SHIFT, 'Return', 28), (0, 'Tab', 15), (SHIFT, 'Tab', 15)]
    chords += [(CONTROL, key, LETTERS[key]) for key in ('a', 'z', 'c', 'x', 'v')]
    chords += [(mask, symbol, code) for mask in (0, SHIFT, CONTROL)
               for symbol, code in [('Home', 102), ('End', 107), ('Left', 105), ('Right', 106), ('Up', 103), ('Down', 108)]]
    for mask, symbol, code in chords:
        require(not any((binding.get('modmask', 0) & ~(2 | 16)) == mask
                        and (str(binding.get('key', '')).lower() in (symbol.lower(), f'code:{code}', f'code:{code + 8}')
                             or binding.get('keycode', 0) in (code, code + 8)) for binding in bindings), 'tested_chords_not_compositor_bound')
    h.original_monitor = {key: h.monitor[key] for key in ('name', 'width', 'height', 'scale', 'x', 'y', 'transform', 'refreshRate')}
    h.workspace = json.loads(h.session('hyprctl', '-j', 'activeworkspace'))['id']
    require(h.workspace > 0, 'ordinary_workspace_required')
    h.original_wallpaper = client['wallpaper']
    h.verify_binary()


def expected_layout(h, settings):
    if h.inspect()['client'].get('named_monitor_hosts'):
        layouts = settings.setdefault('monitor_layouts', {})
        return layouts.setdefault(h.monitor['name'], {key: copy.deepcopy(settings[key]) for key in LAYOUT_KEYS})
    return settings


def suite(h, result):
    preflight(h)
    baseline = h.checkpoint('create'); h.checkpointed = True
    private(h.directory / 'recovery.json', {'guest_checkpoint_relative': '.local/state/lucent/.native-notes-' + h.tag,
                                          'source_commit': h.args.source_commit, 'desktop_sha256': h.args.desktop_sha256})
    private(h.directory / 'original-checkpoint.json', baseline)
    h.notes.add(baseline['settings']['notes']); h.add_layout(baseline['settings'])
    h.checkpoint('awake')
    workspace = 'name:lucent-notes-' + h.tag
    h.session('hyprctl', 'dispatch', 'hl.dsp.focus({ workspace = ' + json.dumps(workspace) + ' })')
    settings = copy.deepcopy(baseline['settings']); layout = expected_layout(h, settings)
    if 'notes' not in layout['visible_widgets']:
        layout['visible_widgets'].append('notes')
    h.add_layout(settings)
    h.mutated = True
    h.cli('widget', 'notes', 'show')
    h.saved()
    h.focus()
    h.saved()
    surface, hit = h.notes_surface()
    require(surface['keyboard'] == 'on-demand' and surface.get('instance') is not None
            if h.inspect()['client'].get('named_monitor_hosts') else surface['keyboard'] == 'on-demand',
            'notes_uses_current_native_on_demand_surface')
    result['checks']['real_native_notes_focus'] = True
    h.replace('a')
    h.notes.update(('a\n', 'a\n\n', 'a\n\nb'))
    h.key(28); h.key(42, 28); h.key(LETTERS['b']); h.saved('a\n\nb')
    result['checks']['enter_and_shift_enter_insert_lf'] = True
    fixture = 'abcd\nx\nabcd'
    h.replace(fixture)
    h.probe('abcd\nx\nabcqd', lambda: (h.key(29, 102), h.key(106), h.key(106), h.key(106), h.key(108), h.key(108)))
    h.probe('abcd\nx\nqabcd', lambda: (h.key(29, 107), h.key(102)))
    h.probe('abcd\nx\nabcdq', lambda: (h.key(107),))
    h.probe('abcd\nxq', lambda: (h.key(29, 107), h.key(42, 103)))
    result['checks'].update(hard_row_up_down_sticky_x=True, row_home_end=True,
                            document_home_end=True, shift_up_selection_replacement=True, native_undo=True,
                            selection_and_navigation_do_not_save=True)
    def keyboard_focus():
        for _ in range(32):
            h.key(15)
            surface, _ = h.notes_surface()
            if surface.get('focus') == 'notes-input' and surface['focus_visible']:
                return
        raise CheckFailure('notes_keyboard_focus_visible')
    h.no_save(keyboard_focus)
    result['checks']['keyboard_focus_visible_without_hover'] = True
    h.focus()
    h.clipboard = h.clip('create')
    result['clipboard'] = h.clipboard
    if h.clipboard['supported']:
        external = 'one\r\n\r\ntwo\rthree\n'; normalized = 'one\n\ntwo\nthree\n'
        for text in (external, normalized, ''):
            h.clipboard_hashes.add(hashlib.sha256(text.encode()).hexdigest())
        h.notes.update((normalized, ''))
        h.clip('set', external.encode())
        h.eventually(lambda: h.clip('read')['sha256'] == hashlib.sha256(external.encode()).hexdigest(), 'external_clipboard_fixture_ready')
        h.key(29, 30); h.key(29, LETTERS['v']); h.saved(normalized)
        h.key(29, 30)
        h.no_save(lambda: h.key(29, LETTERS['c']))
        h.eventually(lambda: h.clip('read')['sha256'] == hashlib.sha256(normalized.encode()).hexdigest(), 'native_copy_external_payload')
        h.key(29, LETTERS['x']); h.saved('')
        h.key(29, LETTERS['v']); h.saved(normalized)
        result['checks'].update(external_paste_crlf_and_cr_normalize=True, empty_lines_and_trailing_lf=True,
                                native_copy_cut_paste=True, copy_does_not_save=True)
        unicode_fixture = 'a👩\u200d💻e\u0301z'
        h.notes.update((unicode_fixture, 'ae\u0301z', 'a👩\u200d💻z'))
        h.clipboard_hashes.add(hashlib.sha256(unicode_fixture.encode()).hexdigest())
        h.clip('set', unicode_fixture.encode())
        h.eventually(lambda: h.clip('read')['sha256'] == hashlib.sha256(unicode_fixture.encode()).hexdigest(), 'unicode_clipboard_fixture_ready')
        h.key(29, 30); h.key(29, LETTERS['v']); h.saved(unicode_fixture)
        h.key(29, 102); h.key(106); h.key(111); h.saved('ae\u0301z')
        h.key(29, LETTERS['z']); h.saved(unicode_fixture)
        h.key(29, 107); h.key(105); h.key(14); h.saved('a👩\u200d💻z')
        h.key(29, LETTERS['z']); h.saved(unicode_fixture)
        result['checks']['native_utf8_grapheme_delete_and_backspace'] = True
        restored = h.clip('restore', json.dumps(sorted(h.clipboard_hashes)).encode())
        require(all(restored.values()), 'clipboard_and_history_restored_before_restart')
        h.clipboard_restored = True
        result['restoration']['clipboard_history_and_service'] = True
    long = '\n'.join(('one two three four', 'five six seven eight', 'nine ten') * 5)
    h.replace(long)
    h.key(29, 102)
    def wheel():
        for _ in range(6):
            h.qmp([{'type': 'btn', 'data': {'button': 'wheel-down', 'down': True}},
                   {'type': 'btn', 'data': {'button': 'wheel-down', 'down': False}}])
            time.sleep(.05)
    h.no_save(wheel)
    result['checks']['native_wheel_preserves_content_and_does_not_save'] = True
    settings = copy.deepcopy(h.saved(long)); expected_layout(h, settings)['widget_sizes']['notes'] = {'width': 320, 'height': 208}
    h.add_layout(settings)
    h.cli('widgets', 'size', 'notes', '320', '208'); h.saved(long)
    h.focus()
    before_height = h.notes_surface()[1]['height']
    settings = copy.deepcopy(h.saved(long)); expected_layout(h, settings)['widget_sizes']['notes'] = {'width': 400, 'height': 400}
    h.add_layout(settings)
    h.cli('widgets', 'size', 'notes', '400', '400')
    h.saved(long)
    h.eventually(lambda: h.notes_surface()[1]['height'] > before_height + 40, 'resized_notes_editor_fills_card')
    h.focus(); h.key(29, 107)
    h.probe(long + 'q', lambda: None)
    result['checks']['resized_card_updates_field_geometry_and_preserves_content'] = True
    h.saved(long)
    h.restart()
    h.saved(long)
    require(h.inspect()['client']['wallpaper'] == h.original_wallpaper, 'restart_preserves_selected_wallpaper')
    result['checks']['multiline_content_survives_restart_35s_stability'] = True


def restore(h, result):
    failures = result.setdefault('restoration_failures', {})
    def label(error):
        return str(error) if isinstance(error, CheckFailure) else type(error).__name__
    def attempt(label, operation):
        try:
            h.progress(label)
            operation()
            result['restoration'][label] = True
            h.progress(label, 'passed')
        except Exception as error:
            result['restoration'][label] = False
            failures[label] = str(error) if isinstance(error, CheckFailure) else type(error).__name__
            h.progress(label, 'failed', failures[label])
    if not h.checkpointed:
        return
    if h.clipboard.get('supported') and not h.clipboard_restored:
        attempt('clipboard_history_and_service', lambda: h.clip('restore', json.dumps(sorted(h.clipboard_hashes)).encode()))
    try:
        h.stop_quiescent()
        guard = json.dumps({'layouts': h.layouts, 'notes': sorted(h.notes)}).encode()
        h.progress('exact_original_settings_stopped_service')
        restored = h.checkpoint('restore', guard)
        require(all(restored.values()), 'original_settings_restore_verified')
        result['restoration']['exact_original_settings_stopped_service'] = True
        h.progress('exact_original_settings_stopped_service', 'passed')
    except Exception as error:
        result['restoration']['exact_original_settings_stopped_service'] = False
        failures['exact_original_settings_stopped_service'] = label(error)
        h.progress('exact_original_settings_stopped_service', 'failed', label(error))
    finally:
        if h.service_stopped:
            attempt('normal_desktop_35s_stability', lambda: (h.session('systemctl', '--user', 'start', 'lucent.service'), h.stable()))
    def workspace():
        h.session('hyprctl', 'dispatch', 'hl.dsp.focus({ workspace = ' + json.dumps(str(h.workspace)) + ' })')
        require(json.loads(h.session('hyprctl', '-j', 'activeworkspace'))['id'] == h.workspace, 'original_workspace_restored')
    attempt('original_workspace', workspace)
    def monitor():
        current = json.loads(h.session('hyprctl', '-j', 'monitors'))
        require(len(current) == 1 and all(current[0][key] == value for key, value in h.original_monitor.items()),
                'original_monitor_unchanged')
    attempt('original_monitor', monitor)
    attempt('exact_wallpaper_source_exports_and_settings', lambda: h.checkpoint('verify'))
    if h.clipboard.get('supported'):
        attempt('clipboard_history_preserved_after_restart', lambda: h.clip('verify'))
    attempt('original_idle_marker', lambda: h.checkpoint('idle-restore'))
    if all(result['restoration'].values()):
        attempt('owned_guest_checkpoint_removed', lambda: h.checkpoint('discard'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--desktop-sha256', required=True)
    args = parser.parse_args()
    def interrupted(*_):
        raise CheckFailure('native_acceptance_interrupted')
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    require(re.fullmatch('[0-9a-f]{40}', args.source_commit) is not None, 'full_source_commit_required')
    require(re.fullmatch('[0-9a-f]{64}', args.desktop_sha256) is not None, 'desktop_sha256_required')
    directory = ROOT / 'reports/local' / ('multiline-notes-native-' + secrets.token_hex(12))
    directory.mkdir(parents=True, mode=0o700)
    result = {'passed': False, 'source_commit': args.source_commit, 'binary_sha256': args.desktop_sha256,
              'checks': {}, 'restoration': {}, 'scope': 'Native single-output ordinary Notes editing; no hardware GPU or performance claim',
              'not_covered': ['native scrollbar thumb and rendered caret/selection clipping',
                              'native soft-wrap affinity and exact scroll-offset assertions',
                              'native PageUp/PageDown and stationary edge-drag autoscroll',
                              'clipboard delayed-source/focus-retirement races', 'IME/bidi/shaping/accessibility'],
              'privacy': {'original_notes_or_clipboard_published': False, 'private_checkpoints_only': True}}
    h = None
    try:
        h = Harness(args, directory)
        suite(h, result)
        result['passed'] = True
    except Exception as error:
        result['failed_check'] = str(error) if isinstance(error, CheckFailure) else type(error).__name__
    finally:
        if h is not None:
            restore(h, result)
        result['passed'] = result['passed'] and bool(result['restoration']) and all(result['restoration'].values())
        private(directory / 'report.json', result)
    print(json.dumps(result, indent=2))
    print('Private evidence: ' + str(directory.relative_to(ROOT)))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
