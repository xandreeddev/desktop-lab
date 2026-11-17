"""Owned Wayland test app, staged by test-launcher-application.py.

The controller copies this fixture beside owned_application_units.py in its
private guest stage. Execute enters through a genuine XDG application entry and
the production adapter; foot and its PTY receiver have finite alarm lifetimes.
The receiver accepts only literal test input and records counts, never payloads.
"""
import argparse,fcntl,hashlib,json,os,re,signal,stat,sys,time
from pathlib import Path
sys.dont_write_bytecode=True
sys.path.insert(0,str(Path(__file__).resolve(strict=True).parent))
from owned_application_units import capture_unit_origin,assert_unit_origin,unit_snapshot,retire_original_unit,_retirement_drained


def read(path,limit=131072):
    with os.fdopen(os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK),'rb') as stream:
        value=os.fstat(stream.fileno());assert stat.S_ISREG(value.st_mode) and value.st_uid==os.getuid() and value.st_size<=limit
        data=stream.read(limit+1);assert len(data)<=limit
        return data
def write(path,value):
    data=json.dumps(value).encode()+b'\n'
    temp=path.with_name('.next-'+path.name)
    with os.fdopen(os.open(temp,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600),'wb') as stream:stream.write(data);stream.flush();os.fsync(stream.fileno())
    os.replace(temp,path)
def context(folder,tag):
    folder=Path(folder);assert folder.resolve(strict=True)==folder and folder.parent==Path.home()/'.cache'
    assert folder.name=='lucent-launcher-application-'+tag and re.fullmatch('[a-f0-9]{24}',tag)
    value=folder.stat();assert value.st_uid==os.getuid() and stat.S_IMODE(value.st_mode)==0o700
    path=folder/'manifest.json'
    if not path.exists():return folder,None
    m=json.loads(read(path));assert m['tag']==tag and m['script_sha256']==hashlib.sha256(read(folder/'fixture.py')).hexdigest()
    assert m['units_sha256']==hashlib.sha256(read(folder/'owned_application_units.py')).hexdigest()
    return folder,m
def execute_argv(folder,tag):return ['/usr/bin/python3',str(folder/'fixture.py'),'execute',str(folder),tag]
def initialize(folder,tag):
    folder,m=context(folder,tag);assert m is None
    applications=Path(os.environ.get('XDG_DATA_HOME',str(Path.home()/'.local/share')))/'applications'
    assert applications.resolve(strict=True)==applications and applications.stat().st_uid==os.getuid()
    app_id='org.lucent.Launcher'+tag+'.desktop';app_class='lucent-launcher-'+tag
    suffix=''.join(chr(ord('a')+int(character,16)) for character in tag)
    name='Lucent launcher fixture '+suffix
    entry=applications/app_id;assert not os.path.lexists(entry)
    argv=execute_argv(folder,tag)
    assert all(re.fullmatch('[A-Za-z0-9_./-]+',item) for item in argv)
    data=('[Desktop Entry]\nType=Application\nName='+name+'\nIcon=application-x-executable\nTerminal=false\nNoDisplay=false\nStartupWMClass='+app_class+'\nExec='+' '.join(argv)+'\n').encode()
    with os.fdopen(os.open(entry,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600),'wb') as stream:stream.write(data);stream.flush();os.fsync(stream.fileno())
    s=entry.stat()
    m={'tag':tag,'app_id':app_id,'class':app_class,'name':name,'entry':str(entry),'entry_sha256':hashlib.sha256(data).hexdigest(),'entry_identity':[s.st_dev,s.st_ino,s.st_mode,s.st_size,s.st_mtime_ns],'script_sha256':hashlib.sha256(read(folder/'fixture.py')).hexdigest(),'units_sha256':hashlib.sha256(read(folder/'owned_application_units.py')).hexdigest()}
    write(folder/'manifest.json',m);write(folder/'events.json',[])
    with os.fdopen(os.open(folder/'events.lock',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'wb'):pass
    print(json.dumps(m))
def execute(folder,tag):
    folder,m=context(folder,tag);assert m and not (folder/'closing').exists()
    assert hashlib.sha256(read(Path(m['entry']))).hexdigest()==m['entry_sha256']
    group=Path('/proc/self/cgroup').read_text().strip();assert group.startswith('0::/') and '/app.slice/' in group
    unit=group.rsplit('/',1)[1];assert re.fullmatch('run-[A-Za-z0-9_-]+.service',unit)
    with os.fdopen(os.open(folder/'events.lock',os.O_RDWR|os.O_NOFOLLOW),'r+b') as stream:
        fcntl.flock(stream,fcntl.LOCK_EX)
        events=json.loads(read(folder/'events.json'));assert len(events)<2 and not(folder/'closing').exists()
        index=len(events);origin=capture_unit_origin(unit,execute_argv(folder,tag))
        events.append({'unit':unit,'origin':origin,'token_present':bool(os.environ.get('XDG_ACTIVATION_TOKEN'))})
        write(folder/'events.json',events);write(folder/('key-'+str(index)+'.json'),{'lines':0})
    signal.signal(signal.SIGALRM,signal.SIG_DFL);signal.alarm(180)
    os.execv('/usr/bin/foot',['/usr/bin/foot','--app-id='+m['class'],'--title=Lucent launcher fixture','/usr/bin/python3',str(folder/'fixture.py'),'receive',str(folder),tag,str(index)])
def receive(folder,tag,index):
    folder,m=context(folder,tag);assert m and index in ('0','1') and os.isatty(0)
    signal.signal(signal.SIGALRM,signal.SIG_DFL);signal.alarm(150)
    count=0
    while True:
        line=sys.stdin.readline(64)
        if not line:return
        assert line=='a\n'
        count+=1;assert count<=4;write(folder/('key-'+index+'.json'),{'lines':count})
def state(folder,tag):
    folder,m=context(folder,tag);assert m
    events=json.loads(read(folder/'events.json'));rows=[]
    for index,event in enumerate(events):
        drained=_retirement_drained(event['origin'])
        current=None if drained else assert_unit_origin(unit_snapshot(event['unit']),event['unit'],execute_argv(folder,tag),event['origin'],hashed=True)
        if current is not None:
            assert Path('/proc',str(current['pid']),'exe').resolve()==Path('/usr/bin/foot')
        rows.append({'pid':event['origin']['process']['pid'],'birth':event['origin']['process']['start'],'drained':drained,'lines':json.loads(read(folder/('key-'+str(index)+'.json')))['lines'],'token_present':event['token_present']})
    print(json.dumps({'app_id':m['app_id'],'class':m['class'],'events':rows}))
def retire(folder,tag):
    folder,m=context(folder,tag);assert m
    events=json.loads(read(folder/'events.json'))
    for event in events:
        if not _retirement_drained(event['origin']):retire_original_unit(event['unit'],execute_argv(folder,tag),event['origin'])
        assert _retirement_drained(event['origin'])
    print(json.dumps({'all_original_application_units_drained':True}))
def cleanup(folder,tag):
    folder,m=context(folder,tag);assert m
    events=json.loads(read(folder/'events.json'));assert all(_retirement_drained(e['origin']) for e in events)
    entry=Path(m['entry']);s=entry.stat();assert [s.st_dev,s.st_ino,s.st_mode,s.st_size,s.st_mtime_ns]==m['entry_identity']
    assert hashlib.sha256(read(entry)).hexdigest()==m['entry_sha256']
    allowed={'fixture.py','owned_application_units.py','manifest.json','events.json','events.lock'}|{'key-'+str(i)+'.json' for i in range(len(events))}
    assert {p.name for p in folder.iterdir()}==allowed
    entry.unlink()
    for p in folder.iterdir():p.unlink()
    folder.rmdir();print(json.dumps({'owned_entry_and_stage_removed':True}))
if __name__=='__main__':
    op,folder,tag,*extra=sys.argv[1:]
    if op=='initialize':initialize(folder,tag)
    elif op=='execute':execute(folder,tag)
    elif op=='receive':receive(folder,tag,*extra)
    elif op=='state':state(folder,tag)
    elif op=='retire':retire(folder,tag)
    elif op=='cleanup':cleanup(folder,tag)
    else:raise ValueError('owned_operation_required')
