#!/usr/bin/env python3
"""Supervise Lucent only. Explicit rollback, rather than a competing crash UI."""
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import sys
import time
from lucent_readiness import desktop_rendered

HOME=Path.home()
RUNTIME=Path(os.environ['XDG_RUNTIME_DIR'])
HELPERS=('lucent-polkit.service','lucent-idle.service','lucent-clipboard.service')
STOP_TIMEOUT=5


def inspect():
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(1)
        client.connect(str(RUNTIME/'lucent.sock'))
        client.sendall(b'inspect\n')
        data=b''
        while chunk:=client.recv(65536): data+=chunk
        return json.loads(data)


def ready(child, stopped=lambda:False):
    deadline=time.monotonic()+30
    while not stopped() and child.poll() is None and time.monotonic()<deadline:
        try:
            state=inspect()
            if desktop_rendered(state):
                return True
        except (OSError,ValueError): pass
        time.sleep(.1)
    if stopped(): return False
    raise RuntimeError('Lucent did not render its desktop surfaces')


def run():
    child=None
    pidfd=None
    previous={}
    requested=False
    cleaning=False
    forced=False
    wake_read,wake_write=os.pipe2(os.O_NONBLOCK|os.O_CLOEXEC)
    def stop(*_):
        nonlocal requested
        # Observe an already-ended child before recording intent. A later stop
        # must not turn a spontaneous failure into a successful service stop.
        if requested or cleaning or child.poll() is not None: return
        requested=True
        child.terminate()
        try: os.write(wake_write,b'\0')
        except BlockingIOError: pass
    try:
        child=subprocess.Popen([str(HOME/'.local/bin/lucent-desktop')])
        pidfd=os.pidfd_open(child.pid)
        for signum in (signal.SIGTERM,signal.SIGINT):
            previous[signum]=signal.signal(signum,stop)
        if ready(child,lambda:requested):
            if not requested and (HOME/'.local/state/lucent/bar-hidden').exists():
                subprocess.run([str(HOME/'.local/bin/lucent-cli'),'bar','hide'],check=True,stdout=subprocess.DEVNULL,timeout=10)
            if not requested and (HOME/'.local/state/lucent/integration-backup/native-shell-enabled').exists():
                subprocess.run(['systemctl','--user','start',*HELPERS],check=True,timeout=10)
        # A pidfd blocks without idle polling or Popen.wait's waitpid lock. The
        # signal handler can therefore poll the real child status, and the wake
        # pipe starts bounded cleanup even when the child ignores SIGTERM.
        while not requested and child.poll() is None:
            select.select([pidfd,wake_read],[],[])
    finally:
        cleaning=True
        try:
            if child is not None and child.poll() is None:
                child.terminate()
                try: child.wait(timeout=STOP_TIMEOUT)
                except subprocess.TimeoutExpired:
                    if child.poll() is None:
                        forced=True
                        child.kill()
                    child.wait()
        finally:
            for signum,handler in previous.items(): signal.signal(signum,handler)
            if pidfd is not None: os.close(pidfd)
            os.close(wake_read)
            os.close(wake_write)
    status=child.returncode
    if requested and (status==-signal.SIGTERM or forced and status==-signal.SIGKILL): return 0
    return status

if __name__=='__main__':
    # Old service units may still call restore during their one-time upgrade.
    if sys.argv[1:]!=['restore']: raise SystemExit(run())
