#!/usr/bin/env python3
"""Inert by default: qualify one source-pinned hosted Arch release archive.

This does not install a desktop, contact a VM, authenticate a session or claim
runtime ABI/native/performance acceptance. Local frozen build helpers are not
used or weakened. All archive paths are portable, relative to its root.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import time

PROFILE_PATH = Path(__file__).with_name('arch-release-profile.json')
PROFILE = json.loads(PROFILE_PATH.read_text())
MANIFEST = ['--manifest-path', 'lucent/Cargo.toml']
RELEASE = [*MANIFEST, '--offline', '--locked', '--release']


def require(value, reason):
    if not value:
        raise RuntimeError(reason)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def output(argv, cwd=None, env=None, timeout=30):
    return subprocess.check_output(argv, cwd=cwd, env=env, text=True, timeout=timeout).strip()


def exact_source(source):
    require(source.is_absolute() and source.resolve(strict=True) == source, 'canonical_source_directory_required')
    require(output(['git', 'rev-parse', 'HEAD'], source) == PROFILE['source_commit'], 'reviewed_source_SHA_required')
    require(output(['git', 'rev-parse', 'HEAD^{tree}'], source) == PROFILE['source_tree'], 'reviewed_source_tree_required')
    require(output(['git', 'status', '--porcelain', '--untracked-files=all'], source) == '', 'source_worktree_must_stay_clean')


def hosted_context(environment):
    require(environment.get('GITHUB_ACTIONS') == 'true'
            and environment.get('GITHUB_REPOSITORY') == PROFILE['repository']
            and environment.get('GITHUB_ACTOR') == PROFILE['actor']
            and environment.get('GITHUB_TRIGGERING_ACTOR') == PROFILE['actor']
            and environment.get('GITHUB_EVENT_NAME') == 'push'
            and environment.get('GITHUB_REF') == 'refs/heads/' + PROFILE['branch'],
            'reviewed_owner_push_only')
    require(environment.get('LUCENT_CI_IMAGE') == PROFILE['container_image'], 'pinned_Arch_image_required')
    for key in ('GITHUB_SHA', 'LUCENT_WORKFLOW_SHA'):
        require(re.fullmatch('[0-9a-f]{40}', environment.get(key, '')), 'exact_workflow_SHA_required')
    for key in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
        require(re.fullmatch('[1-9][0-9]{0,19}', environment.get(key, '')), 'actual_run_identity_required')
    return {key: environment[key] for key in ('GITHUB_REPOSITORY', 'GITHUB_ACTOR', 'GITHUB_TRIGGERING_ACTOR',
            'GITHUB_EVENT_NAME', 'GITHUB_REF', 'GITHUB_SHA', 'LUCENT_WORKFLOW_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT')}


def resource_snapshot(cgroup=Path('/sys/fs/cgroup'), meminfo=Path('/proc/meminfo')):
    cpu = (cgroup / 'cpu.max').read_text().strip().split()
    require(len(cpu) == 2 and all(value.isdecimal() for value in cpu)
            and int(cpu[1]) > 0 and int(cpu[0]) == PROFILE['cpu_limit'] * int(cpu[1]), 'actual_remote_CPU_cgroup_required')
    maximum = (cgroup / 'memory.max').read_text().strip()
    current = (cgroup / 'memory.current').read_text().strip()
    swap = (cgroup / 'memory.swap.max').read_text().strip()
    require(maximum.isdecimal() and current.isdecimal() and int(maximum) == PROFILE['memory_max_bytes']
            and 0 <= int(current), 'actual_remote_memory_cgroup_required')
    require(swap == '0', 'remote_swap_must_not_expand_memory_budget')
    memory_stat = dict(line.split() for line in (cgroup / 'memory.stat').read_text().splitlines())
    require(all(memory_stat.get(name, '').isdecimal() for name in ('anon', 'file')),
            'actual_remote_anon_and_file_memory_required')
    available = next(int(line.split()[1]) // 1024 for line in meminfo.read_text().splitlines()
                     if line.startswith('MemAvailable:'))
    effective = min(available, max(0, (int(maximum) - int(current)) // 1024**2))
    return {'cpu_max': ' '.join(cpu), 'memory_max_bytes': int(maximum), 'memory_current_bytes': int(current),
            'memory_swap_max_bytes': 0, 'host_available_mib': available, 'effective_available_mib': effective,
            'memory_stat_anon_bytes': int(memory_stat['anon']), 'memory_stat_file_bytes': int(memory_stat['file']),
            'build_location': 'github-hosted-Arch-container', 'local_host_policy_changed': False}


def test_summary(text, expected, required=(), full=False):
    summaries = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', text)
    require(len(summaries) == 1 and int(summaries[0][0]) == expected
            and int(summaries[0][1]) == 0 and (not full or int(summaries[0][4]) == 0),
            'exact_unambiguous_actual_test_count_required')
    # Only individually successful tests qualify; a listed/ignored/failed name
    # cannot satisfy a required source regression.
    seen = set()
    starts = list(re.finditer(r'^test ([A-Za-z0-9_:]+) \.\.\.([^\n]*)', text, re.M))
    for index, match in enumerate(starts):
        end = starts[index + 1].start() if index + 1 < len(starts) else len(text)
        block = match[2] + text[match.end():end]
        # With test-threads=1, nocapture output may sit between the name and
        # its standalone final ok (e.g. a deliberately caught panic).
        if re.search(r'^\s*ok\s*$', block, re.M):
            require(match[1] not in seen, 'duplicate_successful_test_names_forbidden')
            seen.add(match[1])
    require(required and len(required) == len(set(required)) == expected
            and set(required) == seen, 'exact_required_named_tests_must_actually_pass')
    return {'tests_passed': expected, 'test_counts': dict(zip(
        ('passed', 'failed', 'ignored', 'measured', 'filtered_out'), map(int, summaries[0]))),
        'required_test_names_verified': list(required)}


def workspace_inventory(metadata, source):
    local = [p for p in metadata['packages'] if Path(p['manifest_path']).is_relative_to(source / 'lucent')]
    require({p['id'] for p in local} == set(metadata['workspace_members']), 'complete_current_workspace_members_required')
    actual = {p['name']: sorted(t['name'] for t in p['targets'] if 'custom-build' not in t['kind']) for p in local}
    require(len(actual) == len(local) and actual == PROFILE['expected_workspace_package_targets'],
            'exact_compiled_workspace_package_targets_required')
    return sorted(actual), sorted({name for names in actual.values() for name in names})


def completed_stage_inventory(rows):
    counts = PROFILE['expected_test_counts']
    require(len(PROFILE['expected_stage_order']) == len(set(PROFILE['expected_stage_order'])) == 21
            and sum(counts.values()) == PROFILE['expected_targeted_test_total'] == 952
            and [row['name'] for row in rows] == PROFILE['expected_stage_order']
            and all(row['exit'] == 0 for row in rows)
            and all(row.get('tests_passed') == counts[row['name']] for row in rows if row['name'] in counts)
            and sum(row.get('tests_passed', 0) for row in rows) == 952,
            'exact21_stages_and952_actual_tests_required')


def completed_memory_rows(path):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    expected = [('initial-admission', i) for i in range(3)] + [
        ('target-cleanup-before', 0), ('target-cleanup-after', 0)] + [
        ('component-lab-admission', i) for i in range(3)]
    require(len(rows) == PROFILE['expected_memory_rows'] == 8
            and [(row['phase'], row['sample_index']) for row in rows] == expected,
            'exact_eight_actual_memory_observations_required')
    return len(rows)


def release_freshness(target, packages, targets):
    stems = {name for target_name in targets for name in (target_name, target_name.replace('-', '_'))}
    directories = ('', 'deps', 'examples', '.fingerprint', 'build', 'incremental')
    residue = []
    for directory in directories:
        parent = target / 'release' / directory
        if not parent.exists():
            continue
        for path in parent.iterdir():
            if directory in ('.fingerprint', 'build'):
                local = any(re.fullmatch(re.escape(name) + '-[0-9a-f]+', path.name) for name in packages)
            else:
                local = any(path.name == prefix or path.name.startswith((prefix + '-', prefix + '.'))
                            for stem in stems for prefix in (stem, 'lib' + stem))
            if local and (path.is_symlink() or not path.is_dir() or any(
                    child.is_symlink() or not child.is_dir() for child in path.rglob('*'))):
                residue.append(str(path.relative_to(target)))
    require(not residue, 'local_release_residue_after_clean')
    return {'release_local_artifacts_absent': True, 'local_packages': sorted(packages),
            'local_targets': sorted(targets), 'checked_directories': list(directories)}


def dependency_evidence(directory):
    require(directory.is_absolute() and directory.resolve(strict=True) == directory, 'dependency_evidence_directory_required')
    names = ('download.log', 'install.log', 'resolved-urls.txt', 'installed-packages.txt',
             'databases.sha256', 'archives.sha256', 'bootstrap-admission.txt', 'signature-policy.txt')
    require(all((directory / name).is_file() and not (directory / name).is_symlink() for name in names),
            'actual_dependency_transaction_evidence_required')
    files = {name: {'bytes': (directory / name).stat().st_size, 'sha256': digest(directory / name)} for name in names}
    require(all(row['bytes'] <= 16 * 1024**2 for row in files.values()), 'bounded_dependency_evidence_required')
    databases = sorted((directory / 'databases').glob('*.db'))
    require(0 < len(databases) <= 8, 'frozen_package_databases_required')
    files.update({'databases/' + path.name: {'bytes': path.stat().st_size, 'sha256': digest(path)} for path in databases})
    return {'files': files, 'scope': 'One downloaded-and-installed signed Arch transaction with frozen DB hashes and package URLs/archive hashes; target package equality is not required. Runtime ABI is a separate gate.'}


def ffmpeg_headers(include=Path('/usr/include')):
    rows = {}
    for directory in ('libavcodec', 'libavformat', 'libavutil', 'libswscale'):
        for path in sorted((include / directory).glob('*.h')):
            require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 2 * 1024**2,
                    'regular_FFmpeg_public_header_required')
            rows[str(path.relative_to(include))] = digest(path)
    require(0 < len(rows) <= 512, 'actual_FFmpeg_public_headers_required')
    return rows


def admit_memory(observe=None):
    samples = []
    for index in range(3):
        sample = resource_snapshot()
        if observe is not None:
            observe(index, sample)
        require(sample['effective_available_mib'] >= PROFILE['admission_floor_mib'],
                'fresh_remote_4500MiB_admission_required')
        samples.append(sample['effective_available_mib'])
        time.sleep(1)
    return samples


def log_memory_sample(path, phase, index, sample):
    with path.open('a') as stream:
        stream.write(json.dumps({'phase': phase, 'sample_index': index, 'snapshot': sample}) + '\n')
        stream.flush()
        os.fsync(stream.fileno())


class PrivateCargoTarget:
    """Authority for only this job's newly created, exact private Cargo target."""
    def __init__(self, target, runner_temp, protected):
        require(shutil.rmtree.avoids_symlink_attacks and target.is_absolute()
                and runner_temp.is_absolute() and runner_temp.resolve(strict=True) == runner_temp
                and target == runner_temp / 'lucent-target-video-focus'
                and not os.path.lexists(target)
                and all(not target.is_relative_to(path) and not path.is_relative_to(target) for path in protected),
                'fresh_exact_private_Cargo_target_required')
        self.path = target
        self.parent_fd = os.open(runner_temp, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            self.parent_identity = self.identity(os.fstat(self.parent_fd))
            os.mkdir(target.name, mode=0o700, dir_fd=self.parent_fd)
            created = os.stat(target.name, dir_fd=self.parent_fd, follow_symlinks=False)
            require(stat.S_ISDIR(created.st_mode) and created.st_uid == os.getuid()
                    and stat.S_IMODE(created.st_mode) == 0o700, 'owned_private_Cargo_target_required')
            self.created_identity = self.identity(created)
        except BaseException:
            os.close(self.parent_fd)
            raise
        self.removed = False

    @staticmethod
    def identity(value):
        return (value.st_dev, value.st_ino, value.st_uid, value.st_mode)

    def remove(self):
        require(not self.removed and self.path.resolve(strict=True) == self.path
                and self.identity(os.fstat(self.parent_fd)) == self.parent_identity
                and self.identity(self.path.parent.stat()) == self.parent_identity
                and self.identity(os.stat(self.path.name, dir_fd=self.parent_fd, follow_symlinks=False))
                == self.created_identity, 'original_owned_Cargo_target_identity_required')
        shutil.rmtree(self.path.name, dir_fd=self.parent_fd)
        require(not os.path.lexists(self.path), 'private_Cargo_target_cleanup_required')
        os.close(self.parent_fd)
        self.removed = True
        return {'path': str(self.path), 'device': self.created_identity[0],
                'inode': self.created_identity[1], 'owner_uid': self.created_identity[2],
                'removed': True, 'scope': 'Only this job-created private Cargo target; no registry/source/output or global caches.'}


def verify_copies(directory, rows):
    verified = {}
    for row in rows:
        path = regular_member(directory, row['path'])
        require(path.stat().st_size == row['bytes'] and digest(path) == row['sha256'],
                'copied_release_bytes_must_survive_target_cleanup')
        verified[row['path']] = row['sha256']
    return verified


def regular_member(root, name):
    require(isinstance(name, str), 'canonical_relative_member_required')
    member = PurePosixPath(name)
    require(member.as_posix() == name and not member.is_absolute()
            and member.parts and '..' not in member.parts, 'canonical_relative_member_required')
    path = root / name
    require(path.resolve(strict=True) == path and path.is_file() and not path.is_symlink(),
            'regular_member_required')
    return path


def component_lab_evidence(source, catalog, summary, log_text, lab):
    expected = PROFILE['component_lab']
    require(summary == {'stories': expected['stories'], 'captures': expected['captures'],
                       'palettes': expected['palettes'], 'source_commit': PROFILE['source_commit'],
                       'rendering_inputs': expected['producer_inputs']}, 'actual_complete_component_lab_matrix_required')
    path = regular_member(catalog, 'manifest.json')
    require(path.stat().st_size <= 4 * 1024**2, 'bounded_component_lab_manifest_required')
    manifest = json.loads(path.read_text())
    renderer = manifest['renderer']
    require(renderer['engine'] == 'lucent-render' and renderer['scope'] == expected['renderer_scope']
            and renderer['source_commit'] == PROFILE['source_commit'], 'new_component_lab_source_and_renderer_required')
    require(len(manifest['stories']) == expected['stories'] and len(manifest['palettes']) == expected['palettes'],
            'complete_component_lab_story_palette_inventory_required')
    inputs = renderer['inputs']
    require(isinstance(inputs, dict) and len(inputs) == expected['producer_inputs']
            and inputs == expected['producer_sha256'], 'complete_current_producer_inventory_required')
    for name, sha in inputs.items():
        require(re.fullmatch('[0-9a-f]{64}', sha) and digest(regular_member(source, name)) == sha,
                'actual_current_component_producer_fingerprint_required')
    images = {}
    for story in manifest['stories']:
        for variant in story['variants']:
            image = variant['image']
            name = image['path']
            require(name.startswith('images/') and name.endswith('.png') and name not in images,
                    'unique_component_capture_required')
            png = regular_member(catalog, name)
            require(0 < png.stat().st_size <= 16 * 1024**2 and digest(png) == image['sha256'],
                    'actual_component_capture_hash_required')
            images[name] = {'bytes': png.stat().st_size, 'sha256': image['sha256']}
    actual = {str(path.relative_to(catalog)) for path in (catalog / 'images').rglob('*') if path.is_file()}
    require(len(images) == expected['captures'] and actual == set(images), 'exact_complete_component_capture_inventory_required')
    adapters = [line for line in log_text.splitlines() if line.startswith('lucent GPU: ')]
    require(len(adapters) == 1 and len(adapters[0]) <= 4096 and 'llvmpipe' in adapters[0].lower()
            and 'backend: Vulkan' in adapters[0] and 'device_type: Cpu' in adapters[0],
            'actual_component_lab_software_Vulkan_adapter_required')
    return {'source_commit': PROFILE['source_commit'], 'source_tree': PROFILE['source_tree'],
            'catalog_path': 'component-lab', 'manifest_path': 'component-lab/manifest.json',
            'manifest_sha256': digest(path), 'standalone_lab_sha256': lab['sha256'],
            'producer_inputs': inputs, 'captures': images, 'matrix': summary,
            'adapter_description': adapters[0].removeprefix('lucent GPU: '),
            'build_location': 'github-hosted-Arch-container', 'software_Vulkan': True,
            'Wayland_compositor_tested': False, 'VM_native_accepted': False, 'performance_accepted': False,
            'cross_driver_pixel_identity_required': False,
            'scope': 'Fresh headless CPU software Vulkan capture from this actual source and Lab binary; not a Wayland or physical-GPU acceptance report.'}


def envelope(package, receipt, destination):
    require(destination.is_absolute() and not destination.exists()
            and destination.parent.resolve(strict=True) == destination.parent,
            'fresh_canonical_envelope_directory_required')
    require(package.is_file() and not package.is_symlink() and package.name == receipt['archive']
            and digest(package) == receipt['archive_sha256'], 'actual_archive_for_envelope_required')
    destination.mkdir(mode=0o700)
    shutil.copy2(package, destination / package.name)
    (destination / 'artifact-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    require(digest(destination / package.name) == receipt['archive_sha256']
            and {path.name for path in destination.iterdir()} == {package.name, 'artifact-receipt.json'},
            'exact_two_file_success_envelope_required')


def build(args):
    source, target, out = args.source_directory, args.target_directory, args.output
    context = hosted_context(os.environ)
    exact_source(source)
    tools = Path(__file__).resolve().parents[1]
    require(output(['git', 'rev-parse', 'HEAD'], tools) == context['GITHUB_SHA']
            and context['GITHUB_SHA'] == context['LUCENT_WORKFLOW_SHA'], 'reviewed_workflow_tools_SHA_required')
    for path in (Path(__file__), PROFILE_PATH):
        committed = subprocess.check_output(['git', 'show', context['GITHUB_SHA'] + ':' + str(path.relative_to(tools))], cwd=tools, timeout=30)
        require(hashlib.sha256(committed).hexdigest() == digest(path), 'committed_workflow_tools_required')
    require(all(path.is_absolute() for path in (target, out, args.envelope))
            and not target.exists() and not out.exists() and not args.envelope.exists()
            and all(not path.is_relative_to(source) for path in (target, out, args.envelope))
            and not args.envelope.is_relative_to(out), 'fresh_separate_target_and_archive_required')
    require(not any(name in os.environ for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
                'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_TARGET')) and not any(
                name.startswith(('CARGO_PROFILE_', 'CARGO_TARGET_')) and name != 'CARGO_TARGET_DIR' for name in os.environ),
            'unreviewed_Cargo_overrides_forbidden')
    require(shutil.disk_usage(out.parent).free >= 6 * 1024**3, 'remote_disk_capacity_required')
    out.mkdir(mode=0o700)
    (out / 'logs').mkdir(mode=0o700)
    memory_log = out / 'logs/memory-admission.log'
    memory_log.touch(mode=0o600, exist_ok=False)
    record = None

    def memory_sample(phase, index, sample):
        # This durable log exists before the first admission or build record.
        # In particular a refused sample is written before its require runs.
        log_memory_sample(memory_log, phase, index, sample)
        if record is not None:
            record['memory_admission_evidence'] = {'path': 'logs/memory-admission.log', 'sha256': digest(memory_log)}
            save()

    samples = admit_memory(lambda index, sample: memory_sample('initial-admission', index, sample))
    budget = resource_snapshot()
    cargo_home = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))).resolve()
    owned_target = PrivateCargoTarget(target, Path(os.environ['RUNNER_TEMP']),
                                     [source, out, args.envelope, cargo_home])
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS=str(PROFILE['cpu_limit']), CARGO_INCREMENTAL='0',
               CARGO_TERM_COLOR='never', LC_ALL='C')
    runtime = out.parent / 'lucent-video-focus-headless-runtime'
    runtime.mkdir(mode=0o700)
    env['XDG_RUNTIME_DIR'] = str(runtime)
    for name in ('WAYLAND_DISPLAY', 'DISPLAY', 'XDG_ACTIVATION_TOKEN', 'DESKTOP_STARTUP_ID'):
        env.pop(name, None)
    version = output(['rustc', '--version', '--verbose'], source, env)
    require('release: ' + PROFILE['rust_release'] + '\n' in version + '\n'
            and 'commit-hash: ' + PROFILE['rust_commit'] + '\n' in version + '\n'
            and 'host: x86_64-unknown-linux-gnu' in version, 'pinned_Rust_toolchain_required')
    icds = list(Path('/usr/share/vulkan/icd.d').glob('*lvp*.json'))
    require(len(icds) == 1 and icds[0].is_file(), 'one_software_Vulkan_ICD_required')
    env['VK_DRIVER_FILES'] = str(icds[0])
    vulkan = output(['vulkaninfo', '--summary'], source, env, timeout=60)
    require('llvmpipe' in vulkan.lower() and 'PHYSICAL_DEVICE_TYPE_CPU' in vulkan, 'actual_software_Vulkan_adapter_required')
    (out / 'software-vulkan-summary.log').write_text(vulkan + '\n')
    deps = dependency_evidence(args.dependency_evidence)
    require(output(['pacman', '-Q']) == (args.dependency_evidence / 'installed-packages.txt').read_text().strip(),
            'installed_packages_must_match_frozen_transaction')
    for name in deps['files']:
        original, destination = args.dependency_evidence / name, out / 'dependencies' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(original, destination)
    headers = ffmpeg_headers()
    for name in headers:
        destination = out / 'abi/include' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(Path('/usr/include') / name, destination)
    record = {'source_commit': PROFILE['source_commit'], 'source_tree': PROFILE['source_tree'],
        'baseline_manifest_sha256': PROFILE['baseline_manifest_sha256'], 'passed': False, 'steps': [],
        'baseline_source_commit': PROFILE['baseline_source_commit'],
        'baseline_manifest_kind': PROFILE['baseline_manifest_kind'],
        'baseline_remote_raw_manifest_sha256': PROFILE['baseline_remote_raw_manifest_sha256'],
        'memory_admission_evidence': {'path': 'logs/memory-admission.log', 'sha256': digest(memory_log)},
        'budget': budget, 'admission_available_mib': samples, 'source_working_tree_changed': False,
        'build_location': 'github-hosted-Arch-container', 'provenance': context,
        'container_image': PROFILE['container_image'], 'container_dated_tag': PROFILE['container_dated_tag'],
        'rustc_verbose': version, 'dependency_transaction': deps, 'software_vulkan': {
            'ICD': str(icds[0]), 'summary_sha256': digest(out / 'software-vulkan-summary.log'),
            'scope': 'Actual remote CPU software Vulkan; no local host GPU, VM or physical-hardware acceptance.'},
        'ffmpeg_package': output(['pacman', '-Q', 'ffmpeg']),
        'FFmpeg_public_header_sha256': headers,
        'FFmpeg_ABI_scope': 'Actual bindgen headers are archived for comparison. Same SONAME and function imports alone do not prove struct/enum/layout compatibility with older target headers.',
        'runtime_ABI_accepted': False, 'native_activated': False, 'accepted_main': False, 'vm_deployed': False,
        'review_status': 'Pinned combined native video/focus hosted build; native delivery, quality, performance and accepted runtime status are separate.',
        'scope': PROFILE['scope']}

    def save():
        (out / 'build.json').write_text(json.dumps(record, indent=2) + '\n')

    def run(name, arguments, expected=None, timeout=2400):
        exact_source(source)
        log = out / 'logs' / (name + '.log')
        started = time.monotonic()
        print(json.dumps({'phase': name, 'source': PROFILE['source_commit']}), flush=True)
        with log.open('x') as stream:
            try:
                code = subprocess.run(['cargo', *arguments], cwd=source, env=env,
                                      stdout=stream, stderr=subprocess.STDOUT, timeout=timeout).returncode
            except subprocess.TimeoutExpired:
                code = 124
        row = {'name': name, 'exit': code, 'seconds': time.monotonic() - started,
               'log_path': 'logs/' + log.name, 'log_sha256': digest(log)}
        if code == 0 and expected is not None:
            row.update(test_summary(log.read_text(), expected, PROFILE['required_test_names'].get(name, []), name == 'desktop-tests'))
        record['steps'].append(row)
        save()
        exact_source(source)
        require(code == 0, 'gate_failed_' + name)

    save()
    with (out / 'dependency-fetch.log').open('x') as log:
        subprocess.run(['cargo', 'fetch', *MANIFEST, '--locked'], cwd=source, env=env,
                       stdout=log, stderr=subprocess.STDOUT, timeout=600, check=True)
    record['dependency_fetch_sha256'] = digest(out / 'dependency-fetch.log')
    run('fmt', ['fmt', *MANIFEST, '--all', '--check'])
    metadata = json.loads(output(['cargo', 'metadata', *MANIFEST, '--offline', '--locked', '--no-deps', '--format-version', '1'], source, env, 60))
    packages, targets = workspace_inventory(metadata, source)
    record['cleaned_workspace_packages'] = packages
    clean = ['clean', *MANIFEST, '--offline', '--locked', *[part for p in packages for part in ['-p', p]]]
    run('clean-local-dev', [*clean, '--profile', 'dev'])
    run('clean-local-release', [*clean, '--release'])
    record['release_freshness'] = release_freshness(target, packages, targets)
    save()
    run('workspace-clippy', ['clippy', *RELEASE, '--workspace', '--all-targets', '--', '-D', 'warnings'])
    counts = PROFILE['expected_test_counts']
    run('video-frame-tests', ['test', *RELEASE, '-p', 'lucent-video-frame', '--lib', '--', '--nocapture', '--test-threads=1'], counts['video-frame-tests'])
    for package in ('lucent-domain', 'lucent-api', 'lucent-ui', 'lucent-wayland'):
        run(package + '-tests', ['test', *RELEASE, '-p', package, '--lib', '--', '--nocapture', '--test-threads=1'], counts[package + '-tests'])
    run('desktop-tests', ['test', *RELEASE, '-p', 'lucent-desktop', '--bin', 'lucent-desktop', '--', '--nocapture', '--test-threads=1'], counts['desktop-tests'])
    for gate, selected in (('video-tests', 'video::'), ('application-adapter-tests', 'applications::')):
        run(gate, ['test', *RELEASE, '-p', 'lucent-services', '--lib', selected, '--', '--nocapture', '--test-threads=1'], counts[gate])
    render = ['test', *RELEASE, '-p', 'lucent-render', '--lib', 'stream_upload::tests::']
    run('upload-pure-tests', [*render, '--', '--show-output', '--test-threads=1'], counts['upload-pure-tests'])
    run('upload-vulkan-tests', [*render, '--', '--ignored', '--show-output', '--test-threads=1'], counts['upload-vulkan-tests'])
    record['vulkan_upload_tests'] = 'seven_actual_remote_software_driver_checks_passed'
    for gate, selected in [('video-plane-vulkan-tests', 'video_planes::tests::video_planes_gpu_'),
                           ('video-scene-vulkan-tests', 'video_scene::tests::video_scene_gpu_')]:
        run(gate, ['test', *RELEASE, '-p', 'lucent-render', '--lib', selected, '--', '--ignored', '--nocapture', '--test-threads=1'], counts[gate])
    run('desktop-bar-pixels-tests', ['test', *RELEASE, '-p', 'lucent-desktop', '--bin', 'lucent-desktop',
        'bar_handoff_carriers_keep_identical_pixels', '--', '--ignored', '--nocapture', '--test-threads=1'], counts['desktop-bar-pixels-tests'])
    record['bar_pixels_tests'] = 'one_actual_remote_software_driver_check_passed'
    run('release', ['build', *RELEASE, *[part for p in ('lucent-desktop', 'lucent-cli', 'lucent-menu',
        'lucent-session', 'lucent-polkit', 'international-text', 'application-intent', 'lucent-lab') for part in ['-p', p]]])
    run('allocation-tests', ['test', *RELEASE, '-p', 'lucent-render', '--lib', 'allocation::tests::', '--', '--nocapture', '--test-threads=1'], counts['allocation-tests'])
    run('video-allocation-example-tests', ['test', *RELEASE, '-p', 'lucent-services', '--example', 'video-allocation-observer', '--', '--nocapture', '--test-threads=1'], counts['video-allocation-example-tests'])
    run('native-video-example-tests', ['test', *RELEASE, '-p', 'native-video-planes', '--bin', 'native-video-planes', '--', '--nocapture', '--test-threads=1'], counts['native-video-example-tests'])
    completed_stage_inventory(record['steps'])

    def copy(original, relative, executable=False):
        destination = out / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(original, destination)
        if executable:
            destination.chmod(0o700)
        require(digest(original) == digest(destination), 'archive_copy_changed')
        return {'path': relative, 'bytes': destination.stat().st_size, 'sha256': digest(destination)}

    binaries = {name: copy(target / 'release' / name, 'bin/' + name, True)
                for name in PROFILE['managed_executables'] + PROFILE['acceptance_executables']}
    support = {name: copy(source / name, 'support/' + name) for name in PROFILE['support_files']}
    examples = {name: copy(source / name, 'examples/' + name) for name in PROFILE['example_sources']}
    changes = []
    for name, item in {**support, **examples}.items():
        committed = subprocess.check_output(['git', 'show', PROFILE['source_commit'] + ':' + name], cwd=source, timeout=30)
        require(hashlib.sha256(committed).hexdigest() == item['sha256'], 'committed_support_and_examples_required')
        prior = PROFILE['baseline_file_hashes'][name]
        if prior != item['sha256']:
            changes.append({'path': name, 'baseline_sha256': prior, 'candidate_sha256': item['sha256']})
    record['support_and_example_changes_from_baseline'] = changes
    record['standalone_lab'] = copy(target / 'release/lucent-lab', 'standalone-lab/lucent-lab', True)
    record['standalone_lab']['scope'] = 'Same-source standalone component client; no toolkit/runtime or native acceptance claim.'
    licenses = {}
    for name, expected in PROFILE['license_files'].items():
        original = regular_member(source, name)
        require(digest(original) == expected, 'pinned_license_source_required')
        licenses[name] = copy(original, 'licenses/' + Path(name).name)
    record['license_files'] = licenses

    # Every release payload is already hash-copied. The remaining Cargo target
    # belongs only to this job and is no longer needed for export or packaging.
    copied = [*binaries.values(), *support.values(), *examples.values(), record['standalone_lab'], *licenses.values()]
    require(len(copied) == PROFILE['expected_payload_origins'] == 45, 'exact45_release_payload_origins_required')
    copies_before = verify_copies(out, copied)
    before_cleanup = resource_snapshot()
    memory_sample('target-cleanup-before', 0, before_cleanup)
    cleanup = owned_target.remove()
    copies_after = verify_copies(out, copied)
    require(copies_before == copies_after, 'all_copied_release_hashes_unchanged_after_cleanup')
    after_cleanup = resource_snapshot()
    memory_sample('target-cleanup-after', 0, after_cleanup)
    record['private_target_cleanup'] = dict(cleanup, before=before_cleanup, after=after_cleanup,
                                             copied_files_sha256=copies_after)
    save()

    # Capture with this actual Lab while its compile-time source directory still
    # exists. A copied/imported Lab cannot find those producers merely via cwd.
    capture_samples = admit_memory(lambda index, sample: memory_sample('component-lab-admission', index, sample))
    exact_source(source)
    catalog = out / 'component-lab'
    export_log = out / 'logs/component-lab-export.log'
    started = time.monotonic()
    print(json.dumps({'phase': 'component-lab-export', 'source': PROFILE['source_commit']}), flush=True)
    with export_log.open('x') as stream:
        try:
            export_code = subprocess.run([str(out / record['standalone_lab']['path']), '--render', str(catalog),
                '--source-commit', PROFILE['source_commit']], cwd=source, env=env,
                stdout=stream, stderr=subprocess.STDOUT,
                timeout=PROFILE['component_lab']['export_timeout_seconds']).returncode
        except subprocess.TimeoutExpired:
            export_code = 124
    capture_run = {'exit': export_code, 'seconds': time.monotonic() - started,
                   'log_path': 'logs/' + export_log.name, 'log_sha256': digest(export_log),
                   'admission_available_mib': capture_samples, 'supplementary_not_a_Rust_test_stage': True}
    record['component_lab_export'] = capture_run
    save()
    require(export_code == 0 and export_log.stat().st_size <= 16 * 1024**2, 'actual_component_lab_export_required')
    checker_log = out / 'logs/component-lab-check.log'
    with checker_log.open('x') as stream:
        check_code = subprocess.run(['python3', PROFILE['component_lab']['checker_source'], '--root', str(source),
            '--catalog', str(catalog)], cwd=source, env=env, stdout=stream, stderr=subprocess.STDOUT,
            timeout=PROFILE['component_lab']['checker_timeout_seconds']).returncode
    require(check_code == 0 and checker_log.stat().st_size <= 1024**2, 'actual_component_lab_checker_required')
    summary = json.loads(checker_log.read_text())
    capture = component_lab_evidence(source, catalog, summary, export_log.read_text(), record['standalone_lab'])
    capture.update(export=capture_run, checker={'exit': check_code, 'path': 'logs/' + checker_log.name,
        'sha256': digest(checker_log), 'source_path': PROFILE['component_lab']['checker_source'],
        'source_sha256': digest(source / PROFILE['component_lab']['checker_source'])},
        software_Vulkan_ICD={'path': str(icds[0]), 'sha256': digest(icds[0])})
    capture_path = out / 'component-lab-evidence.json'
    capture_path.write_text(json.dumps(capture, indent=2) + '\n')
    record['component_lab_export'] = {'path': capture_path.name, 'sha256': digest(capture_path),
        'matrix': summary, 'manifest_sha256': capture['manifest_sha256'],
        'standalone_lab_sha256': record['standalone_lab']['sha256'], 'build_location': record['build_location'],
        'software_Vulkan': True, 'VM_native_accepted': False, 'Wayland_compositor_tested': False}
    exact_source(source)
    save()
    (out / 'abi/ELF').mkdir(parents=True, exist_ok=True)
    record['ELF_metadata'] = {}
    for name, item in {**binaries, 'lucent-lab': record['standalone_lab']}.items():
        raw = subprocess.check_output(['readelf', '--dynamic', '--version-info', '--dyn-syms', '--wide',
                                       str(out / item['path'])], timeout=30)
        require(len(raw) <= 8 * 1024**2, 'bounded_actual_ELF_metadata_required')
        path = out / 'abi/ELF' / (name + '.txt')
        path.write_bytes(raw)
        record['ELF_metadata'][name] = {'path': str(path.relative_to(out)), 'sha256': digest(path)}
    require(ffmpeg_headers() == headers and output(['pacman', '-Q']) ==
            (args.dependency_evidence / 'installed-packages.txt').read_text().strip(),
            'build_headers_and_package_transaction_must_stay_unchanged')
    record['memory_observation_rows'] = completed_memory_rows(memory_log)
    record['passed'] = True
    exact_source(source)
    save()
    manifest = {'source_commit': PROFILE['source_commit'], 'source_tree': PROFILE['source_tree'], 'passed': True,
        'baseline_manifest_sha256': PROFILE['baseline_manifest_sha256'], 'binaries': binaries, 'support_files': support,
        'example_sources': examples, 'managed_executables': PROFILE['managed_executables'],
        'acceptance_executables': PROFILE['acceptance_executables'],
        'license_files': licenses, 'component_lab_export': record['component_lab_export'],
        'baseline_source_commit': PROFILE['baseline_source_commit'],
        'baseline_manifest_kind': PROFILE['baseline_manifest_kind'],
        'baseline_remote_raw_manifest_sha256': PROFILE['baseline_remote_raw_manifest_sha256'],
        'focused_local_gate': {'path': 'build.json', 'sha256': digest(out / 'build.json'),
                               'scope': 'Schema-compatible gate record; actual build_location is off-host, never local/systemd.'},
        'source_working_tree_changed': False, 'build_location': record['build_location'], 'provenance': context,
        'runtime_ABI_accepted': False, 'native_activated': False, 'accepted_main': False, 'vm_deployed': False,
        'review_status': record['review_status'], 'scope': PROFILE['scope']}
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    epoch = int(output(['git', 'show', '-s', '--format=%ct', PROFILE['source_commit']], source))
    archive = out.with_suffix('.tar')
    with tarfile.open(archive, 'x') as tar:
        for path in sorted(out.rglob('*')):
            require(not path.is_symlink() and (path.is_file() or path.is_dir()), 'regular_archive_members_required')
            info = tar.gettarinfo(str(path), str(path.relative_to(out)))
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            info.mtime = epoch
            if path.is_file():
                with path.open('rb') as stream:
                    tar.addfile(info, stream)
            else:
                tar.addfile(info)
    subprocess.run(['zstd', '-3', '-T3', '--no-progress', '--rm', str(archive)], timeout=180, check=True)
    package = archive.with_suffix('.tar.zst')
    receipt = {'source_commit': PROFILE['source_commit'], 'archive': package.name, 'archive_sha256': digest(package),
               'manifest_sha256': digest(out / 'manifest.json'), 'build_sha256': digest(out / 'build.json'),
               'runtime_ABI_accepted': False, 'native_activated': False, 'accepted_main': False}
    (out.parent / 'artifact-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    envelope(package, receipt, args.envelope)
    print(json.dumps(receipt))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--source-directory', type=Path)
    parser.add_argument('--target-directory', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--dependency-evidence', type=Path)
    parser.add_argument('--envelope', type=Path)
    args = parser.parse_args()
    if not args.run:
        print(json.dumps({'prepared_only': True, 'actions': False, 'source_commit': PROFILE['source_commit'],
                          'planned_stages': len(PROFILE['expected_stage_order']), 'planned_tests': sum(PROFILE['expected_test_counts'].values()),
                          'image': PROFILE['container_image'], 'runtime_ABI_accepted': False, 'native_activated': False}))
        return 0
    require(all(value is not None for value in (args.source_directory, args.target_directory, args.output,
                args.dependency_evidence, args.envelope)),
            'explicit_hosted_build_paths_required')
    build(args)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
