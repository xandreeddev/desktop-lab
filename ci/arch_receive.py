#!/usr/bin/env python3
"""Offline coherent-release receiver. Default is inert; never deploy or execute a payload."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import resource
import selectors
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import time
import zipfile

import arch_release as producer
import elf_inspect as elf

MAX_ZIP = 512 * 1024**2
MAX_TAR = 1536 * 1024**2
MAX_MEMBER = 512 * 1024**2
MAX_JSON = 8 * 1024**2
HEX = re.compile('[0-9a-f]{64}')
SHA = re.compile('[0-9a-f]{40}')
API_FILES = ('run.json', 'job.json', 'workflow.json', 'artifact.json')
FALSE_SCOPES = ('runtime_ABI_accepted', 'native_activated', 'accepted_main', 'vm_deployed',
                'standard_source_checks_verified', 'visual_review_accepted', 'performance_accepted')


def require(value, reason):
    if not value:
        raise ValueError(reason)


def safe_name(value):
    require(isinstance(value, str) and len(value) <= 512, 'bounded_member_name_required')
    name = PurePosixPath(value)
    require(name.as_posix() == value and name.parts and not name.is_absolute()
            and '..' not in name.parts and '\\' not in value and not any(ord(c) < 32 or ord(c) == 127 for c in value),
            'canonical_relative_member_required')
    return value


def read(path, expected=None, limit=MAX_JSON):
    path = Path(path)
    require(path.is_absolute() and path.resolve(strict=True) == path, 'canonical_regular_input_required')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid() and 0 <= before.st_size <= limit,
                'owned_bounded_regular_input_required')
        with os.fdopen(os.dup(fd), 'rb') as stream:
            raw = stream.read(limit + 1)
        require(len(raw) == before.st_size and elf.identity(os.fstat(fd)) == elf.identity(before)
                and elf.identity(path.lstat()) == elf.identity(before), 'input_changed_while_reading')
        require(expected is None or (isinstance(expected, str) and HEX.fullmatch(expected)
                and hashlib.sha256(raw).hexdigest() == expected), 'input_hash_mismatch')
        return raw
    finally:
        os.close(fd)


def json_value(raw):
    def unique(pairs):
        value = {}
        for key, item in pairs:
            require(key not in value, 'duplicate_JSON_key_forbidden')
            value[key] = item
        return value
    return json.loads(raw, object_pairs_hook=unique)


def json_file(path, expected=None, limit=MAX_JSON):
    return json_value(read(path, expected, limit))


def write_new(path, raw):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(raw)


def verified_copy(source, destination, expected, limit=MAX_ZIP):
    fd, before = elf.open_pinned(source, expected, limit)
    try:
        with os.fdopen(os.dup(fd), 'rb') as stream:
            stream_member(stream, destination, before.st_size, limit)
        require(elf.identity(os.fstat(fd)) == elf.identity(before)
                and elf.identity(source.lstat()) == elf.identity(before), 'input_changed_while_copying')
    finally:
        os.close(fd)


def stream_member(stream, destination, size, limit):
    require(type(size) is int and 0 <= size <= limit, 'bounded_member_required')
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    count = 0
    with os.fdopen(fd, 'wb') as target:
        while count < size:
            raw = stream.read(min(1024**2, size - count))
            require(raw, 'truncated_member')
            target.write(raw)
            count += len(raw)
    require(count == size, 'exact_member_size_required')


def decompress(source,destination):
    def limits():
        resource.setrlimit(resource.RLIMIT_AS,(256*1024**2,256*1024**2))
        resource.setrlimit(resource.RLIMIT_CPU,(180,180));resource.setrlimit(resource.RLIMIT_CORE,(0,0))
    fd=os.open(destination,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
    process=subprocess.Popen(['/usr/bin/zstd','-d','-c','--memory=128MB','--no-progress',str(source)],
        stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={'LC_ALL':'C'},preexec_fn=limits)
    poll=selectors.DefaultSelector();poll.register(process.stdout,selectors.EVENT_READ,'out');poll.register(process.stderr,selectors.EVENT_READ,'err')
    size=0;error=bytearray();deadline=time.monotonic()+180
    try:
        with os.fdopen(fd,'wb') as stream:
            while poll.get_map():
                require(time.monotonic()<deadline,'bounded_zstd_deadline_required')
                for key,_ in poll.select(.2):
                    raw=os.read(key.fd,65536)
                    if not raw:poll.unregister(key.fileobj);continue
                    if key.data=='out':size+=len(raw);require(size<=MAX_TAR,'bounded_decompressed_tar_required');stream.write(raw)
                    else:error.extend(raw);require(len(error)<=16384,'bounded_zstd_diagnostic_required')
        require(process.wait(timeout=2)==0 and not error,'zstd_decompression_failed')
    finally:
        poll.close()
        if process.poll() is None:process.kill()
        process.wait(timeout=2);process.stdout.close();process.stderr.close()


def zip_directory_bound(path):
    """Check the tiny two-member directory before ZipFile allocates its index."""
    size = path.stat().st_size
    require(22 <= size <= MAX_ZIP, 'bounded_ZIP_file_required')
    with path.open('rb') as stream:
        stream.seek(max(0, size - 65557))
        tail = stream.read(65557)
    marker = tail.rfind(b'PK\x05\x06')
    require(marker >= 0 and len(tail) - marker >= 22, 'ZIP_end_record_required')
    signature, disk, start_disk, disk_count, count, length, offset, comment = struct.unpack(
        '<4s4H2IH', tail[marker:marker + 22])
    require(signature == b'PK\x05\x06' and disk == start_disk == 0 and disk_count == count == 2
            and length <= 65536 and offset + length <= size - len(tail) + marker
            and marker + 22 + comment == len(tail)
            and tail[max(0, marker - 20):marker - 16] != b'PK\x06\x07',
            'bounded_single_disk_two_member_ZIP_directory_required')


def extract_zip(path, destination, receipt_bytes):
    zip_directory_bound(path)
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        require(len(infos) == 2, 'exact_two_ZIP_members_required')
        seen = set()
        for info in infos:
            name = safe_name(info.filename)
            mode = (info.external_attr >> 16) & 0xffff
            require(name not in seen and not info.is_dir() and stat.S_IFMT(mode) in (0, stat.S_IFREG)
                    and not info.flag_bits & 1 and info.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                    'unique_regular_unencrypted_ZIP_member_required')
            require(name in ('artifact-receipt.json', 'lucent-polish.tar.zst'), 'unexpected_ZIP_member')
            seen.add(name)
            limit = MAX_JSON if name == 'artifact-receipt.json' else MAX_ZIP
            with archive.open(info) as stream:
                stream_member(stream, destination/name, info.file_size, limit)
                require(not stream.read(1), 'ZIP_member_exceeds_declared_size')
        require(read(destination/'artifact-receipt.json') == receipt_bytes,
                'supplied_receipt_must_equal_original_ZIP_member')


def extract_tar(path, destination):
    """Parse fixed headers before any extension can allocate an attacker size."""
    files, directories, seen, total = set(), set(), set(), 0
    require(path.stat().st_size <= MAX_TAR, 'bounded_tar_required')
    with path.open('rb') as stream:
        while True:
            header = stream.read(512)
            require(len(header) == 512, 'complete_tar_header_required')
            if header == bytes(512):
                require(stream.read(512) == bytes(512), 'two_tar_end_blocks_required')
                while remaining := stream.read(1024**2):
                    require(not any(remaining), 'unaccounted_tar_trailer_forbidden')
                return files, directories
            member = tarfile.TarInfo.frombuf(header, 'utf-8', 'strict')
            name = safe_name(member.name)
            require(name not in seen and len(seen) < 4096 and member.type in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE)
                    and not member.issparse() and member.uid == member.gid == 0
                    and not member.uname and not member.gname, 'unique_regular_portable_tar_members_required')
            seen.add(name)
            target = destination/name
            if member.isdir():
                require(member.size == 0, 'directory_payload_forbidden')
                target.mkdir(mode=0o700, parents=True, exist_ok=True)
                directories.add(name)
                continue
            total += member.size
            require(total <= MAX_TAR and 0 <= member.size <= MAX_MEMBER, 'bounded_tar_payload_required')
            target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            stream_member(stream, target, member.size, MAX_MEMBER)
            padding = (-member.size) % 512
            require(stream.read(padding) == bytes(padding), 'canonical_tar_padding_required')
            files.add(name)


def verify_api(binding, api, zip_size, zip_sha):
    for key in ('workflow_sha', 'source_commit', 'source_tree'):
        require(isinstance(binding.get(key), str) and SHA.fullmatch(binding[key]), 'explicit_expected_commit_required')
    for key in ('run_id', 'run_attempt', 'job_id', 'workflow_id', 'artifact_id'):
        require(type(binding.get(key)) is int and 0 < binding[key] < 10**20, 'explicit_expected_identity_required')
    require(binding.get('schema_version') == 1
            and binding['repository'] == producer.PROFILE['repository']
            and binding['actor'] == producer.PROFILE['actor']
            and binding['branch'] == producer.PROFILE['branch'], 'reviewed_repository_actor_branch_required')
    run, job, workflow, artifact = (api[name] for name in API_FILES)
    require(run['id'] == binding['run_id'] and run['run_attempt'] == binding['run_attempt']
            and run['head_sha'] == binding['workflow_sha'] and run['head_branch'] == binding['branch']
            and run['event'] == 'push' and run['status'] == 'completed' and run['conclusion'] == 'success'
            and run['workflow_id'] == binding['workflow_id'] and run['repository']['full_name'] == binding['repository']
            and run['actor']['login'] == run['triggering_actor']['login'] == binding['actor'],
            'terminal_successful_expected_run_required')
    require(job['id'] == binding['job_id'] and job['run_id'] == binding['run_id']
            and job['run_attempt'] == binding['run_attempt'] and job['head_sha'] == binding['workflow_sha']
            and job['status'] == 'completed' and job['conclusion'] == 'success' and job['name'] == 'arch-release',
            'terminal_successful_expected_job_required')
    require(workflow['id'] == binding['workflow_id'] and workflow['path'] == '.github/workflows/arch-release.yml'
            and workflow['state'] == 'active', 'expected_release_workflow_required')
    require(artifact['id'] == binding['artifact_id'] and artifact['expired'] is False
            and artifact['name'] == f"lucent-arch-polish-{binding['run_id']}-{binding['run_attempt']}"
            and artifact['workflow_run']['id'] == binding['run_id']
            and artifact['workflow_run']['head_sha'] == binding['workflow_sha']
            and artifact['size_in_bytes'] == zip_size == binding['artifact_bytes']
            and artifact['digest'] == 'sha256:' + zip_sha
            and zip_sha == binding['artifact_zip_sha256'], 'published_GitHub_artifact_digest_identity_required')


def git(source, *arguments):
    return subprocess.check_output(['/usr/bin/git', *arguments], cwd=source, timeout=30)


def exact_source(source, binding):
    require(source.is_absolute() and source.resolve(strict=True) == source, 'canonical_source_checkout_required')
    require(git(source, 'rev-parse', 'HEAD').decode().strip() == binding['source_commit']
            and git(source, 'rev-parse', 'HEAD^{tree}').decode().strip() == binding['source_tree']
            and not git(source, 'status', '--porcelain', '--untracked-files=all').strip(),
            'exact_clean_reviewed_application_checkout_required')


def provenance(binding):
    return dict(GITHUB_REPOSITORY=binding['repository'], GITHUB_ACTOR=binding['actor'],
                GITHUB_TRIGGERING_ACTOR=binding['actor'], GITHUB_EVENT_NAME='push',
                GITHUB_REF='refs/heads/' + binding['branch'], GITHUB_SHA=binding['workflow_sha'],
                LUCENT_WORKFLOW_SHA=binding['workflow_sha'], GITHUB_RUN_ID=str(binding['run_id']),
                GITHUB_RUN_ATTEMPT=str(binding['run_attempt']))


def validate_release(root, receipt, binding, source):
    expected_files = {'manifest.json', 'build.json'}

    def hashed(relative, expected, limit=16*1024**2):
        safe_name(relative)
        raw = read(root/relative, expected, limit)
        expected_files.add(relative)
        return raw

    def item(row, relative, source_name=None, limit=MAX_MEMBER, commit=None):
        safe_name(relative)
        require(row.get('path', relative) == relative and type(row['bytes']) is int
                and 0 < row['bytes'] <= limit, 'exact_payload_path_size_required')
        fd, observed = elf.open_pinned(root/relative, row['sha256'], limit)
        os.close(fd)
        require(observed.st_size == row['bytes'], 'payload_size_mismatch')
        expected_files.add(relative)
        if source_name is not None:
            raw = git(source, 'show', (commit or binding['source_commit']) + ':' + safe_name(source_name))
            require(hashlib.sha256(raw).hexdigest() == row['sha256'], 'payload_must_match_committed_source')

    require(receipt['archive'] == 'lucent-polish.tar.zst'
            and all(receipt[key] is False for key in FALSE_SCOPES), 'unaccepted_portable_receipt_required')
    manifest = json_file(root/'manifest.json', receipt['manifest_sha256'])
    build = json_file(root/'build.json', receipt['build_sha256'])
    for value in (receipt, manifest, build):
        require(value['source_commit'] == binding['source_commit'] and value['source_tree'] == binding['source_tree']
                and value['profile_sha256'] == binding['profile_sha256'] and value['provenance'] == provenance(binding),
                'expected_source_workflow_provenance_required')
    for value in (manifest, build):
        require(value['passed'] is True and value['source_working_tree_changed'] is False
                and value['build_location'] == 'github-hosted-Arch-container'
                and all(value[key] is False for key in FALSE_SCOPES), 'hosted_only_qualification_scope_required')
    require(manifest['hosted_release_gate']['path'] == 'build.json'
            and manifest['hosted_release_gate']['sha256'] == receipt['build_sha256'], 'build_record_binding_required')
    tools = manifest['qualification_tools']
    required_tools = {'qualification-tools/' + name for name in
                      ('ci/arch_release.py', 'ci/arch-release-profile.json', '.github/workflows/arch-release.yml')}
    require(tools == build['qualification_tools'] and set(tools) == required_tools, 'exact_qualification_tool_roster_required')
    for relative, row in tools.items():
        item(row, relative, relative.removeprefix('qualification-tools/'), 2*1024**2, binding['workflow_sha'])
    profile = json_file(root/'qualification-tools/ci/arch-release-profile.json', binding['profile_sha256'])
    require(profile['source_commit'] == binding['source_commit'] and profile['source_tree'] == binding['source_tree']
            and all(profile[key] == binding[key] for key in ('repository', 'actor', 'branch')),
            'reviewed_profile_source_identity_required')
    for key in ('managed_executables', 'acceptance_executables', 'support_files', 'example_sources', 'license_files'):
        require(profile[key] == producer.PROFILE[key], 'reviewed_release_roster_required')
    require(manifest['managed_executables'] == profile['managed_executables']
            and manifest['acceptance_executables'] == profile['acceptance_executables'], 'exact_executable_rosters_required')
    for key, names, prefix in (
            ('binaries', profile['managed_executables'] + profile['acceptance_executables'], 'bin/'),
            ('support_files', profile['support_files'], 'support/'),
            ('example_sources', profile['example_sources'], 'examples/'),
            ('license_files', profile['license_files'], 'licenses/')):
        require(set(manifest[key]) == set(names), 'exact_payload_roster_required')
        for name, row in manifest[key].items():
            item(row, prefix + name, None if key == 'binaries' else name,
                 MAX_MEMBER if key == 'binaries' else 4*1024**2)
    require(manifest['license_files'] == build['license_files']
            and build['support_and_examples_verified_against_source_tree'] is True, 'source_copy_evidence_required')
    item(build['standalone_lab'], 'standalone-lab/lucent-lab')

    stage_names = ['fmt', 'clean-local-dev', 'clean-local-release', 'workspace-clippy',
                   *[gate['name'] for gate in profile['test_gates']], 'release']
    require([step['name'] for step in build['steps']] == stage_names, 'all_declared_qualification_stages_required')
    gates = {gate['name']: gate for gate in profile['test_gates']}
    actual_tests = 0
    for step in build['steps']:
        name = step['name']
        require(step['exit'] == 0 and step['log_path'] == 'logs/' + name + '.log', 'successful_stage_log_required')
        raw = hashed(step['log_path'], step['log_sha256'])
        if name in gates:
            gate = gates[name]
            result = producer.test_summary(raw.decode(), gate.get('required_test_names', []),
                                           gate['full'], gate.get('allow_empty', False))
            require(all(step[key] == value for key, value in result.items()), 'actual_named_test_evidence_required')
            actual_tests += result['tests_passed']
    require(build['test_evidence']['selection'] == profile['test_gates']
            and build['test_evidence']['passed'] == actual_tests, 'actual_test_total_required')
    hashed('dependency-fetch.log', build['dependency_fetch_sha256'])
    vulkan = hashed('software-vulkan-summary.log', build['software_vulkan']['summary_sha256']).decode()
    require('llvmpipe' in vulkan.lower() and 'PHYSICAL_DEVICE_TYPE_CPU' in vulkan, 'actual_software_adapter_required')
    require(build['container_image'] == profile['container_image']
            and 'release: ' + profile['rust_release'] + '\n' in build['rustc_verbose'] + '\n'
            and 'commit-hash: ' + profile['rust_commit'] + '\n' in build['rustc_verbose'] + '\n',
            'reviewed_container_and_toolchain_required')
    dependencies = build['dependency_transaction']['files']
    required_dependencies = {'download.log', 'install.log', 'resolved-urls.txt', 'installed-packages.txt',
                             'databases.sha256', 'archives.sha256', 'bootstrap-admission.txt', 'signature-policy.txt'}
    require(required_dependencies <= dependencies.keys() and 1 <= sum(
        name.startswith('databases/') and name.endswith('.db') for name in dependencies) <= 8,
        'complete_dependency_transaction_required')
    for name, row in dependencies.items():
        item(row, 'dependencies/' + safe_name(name), limit=32*1024**2)
    require(b'PackageRequired' in read(root/'dependencies/signature-policy.txt'), 'required_package_signatures_required')
    headers = build['FFmpeg_public_header_sha256']
    require(isinstance(headers, dict) and 0 < len(headers) <= 512, 'bounded_FFmpeg_header_inventory_required')
    for name, expected in headers.items():
        require(safe_name(name).split('/')[0] in ('libavcodec', 'libavformat', 'libavutil', 'libswscale')
                and name.endswith('.h'), 'public_FFmpeg_header_required')
        hashed('abi/include/' + name, expected, 2*1024**2)
    elfs = dict(manifest['binaries'], **{'lucent-lab': build['standalone_lab']})
    require(set(build['ELF_metadata']) == set(elfs), 'complete_archived_ELF_evidence_required')
    for name, row in build['ELF_metadata'].items():
        require(row['path'] == 'abi/ELF/' + name + '.txt', 'exact_ELF_evidence_path_required')
        hashed(row['path'], row['sha256'], 8*1024**2)

    memory = build['memory_admission_evidence']
    require(memory['path'] == 'logs/memory-admission.log', 'exact_memory_log_required')
    samples = [json_value(line) for line in hashed(memory['path'], memory['sha256']).splitlines()]
    require(len(samples) == 8, 'complete_memory_admission_phases_required')
    for phase in ('initial-admission', 'component-lab-admission'):
        selected = [row for row in samples if row['phase'] == phase]
        require([row['sample_index'] for row in selected] == [0, 1, 2]
                and all(row['snapshot']['effective_available_mib'] >= profile['admission_floor_mib'] for row in selected),
                'three_effective_memory_admissions_required')
    for snapshot in [build['budget'], *[row['snapshot'] for row in samples]]:
        cpu = snapshot['cpu_max'].split()
        require(len(cpu) == 2 and all(value.isdecimal() for value in cpu) and int(cpu[1]) > 0
                and int(cpu[0]) == profile['cpu_limit'] * int(cpu[1])
                and snapshot['memory_max_bytes'] == profile['memory_max_bytes']
                and snapshot['memory_swap_max_bytes'] == 0 and snapshot['pids_max'] == profile['pids_limit']
                and snapshot['container_init'] == 'docker-init' and snapshot['local_host_policy_changed'] is False,
                'actual_remote_container_budget_required')
    require(build['release_freshness']['release_local_artifacts_absent'] is True
            and build['private_target_cleanup']['removed'] is True, 'fresh_private_build_and_cleanup_required')

    capture_binding = build['component_lab_export']
    require(capture_binding == manifest['component_lab_export'] and capture_binding['path'] == 'component-lab-evidence.json',
            'shared_capture_evidence_required')
    capture = json_value(hashed(capture_binding['path'], capture_binding['sha256']))
    checker_path = profile['component_lab']['checker_source']
    local_checker = Path(__file__).resolve().parents[1]/checker_path
    require(read(local_checker, limit=1024**2) == git(source, 'show', binding['source_commit'] + ':' + checker_path),
            'local_reviewed_checker_must_match_pinned_application_source')
    spec = importlib.util.spec_from_file_location('receiver_reviewed_catalog_checker', local_checker)
    checker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(checker)  # Trusted local checker; never archive code.
    summary = checker.validate(source, root/'component-lab')
    export, check = capture['export'], capture['checker']
    require(export['exit'] == check['exit'] == 0 and export['log_path'] == 'logs/component-lab-export.log'
            and check['path'] == 'logs/component-lab-check.log' and check['source_path'] == checker_path
            and check['source_sha256'] == producer.digest(local_checker), 'successful_source_bound_capture_checker_required')
    export_log = hashed(export['log_path'], export['log_sha256']).decode()
    require(json_value(hashed(check['path'], check['sha256'])) == summary, 'actual_capture_summary_required')
    old = producer.PROFILE
    try:
        producer.PROFILE = profile
        recomputed = producer.component_lab_evidence(source, root/'component-lab', summary, export_log, build['standalone_lab'])
    finally:
        producer.PROFILE = old
    require(set(capture) == set(recomputed) | {'export', 'checker', 'software_Vulkan_ICD'}
            and all(capture[key] == value for key, value in recomputed.items())
            and capture_binding['matrix'] == summary, 'exact_fresh_capture_evidence_required')
    expected_files.add('component-lab/manifest.json')
    expected_files.update('component-lab/' + name for name in recomputed['captures'])
    actual_files = {path.relative_to(root).as_posix() for path in root.rglob('*') if path.is_file()}
    actual_dirs = {path.relative_to(root).as_posix() for path in root.rglob('*') if path.is_dir()}
    allowed_dirs = {parent.as_posix() for name in expected_files for parent in PurePosixPath(name).parents
                    if parent.as_posix() != '.'}
    require(actual_files == expected_files and actual_dirs <= allowed_dirs, 'no_missing_or_unaccounted_archive_members')
    return {'files': sorted(expected_files), 'tests_passed': actual_tests, 'component_matrix': summary,
            'ELF_files': elfs, 'FFmpeg_public_header_sha256': headers}


def verify(args):
    binding_raw = read(args.binding, args.binding_sha256)
    binding = json_value(binding_raw)
    require(isinstance(binding, dict) and set(binding) == {
        'schema_version', 'repository', 'actor', 'branch', 'workflow_sha', 'source_commit', 'source_tree',
        'profile_sha256', 'run_id', 'run_attempt', 'job_id', 'workflow_id', 'artifact_id',
        'artifact_bytes', 'artifact_zip_sha256', 'metadata_sha256'}, 'complete_explicit_binding_schema_required')
    require(isinstance(binding['metadata_sha256'], dict)
            and isinstance(binding['profile_sha256'], str) and HEX.fullmatch(binding['profile_sha256'])
            and type(binding['artifact_bytes']) is int and 0 < binding['artifact_bytes'] <= MAX_ZIP,
            'bounded_explicit_binding_values_required')
    require(set(binding['metadata_sha256']) == set(API_FILES), 'exact_saved_API_metadata_required')
    api = {name: json_file(args.metadata/name, binding['metadata_sha256'][name]) for name in API_FILES}
    fd, zip_stat = elf.open_pinned(args.archive, binding['artifact_zip_sha256'], MAX_ZIP)
    os.close(fd)
    verify_api(binding, api, zip_stat.st_size, binding['artifact_zip_sha256'])
    receipt_raw = read(args.receipt, limit=1024**2)
    receipt = json_value(receipt_raw)
    exact_source(args.source, binding)
    reference = None
    require((args.providers is None) == (args.providers_sha256 is None), 'provider_path_and_digest_required_together')
    if args.providers is not None:
        reference = json_file(args.providers, args.providers_sha256, 64*1024**2)
    destination = args.output
    require(destination.is_absolute() and destination.parent.resolve(strict=True) == destination.parent
            and not os.path.lexists(destination), 'fresh_canonical_review_directory_required')
    require(not destination.is_relative_to(args.source), 'review_output_must_be_outside_source_checkout')
    require(shutil.disk_usage(destination.parent).free >= 2*MAX_TAR + 2*MAX_ZIP, 'bounded_receiver_disk_capacity_required')
    destination.mkdir(mode=0o700)
    originals, packed, payload = (destination/name for name in ('originals', 'packed', 'payload'))
    for path in (originals, packed, payload):
        path.mkdir(mode=0o700)
    try:
        write_new(originals/'binding.json', binding_raw)
        write_new(originals/'artifact-receipt.json', receipt_raw)
        for name in API_FILES:
            verified_copy(args.metadata/name, originals/name, binding['metadata_sha256'][name], MAX_JSON)
        verified_copy(args.archive, originals/'artifact.zip', binding['artifact_zip_sha256'])
        if reference is not None:
            verified_copy(args.providers, originals/'target-providers.json', args.providers_sha256, 64*1024**2)
        extract_zip(originals/'artifact.zip', packed, receipt_raw)
        archive = packed/'lucent-polish.tar.zst'
        fd, _ = elf.open_pinned(archive, receipt['archive_sha256'], MAX_ZIP)
        os.close(fd)
        plain = destination/'payload.tar'
        decompress(archive, plain)
        extract_tar(plain, payload)
        proof = validate_release(payload, receipt, binding, args.source)
        exact_source(args.source, binding)
        reports = {}
        for name, row in proof['ELF_files'].items():
            reports[name] = elf.inspect(payload/row['path'], row['sha256'], reference)
        report = {'artifact_verified': True, 'binding_sha256': args.binding_sha256,
            'artifact_zip_sha256': binding['artifact_zip_sha256'], 'source_commit': binding['source_commit'],
            'source_tree': binding['source_tree'], 'workflow_sha': binding['workflow_sha'],
            'run_id': binding['run_id'], 'run_attempt': binding['run_attempt'], 'job_id': binding['job_id'],
            'artifact_id': binding['artifact_id'], 'payload': 'payload', 'verified_files': proof['files'],
            'tests_passed': proof['tests_passed'], 'component_matrix': proof['component_matrix'],
            'static_ELF_inspection': reports, 'provider_reference_sha256': args.providers_sha256,
            'FFmpeg_headers_match_cached_reference': None if reference is None else
                proof['FFmpeg_public_header_sha256'] == reference.get('FFmpeg_public_header_sha256'),
            'receiver_sha256': producer.digest(Path(__file__)), 'ELF_inspector_sha256': producer.digest(Path(elf.__file__)),
            'trust_anchor': 'Caller-reviewed binding digest and authenticated saved GitHub API metadata; this offline receiver made no network request.',
            'missing_acceptance': ['Successful standard checks for this exact application source: all four jobs and controlled protocol suite evidence (49 cases at this preparation).',
                                   'Human review of retained fresh Arch component PNGs.',
                                   'Current target provider/FFmpeg ABI validation and native behavior/performance acceptance.'],
            'downloaded_code_executed': False, 'ELF_executed': False, 'VM_contacted': False,
            **{name: False for name in FALSE_SCOPES}}
        write_new(destination/'receiver-report.json', (json.dumps(report, indent=2) + '\n').encode())
        return report
    except BaseException as error:
        write_new(destination/'verification-failed.json', (json.dumps({
            'artifact_verified': False, 'reason': str(error)[:512], 'ELF_executed': False,
            'VM_contacted': False, **{name: False for name in FALSE_SCOPES}}) + '\n').encode())
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify', action='store_true')
    for name in ('archive', 'receipt', 'binding', 'metadata', 'source', 'output', 'providers'):
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--binding-sha256')
    parser.add_argument('--providers-sha256')
    args = parser.parse_args()
    if not args.verify:
        print(json.dumps({'inert': True, 'actions': False, 'archive_read': False,
                          'requires': 'Explicit --verify with archive, receipt, reviewed binding/digest, saved API metadata, exact source checkout and fresh output.',
                          **{name: False for name in FALSE_SCOPES}}))
        return 0
    require(all(getattr(args, key) is not None for key in
                ('archive', 'receipt', 'binding', 'binding_sha256', 'metadata', 'source', 'output')),
            'explicit_receiver_inputs_required')
    report = verify(args)
    print(json.dumps({'artifact_verified': report['artifact_verified'], 'report': str(args.output/'receiver-report.json'),
                      **{name: False for name in FALSE_SCOPES}}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())


