"""Private bounded clipboard history. No rendering or command execution."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import tempfile
import time

ROOT = Path(os.environ.get('XDG_STATE_HOME',str(Path.home()/'.local/state')))/'lucent/clipboard'
LIMIT = 100
BYTES = 2*1024*1024


def read():
    try: return json.loads((ROOT/'history.json').read_text())
    except FileNotFoundError: return []


def write(items):
    with tempfile.NamedTemporaryFile(mode='w',dir=ROOT,delete=False) as stream:
        temporary=Path(stream.name)
        json.dump(items,stream)
        stream.flush(); os.fsync(stream.fileno())
    temporary.replace(ROOT/'history.json')


def prune(items):
    live = {Path(item['path']).name for item in items if item['type']=='image'}
    for path in ROOT.glob('*.png'):
        if path.name not in live: path.unlink()


def capture(mime, data, sensitive=False):
    if sensitive or not data or len(data)>BYTES: return
    if mime=='text':
        try: text=data.decode('utf-8')
        except UnicodeDecodeError: return
        item={'type':'text','text':text}
    elif mime=='image/png':
        if not data.startswith(b'\x89PNG\r\n\x1a\n'): return
        item={'type':'image'}
    else: raise ValueError('Unsupported clipboard format')
    ROOT.mkdir(parents=True,exist_ok=True,mode=0o700)
    ROOT.chmod(0o700)
    with (ROOT/'history.lock').open('w') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        identifier=hashlib.sha256(mime.encode()+b'\0'+data).hexdigest()
        if item['type']=='image':
            path=ROOT/(identifier+'.png')
            with path.open('wb') as image: image.write(data)
            path.chmod(0o600)
            item['path']=str(path)
        item.update(id=identifier,capturedAt=int(time.time()*1000))
        items=[item]+[old for old in read() if old['id']!=identifier]
        # Bound total serialized text as well as individual entries.
        items=items[:LIMIT]
        while len(json.dumps(items).encode())>BYTES and len(items)>1: items.pop()
        write(items); prune(items)


def clear():
    ROOT.mkdir(parents=True,exist_ok=True,mode=0o700)
    with (ROOT/'history.lock').open('w') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        write([]); prune([])
