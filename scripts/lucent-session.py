#!/usr/bin/env python3
"""Own only the stock bar visibility while Lucent is healthy; retain lock/session services."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time

HOME = Path.home()
FLAG = HOME / '.local/state/omarchy/toggles/bar-off'
RUNTIME = Path(os.environ['XDG_RUNTIME_DIR'])
RECORD = RUNTIME / 'lucent-stock-bar.json'


def sync_bar():
    subprocess.run(['omarchy-shell', 'omarchy.bar', 'syncHidden'], timeout=5, check=False,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def restore():
    if RECORD.exists():
        previous = json.loads(RECORD.read_text())
        if previous['hidden']:
            FLAG.parent.mkdir(parents=True, exist_ok=True)
            FLAG.touch()
        else:
            FLAG.unlink(missing_ok=True)
        sync_bar()
        RECORD.unlink(missing_ok=True)


def run():
    restore()  # Recover from a previously interrupted session.
    child = subprocess.Popen([str(HOME / '.local/bin/lucent-desktop')])
    def stop(_signum, _frame):
        child.terminate()
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        deadline = time.monotonic() + 30
        while child.poll() is None:
            try:
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(1)
                    client.connect(str(RUNTIME / 'lucent.sock'))
                    client.sendall(b'inspect\n')
                    data = b''
                    while chunk := client.recv(65536):
                        data += chunk
                    status = json.loads(data)
                    if len(status['surfaces']) == 3 and all(s['frames'] > 0 for s in status['surfaces']):
                        break
            except (OSError, ValueError):
                pass
            if time.monotonic() > deadline:
                raise RuntimeError('Lucent did not become ready; stock bar retained')
            time.sleep(0.1)
        if child.poll() is not None:
            return child.returncode
        RECORD.write_text(json.dumps({'hidden': FLAG.exists()}))
        FLAG.parent.mkdir(parents=True, exist_ok=True)
        FLAG.touch()
        sync_bar()
        return child.wait()
    finally:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        restore()


if __name__ == '__main__':
    if sys.argv[1:] == ['restore']:
        restore()
    else:
        raise SystemExit(run())
