#!/usr/bin/env python3
"""Handoff stock bar/notifications only while Lucent is healthy, with rollback."""
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


def plugin(enabled):
    result = subprocess.run(['omarchy-shell', 'shell', 'setPluginEnabled',
                             'omarchy.notifications', str(enabled).lower()],
                            text=True, capture_output=True, timeout=5, check=True)
    if result.stdout.strip() != 'ok':
        raise RuntimeError('Installed Omarchy cannot hand off notifications: ' + result.stdout.strip())
    subprocess.run(['omarchy-shell', 'shell', 'rescanPlugins'], check=True, timeout=10, stdout=subprocess.DEVNULL)


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
        if 'notifications_enabled' in previous:
            plugin(previous['notifications_enabled'])
        RECORD.unlink(missing_ok=True)


def inspect():
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(1)
        client.connect(str(RUNTIME / 'lucent.sock'))
        client.sendall(b'inspect\n')
        data = b''
        while chunk := client.recv(65536):
            data += chunk
        return json.loads(data)


def ready(child, predicate, timeout):
    deadline = time.monotonic() + timeout
    while child.poll() is None and time.monotonic() < deadline:
        try:
            if predicate(inspect()):
                return
        except (OSError, ValueError):
            pass
        time.sleep(0.1)
    raise RuntimeError('Lucent did not become ready; restoring stock services')


def run():
    restore()
    child = subprocess.Popen([str(HOME / '.local/bin/lucent-desktop')])
    def stop(_signum, _frame):
        child.terminate()
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        ready(child, lambda s: {'bar', 'dock', 'widgets'} <=
              {surface['id'] for surface in s['surfaces'] if surface['frames'] > 0}, 30)
        config = json.loads((HOME / '.config/omarchy/shell.json').read_text())
        previous = {'hidden': FLAG.exists(),
                    'notifications_enabled': 'omarchy.notifications' not in config.get('disabledPlugins', [])}
        RECORD.write_text(json.dumps(previous))
        plugin(False)
        # Quickshell's notification server is a process-lifetime singleton.
        # Unloading its QML plugin does not release the bus name. The upstream
        # restart command refuses to interrupt a secure stock lock.
        if not inspect()['client']['notifications_ready']:
            locked = subprocess.run(['omarchy-hyprland-session-locked'], check=False)
            if locked.returncode == 0:
                raise RuntimeError('Unlock before handing notification ownership to Lucent')
            subprocess.run(['omarchy-restart-shell'], check=True, timeout=30,
                           stdout=subprocess.DEVNULL)
        ready(child, lambda s: s['client']['notifications_ready'], 15)
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
