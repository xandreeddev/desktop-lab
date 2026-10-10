#!/usr/bin/env python3
"""Capture Wayland clipboard offers; honor password-manager sensitivity hints."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import clipboard_store


def capture(mime):
    sensitive=os.environ.get('CLIPBOARD_STATE')=='sensitive'
    types=subprocess.run(['wl-paste','--list-types'],capture_output=True,timeout=2,check=False).stdout
    sensitive=sensitive or b'x-kde-passwordManagerHint' in types
    if not sensitive:
        clipboard_store.capture(mime,sys.stdin.buffer.read(clipboard_store.BYTES+1))


def watch():
    script=str(Path(__file__).resolve())
    children=[subprocess.Popen(['wl-paste','--type',mime,'--watch','python3',script,'capture',mime]) for mime in ('text','image/png')]
    def stop(*_):
        for child in children: child.terminate()
    signal.signal(signal.SIGTERM,stop)
    try:
        # The two watchers run indefinitely. If one fails, restart both via systemd.
        while all(child.poll() is None for child in children):
            import time
            time.sleep(.5)
        return 1
    finally:
        stop()
        for child in children: child.wait(timeout=3)

if __name__=='__main__':
    if sys.argv[1:2]==['capture']:capture(sys.argv[2])
    else:raise SystemExit(watch())
