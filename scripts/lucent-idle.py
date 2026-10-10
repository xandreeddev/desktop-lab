#!/usr/bin/env python3
"""Headless Wayland idle adapter. swayidle owns compositor/logind protocols."""
import json
import os
from pathlib import Path
import signal
import subprocess
import time

HOME=Path.home()
STAY_AWAKE=HOME/'.local/state/omarchy/indicators/stay-awake'


def command(enabled):
    path=HOME/'.config/lucent/session.json'
    config=json.loads(path.read_text()) if path.exists() else {}
    seconds=config.get('lock_after_seconds',300)
    if not isinstance(seconds,int) or not 30<=seconds<=86400: raise ValueError('lock_after_seconds must be 30…86400')
    helper='python3 "'+str(HOME/'.local/lib/lucent/lock.py')+'"'
    # -w keeps the logind delay inhibitor until the secure lock is acknowledged.
    args=['swayidle','-w','before-sleep',helper+' sleep','lock',helper]
    if enabled: args+=['timeout',str(seconds),helper]
    return args


def main():
    stopped=False
    def stop(*_):
        nonlocal stopped
        stopped=True
    signal.signal(signal.SIGTERM,stop)
    signal.signal(signal.SIGINT,stop)
    child=None
    previous=None
    try:
        while not stopped:
            enabled=not STAY_AWAKE.exists()
            if previous!=enabled:
                if child: child.terminate();child.wait(timeout=5)
                child=subprocess.Popen(command(enabled))
                previous=enabled
            if child.poll() is not None: raise RuntimeError('swayidle exited unexpectedly')
            time.sleep(1)
    finally:
        if child and child.poll() is None: child.terminate();child.wait(timeout=5)

if __name__=='__main__': main()
