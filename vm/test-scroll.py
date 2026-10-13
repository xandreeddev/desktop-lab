#!/usr/bin/env python3
"""Exercise the independent scroll-list client with native Wayland input.

Requires an unlocked single-monitor, scale-1 Lucent guest. Uploads only the
provided example binary; finally removes the owned transient unit and directory.
The installed desktop, authentication binaries and user settings are preserved.
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
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(5)
 s.connect(os.path.join(os.environ['XDG_RUNTIME_DIR'],'scroll-list.sock'))
 s.sendall(b'inspect\\n'); chunks=[]
 while True:
  b=s.recv(65536)
  if not b: break
  chunks.append(b)
 print(b''.join(chunks).decode())'''))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    monitors = json.loads(v.session('hyprctl', 'monitors', '-j'))
    assert len(monitors) == 1 and monitors[0]['scale'] == 1
    monitor = monitors[0]
    assert monitor['x'] == monitor['y'] == 0
    assert not v.remote('sh', '-c', 'pgrep -x scroll-list || true').strip(), 'Another example is running'
    unit = 'lucent-scroll-test-' + uuid.uuid4().hex
    directory = v.remote('python3', '-c', "from pathlib import Path; import tempfile; p=Path.home()/'.cache';p.mkdir(exist_ok=True); print(tempfile.mkdtemp(prefix='lucent-scroll-',dir=p))").strip()
    uploaded = directory + '/scroll-list'
    checks = []
    try:
        with binary.open('rb') as stream:
            subprocess.run(v.lab.ssh_args(v.PROFILE) + ['cat > ' + shlex.quote(uploaded)], stdin=stream, check=True)
        v.remote('chmod', '700', uploaded)
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        assert v.remote('sha256sum', uploaded).split()[0] == digest
        v.session('systemd-run', '--user', '--collect', '--unit', unit, '--setenv=LP_NUM_THREADS=2', uploaded)
        v.eventually(lambda: len(inspect()['surfaces']) == 1, 'Example did not start', timeout=25)

        def layer():
            layers = json.loads(v.session('hyprctl', 'layers', '-j'))
            matches = [entry for output in layers.values() for group in output['levels'].values()
                       for entry in group if entry['namespace'] == 'scroll-list-list']
            assert len(matches) == 1
            return matches[0]

        def hit(name):
            return next(h for s in inspect()['surfaces'] for h in s['hits'] if h['id'] == name)

        def move(x, y):
            pos = layer()
            v.send([{'type': 'abs', 'data': {'axis': axis, 'value': round(value / (extent-1)*32767)}}
                    for axis, value, extent in [('x', x+pos['x'], monitor['width']), ('y', y+pos['y'], monitor['height'])]])

        def click(name):
            target = hit(name)
            move(target['x']+target['width']/2, target['y']+target['height']/2)
            v.button(True); v.button(False)

        def wheel(direction):
            v.send([{'type': 'btn', 'data': {'down': down, 'button': direction}} for down in [True, False]])

        def focus(name):
            v.eventually(lambda: inspect()['surfaces'][0]['focus'] == name, 'Focus missed '+name)

        def visible(name):
            target = hit(name)
            clip = target['clip']
            return (target['x'] >= clip['x'] and target['y'] >= clip['y']
                    and target['x']+target['width'] <= clip['x']+clip['width']+.01
                    and target['y']+target['height'] <= clip['y']+clip['height']+.01)

        assert 'Vulkan' in inspect()['adapter']
        viewport = hit('items')
        move(viewport['x']+viewport['width']/2, viewport['y']+viewport['height']/2)
        wheel('wheel-down')
        v.eventually(lambda: inspect()['client']['offset'] > 0, 'Wheel did not scroll')
        wheel('wheel-up')
        v.eventually(lambda: inspect()['client']['offset'] == 0, 'Wheel did not return')
        checks += ['native_wheel_down_up']
        padding = v.token_value('component.scroll.padding')
        thumb_width = v.token_value('component.scroll.thumb_width')
        x = viewport['x'] + viewport['width'] - padding - thumb_width/2
        y = viewport['y'] + padding + 4
        move(x, y); v.button(True)
        move(x, viewport['y']+viewport['height']+40)
        v.eventually(lambda: visible('item-31'), 'Scrollbar did not reveal last item')
        v.button(False)
        checks += ['scrollbar_capture_outside', 'last_item_reachable']
        v.key(102)  # Home while viewport focused
        v.eventually(lambda: inspect()['client']['offset'] == 0, 'Home did not reset')
        v.key(109)  # Page Down
        v.eventually(lambda: inspect()['client']['offset'] > 0, 'Page Down failed')
        v.key(104)  # Page Up
        v.eventually(lambda: inspect()['client']['offset'] == 0, 'Page Up failed')
        for index in range(32):
            v.key(15)
            focus(f'item-{index}')
            v.eventually(lambda: visible(f'item-{index}'), 'Tab left a row cropped')
        v.key(28)
        v.eventually(lambda: inspect()['client']['selected'] == 31, 'Keyboard activation failed')
        assert inspect()['surfaces'][0]['focus_visible']
        v.screenshot('lucent-scroll-example.png')
        v.key(42, 15); focus('item-30')
        v.key(15); focus('item-31')
        v.key(15); focus('resize')
        checks += ['home_page_keys', 'tab_reveals_all_32_rows', 'backtab', 'keyboard_activation_and_focus']
        original_height = inspect()['surfaces'][0]['height']
        v.key(28)
        v.eventually(lambda: inspect()['surfaces'][0]['height'] < original_height, 'Resize did not configure surface')
        v.key(42, 15); focus('item-31')
        v.eventually(lambda: visible('item-31'), 'Resize/reveal cropped final row')
        click('fewer')
        v.eventually(lambda: inspect()['client']['count'] == 2 and inspect()['client']['offset'] == 0, 'Short list did not reset')
        assert visible('item-0') and visible('item-1')
        click('hide')
        v.eventually(lambda: all(h['id'] != 'items' for h in inspect()['surfaces'][0]['hits']), 'Hidden list retained hits')
        click('hide')
        v.eventually(lambda: not inspect()['client']['hidden'], 'List did not return')
        assert visible('item-0') and visible('item-1')
        checks += ['surface_resize_reveal', 'content_shrink', 'hide_show']
        move(2, 2)
        time.sleep(.5)
        before = inspect()['surfaces'][0]['frames']
        time.sleep(1)
        assert inspect()['surfaces'][0]['frames'] == before, 'Example repaints while idle'
        checks += ['no_idle_redraw_over_one_second']
        click('close')
        v.eventually(lambda: not v.remote('sh', '-c', 'pgrep -x scroll-list || true').strip(), 'Example did not close')
        checks += ['clean_close']
        report = {'checks': checks, 'binary_sha256': digest,
                  'scope': 'Independent scroll-list debug client on single-monitor scale-1 guest; installed desktop unchanged',
                  'not_covered': ['production audio panel', 'native fractional scaling', 'touch/kinetic scrolling', 'physical GPU frame pacing']}
        (v.REPORT / 'lucent-scroll-integration.json').write_text(json.dumps(report, indent=2)+'\n')
        print(json.dumps(report, indent=2))
    finally:
        v.button(False)
        v.session('sh', '-c', 'systemctl --user stop ' + shlex.quote(unit) + ' 2>/dev/null || true')
        v.remote('python3', '-c', "import shutil,sys; from pathlib import Path; p=Path(sys.argv[1]); assert p.parent == Path.home()/'.cache' and p.name.startswith('lucent-scroll-'); shutil.rmtree(p)", directory)


if __name__ == '__main__':
    measurement = runpy.run_path(str(Path(__file__).with_name('measure-framework.py')))
    with measurement['stay_awake'](v.remote):
        main()
