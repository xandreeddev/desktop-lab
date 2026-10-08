#!/usr/bin/env python3
"""Measure service restart to responding shell IPC, not first rendered frame."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('profile', choices=('lucid', 'noctalia'))
p.add_argument('--output', required=True)
args = p.parse_args()
profile = (Path.home()/'.local/state/desktop-lab/profile').read_text().strip()
if profile != args.profile:
    p.error('The requested profile is not the installed profile.')
service = ['systemctl', '--user']
subprocess.run(service + ['stop', 'desktop-lab-shell.service'], check=True)
start = time.monotonic()
subprocess.run(service + ['start', 'desktop-lab-shell.service'], check=True)
command = ['noctalia', 'msg', 'status'] if profile == 'noctalia' else ['qs', 'ipc', 'call', '--', 'widgets', 'list']
while time.monotonic() - start < 60:
    try:
        result = subprocess.run(command, capture_output=True, timeout=2)
        if result.returncode == 0:
            if profile == 'lucid' and re.search(rb'^w\d+\s', result.stdout, re.M):
                break  # The configured Bookends widget host is responding.
            if profile == 'noctalia':
                try:
                    if isinstance(json.loads(result.stdout).get('barVisible'), bool):
                        break
                except (ValueError, AttributeError):
                    pass
    except subprocess.TimeoutExpired:
        pass
    time.sleep(0.1)
else:
    raise SystemExit('Shell IPC did not become ready within 60 seconds.')
report = {
    'profile': profile,
    'service_restart_to_ipc_seconds': round(time.monotonic()-start, 3),
    'scope': 'one warm-cache service restart; includes 100 ms polling; not login or first frame latency',
}
Path(args.output).write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report, indent=2))
