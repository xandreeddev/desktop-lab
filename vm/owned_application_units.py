"""Typed systemd identity checks shared by owned native application acceptance.

Import is inert. Unit inspection uses private session D-Bus connections and the
structured ExecStart argv property; no serialized command substring is authority.
"""

UNIT_IDENTITY = r'''
import json,os,re,stat,time
from pathlib import Path

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
  value=properties.GetAll('org.freedesktop.systemd1.Unit',timeout=5)
  service=properties.GetAll('org.freedesktop.systemd1.Service',timeout=5)
  commands=service['ExecStart'];assert len(commands)==1
  return {'load':str(value['LoadState']),'active':str(value['ActiveState']),
          'transient':bool(value['Transient']),'group':str(service['ControlGroup']),
          'executable':str(commands[0][0]),'argv':[str(arg) for arg in commands[0][1]],
          'main_pid':int(service['MainPID'])}
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

def retire_unit(unit,argv,group=None):
 import dbus
 value=unit_snapshot(unit)
 if value['load']=='not-found':return
 assert_unit_identity(value,unit,argv,group)
 if value['active'] not in ('inactive','failed'):
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
prefix,unit,script,run_id=sys.argv[1:]
assert prefix in ('lucent-application-actions-','lucent-application-intent-',
                  'lucent-application-intent-watchdog-')
assert unit==prefix+run_id+'.service'
retire_unit(unit,owned_argv(prefix,script,run_id))
print(json.dumps({'retired':True}))
'''
