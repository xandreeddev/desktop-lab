#!/usr/bin/env python3
"""Measure native startup, warm launcher submissions, resources and idle CPU/RSS.

Uses the Lucent idle adapter's stay-awake marker, restoring its prior state even
when measurement fails. Hardware/display metadata and binary identity are read
from the running guest. Results and captures remain private until reviewed.
"""
import argparse
from contextlib import contextmanager
import json
from pathlib import Path
import runpy
import statistics
import subprocess
import time


@contextmanager
def stay_awake(remote):
    marker = "from pathlib import Path; p=Path.home()/'.local/state/omarchy/indicators/stay-awake'; "
    previous = remote('python3', '-c', marker + 'print(int(p.exists()))').strip() == '1'
    try:
        remote('python3', '-c', marker + 'p.parent.mkdir(parents=True,exist_ok=True); p.touch()')
        yield
    finally:
        if not previous:
            remote('python3', '-c', marker + 'p.unlink(missing_ok=True)')


def measure(h, seconds):
    cli, inspect, session, remote = h['cli'], h['inspect'], h['session'], h['remote']
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    started = time.monotonic()
    session('systemctl', '--user', 'restart', 'lucent.service')
    h['eventually'](lambda: inspect()['client']['background_ready'], 'Startup did not produce a wallpaper', timeout=60)
    ready_ms = (time.monotonic() - started) * 1000
    cli('launcher', 'close')
    h['move'](960, 400)
    time.sleep(1.5)  # Let the headless idle adapter observe the temporary marker.
    transitions = []
    for action in ['open', 'close', 'open', 'close', 'open', 'close']:
        before = inspect()
        start = before['uptime_ms']
        cli('launcher', action)
        deadline = time.monotonic() + 8
        last = None
        stable_since = time.monotonic()
        while time.monotonic() < deadline:
            state = inspect()
            surface = next(s for s in state['surfaces'] if s['id'] == 'dock')
            if surface['frames'] != last:
                last = surface['frames']
                stable_since = time.monotonic()
            if state['uptime_ms'] - start > 600 and time.monotonic() - stable_since > .25:
                break
            time.sleep(.1)
        else:
            raise AssertionError('Launcher animation did not settle')
        frames = [t for t in surface['frame_times_ms'] if t >= start]
        intervals = [b-a for a, b in zip(frames, frames[1:])]
        if len(intervals) < 2:
            raise AssertionError('Too few submitted frames to measure animation')
        transitions.append({'action': action, 'warmup': len(transitions) < 2, 'frames': len(frames),
                            'median_frame_interval_ms': round(statistics.median(intervals), 2),
                            'max_frame_interval_ms': round(max(intervals), 2),
                            'observed_frame_span_ms': round(frames[-1]-frames[0], 2),
                            'cache_misses': state.get('gpu_cache', {}).get('misses', 0) - before.get('gpu_cache', {}).get('misses', 0)})
    pid = int(remote('pgrep', '-x', 'lucent-desktop').strip())
    before_idle = inspect()
    frames_before = {s['id']: s['frames'] for s in before_idle['surfaces']}
    group = session('systemctl', '--user', 'show', 'lucent.service', '-p', 'ControlGroup', '--value').strip()

    def usage():
        lines = remote('cat', '/sys/fs/cgroup'+group+'/cpu.stat').splitlines()
        return int(next(s.split()[1] for s in lines if s.startswith('usage_usec ')))

    start = time.monotonic()
    cpu_before = usage()
    print(f'Animation sampled; measuring {seconds} seconds of idle time.', flush=True)
    idle = json.loads(session('python3', 'desktop-lab/scripts/benchmark.py', '--pid', str(pid),
                              '--seconds', str(seconds), '--output', '/tmp/lucent-framework-idle.json'))
    duration = time.monotonic()-start
    idle['service_cpu_percent_one_core'] = round((usage()-cpu_before)/1_000_000/duration*100, 3)
    after_idle = inspect()
    frames_after = {s['id']: s['frames'] for s in after_idle['surfaces']}
    extra = {name: frames_after.get(name, count)-count for name, count in frames_before.items()}
    assert extra['dock'] == 0, f'Closed launcher kept repainting: {extra}'
    hardware = json.loads(remote('python3', '-c', "import json,os,platform; from pathlib import Path; mem=next(int(s.split()[1]) for s in Path('/proc/meminfo').read_text().splitlines() if s.startswith('MemTotal:')); print(json.dumps({'vcpus':os.cpu_count(),'guest_memory_mib':round(mem/1024,2),'architecture':platform.machine(),'kernel':platform.release()}))"))
    monitors = json.loads(session('hyprctl', '-j', 'monitors'))
    displays = [{key: monitor.get(key) for key in ('width', 'height', 'refreshRate', 'scale', 'transform')}
                for monitor in monitors]
    root = Path(__file__).resolve().parents[1]
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    dirty = subprocess.run(['git', 'diff', '--quiet', 'HEAD'], cwd=root).returncode != 0
    report = {'source_commit': source, 'source_working_tree_changed': dirty,
              'binary_sha256': remote('sha256sum', '/home/omarchy/.local/bin/lucent-desktop').split()[0],
              'displays': displays, **hardware, 'adapter': after_idle['adapter'],
              'restart_to_background_ready_ms': round(ready_ms, 2),
              'animations': transitions, 'idle': idle, 'idle_extra_frames': extra,
              'gpu_cache_before_idle': before_idle.get('gpu_cache'), 'gpu_cache_after_idle': after_idle.get('gpu_cache'),
              'workload': {'visible_widgets': before_idle['client']['widgets'], 'launcher': 'closed',
                           'applications': before_idle['client']['applications']},
              'scope': {'startup': 'Service restart through wallpaper-ready IPC, including SSH/polling overhead',
                        'animation': 'Render submission intervals, not scanout or isolated GPU execution time',
                        'resources': 'Texture payload only; driver/framebuffer/CPU prepared-image memory excluded',
                        'service_cpu': 'Lucent cgroup including helpers; compositor and separately launched apps excluded'}}
    (h['REPORT']/'lucent-framework-performance.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=int, default=120)
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error('seconds must be positive')
    h = runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='harness')
    with stay_awake(h['remote']):
        measure(h, args.seconds)


if __name__ == '__main__':
    main()
