#!/usr/bin/env python3
"""Read-only matched-workload RSS/CPU and compositor-presentation sampling.

Prepare the requested static/animated workload and optional runtime presentation
instrumentation separately, with the host build lane idle. This helper never
restarts the shell, changes preferences/wallpaper, sends input or opens panels.
Records stay private until reviewed; Linux RSS is not a GPU-memory measurement.
"""
import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import runpy
import shlex
import statistics
import sys
import subprocess
import time


ROOT = Path(__file__).resolve().parents[1]


def load_helpers():
    """Load canonical read-only guest helpers only when main is requested."""
    common = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute',
                             '--git-common-dir'], check=True, capture_output=True, text=True, timeout=5)
    path = Path(common.stdout.strip()).resolve(strict=True).parent / 'vm/lab.py'
    spec = importlib.util.spec_from_file_location('daily_resources_lab', path)
    assert spec is not None and spec.loader is not None
    lab = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(lab)
    previous = sys.modules.get('lab')
    try:
        sys.modules['lab'] = lab
        helpers = runpy.run_path(str(ROOT / 'vm/test-framework.py'), run_name='harness')
    finally:
        if previous is None:
            sys.modules.pop('lab', None)
        else:
            sys.modules['lab'] = previous
    return lab, helpers

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


def collect_presentations(storage, snapshot, since=0, limit=16_384):
    # Bound host collection as well as runtime histories. A long or many-output
    # workload cannot grow this helper without limit; truncation is explicit.
    count = sum(len(values) for values in storage.values())
    truncated = False
    for surface in snapshot['surfaces']:
        key = surface_key(surface)
        if key not in storage and len(storage) >= 64:
            truncated = True
            continue
        values = storage.setdefault(key, {})
        for sample in surface.get('presentation', {}).get('samples', []):
            if sample['requested_at_ms'] < since:
                continue
            if sample['frame'] not in values:
                if count >= limit:
                    truncated = True
                    continue
                count += 1
            values[sample['frame']] = sample
    return truncated



def collect_sources(storage, snapshot, limit=16_384):
    truncated = False
    for producer in snapshot['client']['live_wallpaper']['producers']:
        for sample in (producer.get('source_frames') or {}).get('samples', []):
            key = (sample['stream_id'], sample['revision'])
            if key not in storage and len(storage) >= limit:
                truncated = True
                continue
            previous = storage.get(key)
            if previous is not None:
                assert previous == sample, 'Immutable stream source association changed'
            storage[key] = sample
    return truncated


def stream_cadence(samples, sources):
    """Bound references are distinct from source PTS and from visible pixels."""
    ordered = sorted(samples, key=lambda sample: sample['frame'])
    streams = {}
    missing = truncated = 0
    for sample in ordered:
        trace = sample.get('streams')
        if trace is None:
            missing += 1
            continue
        if trace.get('truncated', False):
            truncated += 1
        by_id = {}
        for item in trace['revisions']:
            by_id.setdefault(item['stream_id'], set()).add(item['revision'])
        for identity, revisions in by_id.items():
            stream = streams.setdefault(identity, {'observations': [], 'ambiguous': 0})
            if len(revisions) != 1:
                stream['ambiguous'] += 1
                continue  # One submission cannot establish one revision's cadence.
            stream['observations'].append((sample, next(iter(revisions))))
    results = []
    for identity, stream in streams.items():
        observations = stream['observations']
        revisions = {revision for _, revision in observations}
        repeats = advances = gaps = backwards = 0
        for (_, before), (_, after) in zip(observations, observations[1:]):
            if after == before:
                repeats += 1
            elif after > before:
                advances += 1
                gaps += after - before - 1
            else:
                backwards += 1
        rate = None
        if len(observations) > 1 and len({sample['clock_id'] for sample, _ in observations}) == 1:
            duration = observations[-1][0]['presented_ns'] - observations[0][0]['presented_ns']
            if duration > 0:
                rate = advances * 1_000_000_000 / duration
        prepared = sorted((value for (sid, _), value in sources.items() if sid == identity),
                          key=lambda value: value['revision'])
        intervals = []
        mapping_gaps = 0
        for before, after in zip(prepared, prepared[1:]):
            mapping_gaps += max(0, after['revision'] - before['revision'] - 1)
            if before['generation'] == after['generation'] and before['cycle'] == after['cycle']:
                delta = after['source_ns'] - before['source_ns']
                if delta > 0:
                    intervals.append(delta)
        results.append({'stream_id': identity, 'reference_samples': len(observations),
                        'unique_prepared_revisions': len(revisions), 'distinct_advancements': advances,
                        'repeated_revision_samples': repeats, 'prepared_revision_gaps_between_presentations': gaps,
                        'backward_revision_steps': backwards, 'ambiguous_multi_revision_samples': stream['ambiguous'],
                        'presented_distinct_advancements_hz': rate,
                        'joined_source_revisions': sum((identity, revision) in sources for revision in revisions),
                        'missing_source_revisions': sum((identity, revision) not in sources for revision in revisions),
                        'observed_prepared_source_samples': len(prepared), 'source_mapping_revision_gaps': mapping_gaps,
                        'observed_prepared_source_pts_hz': (1_000_000_000 / statistics.mean(intervals)) if intervals else None,
                        'source_generations': sorted({value['generation'] for value in prepared}),
                        'observed_pts_cycles': len({(value['generation'], value['cycle']) for value in prepared})})
    return {'streams': results, 'missing_trace_samples': missing, 'truncated_trace_samples': truncated,
            'scope': 'Compositor-presented bound resource references. Distinct revisions, repeated references, revision gaps and prepared media PTS cadence are separate. Gaps do not prove decoder drops; references do not prove visible pixels or hardware scanout.'}


def producer_identities(snapshot):
    keys = ('revision', 'running', 'target', 'frame_size', 'source_aspect')
    return [{key: producer.get(key) for key in keys}
            for producer in snapshot['client']['live_wallpaper']['producers']]

def assert_idle_snapshot(snapshot, phase):
    """Reject activity throughout a matched interval, including newer panels."""
    client = snapshot['client']
    assert client['background_ready'] and not client['wallpaper_busy'], 'Wait for the wallpaper to settle'
    assert not client['launcher'] and not client['widgets_editing'], 'Close the launcher and widget editor'
    assert not snapshot.get('frame_profiling', False), 'Separate CPU-stage profiling from normal resources'
    for name in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'settings',
                 'weather', 'displays', 'application_actions'):
        panel = client.get(name, {})
        assert not any(panel.get(key, False) for key in ('open', 'visible', 'busy', 'refreshing')), 'Close and settle panels before sampling: '+name
    assert not client.get('notification_history_open', False), 'Close notification history'
    assert not client.get('wallpaper_browser', {}).get('busy', False), 'Wait for wallpaper searches/imports'
    assert client.get('settings_save_status') not in ('saving', 'retrying'), 'Wait for preference persistence'
    live = client['live_wallpaper']
    assert not any(live.get(key, False) for key in ('open', 'importing', 'awaiting_first_frame', 'library_loading')), 'Wait for live wallpaper preparation'
    assert bool(live['running']) == (phase == 'animated-background'), 'Prepared phase does not match actual playback'


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
    lab, h = load_helpers()
    inspect, remote, session = h['inspect'], h['remote'], h['session']
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    before = inspect()
    assert_idle_snapshot(before, args.phase)
    live = before['client']['live_wallpaper']
    pid_text = remote('pgrep', '-x', 'lucent-desktop').splitlines()
    assert len(pid_text) == 1
    monitors_before = json.loads(session('hyprctl', '-j', 'monitors'))
    displays = [{k: m.get(k) for k in ('width', 'height', 'scale', 'transform', 'refreshRate')} for m in monitors_before]
    command = ['python3', '-c', SAMPLER, pid_text[0], str(args.seconds), expected]
    process = subprocess.Popen(lab.ssh_args('lucent')+[shlex.join(command)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    samples = {}
    sources = {}
    source_truncated = collect_sources(sources, before)
    truncated = False
    since = before['uptime_ms']
    try:
        while process.poll() is None:
            snapshot = inspect()
            assert_idle_snapshot(snapshot, args.phase)
            assert producer_identities(before) == producer_identities(snapshot), 'Playback generation/target changed'
            truncated |= collect_presentations(samples, snapshot, since)
            source_truncated |= collect_sources(sources, snapshot)
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
    assert_idle_snapshot(after, args.phase)
    truncated |= collect_presentations(samples, after, since)
    source_truncated |= collect_sources(sources, after)
    assert remote('pgrep', '-x', 'lucent-desktop').strip() == pid_text[0]
    assert after['uptime_ms'] > since
    assert json.loads(session('hyprctl', '-j', 'monitors')) == monitors_before, 'Display state changed'
    assert producer_identities(before) == producer_identities(after), 'Playback generation/target changed'
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
              'presentation_sampling_truncated': truncated,
              'presentation_sample_limit': 16_384,
              'presentation': {name: presentation_intervals(list(values.values())) for name, values in samples.items()},
              'stream_cadence': {name: stream_cadence(list(values.values()), sources) for name, values in samples.items()},
              'source_trace_sampling_truncated': source_truncated,
              'source_trace_samples': len(sources), 'source_trace_sample_limit': 16_384,
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
                        'If presentation sampling is truncated, cadence covers retained samples only, not the complete resource interval.',
                        'Inspection every 0.5 seconds and optional instrumentation can affect the measured workload. Compare uninstrumented resource samples separately.']}
    path = ROOT/'reports/local'/('daily-resources-'+args.label+'.json')
    path.parent.mkdir(parents=True, exist_ok=True)
    private(path, result)
    print(json.dumps({'report': str(path), 'phase': args.phase, 'rss_mib_max_interval': resources['rss_mib_max_interval'], 'source': source}))


if __name__ == '__main__':
    main()
