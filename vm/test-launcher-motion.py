#!/usr/bin/env python3
"""Run inside an unlocked guest session; captures stay in a private temp directory.

Requires grim, wf-recorder and ffmpeg. Records ten opening/closing cycles and
detects the old compact dock buffer at the expanded surface's top origin. This
checks compositor pixels, independently of successful FrameSubmitted events.
Usage: python3 scripts/in-session.py python3 vm/test-launcher-motion.py
"""
import argparse
import json
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time


def cli(*args):
    return subprocess.check_output(['lucent-cli', *args], text=True)


def state():
    return json.loads(cli('inspect'))


def settled(opened):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        value = state()
        if (value['client']['launcher'] == opened
                and value['client']['dock_surface_expanded'] == opened
                and not value['client']['launcher_opening']):
            time.sleep(.6)
            return state()
        time.sleep(.05)
    raise AssertionError('Launcher failed to settle')


def misplaced_score(frame, width, samples, wrong_y):
    matches = 0
    for x, y, color in samples:
        offset = ((wrong_y + y) * width + x) * 3
        pixel = frame[offset:offset + 3]
        matches += len(pixel) == 3 and max(abs(a - b) for a, b in zip(color, pixel)) <= 4
    return matches / len(samples)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cycles', type=int, default=10, choices=range(1, 21))
    args = parser.parse_args()
    for command in ['lucent-cli', 'wf-recorder', 'grim', 'ffmpeg']:
        assert shutil.which(command), f'Missing {command}'
    monitors = json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors']))
    assert len(monitors) == 1 and monitors[0]['scale'] == 1, 'Requires single output at 1x'
    marker = Path.home() / '.local/state/omarchy/indicators/stay-awake'
    previous_marker = marker.exists()
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.touch()
    directory = Path(tempfile.mkdtemp(prefix='lucent-launcher-motion-'))
    recorder = None
    try:
        cli('launcher', 'close')
        before = settled(False)
        dock = next(s for s in before['surfaces'] if s['role'] == 'dock')
        panel = next(h for h in dock['hits'] if h['id'] == 'dock-panel')
        height = round(before['client']['desktop_viewport'][1])
        width = round(panel['width'])
        wrong_y = round(panel['y'])
        correct_y = height - dock['height'] + wrong_y
        geometry = f'{round(panel["x"])},0 {width}x{height}'
        baseline = subprocess.check_output(['grim', '-g', geometry, '-t', 'ppm', '-'])
        header, dimensions, maximum, pixels = baseline.split(b'\n', 3)
        assert header == b'P6' and dimensions == f'{width} {height}'.encode() and maximum == b'255'
        samples = []
        # Stay inside rounded corners; include both the opaque fill and icons.
        for y in range(16, round(panel['height']) - 16, 3):
            for x in range(24, width - 24, 3):
                offset = ((correct_y + y) * width + x) * 3
                samples.append((x, y, pixels[offset:offset + 3]))
        assert len({color for _, _, color in samples}) > 8, 'Reference needs visible dock icons'
        assert misplaced_score(pixels, width, samples, wrong_y) < .9, 'Ambiguous top-area reference'
        capture = directory / 'motion.mkv'
        with (directory / 'recorder.log').open('w') as log:
            recorder = subprocess.Popen(['wf-recorder', '-y', '--no-dmabuf', '-D',
                '-g', geometry, '-r', '60', '-c', 'ffv1', '-p', 'threads=1',
                '-f', str(capture)], stdout=log, stderr=log)
            time.sleep(1)
            assert recorder.poll() is None, 'Recorder failed to start'
            for _ in range(args.cycles):
                cli('launcher', 'open'); settled(True)
                cli('launcher', 'close'); settled(False)
            recorder.send_signal(signal.SIGINT)
            assert recorder.wait(timeout=20) == 0, 'Recorder failed'
        decoder = subprocess.Popen(['ffmpeg', '-v', 'error', '-threads', '1', '-i', str(capture),
            '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-'], stdout=subprocess.PIPE)
        frames, misplaced = 0, []
        size = width * height * 3
        while True:
            frame = decoder.stdout.read(size)
            if not frame:
                break
            assert len(frame) == size, 'Incomplete decoded frame'
            score = misplaced_score(frame, width, samples, wrong_y)
            if score >= .985:
                misplaced.append({'frame': frames, 'score': score})
            frames += 1
        assert decoder.wait(timeout=10) == 0 and frames >= args.cycles * 20
        after = settled(False)
        final_dock = next(s for s in after['surfaces'] if s['role'] == 'dock')
        assert final_dock['height'] == dock['height'], 'Compact buffer was not restored'
        report = {'cycles': args.cycles, 'captured_frames': frames,
                  'misplaced_frames': misplaced, 'compact_height': dock['height']}
        (directory / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report), flush=True)
        assert not misplaced, 'Compact dock appeared at the expanded surface top origin'
    finally:
        if recorder is not None and recorder.poll() is None:
            recorder.send_signal(signal.SIGINT)
            recorder.wait(timeout=20)
        cli('launcher', 'close')
        if not previous_marker:
            marker.unlink(missing_ok=True)


if __name__ == '__main__':
    main()
