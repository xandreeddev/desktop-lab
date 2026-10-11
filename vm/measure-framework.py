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
import uuid


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


@contextmanager
def frame_profiling(remote, session, enabled):
    """A private runtime-only unit override, removed even after measurement failure."""
    if not enabled:
        yield
        return
    tag = uuid.uuid4().hex
    prefix = "from pathlib import Path; import os,sys; p=Path('/run/user')/str(os.getuid())/'systemd/user/lucent.service.d'/('90-lucent-measure-'+sys.argv[1]+'.conf'); "
    try:
        remote('python3', '-c', prefix + "p.parent.mkdir(parents=True,exist_ok=True); p.open('x').write('[Service]\\nEnvironment=LUCENT_PROFILE_FRAMES=1\\n')", tag)
        session('systemctl', '--user', 'daemon-reload')
        yield
    finally:
        remote('python3', '-c', prefix + 'p.unlink(missing_ok=True)', tag)
        session('systemctl', '--user', 'daemon-reload')
        session('systemctl', '--user', 'restart', 'lucent.service')


def summarize_profiles(surface, since):
    samples = [sample for sample in surface.get('frame_profiles', []) if sample['submitted_ms'] >= since]
    if not samples:
        return None

    def stats(values):
        return {'median': round(statistics.median(values), 4), 'max': round(max(values), 4)}

    return {'frames': len(samples), 'total_ms': stats([p['total_ms'] for p in samples]),
            'stages_ms': {key: stats([p['stages_ms'][key] for p in samples]) for key in samples[0]['stages_ms']},
            'prepare_breakdown_ms': {key: stats([p['prepare_breakdown_ms'][key] for p in samples]) for key in samples[0]['prepare_breakdown_ms']},
            'resources_total': {key: sum(p['resources'][key] for p in samples) for key in samples[0]['resources']},
            'slowest': max(samples, key=lambda p: p['total_ms']), 'samples': samples}


def resident_breakdown(remote, pid):
    """One Linux smaps snapshot in KiB; no addresses, paths or GPU-memory inference."""
    keys = ('Rss', 'Pss', 'Pss_Anon', 'Pss_File', 'Pss_Shmem', 'Shared_Clean', 'Shared_Dirty',
            'Private_Clean', 'Private_Dirty', 'Anonymous', 'Swap')
    try:
        text = remote('cat', f'/proc/{int(pid)}/smaps_rollup')
    except subprocess.CalledProcessError:
        return None
    return {key: int(value.split()[0]) for line in text.splitlines() if ':' in line
            for key, value in [line.split(':', 1)] if key in keys}


def measure(h, seconds, require_profile=False):
    cli, inspect, session, remote = h['cli'], h['inspect'], h['session'], h['remote']
    # Capture provenance before the workload; development may continue during a long idle run.
    root = Path(__file__).resolve().parents[1]
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    dirty = subprocess.run(['git', 'diff', '--quiet', 'HEAD'], cwd=root).returncode != 0
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    started = time.monotonic()
    session('systemctl', '--user', 'restart', 'lucent.service')
    h['eventually'](lambda: inspect()['client']['background_ready'], 'Startup did not produce a wallpaper', timeout=60)
    ready_ms = (time.monotonic() - started) * 1000
    startup = inspect()
    if require_profile and not startup.get('frame_profiling'):
        raise AssertionError('Frame profiling did not activate; install the profiling-capable runtime and check service overrides')
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
                            'cache_misses': state.get('gpu_cache', {}).get('misses', 0) - before.get('gpu_cache', {}).get('misses', 0),
                            'cpu_profile': summarize_profiles(surface, start)})
    pid = int(remote('pgrep', '-x', 'lucent-desktop').strip())
    before_idle = inspect()
    resident_before = resident_breakdown(remote, pid)
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
    report = {'source_commit': source, 'source_working_tree_changed': dirty,
              'binary_sha256': remote('sha256sum', '/home/omarchy/.local/bin/lucent-desktop').split()[0],
              'displays': displays, **hardware, 'adapter': after_idle['adapter'],
              'restart_to_background_ready_ms': round(ready_ms, 2),
              'animations': transitions, 'idle': idle, 'idle_extra_frames': extra,
              'frame_profiling': before_idle.get('frame_profiling', False),
              'startup_profiles': {s['id']: summarize_profiles(s, 0) for s in startup['surfaces']} if require_profile else None,
              'profile_memory_before_idle': before_idle.get('profile_memory'),
              'profile_memory_after_idle': after_idle.get('profile_memory'),
              'resident_kib_before_idle': resident_before,
              'resident_kib_after_idle': resident_breakdown(remote, pid),
              'gpu_cache_before_idle': before_idle.get('gpu_cache'), 'gpu_cache_after_idle': after_idle.get('gpu_cache'),
              'prepared_assets_before_idle': before_idle['client'].get('prepared_assets'),
              'prepared_assets_after_idle': after_idle['client'].get('prepared_assets'),
              'workload': {'visible_widgets': before_idle['client']['widgets'], 'launcher': 'closed',
                           'applications': before_idle['client']['applications']},
              'scope': {'startup': 'Service restart through wallpaper-ready IPC, including SSH/polling overhead',
                        'animation': 'Render submission intervals, not scanout or isolated GPU execution time',
                        'resources': 'Texture payload only; driver/framebuffer/CPU prepared-image memory excluded',
                        'prepared_assets': 'Client-owned immutable pixel storage, deduplicated by allocation identity. Excludes in-flight work, scenes, decoder scratch, fonts, GPU and driver allocations; not RSS.',
                        'cache_misses': 'Shared application cache, including other surfaces updated during the sample',
                        'service_cpu': 'Lucent cgroup including helpers; compositor and separately launched apps excluded',
                        'cpu_profile': 'Successful frame submissions only; CPU-side wall-clock stages include driver waits. Preparation breakdown is nested, not additive. Instrumentation and inspection can affect the workload.',
                        'resident_breakdown': 'Two smaps_rollup snapshots in KiB; not interval averages or separate GPU-memory measurements'}}
    (h['REPORT']/'lucent-framework-performance.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=int, default=120)
    parser.add_argument('--profile', action='store_true', help='Temporarily enable bounded per-frame CPU-stage profiling')
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error('seconds must be positive')
    h = runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='harness')
    with stay_awake(h['remote']):
        with frame_profiling(h['remote'], h['session'], args.profile):
            measure(h, args.seconds, args.profile)
        if args.profile:
            h['eventually'](lambda: h['inspect']()['client']['background_ready'], 'Original runtime did not become ready after profiling', timeout=60)


if __name__ == '__main__':
    main()
