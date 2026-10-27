#!/usr/bin/env python3
"""Inert unless --run: collect metadata from three existing strict Arch fixtures.

Expected fixture failures remain failures in the report. This is diagnosis,
not release qualification, a policy change or native/performance acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

PROFILE_PATH = Path(__file__).with_name('native-admission-profile.json')
PROFILE = json.loads(PROFILE_PATH.read_text())
PREFIX = 'LUCENT_NATIVE_ADMISSION_META '
REQUIRED = set(('schema phase rejecting_guard dimensions pixel_format color_space primaries range transfer chroma '
                'raw_sar mapped_sar raw_flags mapped_flags interlaced corrupt decode_error_flags crop strides '
                'buffer_zero_present side_data_count side_data_kind_ids side_data_inventory_valid '
                'side_data_null_entries blocked_side_data checks').split())
CHECKS = set('yuv420p progressive square_sar bt709_matrix bt709_primaries known_range known_transfer known_chroma'.split())
BLOCKED = set('icc_profile mastering_display content_light display_matrix'.split())
GUARDS = {'format-interlace-sar', 'color', 'blocked-side-data', 'crop-errors-buffer', 'nonpositive-stride', 'plane-owner'}


def need(value, reason):
    if not value:
        raise RuntimeError(reason)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def output(argv, cwd=None, env=None):
    return subprocess.check_output(argv, cwd=cwd, env=env, text=True, timeout=30).strip()


def exact_source(source):
    need(source.is_absolute() and source.resolve(strict=True) == source, 'canonical_source_required')
    need(output(['git', 'rev-parse', 'HEAD'], source) == PROFILE['source_commit']
         and output(['git', 'rev-parse', 'HEAD^{tree}'], source) == PROFILE['source_tree']
         and output(['git', 'status', '--porcelain', '--untracked-files=all'], source) == '', 'exact_clean_source_required')


def admit(log, phase):
    for index in range(3):
        group = Path('/sys/fs/cgroup')
        need((group/'cpu.max').read_text().strip() == '300000 100000'
             and (group/'memory.max').read_text().strip() == '10737418240'
             and (group/'memory.swap.max').read_text().strip() == '0', 'exact_remote_budget_required')
        current = int((group/'memory.current').read_text())
        available = next(int(line.split()[1])//1024 for line in Path('/proc/meminfo').read_text().splitlines()
                         if line.startswith('MemAvailable:'))
        effective = min(available, max(0, (10737418240-current)//1024**2))
        with log.open('a') as stream:
            stream.write(json.dumps(dict(phase=phase, index=index, effective_mib=effective,
                                         cgroup_current_bytes=current))+'\n')
            stream.flush(); os.fsync(stream.fileno())
        need(effective >= 4500, 'fresh_remote_4500MiB_required')
        time.sleep(1)


def metadata(text):
    rows = []
    for line in text.splitlines():
        at = line.find(PREFIX)
        if at < 0:
            continue
        payload = line[at+len(PREFIX):]
        need(len(payload) <= 16384, 'bounded_metadata_row_required')
        row = json.loads(payload)
        need(type(row) is dict and set(row) == REQUIRED and row['schema'] == 1, 'complete_metadata_schema_required')
        need(row['phase'] in ('decoded-first-frame', 'admission-rejected', 'admission-accepted'), 'fixed_metadata_phase_required')
        need((row['rejecting_guard'] in GUARDS if row['phase'] == 'admission-rejected'
              else row['rejecting_guard'] is None), 'actual_guard_event_required')
        for name in ('pixel_format', 'color_space', 'primaries', 'range', 'transfer', 'chroma'):
            value = row[name]
            need(type(value) is dict and set(value) == {'raw', 'mapped'} and type(value['raw']) is int
                 and type(value['mapped']) is str and len(value['mapped']) <= 64, 'raw_and_mapped_enums_required')
        for name, length in [('dimensions', 2), ('raw_sar', 2), ('mapped_sar', 2), ('crop', 4), ('strides', 3)]:
            need(type(row[name]) is list and len(row[name]) == length
                 and all(type(value) is int for value in row[name]), 'fixed_numeric_metadata_required')
        need(type(row['side_data_kind_ids']) is list and len(row['side_data_kind_ids']) <= 64
             and all(type(value) is int for value in row['side_data_kind_ids']), 'bounded_side_data_kind_ids_required')
        need(type(row['checks']) is dict and set(row['checks']) == CHECKS
             and type(row['blocked_side_data']) is dict and set(row['blocked_side_data']) == BLOCKED
             and all(type(value) is bool for value in [*row['checks'].values(), *row['blocked_side_data'].values()]),
             'boolean_guard_observations_required')
        need(all(type(row[name]) is int for name in ('raw_flags', 'mapped_flags', 'decode_error_flags',
             'side_data_count', 'side_data_null_entries')) and all(type(row[name]) is bool for name in
             ('interlaced', 'corrupt', 'buffer_zero_present', 'side_data_inventory_valid')), 'fixed_scalar_metadata_required')
        rows.append(row)
    need(len(rows) <= 128, 'bounded_metadata_observations_required')
    return rows


def fixture_result(name, code, text):
    summaries = re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', text)
    need(len(summaries) == 1 and code in (0, 101), 'one_actual_fixture_outcome_required')
    result, passed, failed, ignored, measured, _ = summaries[0]
    need((result, int(passed), int(failed), int(ignored), int(measured)) ==
         (('ok', 1, 0, 0, 0) if code == 0 else ('FAILED', 0, 1, 0, 0)), 'strict_fixture_outcome_required')
    need(re.search(r'^test '+re.escape(name)+r' \.\.\.', text, re.M), 'exact_existing_test_required')
    rows = metadata(text)
    need(any(row['phase'] == 'decoded-first-frame' for row in rows)
         and any(row['phase'] in ('admission-rejected', 'admission-accepted') for row in rows),
         'actual_decoded_frame_and_admission_event_required')
    return dict(test=name, exit=code, test_passed=code == 0, metadata=rows)


def run(args):
    need(os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('GITHUB_REPOSITORY') == PROFILE['repository']
         and os.environ.get('GITHUB_ACTOR') == os.environ.get('GITHUB_TRIGGERING_ACTOR') == PROFILE['actor']
         and os.environ.get('GITHUB_EVENT_NAME') == 'push'
         and os.environ.get('GITHUB_REF') == 'refs/heads/'+PROFILE['branch'], 'reviewed_owner_workflow_only')
    head = os.environ.get('GITHUB_SHA', '')
    need(re.fullmatch('[0-9a-f]{40}', head) and head == os.environ.get('LUCENT_WORKFLOW_SHA'), 'exact_workflow_identity_required')
    tools = Path(__file__).resolve().parents[1]
    need(output(['git', 'rev-parse', 'HEAD'], tools) == head, 'exact_tools_checkout_required')
    for path in (Path(__file__), PROFILE_PATH):
        raw = subprocess.check_output(['git', 'show', head+':'+path.relative_to(tools).as_posix()], cwd=tools, timeout=30)
        need(hashlib.sha256(raw).hexdigest() == sha(path), 'committed_tools_required')
    source, target, out = args.source_directory, args.target_directory, args.output
    exact_source(source)
    temporary = Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
    need(target == temporary/'native-admission-target' and out == temporary/'native-admission-diagnostics'
         and not os.path.lexists(target) and not os.path.lexists(out), 'fresh_exact_private_paths_required')
    need(not any(name in os.environ for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
         'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_TARGET')) and not any(
         name.startswith(('CARGO_PROFILE_', 'CARGO_TARGET_')) and name != 'CARGO_TARGET_DIR' for name in os.environ),
         'unreviewed_Cargo_overrides_forbidden')
    out.mkdir(mode=0o700); target.mkdir(mode=0o700)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS='3', CARGO_INCREMENTAL='0', CARGO_TERM_COLOR='never', LC_ALL='C')
    version = output(['rustc', '--version', '--verbose'], source, env)
    need('release: 1.99.0\n' in version+'\n' and 'commit-hash: b940084d7eb6a299eb4bfeb8e34901bc051e7ac4\n' in version+'\n', 'pinned_toolchain_required')
    installed = output(['pacman', '-Q'])
    need(installed == (args.dependencies/'installed-packages.txt').read_text().strip(), 'frozen_signed_packages_required')
    record = dict(mode='DIAGNOSTIC_NOT_RELEASE_QUALIFICATION', source_commit=PROFILE['source_commit'],
                  source_tree=PROFILE['source_tree'], workflow_head=head, run_id=os.environ['GITHUB_RUN_ID'],
                  run_attempt=os.environ['GITHUB_RUN_ATTEMPT'], rustc_verbose=version,
                  ffmpeg_package=output(['pacman', '-Q', 'ffmpeg']), cases=[], collected=False,
                  native_accepted=False, production_policy_changed=False, performance_accepted=False)
    def save(): (out/'report.json').write_text(json.dumps(record, indent=2)+'\n')
    save(); admit(out/'admission.jsonl', 'before-compile')
    with (out/'fetch.log').open('x') as stream:
        subprocess.run(['cargo', 'fetch', '--manifest-path', 'lucent/Cargo.toml', '--locked'], cwd=source, env=env,
                       stdout=stream, stderr=subprocess.STDOUT, timeout=600, check=True)
    for index, name in enumerate([PROFILE['schema_test'], *PROFILE['fixtures']]):
        if index: admit(out/'admission.jsonl', 'before-fixture-'+str(index))
        log = out/f'case-{index}.log'
        with log.open('x') as stream:
            code = subprocess.run(['cargo', 'test', '--manifest-path', 'lucent/Cargo.toml', '--offline', '--locked',
                '--release', '-p', 'lucent-services', '--lib', name, '--', '--exact', '--nocapture', '--test-threads=1'],
                cwd=source, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=1200 if index == 0 else 180).returncode
        need(log.stat().st_size <= 16*1024**2, 'bounded_case_log_required')
        text = log.read_text()
        if index == 0:
            need(code == 0 and re.search(r'test result: ok\. 1 passed; 0 failed;', text), 'metadata_schema_unit_test_required')
            record['metadata_schema_test'] = dict(test=name, exit=0, log_sha256=sha(log))
        else:
            row = fixture_result(name, code, text); row.update(log=log.name, log_sha256=sha(log)); record['cases'].append(row)
        exact_source(source); save()
    need(output(['pacman', '-Q']) == installed, 'packages_must_not_change_during_diagnostic')
    record.update(collected=True, existing_regressions_all_passed=all(row['test_passed'] for row in record['cases']))
    save(); print(json.dumps(dict(collected=True, mode=record['mode'], existing_regressions_all_passed=record['existing_regressions_all_passed'],
                                 exits=[row['exit'] for row in record['cases']], native_accepted=False)))


def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--run', action='store_true')
    for option in ('source-directory', 'target-directory', 'output', 'dependencies'): parser.add_argument('--'+option, type=Path)
    args = parser.parse_args()
    if not args.run:
        print(json.dumps(dict(prepared_only=True, actions=False, source_commit=PROFILE['source_commit'], fixtures=len(PROFILE['fixtures'])))); return 0
    need(all(getattr(args, name) is not None for name in ('source_directory', 'target_directory', 'output', 'dependencies')), 'explicit_hosted_paths_required')
    run(args); return 0

if __name__ == '__main__':
    raise SystemExit(main())
