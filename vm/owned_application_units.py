"""Typed systemd identity checks shared by owned native application acceptance.

Import is inert. Unit inspection uses private session D-Bus connections and the
structured ExecStart argv property; no serialized command substring is authority.
"""

UNIT_IDENTITY = r'''
import hashlib,json,os,re,stat,time
from pathlib import Path

def process_snapshot(pid,hashed=False):
 assert type(pid) is int and pid>1
 root=Path('/proc')/str(pid)
 assert root.stat().st_uid==os.getuid()
 def start():return (root/'stat').read_text().rsplit(')',1)[1].split()[19]
 birth=start()
 def identity(value):return [value.st_dev,value.st_ino,value.st_mode,value.st_uid,
                            value.st_size,value.st_mtime_ns,value.st_ctime_ns]
 with (root/'exe').open('rb') as stream:
  before=os.fstat(stream.fileno())
  assert stat.S_ISREG(before.st_mode) and before.st_size<=128*1024*1024
  executable=identity(before)
  digest=hashlib.file_digest(stream,'sha256').hexdigest() if hashed else None
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
 fd=os.open(directory/'manifest.json',os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as stream:
  value=os.fstat(stream.fileno())
  assert stat.S_ISREG(value.st_mode) and value.st_uid==os.getuid() and value.st_size<=131072
  raw=stream.read(131073);assert len(raw)<=131072 and os.fstat(stream.fileno())==value
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
 argv=[script,'--app',manifest['app_id'],'--action','Probe']
 assert len(manifest['files'])==2
 for item in manifest['files']:
  assert Path(item['name']).name==item['name']
  argv+=['--target',str(directory/item['name'])]
 return argv

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

def retire_unit(unit,argv,group=None,captured=None):
 import dbus
 value=unit_snapshot(unit)
 if value['load']=='not-found':return
 assert_unit_identity(value,unit,argv,group)
 if value['active'] not in ('inactive','failed'):
  if captured is not None:
   assert {k:v for k,v in value.items() if k!='active'}==\
          {k:v for k,v in captured['unit'].items() if k!='active'}
   assert process_snapshot(value['main_pid'],hashed=True)==captured['process']
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
  if value['active'] in ('inactive','failed'):return
  assert time.monotonic()<deadline
  time.sleep(.1)
'''

# The fixture imports these same definitions from its private copied module.
exec(UNIT_IDENTITY)

RETIRE = UNIT_IDENTITY + r'''
import sys
prefix,unit,script,run_id=sys.argv[1:5]
assert prefix in ('lucent-application-actions-','lucent-application-intent-',
                  'lucent-application-intent-watchdog-')
assert unit==prefix+run_id+'.service'
assert len(sys.argv) in (5,6)
captured=json.loads(sys.argv[5]) if len(sys.argv)==6 else None
retire_unit(unit,owned_argv(prefix,script,run_id),captured=captured)
print(json.dumps({'retired':True}))
'''
