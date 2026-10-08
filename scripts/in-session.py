#!/usr/bin/env python3
"""Run a command with the active desktop user's systemd session environment."""
import json
import os
import subprocess
import sys

data = json.loads(subprocess.check_output(['systemctl', '--user', 'show-environment', '--output=json']))
if not data.get('WAYLAND_DISPLAY'):
    raise SystemExit('Log into the graphical desktop first.')
os.environ.update(data)
os.execvp(sys.argv[1], sys.argv[1:])
