#!/usr/bin/env python3
"""Exercise the installed prototype through real Wayland input in its lab VM."""
import json
from pathlib import Path
import re
import shlex
import subprocess
import time

import lab

PROFILE = 'lucent'
NAME = lab.inventory()['vms'][PROFILE]['name']
REPORT = lab.ROOT / 'reports/local'
REPORT.mkdir(parents=True, exist_ok=True)


def remote(*command):
    return subprocess.check_output(lab.ssh_args(PROFILE) + [shlex.join(command)], text=True)


def session(*command):
    return remote('python3', 'desktop-lab/scripts/in-session.py', *command)


def log():
    return remote('cat', '/tmp/lucent-prototype.log')


def frames():
    return len(re.findall(r'^lucent frame \d+$', log(), re.M))


def click(button):
    events = [
        {'type': 'abs', 'data': {'axis': 'x', 'value': 16384}},
        {'type': 'abs', 'data': {'axis': 'y', 'value': 16384}},
        {'type': 'btn', 'data': {'down': True, 'button': button}},
    ]
    for batch in (events, [{'type': 'btn', 'data': {'down': False, 'button': button}}]):
        result = lab.virsh('qemu-monitor-command', NAME, json.dumps({
            'execute': 'input-send-event', 'arguments': {'events': batch}
        }), capture=True)
        if 'error' in json.loads(result.stdout):
            raise RuntimeError(result.stdout)
        time.sleep(0.1)


assert lab.virsh('domstate', NAME, capture=True).stdout.strip() == 'running'
assert remote('pgrep', '-cx', 'Hyprland').strip() == '1'
assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the stock desktop first'
monitors = json.loads(session('hyprctl', '-j', 'monitors'))
assert all(m['dpmsStatus'] for m in monitors), 'Wake the guest display first'
session('bash', '-c', 'if pgrep -x lucent-desktop >/dev/null; then exit 1; fi; '
        'nohup ~/.local/bin/lucent-desktop >/tmp/lucent-prototype.log 2>&1 </dev/null &')
deadline = time.monotonic() + 30
while not frames():
    if time.monotonic() > deadline:
        raise RuntimeError('No first frame: ' + log())
    time.sleep(0.25)
first = frames()
click('left')
time.sleep(1)
clicked = frames()
assert clicked > first, 'Pointer input did not trigger a redraw'
session('grim', '/tmp/lucent-prototype.png')
with (REPORT/'lucent.png').open('wb') as out:
    subprocess.run(lab.ssh_args(PROFILE) + ['cat /tmp/lucent-prototype.png'], stdout=out, check=True)
pid = int(remote('pgrep', '-x', 'lucent-desktop').strip())
print(f'Layer surface and real pointer input passed; measuring PID {pid} idle for 120 seconds.', flush=True)
idle = json.loads(session('python3', 'desktop-lab/scripts/benchmark.py', '--pid', str(pid),
                         '--seconds', '120', '--output', '/tmp/lucent-idle.json'))
assert frames() == clicked, 'Prototype continued repainting while idle'
click('right')
time.sleep(1)
assert re.search(r'closed cleanly; \d+ frame\(s\), 1 click\(s\)', log()), log()
assert remote('bash', '-c', 'pgrep -x lucent-desktop || true').strip() == ''
report = {'first_frame': True, 'left_click': True, 'clean_right_click_exit': True,
          'frames_before_input': first, 'frames_after_input': clicked,
          'extra_frames_during_120_second_idle': 0, 'idle': idle}
(REPORT/'lucent-integration.json').write_text(json.dumps(report, indent=2)+'\n')
(REPORT/'lucent-runtime.log').write_text(log())
print(json.dumps(report, indent=2))
