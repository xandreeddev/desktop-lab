"""Typed systemd identity checks shared by owned native application acceptance.

Import is inert. Unit inspection uses private session D-Bus connections and the
structured ExecStart argv property; no serialized command substring is authority.
"""

UNIT_IDENTITY = r'''
import hashlib,json,os,re,stat,time
from pathlib import Path

def process_snapshot(pid,hashed=False,max_executable_bytes=128*1024*1024):
 assert type(pid) is int and pid>1
 assert type(max_executable_bytes) is int and 0<max_executable_bytes<=512*1024*1024
 root=Path('/proc')/str(pid)
 assert root.stat().st_uid==os.getuid()
 def start():return (root/'stat').read_text().rsplit(')',1)[1].split()[19]
 birth=start()
 def identity(value):return [value.st_dev,value.st_ino,value.st_mode,value.st_uid,
                            value.st_size,value.st_mtime_ns,value.st_ctime_ns]
 with (root/'exe').open('rb') as stream:
  before=os.fstat(stream.fileno())
  assert stat.S_ISREG(before.st_mode) and before.st_size<=max_executable_bytes
  executable=identity(before)
  digest=None
  if hashed:
   out=hashlib.sha256();count=0
   for block in iter(lambda:stream.read(1024*1024),b''):
    count+=len(block);assert count<=before.st_size and count<=max_executable_bytes
    out.update(block)
   assert count==before.st_size
   digest=out.hexdigest()
  assert identity(os.fstat(stream.fileno()))==executable
 group=(root/'cgroup').read_text()
 assert len(group)<16384 and group.startswith('0::') and group.count('\n')==1
 after=(root/'exe').stat()
 assert executable==identity(after)
 assert birth==start()
 return {'pid':pid,'start':birth,'executable':executable,'sha256':digest,
         'group':group.removeprefix('0::').strip()}

def owned_manifest(script,run_id):
 directory=Path(script).parent
 assert directory.is_absolute() and directory.resolve(strict=True)==directory
 value=directory.lstat()
 assert stat.S_ISDIR(value.st_mode) and value.st_uid==os.getuid() and stat.S_IMODE(value.st_mode)==0o700
 assert re.fullmatch(r'[A-Za-z0-9]{6,32}',run_id)
 path=directory/'manifest.json'
 def identity(value):return [value.st_dev,value.st_ino,value.st_mode,value.st_uid,value.st_gid,
                            value.st_size,value.st_mtime_ns,value.st_ctime_ns]
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as stream:
  value=os.fstat(stream.fileno())
  assert stat.S_ISREG(value.st_mode) and value.st_uid==os.getuid() and value.st_size<=131072
  raw=stream.read(131073)
  assert len(raw)<=131072 and identity(os.fstat(stream.fileno()))==identity(value)==identity(path.lstat())
 manifest=json.loads(raw);assert manifest['run_id']==run_id
 return directory,manifest

def owned_argv(prefix,script,run_id):
 directory,manifest=owned_manifest(script,run_id)
 if prefix=='lucent-application-actions-':
  return ['/usr/bin/python3',script,'serve',str(directory),run_id]
 if prefix=='lucent-application-intent-watchdog-':
  return ['/usr/bin/python3',script,'watchdog',str(directory),run_id]
 assert prefix=='lucent-application-intent-'
 assert Path(script).name=='application-intent'
 return ['/usr/bin/python3',str(directory/'fixture.py'),'start-example',str(directory),run_id]

def unit_snapshot(unit):
 import dbus
 bus=dbus.SessionBus(private=True)
 try:
  manager=dbus.Interface(bus.get_object('org.freedesktop.systemd1','/org/freedesktop/systemd1'),
                         'org.freedesktop.systemd1.Manager')
  try:path=manager.GetUnit(unit,timeout=5)
  except dbus.DBusException as error:
   if error.get_dbus_name()=='org.freedesktop.systemd1.NoSuchUnit':return {'load':'not-found'}
   raise
  properties=dbus.Interface(bus.get_object('org.freedesktop.systemd1',path),'org.freedesktop.DBus.Properties')
  def unit_value(key):return properties.Get('org.freedesktop.systemd1.Unit',key,timeout=5)
  def service_value(key):return properties.Get('org.freedesktop.systemd1.Service',key,timeout=5)
  # Do not retrieve unrelated service properties such as its environment.
  commands=service_value('ExecStart');assert len(commands)==1
  return {'load':str(unit_value('LoadState')),'active':str(unit_value('ActiveState')),
          'transient':bool(unit_value('Transient')),'group':str(service_value('ControlGroup')),
          'executable':str(commands[0][0]),'argv':[str(arg) for arg in commands[0][1]],
          'main_pid':int(service_value('MainPID')),
          'invocation':bytes(unit_value('InvocationID')).hex()}
 finally:bus.close()

def assert_unit_identity(value,unit,argv,group=None):
 assert value['load']!='not-found' and value['transient']
 assert value['executable']==argv[0] and value['argv']==argv
 current=value['group']
 assert not current or current.endswith('/'+unit)
 if value['active'] not in ('inactive','failed'):
  assert current and (group is None or current==group)
 elif group is not None:
  assert current in ('',group)

def _same_invocation(value,captured):
 original=captured['unit']
 assert value['invocation']==original['invocation']
 for key in ('load','transient','executable','argv'):assert value[key]==original[key]
 assert value['group'] in ('',original['group'])
 assert value['main_pid'] in (0,original['main_pid'])

def capture_unit_origin(unit,argv,max_executable_bytes=128*1024*1024):
 value=unit_snapshot(unit)
 assert_unit_identity(value,unit,argv)
 assert value['active'] in ('active','activating') and value['main_pid']==os.getpid()
 assert re.fullmatch('[0-9a-f]{32}',value['invocation']) and set(value['invocation'])!={'0'}
 process=process_snapshot(os.getpid(),hashed=True,max_executable_bytes=max_executable_bytes)
 assert process['group']==value['group'] and unit_snapshot(unit)==value
 return {'unit':value,'process':process}

def assert_unit_origin(value,unit,argv,captured,max_executable_bytes=128*1024*1024,hashed=False):
 assert_unit_identity(value,unit,argv,captured['unit']['group'])
 _same_invocation(value,captured)
 if value['active'] in ('inactive','failed'):return None
 assert value['main_pid']==captured['process']['pid']
 current=process_snapshot(value['main_pid'],hashed=hashed,max_executable_bytes=max_executable_bytes)
 assert all(current[key]==captured['process'][key] for key in ('pid','start','group'))
 return current

def retire_original_unit(unit,argv,captured,max_executable_bytes=128*1024*1024):
 value=unit_snapshot(unit)
 if value['load']=='not-found':return
 current=assert_unit_origin(value,unit,argv,captured,max_executable_bytes,hashed=True)
 # Exec may change executable after the original wrapper's durable receipt.
 # InvocationID and PID birth remain original; full current proof is frozen
 # again for the final StopUnit admission and every completion observation.
 retire_unit(unit,argv,captured['unit']['group'],{'unit':value,'process':current},max_executable_bytes)

def retire_unit(unit,argv,group=None,captured=None,max_executable_bytes=128*1024*1024):
 import dbus
 value=unit_snapshot(unit)
 if value['load']=='not-found':return
 assert_unit_identity(value,unit,argv,group)
 if captured is not None:_same_invocation(value,captured)
 if value['active'] not in ('inactive','failed'):
  if captured is not None:
   assert {k:v for k,v in value.items() if k!='active'}==\
          {k:v for k,v in captured['unit'].items() if k!='active'}
   assert process_snapshot(value['main_pid'],hashed=True,max_executable_bytes=max_executable_bytes)==captured['process']
  bus=dbus.SessionBus(private=True)
  try:
   manager=dbus.Interface(bus.get_object('org.freedesktop.systemd1','/org/freedesktop/systemd1'),
                          'org.freedesktop.systemd1.Manager')
   manager.StopUnit(unit,'replace',timeout=5)
  finally:bus.close()
 deadline=time.monotonic()+15
 while True:
  value=unit_snapshot(unit)
  if value['load']=='not-found':return
  assert_unit_identity(value,unit,argv,group)
  if captured is not None:_same_invocation(value,captured)
  if value['active'] in ('inactive','failed'):return
  assert time.monotonic()<deadline
  time.sleep(.1)
'''

# The fixture imports these same definitions from its private copied module.
exec(UNIT_IDENTITY)

RETIRE = UNIT_IDENTITY + r'''
import sys
prefix,unit,script,run_id=sys.argv[1:5]
# Intent wrappers keep immutable pre-exec/admission receipts in their private
# fixture. They must never fall back to this legacy name/argv-only entrypoint.
assert prefix=='lucent-application-actions-'
assert unit==prefix+run_id+'.service'
assert len(sys.argv) in (5,6)
captured=json.loads(sys.argv[5]) if len(sys.argv)==6 else None
retire_unit(unit,owned_argv(prefix,script,run_id),captured=captured)
print(json.dumps({'retired':True}))
'''
