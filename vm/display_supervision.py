"""Read-only evidence and an exact contract for a deliberately crashed UI.

No IO runs on import. The shared preservation fingerprint remains unchanged;
only this explicit crash acceptance permits its proven single unit restart.
"""
import copy
import json
import math

SCRIPT = r'''
import hashlib,json,os,shlex,stat,subprocess,sys,time
from pathlib import Path
def properties():
    keys=['Id','ActiveState','SubState','MainPID','NRestarts','KillMode','FragmentPath','DropInPaths','InvocationID']
    args=['systemctl','--user','show','lucent.service']
    for key in keys:args.extend(['-p',key])
    return dict(line.split('=',1) for line in subprocess.check_output(args,text=True,timeout=5).splitlines())
def captured(path,limit,follow=False):
    fd=os.open(path,os.O_RDONLY|os.O_NONBLOCK|(0 if follow else os.O_NOFOLLOW))
    with os.fdopen(fd,'rb') as stream:
        before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_size<=limit
        identity=lambda m:[m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_size,m.st_mtime_ns]
        size=0;digest=hashlib.sha256()
        for block in iter(lambda:stream.read(65536),b''):
            size+=len(block);assert size<=limit;digest.update(block)
        assert identity(before)==identity(os.fstat(stream.fileno())) and size==before.st_size
        return {'sha256':digest.hexdigest(),'identity':identity(before)}
def birth(entry):return (entry/'stat').read_text().rsplit(')',1)[1].split()[19]
p=properties();assert p['Id']=='lucent.service' and p['ActiveState']=='active' and p['SubState']=='running' and p['KillMode']=='mixed'
pid=int(p['MainPID']);entry=Path('/proc')/str(pid)
assert entry.stat().st_uid==os.getuid()
start=birth(entry)
argv=[a.decode() for a in (entry/'cmdline').read_bytes().split(b'\0') if a]
expected=Path.home()/'.local/lib/lucent/session.py'
assert len(argv)==2 and Path(argv[1])==expected and Path(argv[0]).resolve()==Path('/proc/self/exe').resolve()
exe=captured(entry/'exe',64*1024*1024,True)
unit=Path(p['FragmentPath']);assert unit==Path.home()/'.config/systemd/user/lucent.service'
paths=[unit]+[Path(value) for value in shlex.split(p['DropInPaths'])]
for path in paths[1:]:assert path.parent==unit.with_name('lucent.service.d')
files={str(path):captured(path,256*1024) for path in paths}
assert birth(entry)==start and properties()==p
value={'properties':p,'supervisor':{'pid':pid,'birth':start,'argv':argv,'exe':exe,'cgroup':(entry/'cgroup').read_text()},'unit_files':files}
result={'identity':value}
if sys.argv[1]=='post':
    old,new=json.loads(sys.argv[2]),json.loads(sys.argv[3]);wanted=sys.argv[4]
    current=Path('/proc')/str(new['pid'])
    assert birth(current)==new['start'] and current.stat().st_uid==os.getuid()
    assert (current/'stat').read_text().rsplit(')',1)[1].split()[1]==p['MainPID']==new['supervisor_pid']
    assert len([a for a in (current/'cmdline').read_bytes().split(b'\0') if a])==1
    assert (current/'exe').resolve()==Path.home()/'.local/bin/lucent-desktop'
    program=captured(current/'exe',128*1024*1024,True)
    assert program['sha256']==wanted and program['identity'][:2]==new['exe_identity']
    assert (current/'cgroup').read_text().strip()=='0::'+new['cgroup']
    try:retired=birth(Path('/proc')/str(old['pid']))!=old['start']
    except (FileNotFoundError,ProcessLookupError):retired=True
    assert birth(current)==new['start'] and birth(entry)==start and properties()==p
    result['post']={'old_ui_retired':retired,'current_ui_matches':True,'current_supervisor_matches':True,
                    'old_ui_pid':old['pid'],'old_ui_birth':old['start'],'ui_pid':new['pid'],'ui_birth':new['start'],
                    'supervisor_pid':pid,'supervisor_birth':start,'desktop_sha256':program['sha256'],'exe_identity':program['identity'][:2],
                    'ui_age_seconds':time.clock_gettime(time.CLOCK_BOOTTIME)-int(new['start'])/os.sysconf('SC_CLK_TCK')}
else:assert sys.argv[1]=='identity'
print(json.dumps(result))
'''


def identity(session):
    return json.loads(session('python3', '-c', SCRIPT, 'identity'))['identity']


def recovery_evidence(session, inspect, rendered, before_ui, after_ui, desktop_sha256):
    value = json.loads(session('python3', '-c', SCRIPT, 'post',
                               json.dumps(before_ui), json.dumps(after_ui), desktop_sha256))
    value['post']['native_surfaces_rendered'] = rendered(inspect())
    return value


def recovered_beyond_deadline(value):
    post = value['post']
    return (all(post[key] is True for key in ('old_ui_retired', 'current_ui_matches',
                                             'current_supervisor_matches', 'native_surfaces_rendered'))
            and math.isfinite(post['ui_age_seconds']) and post['ui_age_seconds'] > 15)


def expected_after_ui_crash(before, after, before_ui, after_ui, before_unit,
                            after_unit, recovered, expected_supervisor_sha256):
    """Validate captured authority before allowing exactly two service changes."""
    assert set(before) == set(after)
    assert all(before[key] == after[key] for key in before if key != 'service')
    assert before['files']['supervisor']['sha256'] == expected_supervisor_sha256
    assert recovered_beyond_deadline(recovered)
    assert recovered['identity'] == after_unit
    old, new = before_unit['properties'], after_unit['properties']
    assert set(old) == set(new)
    dynamic = {'MainPID', 'NRestarts', 'InvocationID'}
    assert all(old[key] == new[key] for key in old if key not in dynamic)
    assert old['Id'] == 'lucent.service' and old['ActiveState'] == 'active'
    assert old['SubState'] == 'running' and old['KillMode'] == 'mixed'
    assert old['MainPID'] != new['MainPID']
    assert int(new['NRestarts']) == int(old['NRestarts']) + 1
    for invocation in (old['InvocationID'], new['InvocationID']):
        assert len(invocation) == 32 and all(c in '0123456789abcdef' for c in invocation)
    assert old['InvocationID'] != new['InvocationID']
    assert before_unit['unit_files'] and before_unit['unit_files'] == after_unit['unit_files']
    old_process, new_process = before_unit['supervisor'], after_unit['supervisor']
    for key in ('argv', 'exe', 'cgroup'):
        assert old_process[key] == new_process[key]
    assert old_process['pid'] == int(old['MainPID']) == int(before_ui['supervisor_pid'])
    assert new_process['pid'] == int(new['MainPID']) == int(after_ui['supervisor_pid'])
    assert int(old_process['birth']) < int(new_process['birth'])
    assert int(before_ui['start']) >= int(old_process['birth'])
    assert int(after_ui['start']) >= int(new_process['birth'])
    assert before_ui['pid'] != after_ui['pid']
    assert before_ui['cgroup'] == after_ui['cgroup']
    assert new_process['cgroup'].strip() == '0::' + after_ui['cgroup']
    assert before_ui['exe_identity'] == after_ui['exe_identity'] == before['files']['desktop']['identity'][:2]
    post = recovered['post']
    assert post['old_ui_pid'] == before_ui['pid'] and post['old_ui_birth'] == before_ui['start']
    assert post['ui_pid'] == after_ui['pid'] and post['ui_birth'] == after_ui['start']
    assert post['supervisor_pid'] == new_process['pid'] and post['supervisor_birth'] == new_process['birth']
    assert post['desktop_sha256'] == before['files']['desktop']['sha256']
    assert post['exe_identity'] == after_ui['exe_identity']
    for key in ('ActiveState', 'MainPID', 'NRestarts', 'KillMode'):
        assert before['service'][key] == old[key] and after['service'][key] == new[key]
    expected = copy.deepcopy(before)
    expected['service']['MainPID'] = new['MainPID']
    expected['service']['NRestarts'] = new['NRestarts']
    assert after == expected
    return expected
