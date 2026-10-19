#!/usr/bin/env python3
"""Read-only matched-workload RSS/CPU and compositor-presentation sampling.

Prepare the requested static/animated workload and optional runtime presentation
instrumentation separately, with the host build lane idle. This helper never
restarts the shell, changes preferences/wallpaper, sends input or opens panels.
Records stay private until reviewed; Linux RSS is not a GPU-memory measurement.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import runpy
import shlex
import statistics
import subprocess
import time

import lab

ROOT = Path(__file__).resolve().parents[1]

# Runs inside the guest, samples only /proc and the existing installed files.
SAMPLER = r'''
import hashlib,json,os,stat,sys,time
from pathlib import Path
pid,seconds,expected=int(sys.argv[1]),float(sys.argv[2]),sys.argv[3]
proc=Path('/proc')/str(pid)
def sha(path):
    with open(path,'rb') as stream:
        out=hashlib.sha256()
        for block in iter(lambda:stream.read(1024*1024),b''): out.update(block)
        return out.hexdigest()
def config():
    path=Path.home()/'.local/state/lucent/desktop.json'
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
    with os.fdopen(fd,'rb') as stream:
        before=os.fstat(stream.fileno()); assert stat.S_ISREG(before.st_mode) and before.st_size<=4*1024*1024
        content=stream.read(4*1024*1024+1); after=os.fstat(stream.fileno())
        identity=lambda x:[x.st_dev,x.st_ino,x.st_size,x.st_mtime_ns]
        assert identity(before)==identity(after) and len(content)<=4*1024*1024
        return {'sha256':hashlib.sha256(content).hexdigest(),'identity':identity(after)}
def sample():
    fields=(proc/'stat').read_text().rsplit(')',1)[1].split()
    values={k:int(v.split()[0]) for line in (proc/'status').read_text().splitlines() if ':' in line for k,v in [line.split(':',1)] if k in ('VmRSS','VmHWM','VmSwap')}
    return int(fields[11])+int(fields[12]),int(fields[19]),values
keys={'Rss','Pss','Pss_Anon','Pss_File','Pss_Shmem','Shared_Clean','Shared_Dirty','Private_Clean','Private_Dirty','Anonymous','Swap'}
def smaps():
    return {k:int(v.split()[0]) for line in (proc/'smaps_rollup').read_text().splitlines() if ':' in line for k,v in [line.split(':',1)] if k in keys}
assert (proc/'comm').read_text().strip()=='lucent-desktop'
installed=Path.home()/'.local/bin/lucent-desktop'
assert sha(proc/'exe')==expected==sha(installed)
settings=config(); before=smaps(); ticks,identity,values=sample()
started=time.monotonic(); samples=[values['VmRSS']]; peak=values['VmHWM']; swap=values.get('VmSwap',0)
while time.monotonic()-started<seconds:
    time.sleep(min(.25,max(0,seconds-(time.monotonic()-started))))
    now,same,values=sample(); assert same==identity,'Desktop process restarted'
    samples.append(values['VmRSS']); peak=max(peak,values['VmHWM']); swap=max(swap,values.get('VmSwap',0))
duration=time.monotonic()-started
assert config()==settings,'Preferences changed during sampling'
assert sha(proc/'exe')==expected==sha(installed),'Running or installed executable changed'
print(json.dumps({'seconds':duration,'process_start_ticks':identity,'binary_sha256':expected,
 'settings':settings,'cpu_percent_one_core':(now-ticks)/os.sysconf('SC_CLK_TCK')/duration*100,
 'rss_mib_mean':sum(samples)/len(samples)/1024,
 'rss_mib_max_interval':max(samples)/1024,'process_lifetime_high_water_mib':peak/1024,'swap_mib_max':swap/1024,
 'sample_period_seconds':.25,'rss_sample_count':len(samples),'smaps_kib_before':before,'smaps_kib_after':smaps()}))
'''


def presentation_intervals(samples):
    """Never infer display cadence from CPU submission counters or clocks mixed."""
    ordered = sorted(samples, key=lambda s: s['frame'])
    intervals = []
    clock_changes = 0
    nonincreasing = 0
    for a, b in zip(ordered, ordered[1:]):
        if a['clock_id'] != b['clock_id']:
            clock_changes += 1
            continue
        elapsed = b['presented_ns'] - a['presented_ns']
        if elapsed <= 0:
            nonincreasing += 1
            continue
        intervals.append(elapsed / 1_000_000)
    if not intervals:
        return {'samples': len(ordered), 'intervals': 0, 'fps': None,
                'reason': 'No comparable compositor-presented timestamps',
                'clock_changes': clock_changes, 'nonincreasing': nonincreasing}
    ranked = sorted(intervals)

    def percentile(fraction):
        return ranked[min(len(ranked)-1, math.ceil(fraction*len(ranked))-1)]

    return {'samples': len(ordered), 'intervals': len(intervals),
            'fps': 1000 / statistics.mean(intervals),
            'interval_ms': {'median': statistics.median(intervals), 'p95': percentile(.95),
                            'p99': percentile(.99), 'maximum': max(intervals)},
            'clock_changes': clock_changes, 'nonincreasing': nonincreasing,
            'all_vsync_flag': all(s['flags'] & 1 for s in ordered),
            'all_hw_clock_flag': all(s['flags'] & 2 for s in ordered),
            'all_hw_completion_flag': all(s['flags'] & 4 for s in ordered),
            'scope': 'Compositor-reported presentation cadence. Hardware timing/completion requires the recorded protocol flags; unique video-frame delivery is not established.'}


def surface_key(surface):
    return surface['id']


def collect_presentations(storage, snapshot, since=0):
    for surface in snapshot['surfaces']:
        values = storage.setdefault(surface_key(surface), {})
        for sample in surface.get('presentation', {}).get('samples', []):
            if sample['requested_at_ms'] >= since:
                values[sample['frame']] = sample


def private(path, value):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release-manifest', type=Path, required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--phase', choices=('static-idle', 'animated-background'), required=True)
    parser.add_argument('--seconds', type=int, default=120)
    args = parser.parse_args()
    if not 10 <= args.seconds <= 600 or not re.fullmatch(r'[a-z0-9][a-z0-9-]{0,63}', args.label):
        parser.error('Use 10–600 seconds and a bounded lowercase label')
    release = json.loads(args.release_manifest.read_text())
    source = release['source_commit']
    expected = release['binaries']['lucent-desktop']['sha256']
    assert re.fullmatch(r'[0-9a-f]{40}', source) and re.fullmatch(r'[0-9a-f]{64}', expected)
    subprocess.run(['git', 'cat-file', '-e', source+'^{commit}'], cwd=ROOT, check=True)
    h = runpy.run_path(str(ROOT/'vm/test-framework.py'), run_name='harness')
    inspect, remote, session = h['inspect'], h['remote'], h['session']
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    before = inspect()
    assert before['client']['background_ready'] and not before['client']['wallpaper_busy']
    assert not before['client']['launcher'] and not before['client']['widgets_editing'], 'Prepare an idle closed desktop first'
    assert not before.get('frame_profiling', False), 'Measure normal resources separately from CPU-stage profiling'
    for name in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'settings', 'weather'):
        panel = before['client'].get(name, {})
        assert not panel.get('open', False) and not panel.get('busy', False), 'Close and settle panels before sampling'
    live = before['client']['live_wallpaper']
    assert not live['open'] and not live['importing'] and not live['awaiting_first_frame']
    assert bool(live['running']) == (args.phase == 'animated-background'), 'Prepared phase does not match actual playback'
    pid_text = remote('pgrep', '-x', 'lucent-desktop').splitlines()
    assert len(pid_text) == 1
    monitors_before = json.loads(session('hyprctl', '-j', 'monitors'))
    displays = [{k: m.get(k) for k in ('width', 'height', 'scale', 'transform', 'refreshRate')} for m in monitors_before]
    command = ['python3', '-c', SAMPLER, pid_text[0], str(args.seconds), expected]
    process = subprocess.Popen(lab.ssh_args('lucent')+[shlex.join(command)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    samples = {}
    since = before['uptime_ms']
    try:
        while process.poll() is None:
            collect_presentations(samples, inspect(), since)
            time.sleep(.5)
        output, error = process.communicate(timeout=5)
        assert process.returncode == 0, 'Read-only guest sampler failed: '+error[-1000:]
    except BaseException:
        process.terminate()
        try: process.communicate(timeout=5)
        except subprocess.TimeoutExpired: process.kill(); process.communicate()
        raise
    resources = json.loads(output)
    after = inspect()
    collect_presentations(samples, after, since)
    assert remote('pgrep', '-x', 'lucent-desktop').strip() == pid_text[0]
    assert after['uptime_ms'] > since
    assert json.loads(session('hyprctl', '-j', 'monitors')) == monitors_before, 'Display state changed'
    assert before['client']['live_wallpaper']['producers'] == after['client']['live_wallpaper']['producers'], 'Playback generation/target changed'
    frames_before = {s['id']: s['frames'] for s in before['surfaces']}
    frames_after = {s['id']: s['frames'] for s in after['surfaces']}
    assert frames_before.keys() == frames_after.keys(), 'Surface membership changed'
    extra = {name: frames_after[name]-count for name, count in frames_before.items()}
    assert all(v >= 0 for v in extra.values())
    result = {'schema': 1, 'label': args.label, 'phase': args.phase,
              'release_source_commit': source, 'release_manifest_sha256': hashlib.sha256(args.release_manifest.read_bytes()).hexdigest(),
              'harness_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'displays': displays, 'adapter': before['adapter'], 'resources': resources,
              'cpu_stage_profiling': before.get('frame_profiling', False),
              'presentation_capture': after.get('presentation_feedback'),
              'presentation': {name: presentation_intervals(list(values.values())) for name, values in samples.items()},
              'successful_submission_deltas': extra,
              'observation_uptime_ms': [since, after['uptime_ms']],
              'gpu_cache_before': before.get('gpu_cache'), 'gpu_cache_after': after.get('gpu_cache'),
              'prepared_assets_before': before['client'].get('prepared_assets'), 'prepared_assets_after': after['client'].get('prepared_assets'),
              'retained_video_frame_bytes': live['retained_frame_bytes'],
              'targets': {'normal_rss_mib': 300, 'interval_rss_target_met': resources['rss_mib_max_interval'] <= 300},
              'preservation': {'settings_bytes_inode': True, 'process_and_installed_binary': True, 'output_state': True, 'playback_generation': True},
              'scope': ['Read-only collection; prepare identical workload and release cohorts separately.',
                        'RSS samples include software-driver mappings. High-water RSS spans the process lifetime; GPU texture payload is not total GPU memory.',
                        'Compositor feedback is separate from successful CPU-side submissions. No hardware scanout or 60 unique video frames per second is inferred.',
                        'Inspection every 0.5 seconds and optional instrumentation can affect the measured workload. Compare uninstrumented resource samples separately.']}
    path = ROOT/'reports/local'/('daily-resources-'+args.label+'.json')
    path.parent.mkdir(parents=True, exist_ok=True)
    private(path, result)
    print(json.dumps({'report': str(path), 'phase': args.phase, 'rss_mib_max_interval': resources['rss_mib_max_interval'], 'source': source}))


if __name__ == '__main__':
    main()
