#!/usr/bin/env python3
"""One-time migration: stop this user's installed Omarchy shell and supervisor."""
import os
from pathlib import Path
import signal
import subprocess
import time


def role(argv, root):
    if len(argv)>=2 and Path(argv[0]).name in ('bash','sh') and Path(argv[1]).name=='omarchy-launch-shell':
        return 'supervisor'
    if argv and Path(argv[0]).name in ('quickshell','qs'):
        for i,argument in enumerate(argv[:-1]):
            if argument in ('-p','--path') and argv[i+1]==str(root/'shell'): return 'shell'
    return None


def processes():
    root=Path(os.environ.get('OMARCHY_PATH','/usr/share/omarchy'))
    result=[]
    for path in Path('/proc').iterdir():
        if not path.name.isdecimal(): continue
        try:
            if path.stat().st_uid!=os.getuid(): continue
            argv=(path/'cmdline').read_bytes().decode().rstrip('\0').split('\0')
            kind=role(argv,root)
            if kind: result.append((kind,int(path.name)))
        except (OSError,UnicodeError): pass
    return result


def stop():
    if subprocess.run(['omarchy-hyprland-session-locked'],stdout=subprocess.DEVNULL,check=False).returncode==0:
        raise RuntimeError('Unlock before replacing the running shell')
    for kind in ('supervisor','shell'):
        for found,pid in processes():
            if found!=kind: continue
            try:
                descriptor=os.pidfd_open(pid)
                try: signal.pidfd_send_signal(descriptor,signal.SIGTERM)
                finally: os.close(descriptor)
            except ProcessLookupError: pass
        time.sleep(.2)
    for _ in range(30):
        if not processes(): return
        time.sleep(.1)
    raise RuntimeError('Stock shell has not exited; migration stopped')

if __name__=='__main__': stop()
