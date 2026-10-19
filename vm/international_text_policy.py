"""Pure admission and owned-process checks for the native text fixture only."""
import math

from application_intent_policy import CursorOwnership, absolute_move
from owned_application_units import UNIT_IDENTITY


APP_NAME = 'lucent-international-text'
NAMESPACE = APP_NAME + '-international'
MULTILINE_TEXT = 'سلام\nक्षि\n日本語'
OUTPUT_KEYS = ('id', 'name', 'x', 'y', 'width', 'height', 'scale', 'transform',
               'refreshRate', 'focused', 'dpmsStatus', 'activeWorkspace', 'specialWorkspace')


def require(condition, label):
    if not condition:
        raise ValueError(label)


def output_state(monitors):
    return [{key: monitor.get(key) for key in OUTPUT_KEYS} for monitor in monitors]


def workspace_state(workspace):
    return {key: workspace.get(key) for key in ('id', 'name', 'monitorID')}


def idle_client(client):
    require(isinstance(client, dict) and not client.get('launcher') and not client.get('query')
            and not any(client.get(key) for key in ('notification_count', 'notification_history_open',
                        'widgets_editing', 'widget_grid', 'dock_surface_expanded', 'launcher_opening'))
            and client.get('settings_save_status') == 'saved' and not client.get('wallpaper_busy'),
            'idle_desktop_without_competing_controls')
    for panel in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'live_wallpaper',
                  'displays', 'weather', 'settings', 'application_actions'):
        state = client.get(panel, {})
        require(isinstance(state, dict) and not any(state.get(key) for key in ('open', 'visible', 'busy')),
                'no_foreground_shell_panel')
    require(not client.get('network', {}).get('form')
            and not any(client.get('bluetooth', {}).get(key) for key in ('form', 'requests', 'answering')),
            'no_native_credential_request')


def safe_bindings(bindings):
    require(type(bindings) is list and len(bindings) <= 4096, 'bounded_actual_bindings')
    # Includes the modifier presses and every actual injected chord/button.
    chords = ((0, 'control_l', 29), (4, 'shift_l', 42), (0, 'backspace', 14),
              (0, 'home', 102), (0, 'right', 106), (0, 'mouse:272', 272),
              (4, 'a', 30), (4, 'z', 44), (5, 'z', 44))
    for binding in bindings:
        require(type(binding) is dict and type(binding.get('modmask')) is int, 'complete_actual_binding')
        mask = binding['modmask'] & ~(2 | 16)
        key = str(binding.get('key', '')).lower()
        for modifier, symbol, code in chords:
            aliases = (symbol, 'any', 'catchall', f'code:{code}', f'code:{code + 8}')
            if code == 272:
                aliases += ('mouse:left',)
            require(mask != modifier or (not binding.get('catch_all') and key not in aliases
                    and binding.get('keycode', 0) not in (code, code + 8)), 'no_binding_intercepts_native_fixture_input')


def owned_layers(layers, desktop_pid, example_pid=None):
    require(type(desktop_pid) is int and desktop_pid > 1, 'identified_desktop_process')
    found, visited = [], 0
    def visit(value, depth=0):
        nonlocal visited
        visited += 1
        require(visited <= 8192 and depth <= 16, 'bounded_native_layer_tree')
        if isinstance(value, dict):
            if 'namespace' in value:
                name = value['namespace']
                if name == NAMESPACE:
                    require(type(example_pid) is int and example_pid > 1 and value.get('pid') == example_pid,
                            'exact_owned_example_layer_process')
                    require(isinstance(value.get('address'), str) and value['address'], 'concrete_owned_layer_address')
                    found.append({key: value[key] for key in ('address', 'namespace', 'pid', 'x', 'y', 'w', 'h')})
                else:
                    require(isinstance(name, str) and name.startswith('lucent-')
                            and not name.startswith('lucent-authorization-') and value.get('pid') == desktop_pid,
                            'no_foreign_or_secure_native_layer')
            for child in value.values():
                visit(child, depth + 1)
        elif isinstance(value, list):
            for child in value:
                visit(child, depth + 1)
    visit(layers)
    require(len(found) == (1 if example_pid is not None else 0), 'one_exact_owned_example_layer')
    return found[0] if found else None


def field_geometry(state, layer, field):
    surfaces = [s for s in state['surfaces'] if s.get('role') == 'international']
    require(len(surfaces) == 1, 'one_owned_example_surface')
    surface = surfaces[0]
    require(surface.get('frames', 0) > 0 and surface.get('native_frames', 0) > 0
            and surface.get('pointer_input') in ('all', 'content'), 'submitted_interactive_example')
    hits = [hit for hit in surface['hits'] if hit['id'] == field]
    require(len(hits) == 1, 'one_current_owned_field')
    hit = hits[0]
    clip = hit['clip']
    values = [source.get(key) for source, keys in (
        (surface, ('width', 'height')), (layer, ('x', 'y', 'w', 'h')),
        (hit, ('x', 'y', 'width', 'height')), (clip, ('x', 'y', 'width', 'height')))
        for key in keys]
    require(all(type(value) in (int, float) and math.isfinite(value) for value in values), 'finite_owned_field_geometry')
    require(abs(layer['w'] - surface['width']) <= .5 and abs(layer['h'] - surface['height']) <= .5,
            'current_owned_field_surface_geometry')
    left, top = max(hit['x'], clip['x'], 0), max(hit['y'], clip['y'], 0)
    right = min(hit['x'] + hit['width'], clip['x'] + clip['width'], surface['width'])
    bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'], surface['height'])
    require(right-left > 4 and bottom-top > 4 and right-left >= hit['width']-.5 and bottom-top >= hit['height']-.5,
            'fully_visible_owned_field')
    point = (round(layer['x'] + (left+right)/2), round(layer['y'] + (top+bottom)/2))
    absolute_move(point)
    return surface, hit, {'point': point, 'layer': layer, 'hit': hit,
                          'surface': {key: surface.get(key) for key in ('id', 'role', 'instance', 'output', 'width', 'height')}}


def focused_field(state, field, status):
    require(field in ('single', 'body') and isinstance(state['client'].get(field), str), 'ordinary_owned_field')
    surfaces = [s for s in state['surfaces'] if s.get('role') == 'international']
    require(len(surfaces) == 1 and surfaces[0].get('focus') == field, 'exact_owned_component_focus')
    require(status.get('active') and status.get('matches') and not status.get('unavailable'),
            'native_input_method_confirms_owned_field_focus')


def captured_cleanup(configuration, attempted):
    """A lost/refused start reply never grants cleanup over a lookalike unit."""
    expected = configuration.get('expected', {})
    result = dict(configuration)
    missing = []
    for name in ('example', 'peer'):
        if name not in expected:
            result[name] = None
            if name in attempted:
                missing.append(name)
    return result, missing


# Hash/read the same pinned regular file before and after. Reading may update
# atime under relatime; it grants no content or identity mutation authority.
PINNED_READ_FUNCTIONS = r'''
import hashlib,os,stat
from pathlib import Path

def stable_file_identity(info):
 return [info.st_dev,info.st_ino,info.st_mode,info.st_uid,info.st_gid,
         info.st_size,info.st_mtime_ns,info.st_ctime_ns]

def read_pinned_file(raw,limit,canonical=False):
 path=Path(raw)
 assert path.is_absolute(),'absolute_owned_file_required'
 if canonical:assert path.resolve(strict=True)==path,'canonical_owned_file_required'
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as stream:
  info=os.fstat(stream.fileno())
  assert stat.S_ISREG(info.st_mode) and info.st_uid==os.getuid(),'owned_regular_file_required'
  assert 0<=info.st_size<=limit,'file_size_envelope'
  identity=stable_file_identity(info)
  assert stable_file_identity(path.lstat())==identity,'path_fd_identity_mismatch'
  digest=hashlib.sha256();consumed=0
  while True:
   chunk=stream.read(min(1024*1024,limit-consumed+1))
   if not chunk:break
   consumed+=len(chunk);assert consumed<=limit,'file_size_envelope'
   digest.update(chunk)
  assert stable_file_identity(os.fstat(stream.fileno()))==identity,'file_identity_changed_during_read'
  assert stable_file_identity(path.lstat())==identity,'path_identity_changed_during_read'
  assert consumed==info.st_size,'content_size_changed_during_read'
 return {'identity':identity,'sha256':digest.hexdigest()}
'''

# Functions/imports only: no filesystem or subprocess operations on import.
exec(PINNED_READ_FUNCTIONS)


# Invoked only by the prepared harness through the guest session. Reuses typed
# systemd ExecStart/InvocationID/cgroup and /proc identity checks from App tooling.
OBSERVATION_FUNCTIONS = UNIT_IDENTITY + r'''
import socket,struct,sys,subprocess

def inode(value):return [value.st_dev,value.st_ino,value.st_mode,value.st_uid,value.st_mtime_ns,value.st_ctime_ns]
def owned_socket(name,pid,expected=None):
 assert name in ('lucent','lucent-international-text')
 path=Path(os.environ['XDG_RUNTIME_DIR'])/(name+'.sock')
 before=path.lstat();assert stat.S_ISSOCK(before.st_mode) and before.st_uid==os.getuid()
 identity=inode(before)
 if expected is not None:assert identity==expected
 with socket.socket(socket.AF_UNIX) as connection:
  connection.settimeout(4);connection.connect(str(path))
  peer=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
  assert peer[0]==pid and peer[1]==os.getuid(),'exact_owned_socket_peer'
  assert inode(path.lstat())==identity
  connection.sendall(b'inspect\n');data=bytearray()
  while True:
   chunk=connection.recv(65536)
   if not chunk:break
   data+=chunk;assert len(data)<=4*1024*1024
 assert inode(path.lstat())==identity
 return identity,json.loads(data)

def owned_unit(spec,expected=None):
 unit=spec['unit'];argv=spec['argv']
 assert re.fullmatch(r'lucent-international-(test|peer)-[a-f0-9]{24}\.service',unit)
 assert len(argv)==1 and Path(argv[0]).is_absolute()
 value=unit_snapshot(unit);assert_unit_identity(value,unit,argv)
 assert value['active']=='active' and value['main_pid']>1
 process=process_snapshot(value['main_pid'],hashed=expected is None)
 assert process['group']==value['group']
 if expected is None:assert process['sha256']==spec['sha256']
 else:
  assert value==expected['unit']
  assert {k:v for k,v in process.items() if k!='sha256'}=={k:v for k,v in expected['process'].items() if k!='sha256'}
  process['sha256']=expected['process']['sha256']
 return {'unit':value,'process':process}

'''

OBSERVE = OBSERVATION_FUNCTIONS + r'''
config=json.loads(sys.argv[1]);expected=config.get('expected',{});authority={}
pids=subprocess.check_output(['pgrep','-x','lucent-desktop']).split();assert len(pids)==1
process=process_snapshot(int(pids[0]),hashed='desktop' not in expected)
if 'desktop' in expected:
 assert {k:v for k,v in process.items() if k!='sha256'}=={k:v for k,v in expected['desktop'].items() if k!='sha256'}
 process['sha256']=expected['desktop']['sha256']
else:assert process['sha256']==config['desktop_sha256']
authority['desktop']=process
identity,desktop=owned_socket('lucent',process['pid'],expected.get('desktop_socket'))
authority['desktop_socket']=identity
result={'desktop':desktop}
for name in ('example','peer'):
 if config.get(name) is not None:
  authority[name]=owned_unit(config[name],expected.get(name))
if 'example' in authority:
 identity,state=owned_socket('lucent-international-text',authority['example']['process']['pid'],expected.get('example_socket'))
 authority['example_socket']=identity;result['example']=state
else:
 assert not os.path.lexists(Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-international-text.sock'),'no_existing_example_socket'
result['authority']=authority
print(json.dumps(result))
'''

ABSENT = UNIT_IDENTITY + r'''
import sys
units=json.loads(sys.argv[1]);assert len(units)==2 and len(set(units))==2
for unit in units:
 assert re.fullmatch(r'lucent-international-(test|peer)-[a-f0-9]{24}\.service',unit)
 assert unit_snapshot(unit)=={'load':'not-found'},'initial_owned_unit_name_must_be_absent'
print(json.dumps({'initial_units_absent':True}))
'''

DIAGNOSTIC_FUNCTIONS = r'''
def diagnostic_error(error):
 allowed={'AssertionError','ValueError','FileNotFoundError','PermissionError','OSError',
          'TimeoutExpired','TimeoutError','ConnectionRefusedError','DBusException','Failure'}
 result={'error_class':type(error).__name__ if type(error).__name__ in allowed else 'OtherError'}
 try:
  name=error.get_dbus_name()
 except Exception:name=None
 if isinstance(name,str) and re.fullmatch(r'org\.freedesktop\.(?:DBus\.Error|systemd1)\.[A-Za-z_][A-Za-z0-9_]{0,96}',name):
  result['dbus_error_name']=name
 if type(getattr(error,'errno',None)) is int:result['errno']=error.errno
 return result

def diagnostic_unit(value,captured):
 result={key:value[key] for key in ('load','active','transient','main_pid','invocation') if key in value}
 if value.get('load')!='not-found' and captured is not None:
  original=captured['unit']
  for key in ('executable','argv','group','invocation','main_pid'):
   result[key+'_matches_pinned']=value.get(key)==original.get(key)
 return result
'''
exec('import re\n'+DIAGNOSTIC_FUNCTIONS)


RETIRE = OBSERVATION_FUNCTIONS + DIAGNOSTIC_FUNCTIONS + r'''
import sys
config=json.loads(sys.argv[1]);failures={};trace=[];truncated=False
original_unit_snapshot=unit_snapshot;original_process_snapshot=process_snapshot
current_name=None;captured=None;unit_observations=0
def record(event):
 global truncated
 if len(trace)<48:trace.append(event)
 else:trace[-1]=event;truncated=True
def traced_unit_snapshot(unit):
 global unit_observations
 phase='initial_unit_observation' if unit_observations==0 else 'completion_unit_observation'
 unit_observations+=1
 try:
  value=original_unit_snapshot(unit)
 except Exception as error:
  record(dict(diagnostic_error(error),name=current_name,phase=phase));raise
 record({'name':current_name,'phase':phase,'unit':diagnostic_unit(value,captured)})
 return value
def traced_process_snapshot(pid,*args,**kwargs):
 try:value=original_process_snapshot(pid,*args,**kwargs)
 except Exception as error:
  record(dict(diagnostic_error(error),name=current_name,phase='pinned_process_observation'));raise
 record({'name':current_name,'phase':'pinned_process_observation',
         'process':{key:value[key] for key in ('pid','start','executable')},
         'group_matches_pinned':captured is not None and value['group']==captured['process']['group'],
         'birth_matches_pinned':captured is not None and value['start']==captured['process']['start']})
 return value
unit_snapshot=traced_unit_snapshot;process_snapshot=traced_process_snapshot
for name in ('example','peer'):
 spec=config.get(name)
 if spec is None:continue
 current_name=name;captured=config.get('expected',{}).get(name);unit_observations=0
 try:
  assert re.fullmatch(r'lucent-international-(test|peer)-[a-f0-9]{24}\.service',spec['unit'])
  assert len(spec['argv'])==1 and Path(spec['argv'][0]).is_absolute()
  assert captured is not None,'captured_owned_invocation_required'
  retire_unit(spec['unit'],spec['argv'],group=captured['unit']['group'] if captured else None,captured=captured)
  record({'name':name,'phase':'retire_call_returned'})
 except Exception as error:
  # Continue retiring the other exact owned unit, then fail closed. No input or
  # file cleanup is admitted if either unit's lifetime could not be verified.
  failures[name]=dict(diagnostic_error(error),phase='retire_call')
  try:record({'name':name,'phase':'after_retire_failure','unit':diagnostic_unit(original_unit_snapshot(spec['unit']),captured)})
  except Exception as snapshot_error:record(dict(diagnostic_error(snapshot_error),name=name,phase='after_retire_failure'))
if not failures:
 try:
  if config.get('expected',{}).get('example_socket') is not None:
   path=Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-international-text.sock'
   if os.path.lexists(path):
    assert inode(path.lstat())==config['expected']['example_socket'],'only_exact_owned_retired_socket'
    path.unlink()
  record({'phase':'exact_stale_socket_cleanup_returned'})
 except Exception as error:failures['socket']=dict(diagnostic_error(error),phase='exact_stale_socket_cleanup')
print(json.dumps({'owned_units_retired':not failures,'retirement_failures':failures,
                  'phase_snapshots':trace,'phase_snapshots_truncated':truncated}))
'''
