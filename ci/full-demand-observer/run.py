#!/usr/bin/env python3
"""Inert preparation unless --run: one hosted, source-bound decoder experiment.

Derived from the reviewed Arch release admission/provenance policy and the
immutable retarget-v2 lifecycle/schema policy. No VM or local execution path.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import time

HERE = Path(__file__).resolve().parent
PROFILE = json.loads((HERE / 'profile.json').read_text())
EXAMPLE = 'full-demand-observer'
CONSUMER_FILES = ('observer.rs', 'observer_tests.rs', 'accounting.rs')
SCOPE_FALSE = {'vm_deployed': False, 'native_presentation_accepted': False,
               'whole_shell_memory_accepted': False, 'performance_accepted': False}


def need(value, reason):
    if not value:
        raise ValueError(reason)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def output(argv, cwd=None, env=None, timeout=30):
    return subprocess.check_output(argv, cwd=cwd, env=env, timeout=timeout, text=True).strip()


def context(env):
    need(env.get('GITHUB_ACTIONS') == 'true'
         and env.get('GITHUB_REPOSITORY') == PROFILE['repository']
         and env.get('GITHUB_ACTOR') == env.get('GITHUB_TRIGGERING_ACTOR') == PROFILE['actor']
         and env.get('GITHUB_EVENT_NAME') == 'push'
         and env.get('GITHUB_REF') == 'refs/heads/' + PROFILE['branch']
         and env.get('LUCENT_CI_IMAGE') == PROFILE['container_image'], 'reviewed_owner_hosted_push_only')
    for key in ('GITHUB_SHA', 'LUCENT_WORKFLOW_SHA'):
        need(re.fullmatch('[0-9a-f]{40}', env.get(key, '')), 'exact_tools_identity_required')
    need(env['GITHUB_SHA'] == env['LUCENT_WORKFLOW_SHA'], 'workflow_and_tools_must_match')
    for key in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
        need(re.fullmatch('[1-9][0-9]{0,19}', env.get(key, '')), 'actual_run_identity_required')
    return {key: env[key] for key in ('GITHUB_REPOSITORY', 'GITHUB_ACTOR', 'GITHUB_TRIGGERING_ACTOR',
            'GITHUB_EVENT_NAME', 'GITHUB_REF', 'GITHUB_SHA', 'LUCENT_WORKFLOW_SHA',
            'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT')}


def resource_snapshot(cgroup=Path('/sys/fs/cgroup'), meminfo=Path('/proc/meminfo')):
    cpu = (cgroup / 'cpu.max').read_text().strip().split()
    maximum = (cgroup / 'memory.max').read_text().strip()
    current = (cgroup / 'memory.current').read_text().strip()
    swap = (cgroup / 'memory.swap.max').read_text().strip()
    need(len(cpu) == 2 and all(v.isdecimal() for v in cpu) and int(cpu[1]) > 0
         and int(cpu[0]) == PROFILE['cpu_limit'] * int(cpu[1]), 'actual_three_CPU_cgroup_required')
    need(maximum.isdecimal() and current.isdecimal() and int(maximum) == PROFILE['memory_max_bytes']
         and 0 <= int(current) < int(maximum) and swap == '0', 'actual_ten_GiB_no_swap_cgroup_required')
    rows = [line.split() for line in meminfo.read_text().splitlines() if line.startswith('MemAvailable:')]
    need(len(rows) == 1 and len(rows[0]) == 3 and rows[0][2] == 'kB'
         and rows[0][1].isdecimal(), 'actual_host_MemAvailable_required')
    available = int(rows[0][1]) // 1024
    effective = min(available, (int(maximum) - int(current)) // 1024**2)
    return {'cpu_max': ' '.join(cpu), 'memory_max_bytes': int(maximum),
            'memory_current_bytes': int(current), 'memory_swap_max_bytes': 0,
            'host_available_mib': available, 'effective_available_mib': effective}


def admit():
    samples = []
    for _ in range(3):
        sample = resource_snapshot()
        need(sample['effective_available_mib'] >= PROFILE['admission_floor_mib'],
             'fresh_4500MiB_admission_required')
        samples.append(sample)
        time.sleep(1)
    return samples


def test_summary(raw, expected, names):
    summaries = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', raw)
    need(len(summaries) == 1 and tuple(map(int, summaries[0][:4])) == (expected, 0, 0, 0),
         'exact_unambiguous_actual_test_count_required')
    seen = set()
    starts = list(re.finditer(r'^test ([A-Za-z0-9_:]+) \.\.\.([^\n]*)', raw, re.M))
    for index, match in enumerate(starts):
        end = starts[index + 1].start() if index + 1 < len(starts) else len(raw)
        block = match[2] + raw[match.end():end]
        if re.search(r'^\s*ok\s*$', block, re.M):
            seen.add(match[1])
    need(set(names).issubset(seen) and len(seen) == expected,
         'every_expected_test_must_individually_pass')
    return {'passed': expected, 'actual_passed_names': sorted(seen)}


def exact_source(source):
    need(source.is_absolute() and source.resolve(strict=True) == source, 'canonical_source_required')
    need(output(['git', 'rev-parse', 'HEAD'], source) == PROFILE['source_commit']
         and output(['git', 'rev-parse', 'HEAD^{tree}'], source) == PROFILE['source_tree']
         and output(['git', 'status', '--porcelain', '--untracked-files=all'], source) == '',
         'exact_pristine_backend_source_required')


def forbidden_environment(env):
    exact = {'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
             'CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'CARGO_INCREMENTAL',
             'LD_PRELOAD', 'GLIBC_TUNABLES', 'MALLOC_CONF', 'JEMALLOC_SYS_WITH_MALLOC_CONF'}
    return sorted(key for key in env if key in exact or key.startswith(('MALLOC_', 'CARGO_PROFILE_', 'CARGO_TARGET_')))


def staged_roster(directory):
    return {str(path.relative_to(directory)): digest(path) for path in sorted(directory.rglob('*'))
            if path.is_file() and not path.is_symlink()}


def prepare_consumer(source, tools, scratch):
    archive = scratch.with_suffix('.source.tar')
    subprocess.run(['git', 'archive', '--format=tar', '--output=' + str(archive), PROFILE['source_commit']],
                   cwd=source, timeout=60, check=True)
    scratch.mkdir(mode=0o700)
    with tarfile.open(archive) as stream:
        members = stream.getmembers()
        need(all(member.isfile() or member.isdir() for member in members), 'regular_source_archive_required')
        need(all(not Path(member.name).is_absolute() and '..' not in Path(member.name).parts for member in members),
             'safe_source_archive_required')
        stream.extractall(scratch, filter='data')
    archive.unlink()
    baseline = staged_roster(scratch)
    manifest = 'lucent/crates/lucent-services/Cargo.toml'
    before = (scratch / manifest).read_bytes()
    extra = ('\n[[example]]\nname = "' + EXAMPLE + '"\npath = "examples/' + EXAMPLE + '/observer.rs"\ntest = true\n').encode()
    (scratch / manifest).write_bytes(before + extra)
    for name in CONSUMER_FILES:
        destination = scratch / 'lucent/crates/lucent-services/examples' / EXAMPLE / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(tools / name, destination)
    expected = dict(baseline)
    expected[manifest] = hashlib.sha256(before + extra).hexdigest()
    expected.update({'lucent/crates/lucent-services/examples/' + EXAMPLE + '/' + name: digest(tools / name)
                     for name in CONSUMER_FILES})
    need(staged_roster(scratch) == expected, 'only_consumer_target_and_sources_may_differ')
    return expected, {'backend_archive_roster_sha256': hashlib.sha256(json.dumps(baseline, sort_keys=True).encode()).hexdigest(),
                      'backend_file_count': len(baseline), 'consumer_manifest_append': extra.decode(),
                      'original_Cargo_lock_sha256': baseline['lucent/Cargo.lock'],
                      'consumer_source_sha256': {name: digest(tools / name) for name in CONSUMER_FILES}}


def probe_frames(value):
    need(type(value) is dict and {'streams', 'frames'}.issubset(value)
         and set(value).issubset({'streams', 'frames', 'programs', 'stream_groups'})
         and not value.get('programs') and not value.get('stream_groups') and len(value['streams']) == 1,
         'one_actual_video_stream_required')
    stream = value['streams'][0]
    need(stream.get('codec_name') == 'h264' and stream.get('width') == 3840 and stream.get('height') == 2160
         and stream.get('pix_fmt') == 'yuv420p' and stream.get('r_frame_rate') == stream.get('avg_frame_rate') == '60/1'
         and stream.get('sample_aspect_ratio') == '1:1' and int(stream.get('nb_frames', 0)) == 240,
         'actual_4K60_opaque_H264_required')
    numerator, denominator = map(int, stream['time_base'].split('/'))
    need(numerator > 0 and denominator > 0 and len(value['frames']) == 240, 'actual_frame_pts_table_required')
    pts = [int(frame['best_effort_timestamp']) for frame in value['frames']]
    need(pts[0] == 0 and all(a < b for a, b in zip(pts, pts[1:])), 'strict_source_presentation_order_required')
    # Match the backend f64 time-base normalization, allowing one ns at the
    # receiving edge for Python/Rust rounding of the same finite source PTS.
    base = numerator / denominator
    nanos = [round((point * base - pts[0] * base) * 1_000_000_000) for point in pts]
    need(nanos[0] == 0 and nanos[-1] < 4_000_000_000, 'four_second_source_pts_required')
    return nanos


def allocator(value):
    need(type(value) is dict and value.get('kind') == 'glibc_mallinfo2' and type(value.get('available')) is bool,
         'fixed_allocator_schema_required')
    if value['available'] is False:
        need(set(value) == {'kind', 'available', 'reason'} and value['reason'] in
             ('unsupported_target', 'invalid_runtime_version', 'unsupported_glibc_version', 'counter_out_of_range'),
             'fixed_allocator_unavailable_required')
        return
    need(set(value) == {'kind', 'available', 'glibc_version', 'bytes', 'counts'}
         and re.fullmatch(r'[0-9]+(?:\.[0-9]+)+', value['glibc_version'])
         and tuple(map(int, value['glibc_version'].split('.')[:2])) >= (2, 33), 'actual_allocator_runtime_required')
    need(set(value['bytes']) == {'arena', 'arena_accounted_in_use', 'arena_free', 'direct_mmap', 'malloc_accounted_in_use_total'}
         and set(value['counts']) == {'arena_free_chunks', 'direct_mmap_regions'}, 'fixed_allocator_units_required')
    need(all(type(v) is int and 0 <= v <= 2**64 - 1 for k, v in value['bytes'].items()
             if k != 'malloc_accounted_in_use_total') and all(type(v) is int and 0 <= v <= 2**64 - 1
             for v in value['counts'].values()), 'unsigned_allocator_counts_required')
    total = value['bytes']['arena_accounted_in_use'] + value['bytes']['direct_mmap']
    observed = value['bytes']['malloc_accounted_in_use_total']
    need((type(observed) is int and observed == total) if total <= 2**64 - 1 else observed is None,
         'checked_allocator_total_required')


def sample(value, stopped):
    need(set(value) == {'moment', 'elapsed_ms', 'memory'} | ({'allocator'} if stopped else set())
         and type(value['elapsed_ms']) is int and 0 <= value['elapsed_ms'] <= 150000
         and set(value['memory']) == {'rss_kib', 'anonymous_kib', 'rss_high_water_kib', 'swap_kib'}
         and all(v is None or type(v) is int and v >= 0 for v in value['memory'].values()),
         'fixed_scoped_memory_sample_required')
    need(value['memory']['swap_kib'] == 0, 'observer_swap_must_remain_zero')
    if stopped:
        allocator(value['allocator'])


def matrix_cases():
    expected = [('known_targets','reopen'),('known_targets','retarget'),
                ('source_and_targets','reopen'),('source_and_targets','retarget')]
    need(PROFILE['case_order'] == [list(row) for row in expected]
         and set(PROFILE['sequences']) == {'known_targets','source_and_targets'},
         'four_explicit_known_and_source_cases_required')
    return expected


def validate_matrix(cases, binary_sha256, input_sha256):
    expected = matrix_cases()
    need(len(cases) == len(expected)
         and [(row['sequence'],row['mode']) for row in cases] == expected,
         'both_complete_sequence_comparisons_required')
    need(all(type(row['pid']) is int and row['pid'] > 1 for row in cases)
         and len({row['pid'] for row in cases}) == 4, 'four_distinct_fresh_processes_required')
    need(all(row['binary_sha256'] == binary_sha256 and row['input_sha256'] == input_sha256 for row in cases),
         'one_identical_compiled_binary_and_encoded_input_required')
    need(all(type(row[key]) is int and row[key] > 0 for row in cases for key in
             ('host_process_started_monotonic_ns','host_process_finished_monotonic_ns'))
         and all(row['host_process_started_monotonic_ns'] < row['host_process_finished_monotonic_ns'] for row in cases)
         and all(a['host_process_finished_monotonic_ns'] < b['host_process_started_monotonic_ns']
                 for a,b in zip(cases,cases[1:])), 'four_process_nonoverlap_required')


def validate_case(value, sequence, mode, pid, expected_pts):
    need(sequence in PROFILE['sequences'] and mode in ('reopen','retarget'), 'known_typed_case_required')
    planned = PROFILE['sequences'][sequence]
    keys = {'kind', 'mode', 'sequence', 'pid', 'phases', 'play_calls', 'exact_worker_joins',
            'stopped_allocator_observations', 'final_latest_capacity_before_drop',
            'consumer_held_pixel_capacity_after_drop', 'renderer_used', 'fps_measured',
            'whole_shell_memory_accepted', 'native_presentation_accepted'}
    need(type(value) is dict and set(value) == keys and value['kind'] == 'full_demand_observation'
         and value['mode'] == mode and value['sequence'] == sequence
         and value['pid'] == pid and type(pid) is int and pid > 1,
         'exact_fresh_process_case_required')
    need(all(value[key] is False for key in ('renderer_used', 'fps_measured', 'whole_shell_memory_accepted', 'native_presentation_accepted'))
         and type(value['consumer_held_pixel_capacity_after_drop']) is int
         and value['consumer_held_pixel_capacity_after_drop'] == 0, 'backend_only_scope_required')
    expected_workers = 1 if mode == 'retarget' else len(planned)
    need(type(value['play_calls']) is int and value['play_calls'] == expected_workers
         and len(value['exact_worker_joins']) == expected_workers,
         'exact_play_and_join_count_required')
    for ordinal, row in enumerate(value['exact_worker_joins'], 1):
        need(set(row) == {'worker_ordinal', 'stop_requested', 'watch_joined', 'cancellation_observed', 'join_elapsed_ms'}
             and type(row['worker_ordinal']) is int and row['worker_ordinal'] == ordinal and all(row[key] is True for key in
                 ('stop_requested', 'watch_joined', 'cancellation_observed'))
             and type(row['join_elapsed_ms']) is int and 0 <= row['join_elapsed_ms'] < 10000,
             'each_exact_worker_must_cancel_and_join_before_replacement')
    need(len(value['phases']) == len(planned), 'exact_sequence_phase_count_required')
    phase_keys = {'phase', 'revision', 'requested_size', 'worker_ordinal', 'target_ready_elapsed_ms',
        'phase_observed_ms', 'phase_request_clock_ms', 'publications', 'first_size', 'last_size', 'rgba_bytes',
        'aspect_size', 'maximum_capacity_bytes', 'latest_capacity_bytes', 'first_pts_ns', 'last_pts_ns',
        'observed_loop_wraps_total', 'stale_publications_total', 'all_admitted_pts_match_ffprobe', 'memory_samples'}
    clocks = []
    for index, (row, (label, target)) in enumerate(zip(value['phases'], planned), 1):
        size = target or [3840, 2160]
        extent = size[0] * size[1] * 4
        need(set(row) == phase_keys and row['phase'] == label and type(row['revision']) is int and row['revision'] == index
             and row['requested_size'] == target and row['first_size'] == row['last_size'] == size
             and row['aspect_size'] == [3840, 2160] and row['rgba_bytes'] == extent,
             'exact_target_revision_frame_bytes_and_aspect_required')
        need(type(row['worker_ordinal']) is int and row['worker_ordinal'] == (1 if mode == 'retarget' else index)
             and type(row['target_ready_elapsed_ms']) is int and 0 <= row['target_ready_elapsed_ms'] < 5000
             and type(row['phase_observed_ms']) is int and 12000 <= row['phase_observed_ms'] < 15000
             and type(row['publications']) is int and row['publications'] > 0
             and all(type(row[key]) is int for key in ('maximum_capacity_bytes','latest_capacity_bytes'))
             and row['maximum_capacity_bytes'] >= row['latest_capacity_bytes'] >= extent,
             'bounded_actual_target_observation_required')
        need(row['all_admitted_pts_match_ffprobe'] is True
             and all(type(row[key]) is int and any(abs(row[key] - pts) <= 1 for pts in expected_pts)
                     for key in ('first_pts_ns', 'last_pts_ns')),
             'actual_normalized_FFmpeg_PTS_required')
        need(all(type(row[key]) is int and 0 <= row[key] <= 2**64-1 for key in
                 ('phase_request_clock_ms', 'observed_loop_wraps_total', 'stale_publications_total')),
             'unsigned_phase_clocks_and_counts_required')
        clocks.append(row['phase_request_clock_ms'])
        points = row['memory_samples']
        need(3 <= len(points) <= 64 and [point['moment'] for point in points] ==
             ['before_target', 'target_observed'] + ['during_target'] * (len(points)-2),
             'bounded_memory_only_per_target_observations_required')
        for point in points:
            sample(point, False)
    need(all(a < b for a, b in zip(clocks, clocks[1:])) and value['phases'][-1]['observed_loop_wraps_total'] > 0,
         'actual_ordered_phase_clock_and_EOF_wrap_required')
    need(all(a['elapsed_ms'] <= b['elapsed_ms'] for a, b in zip(
         [p for row in value['phases'] for p in row['memory_samples']],
         [p for row in value['phases'] for p in row['memory_samples']][1:])),
         'memory_phase_clocks_must_not_regress')
    points = value['stopped_allocator_observations']
    need(len(points) == 3 and [p['moment'] for p in points] ==
         ['before_play', 'after_join_latest_held', 'after_latest_dropped'], 'three_stopped_only_allocator_points_required')
    for point in points:
        sample(point, True)
    need(points[0]['elapsed_ms'] <= value['phases'][0]['memory_samples'][0]['elapsed_ms']
         and points[1]['elapsed_ms'] <= points[2]['elapsed_ms']
         and points[1]['elapsed_ms'] >= value['phases'][-1]['memory_samples'][-1]['elapsed_ms'],
         'stopped_allocator_clock_must_follow_final_active_observation')
    final_size = planned[-1][1] or [3840,2160]
    need(type(value['final_latest_capacity_before_drop']) is int
         and value['final_latest_capacity_before_drop'] >= final_size[0]*final_size[1]*4,
         'final_sequence_storage_required')
    return value


def run(args):
    provenance = context(os.environ)
    source, work, envelope = args.source, args.work, args.envelope
    exact_source(source)
    tools_repo = HERE.parents[1]
    need(output(['git', 'rev-parse', 'HEAD'], tools_repo) == provenance['GITHUB_SHA'], 'exact_tools_checkout_required')
    need(not forbidden_environment(os.environ), 'unreviewed_build_or_allocator_overrides_forbidden')
    need(all(p.is_absolute() and p.parent.resolve(strict=True) == p.parent and not p.exists() for p in (work, envelope))
         and not any(p.is_relative_to(source) or p.is_relative_to(tools_repo) for p in (work, envelope))
         and not work.is_relative_to(envelope) and not envelope.is_relative_to(work), 'fresh_separate_experiment_paths_required')
    tools_paths = [HERE/name for name in (*CONSUMER_FILES, 'run.py', 'profile.json', 'tests.py', 'README.md')]
    tools_paths += [tools_repo/'.github/workflows'/name for name in ('full-demand-observer.yml','check.yml')]
    for path in tools_paths:
        committed = subprocess.check_output(['git', 'show', provenance['GITHUB_SHA'] + ':' +
            str(path.relative_to(tools_repo))], cwd=tools_repo, timeout=30)
        need(hashlib.sha256(committed).hexdigest() == digest(path), 'committed_tools_bytes_required')
    need(shutil.disk_usage(work.parent).free >= 8*1024**3, 'fresh_remote_disk_capacity_required')
    admissions = {'before_build': admit()}
    work.mkdir(mode=0o700)
    package = work / 'package'
    package.mkdir(mode=0o700)
    logs = package / 'logs'
    logs.mkdir()
    for path in tools_paths:
        destination = package/'tools'/path.relative_to(tools_repo)
        destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(path,destination)
    scratch, target_dir = work / 'consumer-source', work / 'target'
    roster, derivation = prepare_consumer(source, HERE, scratch)
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(target_dir), CARGO_BUILD_JOBS='3', CARGO_INCREMENTAL='0',
               CARGO_TERM_COLOR='never', LC_ALL='C')
    version = output(['rustc', '--version', '--verbose'], source, env)
    need('release: '+PROFILE['rust_release']+'\n' in version+'\n'
         and 'commit-hash: '+PROFILE['rust_commit']+'\n' in version+'\n', 'exact_Rust_toolchain_required')
    record = {'source_commit': PROFILE['source_commit'], 'source_tree': PROFILE['source_tree'],
              'consumer_tools_commit': provenance['GITHUB_SHA'], 'provenance': provenance,
              'backend_derivation': derivation, 'steps': [], 'admissions': admissions,
              'source_qualification': PROFILE['source_qualification'],
              'tools_file_sha256': staged_roster(package/'tools'),
              'scope': PROFILE['scope'], 'rustc_verbose': version, 'passed': False, **SCOPE_FALSE}

    def save():
        (package / 'experiment.json').write_text(json.dumps(record, indent=2)+'\n')

    def stage(name, argv, expected=None, names=(), timeout=2400):
        exact_source(source)
        need(staged_roster(scratch) == roster, 'backend_and_consumer_source_must_stay_exact')
        path = logs / (name+'.log')
        started = time.monotonic()
        save()
        with path.open('x') as stream:
            code = subprocess.run(argv, cwd=scratch, env=env, stdout=stream,
                                  stderr=subprocess.STDOUT, timeout=timeout).returncode
        step = {'name': name, 'exit': code, 'elapsed_seconds': time.monotonic()-started,
                'log_sha256': digest(path), 'log_bytes': path.stat().st_size}
        need(path.stat().st_size <= 16*1024**2, 'bounded_stage_log_required')
        if code == 0 and expected is not None:
            step.update(test_summary(path.read_text(), expected, names))
        record['steps'].append(step)
        save()
        need(code == 0, 'gate_failed_'+name)
        exact_source(source)
        need(staged_roster(scratch) == roster, 'backend_and_consumer_source_must_stay_exact')

    manifest = ['--manifest-path', str(scratch/'lucent/Cargo.toml')]
    release = [*manifest, '--locked', '--offline', '--release']
    stage('fetch', ['cargo', 'fetch', *manifest, '--locked'], timeout=600)
    stage('fmt', ['cargo', 'fmt', *manifest, '--all', '--check'])
    stage('clippy', ['cargo', 'clippy', *release, '-p', 'lucent-domain', '-p', 'lucent-services', '--all-targets', '--', '-D', 'warnings'])
    for name, selection in [('domain', ['-p','lucent-domain','--lib']),
                            ('video', ['-p','lucent-services','--lib','video::']),
                            ('observer', ['-p','lucent-services','--example','video-allocation-observer']),
                            ('consumer', ['-p','lucent-services','--example',EXAMPLE])]:
        names = PROFILE['consumer_test_names'] if name == 'consumer' else PROFILE['required_test_names'][name]
        if name == 'consumer':
            names = ['observer_tests::'+value for value in names]
        stage(name+'-tests', ['cargo','test',*release,*selection,'--','--nocapture','--test-threads=1'],
              PROFILE['test_counts'][name], names)
    stage('single-observer-release', ['cargo','build',*release,'-p','lucent-services','--example',EXAMPLE])
    binary = package / EXAMPLE
    shutil.copy2(target_dir/'release/examples'/EXAMPLE, binary)
    binary.chmod(0o500)
    record['binary'] = {'path': binary.name, 'sha256': digest(binary), 'bytes': binary.stat().st_size}
    stage('ELF', ['readelf','--dynamic','--version-info','--dyn-syms','--wide',str(binary)], timeout=30)
    stage('FFmpeg-version', ['ffmpeg','-version'], timeout=30)
    stage('encoder-help', ['ffmpeg','-hide_banner','-h','encoder=libx264'], timeout=30)
    dependency_source = args.dependencies
    dependency_names = ('download.log','install.log','resolved-urls.txt','installed-packages.txt',
                        'databases.sha256','archives.sha256','bootstrap-admission.txt','signature-policy.txt')
    need(all((dependency_source/name).is_file() and not (dependency_source/name).is_symlink()
             and (dependency_source/name).stat().st_size <= 16*1024**2
             for name in dependency_names), 'frozen_signed_transaction_evidence_required')
    databases = sorted((dependency_source/'databases').iterdir())
    need(0 < len(databases) <= 8 and all(path.is_file() and not path.is_symlink()
         and path.suffix == '.db' and path.stat().st_size <= 32*1024**2 for path in databases),
         'bounded_frozen_package_databases_required')
    need(output(['pacman','-Q']) == (dependency_source/'installed-packages.txt').read_text().strip(), 'unchanged_signed_package_transaction_required')
    dependencies = package/'dependencies'
    dependencies.mkdir()
    for name in dependency_names:
        shutil.copy2(dependency_source/name, dependencies/name)
    shutil.copytree(dependency_source/'databases', dependencies/'databases')
    record['dependency_evidence'] = staged_roster(dependencies)
    record['FFmpeg_public_headers'] = {}
    for directory in ('libavcodec','libavformat','libavutil','libswscale'):
        for header in sorted((Path('/usr/include')/directory).glob('*.h')):
            need(header.is_file() and not header.is_symlink() and header.stat().st_size <= 2*1024**2,
                 'regular_FFmpeg_headers_required')
            destination = package/'abi/include'/directory/header.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(header,destination)
            record['FFmpeg_public_headers'][str(header.relative_to('/usr/include'))] = digest(header)
    need(0 < len(record['FFmpeg_public_headers']) <= 512, 'actual_FFmpeg_header_roster_required')
    media = package/'state/media-generated'
    media.mkdir(parents=True)
    recipe = ['ffmpeg','-nostdin','-hide_banner','-loglevel','info','-filter_threads','1','-f','lavfi',
              '-i','testsrc2=size=3840x2160:rate=60','-frames:v','240','-an','-c:v','libx264',
              '-preset','veryfast','-crf','28','-pix_fmt','yuv420p','-g','120','-keyint_min','120',
              '-sc_threshold','0','-bf','3','-refs','3','-threads:v','2',str(media/'source.mp4')]
    admissions['before_encode'] = admit()
    stage('encode-generated-input', recipe, timeout=600)
    stage('poster', ['ffmpeg','-nostdin','-hide_banner','-loglevel','error','-threads','1','-i',str(media/'source.mp4'),
                     '-frames:v','1','-vf','scale=320:180','-threads','1',str(media/'poster.png')], timeout=60)
    probe = json.loads(output(['ffprobe','-v','error','-select_streams','v:0','-show_entries',
        'stream=codec_name,width,height,pix_fmt,r_frame_rate,avg_frame_rate,sample_aspect_ratio,nb_frames,time_base:frame=best_effort_timestamp',
        '-of','json',str(media/'source.mp4')], timeout=60))
    pts = probe_frames(probe)
    (package/'ffprobe.json').write_text(json.dumps(probe,indent=2)+'\n')
    record['input'] = {'path':'state/media-generated/source.mp4','sha256':digest(media/'source.mp4'),
                       'bytes':(media/'source.mp4').stat().st_size,'recipe':recipe[:-1]+['<generated-source.mp4>'],
                       'recipe_scope':'Reproducible recipe, not a promise of byte-identical encoding across different FFmpeg/x264 builds.',
                       'normalized_pts_ns':pts,'ffprobe_sha256':digest(package/'ffprobe.json')}
    need(0 < record['input']['bytes'] <= 256*1024**2, 'bounded_actual_generated_input_required')
    for path in media.iterdir():
        path.chmod(0o400)
    cases = []
    for sequence,mode in matrix_cases():
        name = sequence+'-'+mode
        admissions['before_'+name] = admit()
        need(digest(binary) == record['binary']['sha256'] and digest(media/'source.mp4') == record['input']['sha256'],
             'same_compiled_binary_and_input_required')
        request = {'state':str(package/'state'),'asset':{'path':str(media/'source.mp4'),
            'source':{'name':'Generated 4K60','path':str(media/'source.mp4')},
            'poster':{'name':'Generated poster','path':str(media/'poster.png')}},
            'mode':mode,'sequence':sequence,'phase_seconds':12,'expected_pts_ns':pts}
        stderr = logs/(name+'-native-stderr.log')
        started = time.monotonic_ns()
        with stderr.open('xb') as errors:
            process = subprocess.Popen([str(binary),'--run'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=errors,env=env)
            try:
                raw,_ = process.communicate(json.dumps(request).encode(),timeout=150)
            except BaseException:
                process.kill()
                process.wait(timeout=10)
                raise
        need(process.returncode == 0 and 0 < len(raw) <= 512*1024, 'complete_bounded_observer_process_required')
        need(stderr.stat().st_size <= 16*1024**2, 'bounded_native_stderr_required')
        value = validate_case(json.loads(raw),sequence,mode,process.pid,pts)
        need(all(process.pid != prior['pid'] for prior in cases), 'four_fresh_observer_processes_required')
        value.update({'host_process_started_monotonic_ns':started,'host_process_finished_monotonic_ns':time.monotonic_ns(),
                      'binary_sha256':digest(binary),'input_sha256':digest(media/'source.mp4'),
                      'stderr_sha256':digest(stderr),'stderr_bytes':stderr.stat().st_size})
        (package/(name+'.json')).write_text(json.dumps(value,indent=2)+'\n')
        cases.append(value)
        save()
    validate_matrix(cases,record['binary']['sha256'],record['input']['sha256'])
    need(staged_roster(scratch) == roster, 'final_backend_source_changed')
    exact_source(source)
    need(digest(binary) == record['binary']['sha256'] and digest(media/'source.mp4') == record['input']['sha256'],
         'final_exact_binary_and_input_binding_required')
    need(all(digest(Path('/usr/include')/name) == sha for name,sha in record['FFmpeg_public_headers'].items()),
         'FFmpeg_build_and_experiment_headers_must_stay_exact')
    need(output(['pacman','-Q']) == (dependency_source/'installed-packages.txt').read_text().strip(), 'final_packages_changed')
    record['final_budget'] = resource_snapshot()
    record['cases'] = [{'mode':row['mode'],'sequence':row['sequence'],'pid':row['pid'],
                        'path':row['sequence']+'-'+row['mode']+'.json',
                        'sha256':digest(package/(row['sequence']+'-'+row['mode']+'.json'))} for row in cases]
    record['passed'] = True
    record['mechanism_scope'] = 'Two sequences in four finite fresh-process backend observations; publication counts are not presentation FPS. No claimed memory improvement threshold.'
    save()
    envelope.mkdir(mode=0o700)
    archive = envelope/'observer-experiment.tar'
    with tarfile.open(archive,'x') as stream:
        for path in sorted(package.rglob('*')):
            need(not path.is_symlink() and (path.is_file() or path.is_dir()), 'regular_portable_archive_required')
            info = stream.gettarinfo(str(path),str(path.relative_to(package)))
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            if path.is_file():
                with path.open('rb') as content:
                    stream.addfile(info,content)
            else:
                stream.addfile(info)
    subprocess.run(['zstd','-3','-T3','--no-progress','--rm',str(archive)],timeout=180,check=True)
    compressed = archive.with_suffix('.tar.zst')
    receipt = {'source_commit':PROFILE['source_commit'],'consumer_tools_commit':provenance['GITHUB_SHA'],
               'archive':compressed.name,'archive_sha256':digest(compressed),
               'experiment_sha256':digest(package/'experiment.json'),'binary_sha256':record['binary']['sha256'],
               'input_sha256':record['input']['sha256'],'provenance':provenance,**SCOPE_FALSE}
    (envelope/'artifact-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(receipt))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run',action='store_true')
    for name in ('source','work','envelope','dependencies'):
        parser.add_argument('--'+name,type=Path)
    args = parser.parse_args(argv)
    if not args.run:
        print(json.dumps({'prepared_only':True,'native_actions':False,'source_commit':PROFILE['source_commit'],
                          'planned_source_tests':PROFILE['test_counts'],'artifact_bound':False,**SCOPE_FALSE}))
        return 0
    need(all(getattr(args,name) is not None for name in ('source','work','envelope','dependencies')),
         'explicit_hosted_paths_required')
    run(args)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
