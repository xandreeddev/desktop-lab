#!/usr/bin/env python3
"""Supervise Lucent only. Explicit rollback, rather than a competing crash UI."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time

HOME=Path.home()
RUNTIME=Path(os.environ['XDG_RUNTIME_DIR'])
HELPERS=('lucent-polkit.service','lucent-idle.service','lucent-clipboard.service')


def inspect():
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(1)
        client.connect(str(RUNTIME/'lucent.sock'))
        client.sendall(b'inspect\n')
        data=b''
        while chunk:=client.recv(65536): data+=chunk
        return json.loads(data)


def ready(child):
    deadline=time.monotonic()+30
    while child.poll() is None and time.monotonic()<deadline:
        try:
            state=inspect()
            if {'background','bar','dock','widgets'} <= {s['id'] for s in state['surfaces'] if s['frames']>0}:
                return
        except (OSError,ValueError): pass
        time.sleep(.1)
    raise RuntimeError('Lucent did not render its desktop surfaces')


def run():
    child=subprocess.Popen([str(HOME/'.local/bin/lucent-desktop')])
    def stop(*_): child.terminate()
    signal.signal(signal.SIGTERM,stop)
    signal.signal(signal.SIGINT,stop)
    try:
        ready(child)
        if (HOME/'.local/state/lucent/bar-hidden').exists():
            subprocess.run([str(HOME/'.local/bin/lucent-cli'),'bar','hide'],check=True,stdout=subprocess.DEVNULL)
        if (HOME/'.local/state/lucent/integration-backup/native-shell-enabled').exists():
            subprocess.run(['systemctl','--user','start',*HELPERS],check=True)
        return child.wait()
    finally:
        if child.poll() is None:
            child.terminate()
            try: child.wait(timeout=5)
            except subprocess.TimeoutExpired: child.kill();child.wait()

if __name__=='__main__':
    # Old service units may still call restore during their one-time upgrade.
    if sys.argv[1:]!=['restore']: raise SystemExit(run())
