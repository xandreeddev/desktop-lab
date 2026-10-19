"""Read-only guest fingerprint for native acceptance helpers.

The host keeps the returned hashes/identities private. No payload is logged or
restored: concurrent changes fail preservation instead of being overwritten.
The caller supplies a session wrapper; this module performs no IO on import.
"""
SCRIPT = r'''
import hashlib,json,os,stat,subprocess
from pathlib import Path
home=Path.home();base=home/'.local/state/lucent'
def identity(m):return [m.st_dev,m.st_ino,m.st_mode,m.st_size,m.st_mtime_ns,m.st_uid]
def regular(path,limit,owners=None):
    if owners is None:owners={os.getuid()}
    descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(descriptor,'rb') as stream:
        before=os.fstat(stream.fileno())
        assert stat.S_ISREG(before.st_mode) and before.st_uid in owners and before.st_size<=limit
        digest=hashlib.sha256();count=0
        for block in iter(lambda:stream.read(1024*1024),b''):
            count+=len(block);assert count<=limit;digest.update(block)
        assert count==before.st_size and identity(before)==identity(os.fstat(stream.fileno()))==identity(path.lstat())
        return {'sha256':digest.hexdigest(),'identity':identity(before)}
def optional(path,limit):
    try:return regular(path,limit)
    except FileNotFoundError:return None
files={}
for name,limit in [('settings',4*1024*1024),('supervisor',1024*1024),('desktop',128*1024*1024),('legacy-weather',4096),('monitors',256*1024)]:
    path={'settings':base/'desktop.json','supervisor':home/'.local/lib/lucent/session.py','desktop':home/'.local/bin/lucent-desktop','legacy-weather':home/'.config/lucent/weather.json','monitors':home/'.config/hypr/monitors.lua'}[name]
    files[name]=optional(path,limit)
assert all(files[name] is not None for name in ('settings','supervisor','desktop'))
links={}
for path in [base/'wallpaper',home/'.local/state/omarchy/current/background']:
    try:meta=path.lstat()
    except FileNotFoundError:links[str(path)]=None;continue
    assert stat.S_ISLNK(meta.st_mode) and meta.st_uid==os.getuid()
    target=os.readlink(path);resolved=path.resolve(strict=True)
    links[str(path)]={'identity':identity(meta),'target_sha256':hashlib.sha256(target.encode()).hexdigest(),'file':regular(resolved,128*1024*1024,{0,os.getuid()})}
exports={}
for folder in [base/'theme',Path('/var/lib/lucent/wallpapers')/str(os.getuid())]:
    for path in folder.glob('*'):
        assert len(exports)<128
        meta=path.lstat()
        if stat.S_ISREG(meta.st_mode):exports[str(path)]=regular(path,128*1024*1024,{0,os.getuid()})
path=home/'.config/omarchy/themes/lucent-tokens-active/colors.toml';exports[str(path)]=optional(path,128*1024*1024)
media={};entries=0;budget=4*1024*1024*1024
for label,folder,depth in [('library',home/'Videos/Wallpapers',0),('owned',base/'live-wallpapers',2),('clipboard',base/'clipboard',0)]:
    try:meta=folder.lstat()
    except FileNotFoundError:media[label]=None;continue
    assert stat.S_ISDIR(meta.st_mode) and meta.st_uid==os.getuid() and folder.resolve(strict=True)==folder
    inventory={};pending=[(folder,depth)]
    while pending:
        parent,remaining=pending.pop()
        with os.scandir(parent) as children:
            for item in children:
                entries+=1;assert entries<=4096
                path=Path(item.path);value=path.lstat();assert value.st_uid==os.getuid()
                key=str(path.relative_to(folder))
                if stat.S_ISREG(value.st_mode):
                    budget-=value.st_size;assert budget>=0
                    inventory[key]=regular(path,512*1024*1024)
                elif stat.S_ISDIR(value.st_mode):
                    inventory[key]={'directory_identity':identity(value)}
                    if remaining:pending.append((path,remaining-1))
                elif stat.S_ISLNK(value.st_mode):inventory[key]={'identity':identity(value),'target_sha256':hashlib.sha256(os.readlink(path).encode()).hexdigest()}
                else:raise AssertionError('Unsupported preserved file type')
    media[label]={'root_identity':identity(meta),'inventory':inventory}
unit=subprocess.run(['systemctl','--user','show','lucent.service','-p','ActiveState','-p','MainPID','-p','NRestarts','-p','KillMode'],capture_output=True,text=True,timeout=5)
assert unit.returncode==0
service=dict(line.split('=',1) for line in unit.stdout.splitlines() if '=' in line)
assert service['ActiveState']=='active' and service['MainPID'].isdecimal() and service['MainPID']!='0' and service['KillMode']=='mixed'
print(json.dumps({'files':files,'links':links,'exports':exports,'media':media,'service':service}))
'''


def fingerprint(session):
    """Return bounded hash/identity evidence; no IO or mutation at module import."""
    import json
    return json.loads(session('python3', '-c', SCRIPT))
