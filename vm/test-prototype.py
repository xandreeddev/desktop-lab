#!/usr/bin/env python3
"""Exercise Vulkan, animation, drag, persistence and click-through in the real VM."""
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


def send(events):
    result = lab.virsh('qemu-monitor-command', NAME, json.dumps({
        'execute': 'input-send-event', 'arguments': {'events': events}
    }), capture=True)
    if 'error' in json.loads(result.stdout):
        raise RuntimeError(result.stdout)


def move(x, y):
    send([{'type': 'abs', 'data': {'axis': axis, 'value': round(value / (size-1)*32767)}}
          for axis, value, size in [('x', x, width), ('y', y, height)]])


def button(name, down):
    send([{'type': 'btn', 'data': {'down': down, 'button': name}}])


def click(name, x, y):
    move(x, y)
    time.sleep(0.25)
    button(name, True)
    time.sleep(0.1)
    button(name, False)
    time.sleep(0.6)


def start():
    session('bash', '-c', 'nohup env XDG_STATE_HOME=/tmp/lucent-integration-state '
            '~/.local/bin/lucent-desktop >/tmp/lucent-prototype.log 2>&1 </dev/null &')
    deadline = time.monotonic() + 30
    while not frames():
        if time.monotonic() > deadline:
            raise RuntimeError('No first frame: ' + log())
        time.sleep(0.25)
    time.sleep(0.8)
    assert 'backend: Vulkan' in log(), log()
    assert 'lucent alpha: PreMultiplied' in log(), log()


assert lab.virsh('domstate', NAME, capture=True).stdout.strip() == 'running'
assert remote('pgrep', '-cx', 'Hyprland').strip() == '1'
assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the stock desktop first'
monitors = json.loads(session('hyprctl', '-j', 'monitors'))
assert len(monitors) == 1 and monitors[0]['scale'] == 1, 'Harness expects one scale-1 output'
assert all(m['dpmsStatus'] for m in monitors), 'Wake the guest display first'
width, height = monitors[0]['width'], monitors[0]['height']
assert remote('bash', '-c', 'pgrep -x lucent-desktop || true').strip() == '', 'Close the existing demo first'
remote('mkdir', '-p', '/tmp/lucent-integration-state/lucent')
remote('rm', '-f', '/tmp/lucent-integration-state/lucent/position', '/tmp/lucent-input.received')
move(50, 100)
start()
first = frames()
assert first > 2, 'Opening animation did not produce multiple frames'

# A real terminal mouse-reporting client underneath the transparent overlay.
probe = '''import os,sys,tty,termios,select,time
from pathlib import Path
fd=sys.stdin.fileno()
old=termios.tcgetattr(fd)
try:
 tty.setraw(fd)
 os.write(1,b'\\x1b[?1000h\\x1b[?1006hClick-through test')
 deadline=time.monotonic()+30
 while time.monotonic()<deadline:
  if select.select([fd],[],[],0.2)[0]:
   data=os.read(fd,1024)
   if b'\\x1b[<' in data:
    Path('/tmp/lucent-input.received').write_bytes(data)
    break
finally:
 os.write(1,b'\\x1b[?1000l\\x1b[?1006l')
 termios.tcsetattr(fd,termios.TCSANOW,old)
'''
session('bash', '-c', 'nohup ' + shlex.join(['foot', '--app-id=lucent-input-test', 'python3', '-c', probe])
        + ' >/tmp/lucent-input.log 2>&1 </dev/null &')
time.sleep(1)
clients = json.loads(session('hyprctl', '-j', 'clients'))
probe_window = next(c for c in clients if c['class'] == 'lucent-input-test')
click('left', probe_window['at'][0] + 45, probe_window['at'][1] + 65)
assert remote('cat', '/tmp/lucent-input.received').startswith('\x1b[<'), 'Click did not reach the underlying app'
assert 'lucent click' not in log(), 'Click outside the card reached Lucent'

cx, cy = width/2, height/2
click('left', cx, cy)
assert 'lucent click 1' in log(), log()
clicked = frames()
assert clicked > first + 2, 'Color transition did not animate'
# Keep the button held while moving. The implicit Wayland grab must preserve
# coordinates even as the rounded input mask moves under the pointer.
move(cx, cy)
button('left', True)
for step in range(1, 13):
    move(cx + step*20, cy - step*10)
    time.sleep(0.025)
button('left', False)
time.sleep(0.7)
position = remote('cat', '/tmp/lucent-integration-state/lucent/position').split()
x, y = map(float, position[1:])
expected = ((width-540)/2+240, (height-220)/2-120)
assert abs(x-expected[0]) < 3 and abs(y-expected[1]) < 3, (position, expected)
assert re.findall(r'^lucent click (\d+)$', log(), re.M) == ['1'], 'Dragging incorrectly counted as clicking'
assert log().count('lucent text upload') == 2, 'Animation/drag rerasterized unchanged text'
move(50, 100)
time.sleep(0.6)
settled = frames()
session('grim', '/tmp/lucent-prototype.png')
with (REPORT/'lucent.png').open('wb') as out:
    subprocess.run(lab.ssh_args(PROFILE) + ['cat /tmp/lucent-prototype.png'], stdout=out, check=True)
pid = int(remote('pgrep', '-x', 'lucent-desktop').strip())
print(f'Vulkan, transparency, click-through, color animation and drag passed. Measuring PID {pid} idle for 120 seconds.', flush=True)
idle = json.loads(session('python3', 'desktop-lab/scripts/benchmark.py', '--pid', str(pid),
                         '--seconds', '120', '--output', '/tmp/lucent-idle.json'))
assert frames() == settled, 'Prototype continued repainting while idle'
click('right', x+270, y+110)
assert re.search(r'closed cleanly; \d+ frame\(s\), 1 click\(s\)', log()), log()
assert remote('bash', '-c', 'pgrep -x lucent-desktop || true').strip() == ''
first_log = log()
move(50, 100)
start()
configured = re.search(r'lucent configured \d+ \d+ position ([\d.]+) ([\d.]+)', log())
assert configured and abs(float(configured[1])-x) < 0.1 and abs(float(configured[2])-y) < 0.1
click('right', x+270, y+110)
assert 'closed cleanly' in log(), log()
report = {'vulkan': True, 'premultiplied_alpha': True, 'click_through_to_terminal': True,
          'opening_animation': True, 'color_animation': True, 'drag': True,
          'drag_position': [x, y], 'position_restored_after_restart': True,
          'clean_animated_right_click_exit': True, 'frames_before_input': first,
          'frames_after_click': clicked, 'frames_after_drag_and_settle': settled,
          'text_uploads': 2, 'extra_frames_during_120_second_idle': 0, 'idle': idle}
(REPORT/'lucent-integration.json').write_text(json.dumps(report, indent=2)+'\n')
(REPORT/'lucent-runtime.log').write_text(first_log)
print(json.dumps(report, indent=2))
