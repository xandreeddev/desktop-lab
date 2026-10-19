#!/usr/bin/env python3
"""Guarded native live-library preview acceptance; importing is inert.

Run on the existing unlocked Lucent guest with one origin output at 1x, no
application windows or foreground panels, and no concurrent keyboard/pointer
use. The supplied immutable release manifest must match both installed binaries.

Three tiny synthetic animated GIF sources belong exclusively to this invocation;
one starts invalid to exercise Retry and same-path Refresh. User source files
are only read. Card/Prev/Next/Tab/wheel/close/reopen browsing must preserve exact
settings identity and all inventoried stores, sources, links and exports. Cropped
private native captures must actually change between fixture colors.

Apply is exercised on the single monitor's override, leaving the selected
static wallpaper and its theme/login exports unchanged. After normal full
service stop and exact-cgroup quiescence, restore the original settings hardlink
only if current settings are still the admitted test profile. Independent edits
abort restoration and retain private recovery material. No arbitrary form draft,
scroll position, keyboard ownership, hardware cadence or GPU timing is claimed.
The inspector's active flag is NOT a decoder-count measurement: <=1 decoder
body is covered by the coordinator concurrency tests/source lifetime contract.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import secrets
import shlex
import stat
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]


class Failure(Exception):
    """Only fixed labels, never guest command output or original contents."""


def require(condition, label):
    if not condition:
        raise Failure(label)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def private(path, data):
    if not isinstance(data, bytes):
        data = (json.dumps(data, indent=2) + '\n').encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)


def run(argv, *, data=None, timeout=30):
    try:
        value = subprocess.run(argv, input=data, capture_output=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        raise Failure('bounded_command_failed') from None
    require(value.returncode == 0, 'bounded_command_failed')
    return value.stdout


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# The full guest transaction is sent over stdin/SSH, never written into a report.
# Private manifests contain paths/settings solely inside the owned guest dir.
TRANSACTION = r'''
import copy,hashlib,itertools,json,os,shutil,stat,struct,sys,unicodedata
from pathlib import Path
operation,tag=sys.argv[1:]
assert len(tag)==24 and tag.isalnum(),'checked_transaction_tag_required'
data=json.load(sys.stdin)
home=Path.home();base=home/'.local/state/lucent';settings=base/'desktop.json'
directory=base/('.native-live-preview-'+tag);library=home/'Videos/Wallpapers'
prefix='000-lucent-preview-'+tag+'-'
def identity(v):
 return [v.st_dev,v.st_ino,v.st_mode,v.st_size,v.st_mtime_ns,v.st_uid]
def read(path,limit=4*1024*1024):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as stream:
  before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_size<=limit,'bounded_regular_file_required'
  value=stream.read(limit+1)
  assert len(value)<=limit and identity(before)==identity(os.fstat(stream.fileno()))==identity(path.lstat()),'file_changed_during_read'
 return value,identity(before)
def fact(path,limit=512*1024*1024):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as stream:
  before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_size<=limit,'bounded_regular_file_required'
  value=hashlib.sha256();count=0
  for block in iter(lambda:stream.read(1048576),b''):
   count+=len(block);assert count<=limit,'hash_budget_exceeded';value.update(block)
  assert count==before.st_size and identity(before)==identity(os.fstat(stream.fileno()))==identity(path.lstat()),'file_changed_during_hash'
 return {'identity':identity(before),'sha256':value.hexdigest()}
def owned(path):
 value=path.lstat()
 assert value.st_uid==os.getuid(),'owned_file_required'
 return value
def write(path,value):
 fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'wb') as stream:
  os.fchmod(stream.fileno(),0o600);stream.write(value);stream.flush();os.fsync(stream.fileno())
def canonical_dir(path):
 value=owned(path)
 assert stat.S_ISDIR(value.st_mode) and path.resolve(strict=True)==path,'owned_canonical_directory_required'
 # Parent mtimes change when our exclusive entries are added/removed.
 return [value.st_dev,value.st_ino,value.st_mode,value.st_uid]
def tree(path,depth,exclude=()):
 if not path.exists():return {'absent':True}
 root=canonical_dir(path);result={};pending=[(path,depth)];budget=4*1024*1024*1024;count=0
 while pending:
  parent,remaining=pending.pop()
  with os.scandir(parent) as children:
   for entry in children:
    if parent==path and entry.name in exclude:continue
    count+=1;assert count<=8192,'bounded_inventory_required'
    current=Path(entry.path);info=owned(current);key=str(current.relative_to(path))
    if stat.S_ISREG(info.st_mode):
     budget-=info.st_size;assert budget>=0,'bounded_inventory_hash_bytes_required'
     result[key]=fact(current)
    elif stat.S_ISDIR(info.st_mode):
     result[key]={'directory':canonical_dir(current)}
     if remaining:pending.append((current,remaining-1))
    elif stat.S_ISLNK(info.st_mode):
     result[key]={'link_identity':identity(info),'target_sha256':hashlib.sha256(os.readlink(current).encode()).hexdigest()}
    else:raise AssertionError('special_source_file_not_supported')
 return {'root':root,'entries':result}
def external():
 links={}
 for path in (base/'wallpaper',home/'.local/state/omarchy/current/background'):
  if path.is_symlink():links[str(path)]={'identity':identity(path.lstat()),'target':os.readlink(path)}
  else:assert not path.exists(),'selection_link_required';links[str(path)]=None
 target=(base/'wallpaper').resolve(strict=True)
 exports={}
 paths=list((base/'theme').glob('*'))+list((Path('/var/lib/lucent/wallpapers')/str(os.getuid())).glob('*'))
 paths += [home/'.config/omarchy/themes/lucent-tokens-active/colors.toml']
 assert len(paths)<=128,'bounded_exports_required'
 for path in paths:
  if path.is_file() and not path.is_symlink():exports[str(path)]=fact(path,128*1024*1024)
 legacy=home/'.config/lucent/weather.json'
 try:legacy_value=fact(legacy,4096)
 except FileNotFoundError:legacy_value=None
 marker=home/'.local/state/omarchy/indicators/stay-awake'
 return {'links':links,'wallpaper':fact(target,128*1024*1024),'exports':exports,
         'legacy_weather':legacy_value,'idle_marker':identity(marker.lstat()) if marker.exists() else None}
def snapshot(fixtures):
 return {'settings':fact(settings,4*1024*1024),'external':external(),
         'library':tree(library,0,[item['filename'] for item in fixtures]),
         'media':tree(base/'live-wallpapers',2),'static':tree(base/'wallpaper-assets',2)}
def gif(width,height,phase):
 # Clear before each literal: code width stays three bits; no encoder package,
 # external process or network is required. Each source has six changing frames.
 out=bytearray(b'GIF89a'+struct.pack('<HH',width,height)+b'\x81\0\0')
 out+=bytes([0,0,0,240,24,24,24,240,24,24,24,240])
 out+=b'\x21\xff\x0bNETSCAPE2.0\x03\x01\0\0\0'
 for frame in range(6):
  color=(frame+phase)%3+1
  out+=b'\x21\xf9\x04\x04'+struct.pack('<H',15)+b'\0\0'
  out+=b'\x2c'+struct.pack('<HHHH',0,0,width,height)+b'\0\x02'
  codes=[item for _ in range(width*height) for item in (4,color)]+[5]
  bits=0;used=0;packed=bytearray()
  for code in codes:
   bits|=code<<used;used+=3
   while used>=8:packed.append(bits&255);bits>>=8;used-=8
  if used:packed.append(bits&255)
  for at in range(0,len(packed),255):out.append(min(255,len(packed)-at));out+=packed[at:at+255]
  out.append(0)
 out.append(0x3b);return bytes(out)
manifest_path=directory/'manifest.json'
def save(value):
 temporary=directory/('manifest-'+os.urandom(8).hex()+'.new')
 write(temporary,json.dumps(value).encode());os.replace(temporary,manifest_path)
def fixture_check(value):
 for item in value['fixtures']:
  path=library/item['filename'];assert fact(path)==item['fact'],'test_fixture_replaced_independently'
def settings_check(value,allow_applied):
 expected=value.get('applied_settings') if allow_applied else None
 assert fact(settings,4*1024*1024)==(expected or value['original']['settings']),'settings_changed_independently_no_restore'
def immutable_check(value,allow_applied=False):
 fixture_check(value);current=snapshot(value['fixtures'])
 for key in ('external','library','static'):
  assert current[key]==value['original'][key],'source_export_or_static_store_changed_no_restore'
 expected=value.get('applied_media') if allow_applied else None
 assert current['media']==(expected or value['original']['media']),'live_store_changed_independently_no_restore'
 settings_check(value,allow_applied)
 return current
if operation=='create':
 canonical_dir(base);canonical_dir(library);before,ident=read(settings)
 assert ident[-1]==os.getuid(),'owned_settings_required'
 directory.mkdir(mode=0o700);fixtures=[]
 try:
  os.link(settings,directory/'original.link',follow_symlinks=False);write(directory/'original.bytes',before)
  for index,name in enumerate(('a','b','c')):
   filename=prefix+name+'.gif';source=directory/filename
   write(source,gif(64,40,index) if index<2 else b'Not animated media')
   # Record the source inode before publishing its library hardlink. A failure
   # after link() must not orphan a source whose ownership was never journaled.
   fixtures.append({'filename':filename,'fact':fact(source)})
   journal=directory/('fixture-journal-'+os.urandom(8).hex()+'.new')
   write(journal,json.dumps(fixtures).encode());os.replace(journal,directory/'fixture-journal.json')
   os.link(source,library/filename,follow_symlinks=False)
   assert fact(library/filename)==fixtures[-1]['fact'],'published_fixture_changed'
  value={'monitor':data['monitor'],'original_json':json.loads(before),'fixtures':fixtures,
         'original':snapshot(fixtures),'armed':None,'applied_settings':None,'applied_media':None}
  assert value['original']['settings']['identity']==ident,'settings_changed_while_checkpointing'
  save(value)
 except BaseException:
  # Partial fixtures are removed only when the exact owned hardlink still agrees.
  cleanup_complete=True
  for item in fixtures:
   path=library/item['filename']
   try:current=fact(path)
   except FileNotFoundError:continue
   except BaseException:cleanup_complete=False;continue
   if current==item['fact']:path.unlink()
   else:cleanup_complete=False
  # Retain private source/journal evidence if any published name is no longer
  # exactly ours. Never erase another file or guess ownership after an error.
  if cleanup_complete:shutil.rmtree(directory)
  raise
else:
 info=owned(directory);assert stat.S_ISDIR(info.st_mode) and stat.S_IMODE(info.st_mode)==0o700,'private_checkpoint_required'
 raw,_=read(manifest_path);value=json.loads(raw)
if operation=='create':
 print(json.dumps({'created':True}))
elif operation=='discover':
 fixture_check(value);choices=[]
 with os.scandir(library) as entries:
  for entry in itertools.islice(entries,1024):
   if not entry.is_file(follow_symlinks=False):continue
   path=Path(entry.path)
   if path.suffix.lower() not in ('.mp4','.webm','.gif') or not 0<entry.stat().st_size<=512*1024*1024:continue
   try:str(path).encode('utf8')
   except UnicodeEncodeError:continue
   name=''.join(c for c in path.stem.replace('_',' ').replace('-',' ') if unicodedata.category(c)!='Cc')[:64]
   choices.append((name,str(path)))
 choices=sorted(choices)[:32];indices=[]
 for item in value['fixtures']:
  found=[i for i,(_,path) in enumerate(choices) if path==str(library/item['filename'])]
  assert len(found)==1,'owned_fixtures_must_be_in_bounded_visible_library'
  indices.append(found[0])
 print(json.dumps({'count':len(choices),'indices':indices}))
elif operation=='input-check':
 fixture_check(value);settings_check(value,bool(value['applied_settings']));print(json.dumps({'settings_and_owned_sources_unchanged':True}))
elif operation=='browse-check':
 immutable_check(value);print(json.dumps({'unchanged':True}))
elif operation=='repair':
 immutable_check(value);index=data['index'];assert index==2,'only_owned_failure_fixture_may_change'
 item=value['fixtures'][index];path=library/item['filename']
 temporary=directory/('repair-'+os.urandom(8).hex()+'.gif')
 write(temporary,gif(*data['size'],data['phase']))
 assert fact(path)==item['fact'],'owned_source_changed_before_replace'
 os.replace(temporary,path);item['fact']=fact(path);save(value)
 print(json.dumps({'owned_fixture_replaced':True}))
elif operation=='arm':
 immutable_check(value);index=data['index'];assert index in (0,1),'valid_owned_apply_source_required'
 value['armed']=index;save(value);print(json.dumps({'armed':True}))
elif operation=='admit':
 # The only accepted preference delta is the target monitor profile, with an
 # exact source descriptor supplied by our selected synthetic library source.
 assert value['armed'] is not None,'explicit_native_apply_must_be_armed'
 current=snapshot(value['fixtures']);before=value['original']
 for key in ('external','library','static'):assert current[key]==before[key],'independent_external_change_no_restore'
 content,current_id=read(settings);saved=json.loads(content);original=value['original_json']
 candidate=copy.deepcopy(original);live=saved['live_wallpapers'];profile=live['monitors'][value['monitor']]
 source=library/value['fixtures'][value['armed']]['filename']
 assert profile['asset']['source']['path']==str(source),'applied_profile_source_mismatch'
 assert set(profile)=={'asset','fps','paused','enabled'} and profile['fps']=='source' and profile['paused'] is False and profile['enabled'] is True,'unexpected_applied_profile_policy'
 asset=profile['asset'];movie=Path(asset['path']);poster=Path(asset['poster']['path']);store=(base/'live-wallpapers').resolve(strict=True)
 assert movie.parent.parent==store and movie.parent.name.startswith('media-') and movie.name=='source.gif' and poster==movie.parent/'poster.png','owned_applied_asset_required'
 assert movie.resolve(strict=True)==movie and poster.resolve(strict=True)==poster,'canonical_applied_asset_required'
 metadata,_=read(movie.parent/'asset.json');assert json.loads(metadata)==asset,'exact_applied_metadata_required'
 assert fact(movie)['sha256']==value['fixtures'][value['armed']]['fact']['sha256'],'copied_fixture_digest_required'
 candidate['live_wallpapers']['monitors'][value['monitor']]=profile
 assert saved==candidate,'independent_preferences_changed_no_restore'
 entries=current['media']['entries'];old=before['media'].get('entries',{})
 assert all(entries.get(key)==item for key,item in old.items()),'preexisting_media_changed_no_restore'
 extra=set(entries)-set(old);folder=movie.parent.name
 assert extra=={folder,folder+'/source.gif',folder+'/poster.png',folder+'/asset.json'},'unexpected_imported_store_entries_no_restore'
 if value['applied_settings'] is not None:
  assert current['settings']==value['applied_settings'] and current['media']==value['applied_media'],'admitted_candidate_changed_no_restore'
 else:
  value['applied_settings']=current['settings'];value['applied_media']=current['media'];value['new_folder']=folder;save(value)
 assert current_id==current['settings']['identity'],'settings_changed_during_admission'
 fixture_check(value)
 def asset_count(items):
  return sum('/' not in key and key.startswith('media-') and 'directory' in item
             for key,item in items.items())
 print(json.dumps({'checks':{'only_expected_monitor_profile_saved':True,'source_rate_saved':True,'one_owned_asset_imported':True},
                   'metrics':{'original_owned_assets_count':asset_count(old),
                              'after_apply_owned_assets_count':asset_count(entries)}}))
elif operation=='guard':
 immutable_check(value,allow_applied=bool(value['applied_settings']));print(json.dumps({'unchanged':True}))
elif operation=='restore':
 # SERVICE_QUIESCENCE is prepended by the host. A running writer is never
 # compatible with preference rollback, even if its MainPID is briefly zero.
 require_service_quiescent(data['service_scope'])
 if value['applied_settings'] is None:
  current=fact(settings,4*1024*1024)
  if current!=value['original']['settings']:raise AssertionError('unadmitted_settings_change_manual_recovery_required')
 else:immutable_check(value,True)
 original,original_id=read(directory/'original.link')
 assert original==(directory/'original.bytes').read_bytes() and original_id==value['original']['settings']['identity'],'original_checkpoint_changed'
 current=fact(settings,4*1024*1024)
 if current!=value['original']['settings']:
  temporary=base/('.native-live-preview-restore-'+tag)
  os.link(directory/'original.link',temporary,follow_symlinks=False)
  assert fact(settings,4*1024*1024)==current,'settings_changed_before_restore'
  require_service_quiescent(data['service_scope']);os.replace(temporary,settings)
 assert fact(settings,4*1024*1024)==value['original']['settings'],'exact_original_settings_not_restored'
 # The current original preferences cannot reference this invocation's source.
 if value['applied_media'] is not None:
  assert tree(base/'live-wallpapers',2)==value['applied_media'],'media_changed_before_owned_cleanup'
  folder=base/'live-wallpapers'/value['new_folder']
  for name in ('source.gif','poster.png','asset.json'):
   path=folder/name;key=value['new_folder']+'/'+name
   assert fact(path)==value['applied_media']['entries'][key],'owned_asset_changed_before_cleanup'
  require_service_quiescent(data['service_scope'])
  for name in ('source.gif','poster.png','asset.json'):(folder/name).unlink()
  folder.rmdir()
  if value['original']['media'].get('absent'):
   root=base/'live-wallpapers';assert tree(root,2)['entries']=={},'independent_new_media_remains';root.rmdir()
 value['applied_settings']=None;value['applied_media']=None;save(value)
 immutable_check(value)
 print(json.dumps({'exact_settings_bytes_and_inode_restored':True,'only_owned_applied_asset_removed':True}))
elif operation=='verify':
 immutable_check(value);print(json.dumps({'settings_sources_stores_exports_and_idle_exact':True}))
elif operation=='discard':
 immutable_check(value)
 for item in value['fixtures']:
  path=library/item['filename'];assert fact(path)==item['fact'],'fixture_changed_before_cleanup';path.unlink()
 # This private namespace contains only files this invocation created.
 shutil.rmtree(directory);print(json.dumps({'owned_fixtures_and_checkpoint_removed':True}))
else:raise AssertionError('known_transaction_operation_required')
'''

# Filled with the reviewed generic exact-cgroup proof below. It accepts a
# deliberately stopped unit in inactive OR failed state only after every PID,
# job and recursively populated cgroup is absent. It never kills a process.
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

CLIPBOARD = r'''
import hashlib,json,os,selectors,stat,subprocess,time
from pathlib import Path
child=subprocess.Popen(['wl-paste','--list-types'],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
selector=selectors.DefaultSelector();output=[bytearray(),bytearray()];deadline=time.monotonic()+4
try:
 for index,stream in enumerate((child.stdout,child.stderr)):
  os.set_blocking(stream.fileno(),False);selector.register(stream,selectors.EVENT_READ,index)
 while selector.get_map():
  assert time.monotonic()<deadline,'clipboard_metadata_timed_out'
  for key,_ in selector.select(.05):
   part=os.read(key.fd,4096)
   if not part:selector.unregister(key.fileobj);continue
   output[key.data].extend(part);assert len(output[key.data])<=16384,'clipboard_metadata_too_large'
 code=child.wait(timeout=1)
 assert code==1 and not output[0] and b'Nothing is copied' in output[1],'initially_empty_clipboard_required_no_payload_read'
finally:
 selector.close()
 if child.poll() is None:child.kill();child.wait(timeout=1)
 child.stdout.close();child.stderr.close()
def file(path,limit):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as stream:
  v=os.fstat(stream.fileno());assert stat.S_ISREG(v.st_mode) and v.st_uid==os.getuid() and v.st_size<=limit
  content=stream.read(limit+1);ident=lambda x:[x.st_dev,x.st_ino,x.st_mode,x.st_size,x.st_mtime_ns,x.st_uid]
  assert len(content)<=limit and ident(v)==ident(os.fstat(stream.fileno()))==ident(path.lstat())
 return {'identity':ident(v),'sha256':hashlib.sha256(content).hexdigest()}
base=Path.home()/'.local/state/lucent/clipboard';history=base/'history.json'
try:original=file(history,2*1024*1024)
except FileNotFoundError:original=None
images=list(base.glob('*.png'));assert len(images)<=128
expected_active=json.load(__import__('sys').stdin)['expected_active']
result=subprocess.run(['systemctl','--user','show','lucent-clipboard.service','-p','ActiveState','-p','SubState','-p','MainPID','-p','ControlPID','-p','Job'],capture_output=True,text=True,timeout=5)
assert result.returncode==0,'clipboard_helper_state_unavailable'
unit=dict(line.split('=',1) for line in result.stdout.splitlines() if '=' in line)
if expected_active:assert unit['ActiveState']=='active','normal_clipboard_helper_required'
else:
 assert unit['ActiveState'] in ('inactive','failed') and unit['SubState'] in ('dead','failed'),'intentionally_stopped_clipboard_helper_required'
 assert unit['MainPID']=='0' and unit['ControlPID']=='0' and unit.get('Job','') in ('','0'),'stopped_clipboard_helper_must_be_quiescent'
print(json.dumps({'empty':True,'history':original,'images':{p.name:file(p,2*1024*1024) for p in images}}))
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--desktop-sha256', required=True)
    parser.add_argument('--build-manifest', required=True, type=Path)
    args = parser.parse_args()
    require(re.fullmatch('[0-9a-f]{40}', args.source_commit), 'full_source_commit_required')
    require(re.fullmatch('[0-9a-f]{64}', args.desktop_sha256), 'full_desktop_digest_required')
    common = Path(run(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute', '--git-common-dir']).decode().strip()).parent
    lab = load('preview_root_lab', common / 'vm/lab.py')
    native = load('preview_native', ROOT / 'vm/native_surfaces.py')
    ready = load('preview_readiness', ROOT / 'scripts/lucent_readiness.py')
    checkpoint_module = load('preview_checkpoint', ROOT / 'vm/wallpaper_checkpoint.py')
    build = json.loads(args.build_manifest.read_bytes())
    require(build['source_commit'] == args.source_commit
            and build['binaries']['lucent-desktop']['sha256'] == args.desktop_sha256
            and set(build['binaries']) == {'lucent-desktop', 'lucent-cli'}, 'supplied_complete_release_manifest_required')
    run(['git', '-C', str(ROOT), 'cat-file', '-e', args.source_commit + '^{commit}'])
    inventory = lab.inventory()
    require(inventory['uri'] == 'qemu:///session' and inventory['user'] == 'omarchy'
            and inventory['vms']['lucent']['name'] == 'desktop-lab-lucent'
            and inventory['vms']['lucent']['port'] == 2243, 'existing_lucent_guest_required')
    directory = common / 'reports/local' / ('live-previews-native-' + secrets.token_hex(12))
    directory.mkdir(mode=0o700)
    tag = secrets.token_hex(12)
    lock = os.open(common / '.cache/live-previews-native.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    info = os.fstat(lock)
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o600, 'owned_private_lock_required')
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise Failure('another_preview_acceptance_is_running') from None
    name = inventory['vms']['lucent']['name']
    report = {'passed': False, 'source_commit': args.source_commit, 'desktop_sha256': args.desktop_sha256,
              'helper_sha256': sha(Path(__file__).read_bytes()), 'checks': {}, 'metrics': {}, 'restoration': {}, 'captures': [],
              'scope': 'One existing 1x native output; synthetic animated GIF carousel and monitor-specific Apply',
              'precondition': 'Controlled idle guest; no concurrent keyboard or pointer input',
              'decoder_body_concurrency': 'Not measured natively; coordinator tests and source lifetime contract provide <=1 body proof',
              'timing': 'Changing native cropped pixels only; no native source-FPS, smoothness, GPU or scanout claim'}
    create_attempted = created = applied = stopped = cleaning = cursor_mutated = False
    service_scope = wallpaper_checkpoint = original = original_monitor = None
    last_cursor = original_cursor = process = expected_open = original_clipboard = parent_epoch = supervisor_fact = None
    started = time.monotonic()
    phase = 'preflight'

    def remote(*argv, data=None, timeout=35):
        return run(lab.ssh_args('lucent') + [shlex.join(argv)], data=data, timeout=timeout)
    def session(*argv, data=None, timeout=35):
        return remote('python3', 'desktop-lab/scripts/in-session.py', *argv, data=data, timeout=timeout)
    def cli(*argv):
        return session('/home/omarchy/.local/bin/lucent-cli', *argv)
    def inspect():
        return json.loads(cli('inspect'))
    def eventually(predicate, label, timeout=25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except (Failure, KeyError, LookupError, ValueError):
                pass
            time.sleep(.12)
        raise Failure(label)
    def guest(operation, payload=None):
        return json.loads(session('python3', '-c', SERVICE_QUIESCENCE + TRANSACTION,
                                  operation, tag, data=json.dumps(payload or {}).encode(), timeout=100))
    def clipboard():
        if stopped:
            quiescent()
        return json.loads(session('python3', '-c', CLIPBOARD,
                                  data=json.dumps({'expected_active': not stopped}).encode(), timeout=15))
    def binary():
        code = r'''import hashlib,json,os,subprocess
from pathlib import Path
p=subprocess.check_output(['pgrep','-x','lucent-desktop'],text=True).strip();assert p.isdecimal()
raw=(Path('/proc')/p/'stat').read_text();start=raw.rsplit(')',1)[1].split()[19]
files={}
for name in ('lucent-desktop','lucent-cli'):
 path=Path.home()/'.local/bin'/name
 assert not path.is_symlink() and path.is_file()
 files[name]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'bytes':path.stat().st_size}
print(json.dumps({'pid':int(p),'starttime':start,'running_sha256':hashlib.sha256((Path('/proc')/p/'exe').read_bytes()).hexdigest(),'files':files}))
'''
        value = json.loads(remote('python3', '-c', code))
        require(value['running_sha256'] == args.desktop_sha256
                and value['files'] == build['binaries'], 'installed_running_supplied_binary_cohort_required')
        return {key: value[key] for key in ('pid', 'starttime', 'running_sha256')}
    def supervisor():
        code = r'''import hashlib,json,os,stat,subprocess
from pathlib import Path
v=subprocess.check_output(['systemctl','--user','show','lucent.service','-p','ActiveState','-p','SubState','-p','MainPID','-p','NRestarts','-p','KillMode'],text=True,timeout=5)
unit=dict(line.split('=',1) for line in v.splitlines() if '=' in line)
assert unit['ActiveState']=='active' and unit['SubState']=='running' and unit['KillMode']=='mixed'
pid=unit['MainPID'];assert pid.isdecimal() and int(pid)>0
start=(Path('/proc')/pid/'stat').read_text().rsplit(')',1)[1].split()[19]
path=Path.home()/'.local/lib/lucent/session.py'
fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
with os.fdopen(fd,'rb') as stream:
 before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_size<128*1024
 raw=stream.read(128*1024);after=os.fstat(stream.fileno())
 ident=lambda v:[v.st_dev,v.st_ino,v.st_mode,v.st_size,v.st_mtime_ns,v.st_uid]
 assert ident(before)==ident(after)==ident(path.lstat())
print(json.dumps({'epoch':[int(pid),start,unit['NRestarts']],'script':{'identity':ident(before),'sha256':hashlib.sha256(raw).hexdigest()}}))
'''
        result = json.loads(remote('python3', '-c', code))
        if supervisor_fact is not None:
            require(result['script'] == supervisor_fact, 'supervisor_changed_no_restart_or_restore')
        return result
    def cursor():
        value = json.loads(session('hyprctl', '-j', 'cursorpos'))
        require(all(type(value.get(k)) in (int, float) and math.isfinite(value[k]) for k in ('x', 'y')), 'finite_cursor_required')
        return {k: value[k] for k in ('x', 'y')}
    def monitor():
        values = json.loads(session('hyprctl', '-j', 'monitors'))
        require(len(values) == 1, 'single_existing_output_required')
        return values[0]
    def monitor_key(value):
        return {k: value.get(k) for k in ('name', 'width', 'height', 'scale', 'x', 'y', 'transform', 'activeWorkspace', 'specialWorkspace', 'reserved')}
    def input_guard():
        code = r'''import json,subprocess
def get(*argv):return subprocess.check_output(argv,timeout=4)
assert get('omarchy-shell','lock','isLocked').strip()==b'false'
assert not json.loads(get('hyprctl','-j','clients'))
assert b'lucent-authorization-' not in get('hyprctl','-j','layers')
print('safe')
'''
        require(session('python3', '-c', code, timeout=16).strip() == b'safe', 'unlocked_guest_without_clients_or_authorization_required')
    def no_other_panels(state):
        c = state['client']
        require(not c['launcher'] and not c['notification_history_open'], 'unrelated_foreground_ui_active')
        for key in ('audio', 'network', 'bluetooth', 'power', 'tray', 'media', 'weather', 'settings'):
            item = c.get(key, {})
            require(not item.get('open') and not item.get('busy'), 'unrelated_foreground_operation_active')
        require(not c.get('widgets_editing') and not c.get('widget_grid')
                and all(not item.get('editing') and not item.get('grid_visible') for item in c.get('monitor_hosts', [])), 'widget_edit_active')
        require(not c['wallpaper_browser']['busy'] and not c['wallpaper_busy'], 'wallpaper_transaction_active')
    def safe(*, settled=True, check_open=True):
        require(cleaning or time.monotonic() - started < 600, 'bounded_preview_acceptance_deadline')
        input_guard()
        require(binary() == process, 'exact_native_process_changed')
        require(supervisor()['epoch'] == parent_epoch, 'normal_supervisor_epoch_changed')
        require(monitor_key(monitor()) == monitor_key(original_monitor), 'output_workspace_or_reservation_changed')
        state = inspect()
        no_other_panels(state)
        require(not state['client']['live_wallpaper']['importing'], 'unexpected_import_in_progress')
        if settled:
            require(state['client']['settings_save_status'] == 'saved' and not state['client']['settings_save_error'], 'saved_preferences_required')
        if check_open and expected_open is not None:
            require(state['client']['live_wallpaper']['open'] is expected_open, 'foreground_ui_ownership_lost')
        return state
    def persistence_guard(full=False):
        if full:
            require(clipboard() == original_clipboard, 'clipboard_or_history_changed_no_restore')
        return guest(('guard' if applied else 'browse-check') if full else 'input-check')
    def qmp(events, guard=True):
        if guard:
            input_guard()
            require(last_cursor is not None and cursor() == last_cursor, 'cursor_ownership_lost_before_input')
        result = run(['virsh', '-c', lab.URI, 'qemu-monitor-command', name,
                      json.dumps({'execute': 'input-send-event', 'arguments': {'events': events}})])
        require('error' not in json.loads(result), 'native_pointer_delivery_failed')
    def move(x, y):
        nonlocal last_cursor, cursor_mutated
        safe()
        require(last_cursor is not None and cursor() == last_cursor, 'independent_cursor_motion_no_overwrite')
        require(0 <= x < original_monitor['width'] and 0 <= y < original_monitor['height'], 'bounded_pointer_required')
        input_guard()
        require(cursor() == last_cursor, 'cursor_changed_before_motion')
        cursor_mutated = True
        # Final ownership check is immediately followed by the actual transport.
        result = run(['virsh', '-c', lab.URI, 'qemu-monitor-command', name,
                      json.dumps({'execute': 'input-send-event', 'arguments': {'events': [
                          {'type': 'abs', 'data': {'axis': axis, 'value': round(value / (extent - 1) * 32767)}}
                          for axis, value, extent in (('x', x, original_monitor['width']), ('y', y, original_monitor['height']))]}})])
        require('error' not in json.loads(result), 'native_motion_failed')
        last_cursor = None
        def arrived():
            nonlocal last_cursor
            current = cursor()
            if abs(current['x'] - x) <= 2 and abs(current['y'] - y) <= 2:
                last_cursor = current
                return True
            return False
        eventually(arrived, 'native_cursor_did_not_arrive', timeout=3)
    def surface(state=None):
        return native.find_surface(state or inspect(), 'live-wallpaper')
    def origin(value):
        result = []
        def visit(node):
            if isinstance(node, dict):
                if node.get('namespace') == 'lucent-' + value['id']:
                    result.append(node)
                for child in node.values():
                    visit(child)
            elif isinstance(node, list):
                for child in node:
                    visit(child)
        visit(json.loads(session('hyprctl', '-j', 'layers')))
        require(len(result) == 1, 'exact_current_native_layer_required')
        answer = result[0]
        require(abs(answer['w'] - value['width']) <= .5 and abs(answer['h'] - value['height']) <= .5, 'current_native_extent_required')
        return answer
    def visible(hit):
        clip = hit['clip']
        return (max(hit['x'], clip['x']), max(hit['y'], clip['y']),
                min(hit['x'] + hit['width'], clip['x'] + clip['width']),
                min(hit['y'] + hit['height'], clip['y'] + clip['height']))
    def target(identifier):
        state = safe()
        value = surface(state)
        require(value['keyboard'] == 'exclusive'
                and [native.role(item) for item in state['surfaces'] if item['keyboard'] == 'exclusive'] == ['live-wallpaper'],
                'one_exact_live_panel_keyboard_owner_required')
        hits = [hit for hit in value['hits'] if hit['id'] == identifier]
        require(len(hits) == 1, 'unique_current_native_hit_required')
        bounds = visible(hits[0])
        require(bounds[2] - bounds[0] > 4 and bounds[3] - bounds[1] > 4, 'visible_current_native_hit_required')
        return value, hits[0], origin(value), bounds
    def key_tab():
        safe()
        persistence_guard()
        input_guard()
        require(last_cursor is not None and cursor() == last_cursor, 'cursor_ownership_lost_before_tab')
        run(['virsh', '-c', lab.URI, 'send-key', name, '--codeset', 'linux', '--holdtime', '25', '15'])
        time.sleep(.045)
    def reveal(identifier):
        for _ in range(64):
            state = safe()
            if surface(state).get('focus') == identifier and surface(state).get('focus_visible'):
                _, _, _, bounds = target(identifier)
                if bounds[3] - bounds[1] > 4:
                    return
            key_tab()
        raise Failure('keyboard_reveal_or_focus_visibility_failed')
    def click(identifier):
        value, _, where, bounds = target(identifier)
        identity = (value['id'], value.get('instance'), bounds, where['x'], where['y'])
        def wins(point, candidate):
            hits = [hit['id'] for hit in candidate['hits']
                    if visible(hit)[0] <= point[0] < visible(hit)[2] and visible(hit)[1] <= point[1] < visible(hit)[3]]
            return hits and hits[-1] == identifier
        points = [(bounds[0] + (bounds[2] - bounds[0]) * x, bounds[1] + (bounds[3] - bounds[1]) * y)
                  for x, y in ((.5, .35), (.15, .35), (.85, .35), (.5, .65))]
        point = next((p for p in points if wins(p, value)), None)
        require(point is not None, 'uncovered_native_hit_required')
        move(where['x'] + point[0], where['y'] + point[1])
        current, _, now, rectangle = target(identifier)
        require((current['id'], current.get('instance'), rectangle, now['x'], now['y']) == identity and wins(point, current), 'native_target_changed_before_press')
        safe()
        persistence_guard()
        input_guard()
        require(last_cursor is not None and cursor() == last_cursor, 'cursor_ownership_lost_before_press')
        try:
            qmp([{'type': 'btn', 'data': {'button': 'left', 'down': True}}], guard=False)
            time.sleep(.04)
        finally:
            # A transport failure may occur after our press reached the guest.
            qmp([{'type': 'btn', 'data': {'button': 'left', 'down': False}}], guard=False)
    def panel(opened, *, rapid=False):
        nonlocal expected_open
        safe()
        persistence_guard()
        input_guard()
        require(last_cursor is not None and cursor() == last_cursor, 'cursor_ownership_lost_before_panel_command')
        cli('wallpaper', 'live', 'open' if opened else 'close')
        expected_open = opened
        eventually(lambda: inspect()['client']['live_wallpaper']['open'] is opened, 'owned_panel_command_not_admitted', timeout=8)
        if not rapid:
            if opened:
                eventually(lambda: not inspect()['client']['live_wallpaper']['library_loading']
                           and not inspect()['client']['live_wallpaper']['awaiting_first_frame']
                           and surface()['frames'] > 0, 'native_preview_panel_not_ready', timeout=15)
                time.sleep(.9)
            else:
                eventually(lambda: not inspect()['client']['live_wallpaper']['preview']['active']
                           and not inspect()['client']['live_wallpaper']['preview']['selected']
                           and inspect()['client']['live_wallpaper']['preview']['cached'] == 0,
                           'preview_coordinator_did_not_retire_after_close', timeout=15)
                time.sleep(.9)
    def select_card(index):
        # Open preserves the previous selected source. The target fixture need
        # not be in the carousel's currently mounted range, even at index zero.
        for _ in range(33):
            current = safe()['client']['live_wallpaper']['selection']
            if current == index:
                identifier = 'live-library-' + str(index)
                reveal(identifier)
                click(identifier)
                return
            identifier = 'live-next' if current < index else 'live-previous'
            candidate = surface()
            hits = [hit for hit in candidate['hits'] if hit['id'] == identifier]
            if (len(hits) != 1 or visible(hits[0])[2] - visible(hits[0])[0] <= 4
                    or visible(hits[0])[3] - visible(hits[0])[1] <= 4):
                reveal(identifier)
            click(identifier)
            expected = current + (1 if current < index else -1)
            eventually(lambda: inspect()['client']['live_wallpaper']['selection'] == expected,
                       'native_browse_step_not_admitted', timeout=5)
        raise Failure('native_browse_cannot_reach_fixture')
    def preview_ready(index, size=None):
        def ready_preview():
            state = safe()
            live = state['client']['live_wallpaper']
            preview = live['preview']
            frame = preview['frame_size']
            require(type(preview['active']) is bool and type(preview['retiring']) is bool
                    and 0 <= preview['cached'] <= 5, 'bounded_admitted_preview_state_required')
            return (live['selection'] == index and preview['selected'] and preview['active']
                    and not preview['retiring'] and not preview['loading'] and not preview['failed']
                    and frame is not None and all(0 < edge <= 1024 for edge in frame)
                    and (size is None or frame == size))
        eventually(ready_preview, 'latest_selected_native_preview_not_ready', timeout=30)
        persistence_guard()
        time.sleep(.4)
    def capture_colors(index, label):
        # Only a 24x24 interior image crop is captured; no full desktop, captions,
        # user filenames or forms enter the local capture artifacts.
        value, hit, where, bounds = target('live-library-' + str(index))
        crop = (round(where['x'] + hit['x'] + hit['width'] / 2 - 12),
                round(where['y'] + hit['y'] + hit['height'] * .32 - 12), 24, 24)
        require(crop[0] >= where['x'] + bounds[0] and crop[1] >= where['y'] + bounds[1]
                and crop[0] + 24 <= where['x'] + bounds[2] and crop[1] + 24 <= where['y'] + bounds[3],
                'capture_crop_must_be_inside_selected_card')
        signature = (value['id'], value.get('instance'), bounds, where['x'], where['y'])
        observed = set()
        for number in range(16):
            state = safe()
            require(state['client']['live_wallpaper']['selection'] == index, 'capture_selection_changed')
            current, _, now, rectangle = target('live-library-' + str(index))
            require((current['id'], current.get('instance'), rectangle, now['x'], now['y']) == signature, 'capture_native_geometry_changed')
            data = session('grim', '-g', f'{crop[0]},{crop[1]} {crop[2]}x{crop[3]}', '-t', 'ppm', '-')
            # P6 header is short and fixed for grim. Consume exactly its delimiter,
            # never strip a binary pixel byte that happens to be whitespace.
            match = re.match(rb'P6\s+(\d+)\s+(\d+)\s+255(\r\n|[\t\n\r ])', data)
            require(match is not None and [int(match[1]), int(match[2])] == [24, 24]
                    and len(data) - match.end() == 24 * 24 * 3, 'cropped_native_ppm_required')
            pixels = data[match.end():]
            rgb = [sum(pixels[channel::3]) / (24 * 24) for channel in range(3)]
            order = sorted(range(3), key=rgb.__getitem__, reverse=True)
            if rgb[order[0]] >= 150 and rgb[order[1]] <= 100:
                observed.add(order[0])
            filename = f'{label}-{number:02}.ppm'
            private(directory / filename, data)
            report['captures'].append({'file': filename, 'sha256': sha(data), 'width': 24, 'height': 24})
            if len(observed) >= 2:
                report['checks'][label + '_actual_animated_crop'] = True
                return
            time.sleep(.17)
        raise Failure('selected_native_preview_pixels_did_not_animate')
    def wheel(down):
        value, _, where, bounds = target('live-scroll')
        move(where['x'] + bounds[0] + 12, where['y'] + (bounds[1] + bounds[3]) / 2)
        safe()
        persistence_guard()
        button = 'wheel-down' if down else 'wheel-up'
        for _ in range(4):
            qmp([{'type': 'btn', 'data': {'button': button, 'down': True}},
                 {'type': 'btn', 'data': {'button': button, 'down': False}}])
            time.sleep(.05)
    def quiescent():
        require(service_scope is not None, 'captured_service_scope_required')
        result = json.loads(session('python3', '-c', SERVICE_QUIESCENCE + '\nprint(json.dumps(require_service_quiescent(json.load(__import__("sys").stdin))))',
                                    data=json.dumps(service_scope).encode()))
        require(all(result.values()), 'exact_service_cgroup_not_quiescent')
    try:
        input_guard()
        original_monitor = monitor()
        require(original_monitor['scale'] == 1 and original_monitor['x'] == original_monitor['y'] == 0
                and original_monitor.get('transform', 0) == 0
                and not original_monitor.get('specialWorkspace', {}).get('id', 0), 'ordinary_origin_output_at_1x_required')
        process = binary()
        parent = supervisor()
        parent_epoch, supervisor_fact = parent['epoch'], parent['script']
        original = inspect()
        require(ready.desktop_rendered(original) and 'Vulkan' in original['adapter']
                and original['client']['settings_writable'] and original['client']['settings_save_status'] == 'saved'
                and original['client']['background_ready'] and original['client']['wallpaper'], 'initial_rendered_saved_desktop_required')
        require(original['client'].get('named_monitor_hosts') is True, 'observed_named_monitor_target_required')
        expected_open = False
        safe()
        original_cursor = last_cursor = cursor()
        original_clipboard = clipboard()
        original_wallpaper = original['client']['wallpaper']
        wallpaper_checkpoint = checkpoint_module.GuestWallpaperCheckpoint(
            lambda *argv: remote(*argv).decode(), original_wallpaper)
        private(directory / 'recovery.json', {
            'tag': tag, 'guest_settings_checkpoint': '.local/state/lucent/.native-live-preview-' + tag,
            'guest_wallpaper_checkpoint': wallpaper_checkpoint.path,
            'policy': 'Private locator only; inspect current ownership and exact cgroup before manual recovery. No blind restore.'})
        phase = 'private_fixtures_and_checkpoint'
        create_attempted = True
        guest('create', {'monitor': original_monitor['name']})
        created = True
        library = guest('discover')
        indices = library['indices']
        require(len(indices) == 3 and indices == list(range(indices[0], indices[0] + 3)), 'three_adjacent_owned_library_fixtures_required')
        report['library_count'] = library['count']
        report['checks']['original_settings_hardlink_checkpointed'] = True
        phase = 'select_only_and_native_navigation'
        panel(True)
        require(inspect()['client']['live_wallpaper']['library_count'] == library['count'], 'native_library_matches_current_discovery')
        select_card(indices[0])
        preview_ready(indices[0], [64, 40])
        capture_colors(indices[0], 'selected_a')
        # The adjacent card itself selects; it cannot import or save.
        click('live-library-' + str(indices[1]))
        preview_ready(indices[1], [64, 40])
        click('live-previous')
        preview_ready(indices[0], [64, 40])
        click('live-next')
        preview_ready(indices[1], [64, 40])
        persistence_guard(full=True)
        report['checks']['native_cards_previous_next_select_only'] = True
        reveal('live-path')
        require(surface()['focus'] == 'live-path' and surface()['focus_visible'], 'native_keyboard_focus_visible')
        before_scroll = next(hit for hit in surface()['hits'] if hit['id'] == 'live-library-refresh')['y']
        wheel(False)
        after_scroll = next(hit for hit in surface()['hits'] if hit['id'] == 'live-library-refresh')['y']
        require(after_scroll > before_scroll, 'native_scroll_did_not_move_content')
        reveal('live-previous')
        report['checks']['native_tab_reveal_focus_and_wheel_scroll'] = True
        phase = 'same_path_failure_retry_refresh'
        click('live-next')
        eventually(lambda: inspect()['client']['live_wallpaper']['selection'] == indices[2]
                   and inspect()['client']['live_wallpaper']['preview']['failed'], 'invalid_owned_preview_not_reported', timeout=30)
        persistence_guard()
        guest('repair', {'index': 2, 'size': [40, 64], 'phase': 1})
        reveal('live-preview-retry')
        click('live-preview-retry')
        preview_ready(indices[2], [40, 64])
        report['checks']['native_same_path_retry_recovers_without_import'] = True
        guest('repair', {'index': 2, 'size': [80, 32], 'phase': 2})
        reveal('live-library-refresh')
        click('live-library-refresh')
        eventually(lambda: not inspect()['client']['live_wallpaper']['library_loading'], 'native_refresh_not_completed')
        preview_ready(indices[2], [80, 32])
        persistence_guard(full=True)
        report['checks']['native_refresh_replaces_same_path_cached_preview'] = True
        phase = 'rapid_close_reopen_latest_demand'
        for _ in range(3):
            panel(False, rapid=True)
            panel(True, rapid=True)
        eventually(lambda: not inspect()['client']['live_wallpaper']['library_loading']
                   and not inspect()['client']['live_wallpaper']['awaiting_first_frame'], 'latest_reopen_not_ready')
        time.sleep(.9)
        preview_ready(indices[2], [80, 32])
        panel(False)
        report['checks']['native_close_reopen_latest_demand_and_zero_admitted_after_close'] = True
        persistence_guard(full=True)
        report['checks']['all_browsing_exact_settings_inode_sources_stores_exports_preserved'] = True
        phase = 'explicit_native_monitor_apply'
        panel(True)
        select_card(indices[0])
        preview_ready(indices[0], [64, 40])
        target_id = 'live-target-' + original_monitor['name']
        reveal(target_id)
        click(target_id)
        reveal('live-apply')
        guest('arm', {'index': 0})
        click('live-apply')
        # No further foreground input until this one explicitly armed profile
        # has saved and been admitted against every unrelated preference.
        eventually(lambda: inspect()['client']['settings_save_status'] == 'saved'
                   and not inspect()['client']['live_wallpaper']['importing']
                   and inspect()['client']['live_wallpaper']['running'] == 1
                   and inspect()['client']['live_wallpaper']['players'] == 1, 'native_monitor_apply_not_ready', timeout=35)
        admission = guest('admit')
        applied = True
        report['checks'].update(admission['checks'])
        report['metrics'].update(admission['metrics'])
        panel(False)
        frames = sum(item['frames'] for item in inspect()['surfaces'] if native.role(item) == 'background')
        eventually(lambda: sum(item['frames'] for item in inspect()['surfaces'] if native.role(item) == 'background') > frames + 2,
                   'applied_native_background_not_submitting', timeout=10)
        persistence_guard(full=True)
        report['checks']['native_monitor_apply_advances_background'] = True
        report['checks']['monitor_apply_keeps_static_wallpaper_theme_login_exports_exact'] = True
    except BaseException as error:
        report['failed_phase'] = phase
        report['failed_check'] = str(error) if isinstance(error, Failure) else type(error).__name__
    finally:
        cleaning = True
        def restore(label, operation):
            try:
                operation()
                report['restoration'][label] = True
            except BaseException as error:
                report['restoration'][label] = False
                report['restoration'][label + '_failure'] = str(error) if isinstance(error, Failure) else type(error).__name__
        if created:
            def restore_owned_state():
                nonlocal service_scope, stopped, process, expected_open, applied, parent_epoch
                safe()
                require(last_cursor is not None and cursor() == last_cursor, 'cleanup_cursor_ownership_lost_no_mutation')
                require(clipboard() == original_clipboard, 'clipboard_changed_no_restore')
                if not applied:
                    # A failure immediately after Apply may still have saved its
                    # one armed candidate. Admit only that exact allowed delta;
                    # an arbitrary change cannot gain rollback authority.
                    try:
                        guest('browse-check')
                    except Failure:
                        guest('admit')
                        applied = True
                guest('guard' if applied else 'browse-check')
                if expected_open:
                    panel(False)
                service_scope = json.loads(session('python3', '-c', SERVICE_QUIESCENCE + '\nprint(json.dumps(capture_service_scope()))'))
                stopped = True
                session('systemctl', '--user', 'stop', 'lucent.service', timeout=30)
                quiescent()
                guest('restore', {'service_scope': service_scope})
                applied = False
                require(clipboard() == original_clipboard, 'clipboard_changed_while_stopped_no_restart')
                wallpaper_checkpoint.restore()
                session('systemctl', '--user', 'start', 'lucent.service')
                stopped = False
                eventually(lambda: ready.desktop_rendered(inspect()) and inspect()['client']['settings_save_status'] == 'saved'
                           and inspect()['client']['wallpaper'] == original['client']['wallpaper'], 'original_rendered_wallpaper_not_restored', timeout=45)
                process = binary()
                parent_epoch = supervisor()['epoch']
                expected_open = False
                safe()
                guest('verify')
                require(clipboard() == original_clipboard, 'clipboard_history_not_preserved')
                # Stable parent/readiness window is longer than startup timeout.
                until = time.monotonic() + 35
                while time.monotonic() < until:
                    safe()
                    require(ready.desktop_rendered(inspect()), 'restored_desktop_lost_readiness')
                    time.sleep(min(2, max(0, until - time.monotonic())))
                guest('verify')
            restore('exact_original_settings_inode_wallpaper_and_stable_runtime', restore_owned_state)
            if report['restoration'].get('exact_original_settings_inode_wallpaper_and_stable_runtime'):
                def cleanup_fixtures():
                    persistence_guard(full=True)
                    guest('discard')
                    wallpaper_checkpoint.discard()
                restore('only_owned_fixtures_import_and_checkpoint_removed', cleanup_fixtures)
        elif wallpaper_checkpoint is not None and not create_attempted:
            restore('unused_wallpaper_checkpoint_discarded', wallpaper_checkpoint.discard)
        elif create_attempted:
            # A lost create reply is uncertain, not proof no guest files exist.
            # Keep both recovery locators/checkpoints for independent review.
            report['restoration']['uncertain_create_left_for_review'] = False
        if cursor_mutated and original_cursor is not None and not stopped:
            def restore_cursor():
                nonlocal last_cursor
                safe()
                require(cursor() == last_cursor, 'independent_cursor_motion_preserved_no_restore')
                move(original_cursor['x'], original_cursor['y'])
            restore('owned_cursor_position', restore_cursor)
        if original is not None and not stopped:
            try:
                state = inspect()
                report['restoration']['original_bar_and_wallpaper_observed'] = (
                    state['client']['bar_visible'] == original['client']['bar_visible']
                    and state['client']['wallpaper'] == original['client']['wallpaper'])
            except BaseException:
                report['restoration']['original_bar_and_wallpaper_observed'] = False
        report['passed'] = (not report.get('failed_check') and bool(report['checks']) and all(value is True for value in report['checks'].values())
                            and bool(report['restoration']) and all(v is True for k, v in report['restoration'].items() if not k.endswith('_failure')))
        report['elapsed_seconds'] = round(time.monotonic() - started, 3)
        report['recovery_material_retained'] = bool(create_attempted and not report['restoration'].get('only_owned_fixtures_import_and_checkpoint_removed'))
        private(directory / 'report.json', report)
        print(json.dumps({'passed': report['passed'], 'checks': report['checks'], 'metrics': report['metrics'], 'restoration': report['restoration'],
                          'recovery_material_retained': report['recovery_material_retained']}), flush=True)
        os.close(lock)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
