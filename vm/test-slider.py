#!/usr/bin/env python3
"""Run the independent slider example with real guest pointer and keyboard input.

Requires an unlocked, single-monitor, scale-1 Lucent guest. Uploads only the
provided hello-layer binary to private temporary storage; never replaces the
shell, login client, settings or authentication. The owned test unit and binary
are removed in finally. No screenshots or credentials are published.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import runpy
import shlex
import subprocess
import time
import uuid

spec = importlib.util.spec_from_file_location('framework_test', Path(__file__).with_name('test-framework.py'))
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def inspect():
    return json.loads(v.session('python3', '-c', '''import os,socket
s=socket.socket(socket.AF_UNIX); s.settimeout(5)
s.connect(os.path.join(os.environ['XDG_RUNTIME_DIR'],'hello-layer.sock'))
s.sendall(b'inspect\\n'); chunks=[]
while True:
 b=s.recv(65536)
 if not b: break
 chunks.append(b)
print(b''.join(chunks).decode())'''))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    args = p.parse_args()
    binary = args.binary.resolve(strict=True)
    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    monitors = json.loads(v.session('hyprctl', 'monitors', '-j'))
    assert len(monitors) == 1 and monitors[0]['scale'] == 1
    monitor = monitors[0]
    assert monitor['x'] == monitor['y'] == 0
    assert v.remote('sh', '-c', 'pgrep -x hello-layer || true').strip() == '', 'Another example is running'
    unit = 'lucent-slider-test-' + uuid.uuid4().hex
    directory = v.remote('python3', '-c', "from pathlib import Path; import tempfile; p=Path.home()/'.cache';p.mkdir(exist_ok=True); print(tempfile.mkdtemp(prefix='lucent-slider-',dir=p))").strip()
    uploaded = directory + '/hello-layer'
    checks = []
    try:
        with binary.open('rb') as stream:
            subprocess.run(v.lab.ssh_args(v.PROFILE) + ['cat > ' + shlex.quote(uploaded)], stdin=stream, check=True)
        v.remote('chmod', '700', uploaded)
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        assert v.remote('sha256sum', uploaded).split()[0] == digest
        v.session('systemd-run', '--user', '--collect', '--unit', unit,
                  '--setenv=LP_NUM_THREADS=2', uploaded)
        v.eventually(lambda: len(inspect()['surfaces']) == 1, 'Example did not start', timeout=25)
        layers = json.loads(v.session('hyprctl', 'layers', '-j'))
        matching = [layer for output in layers.values() for group in output['levels'].values()
                    for layer in group if layer['namespace'] == 'hello-layer-example']
        assert len(matching) == 1
        layer = matching[0]

        def move(x, y):
            v.send([{'type': 'abs', 'data': {'axis': axis, 'value': round(pos / (extent-1)*32767)}}
                    for axis, pos, extent in [('x', x, monitor['width']), ('y', y, monitor['height'])]])

        def hit(name):
            return next(h for s in inspect()['surfaces'] for h in s['hits'] if h['id'] == name)

        def point(h):
            return layer['x'] + h['x'] + h['width']/2, layer['y'] + h['y'] + h['height']/2

        def click(name):
            move(*point(hit(name))); v.button(True); v.button(False)

        def level(value):
            v.eventually(lambda: abs(inspect()['client']['level'] - value) < .001,
                         f'Expected slider level {value}')

        assert 'Vulkan' in inspect()['adapter']
        slider = hit('demo-level')
        move(*point(slider)); v.button(True)
        level(.5)  # The press itself emits; no motion/release required.
        move(layer['x'] + slider['x'] + slider['width'] + 60, point(slider)[1])
        level(1.)
        v.button(False)
        move(*point(slider)); v.button(True)
        move(layer['x'] + slider['x'] - 60, point(slider)[1])
        level(0.)
        v.button(False)
        checks += ['pointer_press_updates', 'captured_drag_outside_both_ends', 'outside_release']
        v.key(106); level(.05)
        for _ in range(3): v.key(106)
        level(.2)
        v.key(107); level(1.)
        v.key(102); level(0.)
        state = inspect()['surfaces'][0]
        assert state['focus'] == 'demo-level' and state['focus_visible']
        v.key(15)
        v.eventually(lambda: inspect()['surfaces'][0]['focus'] == 'close', 'Tab missed Close')
        v.key(42, 15)
        v.eventually(lambda: inspect()['surfaces'][0]['focus'] == 'demo-level', 'BackTab missed slider')
        v.key(106); level(.05)
        checks += ['keyboard_steps', 'keyboard_home_end', 'keyboard_focus_visible', 'tab_backtab']
        click('count')
        v.eventually(lambda: inspect()['client']['count'] == 1, 'Button regression')
        time.sleep(.5)
        before = inspect()['surfaces'][0]['frames']
        time.sleep(1)
        assert inspect()['surfaces'][0]['frames'] == before, 'Example repaints while idle'
        checks += ['existing_button_and_animation', 'no_idle_redraw_over_one_second']
        click('close')
        v.eventually(lambda: v.remote('sh', '-c', 'pgrep -x hello-layer || true').strip() == '',
                     'Example did not close')
        checks += ['clean_close']
        report = {'checks': checks, 'binary_sha256': digest,
                  'scope': 'Independent hello-layer debug binary on the prepared single-monitor scale-1 guest; desktop shell unchanged',
                  'not_covered': ['system audio integration', 'touch', 'fractional scale', 'physical GPU frame pacing']}
        (v.REPORT / 'lucent-slider-integration.json').write_text(json.dumps(report, indent=2)+'\n')
        print(json.dumps(report, indent=2))
    finally:
        v.session('sh', '-c', 'systemctl --user stop ' + shlex.quote(unit) + ' 2>/dev/null || true')
        v.remote('python3', '-c', "import shutil,sys; from pathlib import Path; p=Path(sys.argv[1]); assert p.parent == Path.home()/'.cache' and p.name.startswith('lucent-slider-'); shutil.rmtree(p)", directory)


if __name__ == '__main__':
    measurement = runpy.run_path(str(Path(__file__).with_name('measure-framework.py')))
    with measurement['stay_awake'](v.remote):
        main()
