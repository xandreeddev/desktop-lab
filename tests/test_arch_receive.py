"""Owned byte fixtures only: no compiler, downloaded binary, VM or network."""
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import sys
import tarfile
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'ci'))
import arch_receive as receiver
import elf_inspect as elf
sys.path.pop(0)


class ArchReceiver(unittest.TestCase):
    def test_default_is_inert_and_reads_no_inputs(self):
        output = io.StringIO()
        with (patch.object(sys, 'argv', ['receiver']), contextlib.redirect_stdout(output),
              patch.object(receiver, 'verify', side_effect=AssertionError('no verification'))):
            self.assertEqual(receiver.main(), 0)
        result = json.loads(output.getvalue())
        self.assertFalse(result['actions'])
        self.assertFalse(result['archive_read'])
        self.assertTrue(all(result[key] is False for key in receiver.FALSE_SCOPES))

    def test_verify_requires_explicit_inputs(self):
        with patch.object(sys, 'argv', ['receiver', '--verify']), self.assertRaisesRegex(ValueError, 'explicit_receiver'):
            receiver.main()

    def api_fixture(self):
        binding = dict(schema_version=1, repository=receiver.producer.PROFILE['repository'],
                       actor=receiver.producer.PROFILE['actor'], branch=receiver.producer.PROFILE['branch'],
                       workflow_sha='a'*40, source_commit='b'*40, source_tree='c'*40,
                       run_id=12, run_attempt=2, job_id=23, workflow_id=34, artifact_id=45,
                       artifact_zip_sha256='d'*64, artifact_bytes=123)
        run = dict(id=12, run_attempt=2, head_sha='a'*40, head_branch=binding['branch'], event='push',
                   status='completed', conclusion='success', workflow_id=34,
                   repository={'full_name': binding['repository']}, actor={'login': binding['actor']},
                   triggering_actor={'login': binding['actor']})
        job = dict(id=23, run_id=12, run_attempt=2, head_sha='a'*40, status='completed', conclusion='success', name='arch-release')
        workflow = dict(id=34, path='.github/workflows/arch-release.yml', state='active')
        artifact = dict(id=45, expired=False, name='lucent-arch-polish-12-2', workflow_run={'id': 12, 'head_sha': 'a'*40},
                        size_in_bytes=123, digest='sha256:' + 'd'*64)
        return binding, dict(zip(receiver.API_FILES, (run, job, workflow, artifact)))

    def test_api_requires_terminal_successful_exact_run_job_attempt_and_artifact_digest(self):
        binding, api = self.api_fixture()
        receiver.verify_api(binding, api, 123, 'd'*64)
        for file, key, value in [('run.json', 'status', 'in_progress'), ('run.json', 'conclusion', 'failure'),
                                  ('run.json', 'run_attempt', 1), ('run.json', 'head_sha', 'f'*40),
                                  ('job.json', 'status', 'queued'), ('job.json', 'run_attempt', 1),
                                  ('job.json', 'head_sha', 'b'*40), ('job.json', 'conclusion', 'cancelled'),
                                  ('workflow.json', 'path', '.github/workflows/check.yml'),
                                  ('artifact.json', 'digest', 'sha256:' + 'e'*64),
                                  ('artifact.json', 'name', 'lucent-arch-polish-12-1'), ('artifact.json', 'expired', True)]:
            changed = copy.deepcopy(api); changed[file][key] = value
            with self.subTest(file=file, key=key), self.assertRaises(ValueError):
                receiver.verify_api(binding, changed, 123, 'd'*64)
        with self.assertRaises(ValueError):
            receiver.verify_api(binding, api, 124, 'd'*64)

    def test_workflow_and_application_identities_remain_independent_expected_values(self):
        binding, api = self.api_fixture()
        self.assertNotEqual(binding['workflow_sha'], binding['source_commit'])
        self.assertEqual(receiver.provenance(binding)['GITHUB_SHA'], binding['workflow_sha'])
        for key in ('workflow_sha', 'source_commit', 'source_tree'):
            with self.subTest(key=key), self.assertRaises(ValueError):
                receiver.verify_api(dict(binding, **{key: 'main'}), api, 123, 'd'*64)

    def test_reads_reject_symlink_fifo_changed_hash_oversize_and_duplicate_JSON(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            regular = root/'regular'; regular.write_bytes(b'fixture')
            symlink = root/'link'; symlink.symlink_to(regular)
            fifo = root/'fifo'; os.mkfifo(fifo)
            for path in (symlink, fifo):
                with self.assertRaises(ValueError):
                    receiver.read(path)
            with self.assertRaisesRegex(ValueError, 'hash'):
                receiver.read(regular, '0'*64)
            with self.assertRaisesRegex(ValueError, 'bounded'):
                receiver.read(regular, limit=1)
            with self.assertRaisesRegex(ValueError, 'duplicate_JSON'):
                receiver.json_value(b'{"id":1,"id":2}')

    def zip_fixture(self, root, rows=None):
        path = root/'fixture.zip'
        with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for name, data in rows or [('artifact-receipt.json', b'{"fixture":true}'), ('lucent-polish.tar.zst', b'not-a-real-archive')]:
                archive.writestr(name, data)
        return path

    def test_zip_requires_exact_receipt_bytes_and_two_regular_members(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); destination = root/'out'; destination.mkdir()
            path = self.zip_fixture(root)
            receiver.extract_zip(path, destination, b'{"fixture":true}')
            self.assertEqual((destination/'lucent-polish.tar.zst').read_bytes(), b'not-a-real-archive')
            self.assertEqual(stat.S_IMODE((destination/'lucent-polish.tar.zst').stat().st_mode), 0o600)
            other = root/'wrong'; other.mkdir()
            with self.assertRaisesRegex(ValueError, 'supplied_receipt'):
                receiver.extract_zip(path, other, b'{"fixture": true}')

    def test_zip_refuses_duplicate_extra_traversal_and_symlink_members(self):
        cases = [[('artifact-receipt.json', b'{}'), ('../escape', b'x')],
                 [('artifact-receipt.json', b'{}'), ('artifact-receipt.json', b'{}')],
                 [('artifact-receipt.json', b'{}'), ('lucent-polish.tar.zst', b'x'), ('extra', b'x')]]
        symlink = zipfile.ZipInfo('lucent-polish.tar.zst'); symlink.create_system = 3
        symlink.external_attr = (stat.S_IFLNK | 0o777) << 16
        cases.append([('artifact-receipt.json', b'{}'), (symlink, b'/tmp/escape')])
        for rows in cases:
            with self.subTest(rows=rows), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); out = root/'out'; out.mkdir()
                with contextlib.redirect_stderr(io.StringIO()):
                    path = self.zip_fixture(root, rows)
                with self.assertRaises(ValueError):
                    receiver.extract_zip(path, out, b'{}')
                self.assertFalse((root/'escape').exists())

    def test_zip_directory_bound_precedes_index_allocation(self):
        for field, offset, fmt, value in [('entry-count', 10, '<H', 65535),
                                           ('directory-size', 12, '<I', 65537)]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); destination = root/'out'; destination.mkdir()
                path = self.zip_fixture(root)
                raw = bytearray(path.read_bytes())
                marker = raw.rfind(b'PK\x05\x06')
                if field == 'directory-size':
                    # Keep offset + size aligned with the end record, so this
                    # specifically exercises the directory allocation cap.
                    previous_size = struct.unpack_from('<I', raw, marker + offset)[0]
                    padding = value - previous_size
                    raw[marker:marker] = bytes(padding)
                    marker += padding
                struct.pack_into(fmt, raw, marker + offset, value)
                path.write_bytes(raw)
                with patch.object(receiver.zipfile, 'ZipFile', side_effect=AssertionError('must reject before parsing index')):
                    with self.assertRaisesRegex(ValueError, 'bounded_single_disk'):
                        receiver.extract_zip(path, destination, b'{}')

    def tar_fixture(self, root, rows):
        path = root/'fixture.tar'
        with tarfile.open(path, 'w', format=tarfile.USTAR_FORMAT) as archive:
            for name, kind, data in rows:
                info = tarfile.TarInfo(name); info.type = kind; info.size = len(data)
                if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                    info.linkname = '/tmp/escape'
                archive.addfile(info, io.BytesIO(data))
        return path

    def test_tar_creates_only_owned_nonexecutable_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); out = root/'out'; out.mkdir()
            path = self.tar_fixture(root, [('bin', tarfile.DIRTYPE, b''), ('bin/client', tarfile.REGTYPE, b'fixture')])
            files, dirs = receiver.extract_tar(path, out)
            self.assertEqual(files, {'bin/client'}); self.assertEqual(dirs, {'bin'})
            self.assertEqual((out/'bin/client').read_bytes(), b'fixture')
            self.assertEqual(stat.S_IMODE((out/'bin/client').stat().st_mode), 0o600)

    def test_tar_refuses_links_devices_sparse_extensions_duplicates_and_traversal(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE,
                     tarfile.GNUTYPE_SPARSE, tarfile.XHDTYPE, tarfile.GNUTYPE_LONGNAME):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); out = root/'out'; out.mkdir()
                with self.assertRaises(ValueError):
                    receiver.extract_tar(self.tar_fixture(root, [('bad', kind, b'')]), out)
        for rows in [[('../escape', tarfile.REGTYPE, b'bad')],
                     [('same', tarfile.REGTYPE, b'a'), ('same', tarfile.REGTYPE, b'b')]]:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory); out = root/'out'; out.mkdir()
                with self.assertRaises(ValueError):
                    receiver.extract_tar(self.tar_fixture(root, rows), out)
                self.assertFalse((root/'escape').exists())

    def test_tar_refuses_truncation_oversize_and_hidden_trailer(self):
        for mode in ('truncated', 'oversize', 'trailer'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); out = root/'out'; out.mkdir()
                path = self.tar_fixture(root, [('file', tarfile.REGTYPE, b'content')])
                raw = path.read_bytes()
                if mode == 'truncated': raw = raw[:513]
                if mode == 'trailer': raw += b'extra'
                path.write_bytes(raw)
                with patch.object(receiver, 'MAX_MEMBER', 1 if mode == 'oversize' else receiver.MAX_MEMBER):
                    with self.assertRaises(ValueError):
                        receiver.extract_tar(path, out)

    def test_stream_copy_cannot_overwrite_existing_data_or_accept_short_stream(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); target = root/'keep'; target.write_bytes(b'original')
            with self.assertRaises(FileExistsError):
                receiver.stream_member(io.BytesIO(b'new'), target, 3, 10)
            self.assertEqual(target.read_bytes(), b'original')
            with self.assertRaisesRegex(ValueError, 'truncated'):
                receiver.stream_member(io.BytesIO(b'a'), root/'short', 2, 10)

    def release_fixture(self, root):
        """Complete owned artifact shape; only Git/process boundaries are stubbed."""
        payload, source = root/'payload', root/'source'
        payload.mkdir(); source.mkdir()
        binding, _ = self.api_fixture()
        profile = copy.deepcopy(receiver.producer.PROFILE)
        profile.update(source_commit=binding['source_commit'], source_tree=binding['source_tree'])
        committed = {}

        def add(relative, raw=b'fixture'):
            path = payload/relative; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(raw)
            return {'path': relative, 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}

        def encoded(value):
            return (json.dumps(value, indent=2) + '\n').encode()

        scope = dict(source_commit=binding['source_commit'], source_tree=binding['source_tree'],
                     passed=True, source_working_tree_changed=False, build_location='github-hosted-Arch-container',
                     provenance=receiver.provenance(binding), **{key: False for key in receiver.FALSE_SCOPES})
        build, manifest = dict(scope), dict(scope)
        tools = {}
        for name in ('ci/arch_release.py', 'ci/arch-release-profile.json', '.github/workflows/arch-release.yml'):
            raw = encoded(profile) if name.endswith('.json') else ('reviewed tool ' + name).encode()
            committed[(binding['workflow_sha'], name)] = raw
            row = add('qualification-tools/' + name, raw); row.pop('path')
            tools['qualification-tools/' + name] = row
        binding['profile_sha256'] = tools['qualification-tools/ci/arch-release-profile.json']['sha256']
        for value in (build, manifest):
            value.update(profile_sha256=binding['profile_sha256'], qualification_tools=tools)
        manifest.update(managed_executables=profile['managed_executables'], acceptance_executables=profile['acceptance_executables'])
        for key, names, prefix in (
                ('binaries', profile['managed_executables'] + profile['acceptance_executables'], 'bin/'),
                ('support_files', profile['support_files'], 'support/'),
                ('example_sources', profile['example_sources'], 'examples/'),
                ('license_files', profile['license_files'], 'licenses/')):
            manifest[key] = {}
            for name in names:
                raw = ('owned fixture ' + name).encode()
                if key == 'license_files':
                    path = source/name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(raw)
                else:
                    manifest[key][name] = add(prefix + name, raw)
                if key != 'binaries': committed[(binding['source_commit'], name)] = raw
        manifest['license_files'] = receiver.producer.copy_license_files(
            source, payload, lambda name: committed[(binding['source_commit'], name)])
        build.update(license_files=manifest['license_files'], support_and_examples_verified_against_source_tree=True,
                     standalone_lab=add('standalone-lab/lucent-lab', b'owned Lab fixture'), steps=[])
        gates = {gate['name']: gate for gate in profile['test_gates']}
        for name in ['fmt', 'clean-local-dev', 'clean-local-release', 'workspace-clippy', *gates, 'release']:
            gate = gates.get(name)
            test_name = gate.get('required_test_names', ['fixture_pass'])[0] if gate else 'fixture_pass'
            text = ('test module::' + test_name + ' ... ok\n'
                    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n')
            log = add('logs/' + name + '.log', text.encode() if gate else b'success')
            step = dict(name=name, exit=0, log_path=log['path'], log_sha256=log['sha256'])
            if gate:
                step.update(receiver.producer.test_summary(text, gate.get('required_test_names', []), gate['full'], gate.get('allow_empty', False)))
            build['steps'].append(step)
        build['test_evidence'] = {'selection': profile['test_gates'], 'passed': len(gates)}
        build['dependency_fetch_sha256'] = add('dependency-fetch.log')['sha256']
        build['software_vulkan'] = {'summary_sha256': add('software-vulkan-summary.log', b'llvmpipe PHYSICAL_DEVICE_TYPE_CPU')['sha256']}
        build.update(container_image=profile['container_image'], rustc_verbose='release: ' + profile['rust_release'] + '\ncommit-hash: ' + profile['rust_commit'] + '\n')
        dependencies = {}
        for name in ('download.log', 'install.log', 'resolved-urls.txt', 'installed-packages.txt', 'databases.sha256',
                     'archives.sha256', 'bootstrap-admission.txt', 'signature-policy.txt', 'databases/core.db'):
            row = add('dependencies/' + name, b'PackageRequired' if name == 'signature-policy.txt' else b'fixture evidence')
            row.pop('path'); dependencies[name] = row
        build['dependency_transaction'] = {'files': dependencies}
        build['FFmpeg_public_header_sha256'] = {name + '/version.h': add('abi/include/' + name + '/version.h')['sha256']
                                                for name in ('libavcodec', 'libavformat', 'libavutil', 'libswscale')}
        build['ELF_metadata'] = {name: {key: value for key, value in add('abi/ELF/' + name + '.txt').items() if key != 'bytes'}
                                 for name in [*manifest['binaries'], 'lucent-lab']}
        snapshot = dict(cpu_max='300000 100000', memory_max_bytes=profile['memory_max_bytes'], memory_swap_max_bytes=0,
                        pids_max=profile['pids_limit'], container_init='docker-init', local_host_policy_changed=False,
                        effective_available_mib=5000)
        memory = [{'phase': phase, 'sample_index': index, 'snapshot': snapshot}
                  for phase, count in [('initial-admission', 3), ('target-cleanup-before', 1),
                                       ('target-cleanup-after', 1), ('component-lab-admission', 3)] for index in range(count)]
        row = add('logs/memory-admission.log', b''.join(json.dumps(item).encode() + b'\n' for item in memory))
        build.update(memory_admission_evidence={'path': row['path'], 'sha256': row['sha256']}, budget=snapshot,
                     release_freshness={'release_local_artifacts_absent': True}, private_target_cleanup={'removed': True})
        catalog, matrix = self.catalog_fixture(source, payload, binding['source_commit'])
        export_text = 'lucent GPU: AdapterInfo { name: "llvmpipe", device_type: Cpu, backend: Vulkan }\n'
        export = add('logs/component-lab-export.log', export_text.encode())
        checker = add('logs/component-lab-check.log', json.dumps(matrix).encode())
        checker_name = profile['component_lab']['checker_source']
        checker_raw = (ROOT/checker_name).read_bytes()
        committed[(binding['source_commit'], checker_name)] = checker_raw
        with patch.object(receiver.producer, 'PROFILE', profile):
            recomputed = receiver.producer.component_lab_evidence(source, catalog, matrix, export_text, build['standalone_lab'])
        capture = dict(recomputed,
                       export={'exit': 0, 'log_path': export['path'], 'log_sha256': export['sha256']},
                       checker={'exit': 0, 'path': checker['path'], 'sha256': checker['sha256'], 'source_path': checker_name,
                                'source_sha256': hashlib.sha256(checker_raw).hexdigest()}, software_Vulkan_ICD={})
        row = add('component-lab-evidence.json', encoded(capture))
        build['component_lab_export'] = manifest['component_lab_export'] = {'path': row['path'], 'sha256': row['sha256'], 'matrix': matrix}
        receipt = dict(source_commit=binding['source_commit'], source_tree=binding['source_tree'],
                       provenance=receiver.provenance(binding), profile_sha256=binding['profile_sha256'],
                       archive='lucent-polish.tar.zst', **{key: False for key in receiver.FALSE_SCOPES})

        def save():
            receipt['build_sha256'] = add('build.json', encoded(build))['sha256']
            manifest['hosted_release_gate'] = {'path': 'build.json', 'sha256': receipt['build_sha256']}
            receipt['manifest_sha256'] = add('manifest.json', encoded(manifest))['sha256']
        save()
        return SimpleNamespace(payload=payload, source=source, binding=binding, build=build, manifest=manifest,
                               receipt=receipt, matrix=matrix, recomputed=recomputed, committed=committed, save=save)

    def catalog_fixture(self, source, payload, commit):
        """Four tiny header/hash PNG fixtures; no image rendering or decoding."""
        directories = ('lucent/apps/lucent-lab', 'lucent/apps/lucent-menu', 'lucent/crates/lucent-api',
                       'lucent/crates/lucent-design', 'lucent/crates/lucent-ui', 'lucent/crates/lucent-render',
                       'lucent/crates/lucent-domain', 'lucent/crates/lucent-usecases')
        files = {'design/tokens.json': b'{"fixture":{"$value":1}}',
                 'configs/lucent/palettes.json': b'[{"id":"fixture","name":"Fixture"}]',
                 'lucent/Cargo.lock': b'fixture lock', 'lucent/Cargo.toml': b'fixture workspace',
                 'lucent/crates/lucent-design/src/tokens.rs': b'fixture tokens',
                 'lucent/apps/lucent-desktop/assets/LucentSans.ttf': b'fixture font bytes',
                 'lucent/apps/lucent-desktop/assets/LucentDisplay.ttf': b'fixture font bytes'}
        for directory in directories:
            files[directory + '/Cargo.toml'] = b'fixture package'
            files[directory + '/src/lib.rs'] = b'fixture source'
        for name, raw in files.items():
            path = source/name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(raw)
        inputs = {name: hashlib.sha256(raw).hexdigest() for name, raw in files.items()}
        catalog = payload/'component-lab'; (catalog/'images').mkdir(parents=True)
        variants = []
        for density in (1, 2):
            for viewport in ('regular', 'narrow'):
                width = 2 if viewport == 'regular' else 1
                name = f'images/fixture-{viewport}-{density}.png'
                raw = b'\x89PNG\r\n\x1a\n' + struct.pack('>I', 13) + b'IHDR' + struct.pack('>II', width*density, density)
                (catalog/name).write_bytes(raw)
                variants.append({'id': f'{viewport}-{density}', 'palette': 'fixture', 'state': 'normal',
                                 'density': {'numerator': density, 'denominator': 1},
                                 'viewport': {'id': viewport, 'width': width, 'height': 1}, 'time_ms': 0,
                                 'image': {'path': name, 'width': width*density, 'height': density,
                                           'sha256': hashlib.sha256(raw).hexdigest()}})
        manifest = {'schema_version': 1, 'renderer': {'engine': 'lucent-render', 'scope': 'offscreen-native',
                    'source_commit': commit, 'token_sha256': inputs['design/tokens.json'], 'inputs': inputs},
                    'palettes': [{'id': 'fixture', 'name': 'Fixture'}],
                    'stories': [{'id': 'fixture', 'family': 'fixture', 'title': 'Fixture', 'description': 'Owned fixture',
                                 'recipe': 'fixture', 'source': {'path': 'lucent/apps/lucent-lab/src/lib.rs', 'symbol': 'fixture'},
                                 'tokens': ['fixture'], 'variants': variants}]}
        raw = json.dumps(manifest).encode(); (catalog/'manifest.json').write_bytes(raw)
        reference = source/'site/public/component-lab/manifest.json'; reference.parent.mkdir(parents=True)
        reference.write_bytes(raw)
        spec = importlib.util.spec_from_file_location('receiver_fixture_catalog', ROOT/'scripts/check-component-lab.py')
        checker = importlib.util.module_from_spec(spec); spec.loader.exec_module(checker)
        return catalog, checker.validate(source, catalog)

    @contextlib.contextmanager
    def source_boundaries(self, fixture):
        def git_bytes(source, operation, object_name):
            self.assertEqual(source, fixture.source); self.assertEqual(operation, 'show')
            commit, name = object_name.split(':', 1)
            return fixture.committed[(commit, name)]
        with patch.object(receiver, 'git', side_effect=git_bytes):
            yield

    def test_complete_release_rosters_source_bytes_logs_and_evidence_are_verified(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = self.release_fixture(Path(directory))
            with self.source_boundaries(fixture):
                proof = receiver.validate_release(fixture.payload, fixture.receipt, fixture.binding, fixture.source)
            self.assertEqual(len(proof['ELF_files']), 10)
            self.assertEqual(len(fixture.manifest['support_files']), 29)
            self.assertEqual(len(fixture.manifest['managed_executables']), 6)
            self.assertEqual(proof['tests_passed'], len(receiver.producer.PROFILE['test_gates']))
            self.assertEqual(set(proof['files']), {path.relative_to(fixture.payload).as_posix()
                                                  for path in fixture.payload.rglob('*') if path.is_file()})

    def test_archive_rejects_altered_payload_missing_secure_client_and_extra_member(self):
        for change in ('changed-support', 'missing-secure', 'extra', 'source-mismatch', 'false-scope', 'wrong-workflow'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                fixture = self.release_fixture(Path(directory))
                if change == 'changed-support': (fixture.payload/'support/scripts/clipboard_store.py').write_bytes(b'changed')
                if change == 'missing-secure': fixture.manifest['binaries'].pop('lucent-greeter'); fixture.save()
                if change == 'extra': (fixture.payload/'unaccounted').write_bytes(b'extra')
                if change == 'source-mismatch': fixture.committed[(fixture.binding['source_commit'], 'scripts/clipboard_store.py')] = b'other source'
                if change == 'false-scope': fixture.build['runtime_ABI_accepted'] = True; fixture.save()
                if change == 'wrong-workflow': fixture.build['provenance'] = dict(fixture.build['provenance'], GITHUB_SHA='f'*40); fixture.save()
                with self.source_boundaries(fixture), self.assertRaises(ValueError):
                    receiver.validate_release(fixture.payload, fixture.receipt, fixture.binding, fixture.source)

    def test_archive_rejects_test_evidence_forgery_and_missing_resource_admission(self):
        for change in ('test-count', 'test-stage', 'budget', 'admission'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                fixture = self.release_fixture(Path(directory))
                if change == 'test-count': fixture.build['steps'][4]['tests_passed'] = 999
                if change == 'test-stage': fixture.build['steps'].pop(4)
                if change == 'budget': fixture.build['budget'] = dict(fixture.build['budget'], container_init='sleep')
                if change == 'admission':
                    path = fixture.payload/'logs/memory-admission.log'
                    rows = [json.loads(line) for line in path.read_text().splitlines()]
                    rows[0]['snapshot']['effective_available_mib'] = 100
                    raw = b''.join(json.dumps(row).encode() + b'\n' for row in rows)
                    path.write_bytes(raw)
                    fixture.build['memory_admission_evidence']['sha256'] = hashlib.sha256(raw).hexdigest()
                fixture.save()
                with self.source_boundaries(fixture), self.assertRaises(ValueError):
                    receiver.validate_release(fixture.payload, fixture.receipt, fixture.binding, fixture.source)

    def test_actual_catalog_checker_rejects_changed_missing_extra_pixels_and_stale_inputs(self):
        for cause in ('changed-PNG', 'missing-PNG', 'extra-PNG', 'changed-producer', 'missing-variant'):
            with self.subTest(cause=cause), tempfile.TemporaryDirectory() as directory:
                fixture = self.release_fixture(Path(directory))
                catalog = fixture.payload/'component-lab'
                first = sorted((catalog/'images').iterdir())[0]
                if cause == 'changed-PNG': first.write_bytes(first.read_bytes() + b'changed')
                if cause == 'missing-PNG': first.unlink()
                if cause == 'extra-PNG': (catalog/'images/extra.png').write_bytes(first.read_bytes())
                if cause == 'changed-producer': (fixture.source/'lucent/crates/lucent-ui/src/lib.rs').write_bytes(b'changed')
                if cause == 'missing-variant':
                    value = json.loads((catalog/'manifest.json').read_text()); value['stories'][0]['variants'].pop()
                    (catalog/'manifest.json').write_text(json.dumps(value))
                with self.source_boundaries(fixture), self.assertRaises(ValueError):
                    receiver.validate_release(fixture.payload, fixture.receipt, fixture.binding, fixture.source)

    def test_actual_receiver_accepts_distinct_hashed_Arch_pixels_without_changing_reviewed_reference(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = self.release_fixture(Path(directory))
            catalog = fixture.payload/'component-lab'
            reference = fixture.source/'site/public/component-lab/manifest.json'
            old_reference = reference.read_bytes()
            value = json.loads((catalog/'manifest.json').read_text())
            first = value['stories'][0]['variants'][0]['image']
            path = catalog/first['path']; path.write_bytes(path.read_bytes() + b'owned distinct Arch fixture bytes')
            first['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
            (catalog/'manifest.json').write_text(json.dumps(value))
            profile = json.loads((fixture.payload/'qualification-tools/ci/arch-release-profile.json').read_text())
            evidence_path = fixture.payload/'component-lab-evidence.json'
            evidence = json.loads(evidence_path.read_text())
            with patch.object(receiver.producer, 'PROFILE', profile):
                actual = receiver.producer.component_lab_evidence(
                    fixture.source, catalog, fixture.matrix,
                    (fixture.payload/'logs/component-lab-export.log').read_text(), fixture.build['standalone_lab'])
            evidence.update(actual)
            raw = json.dumps(evidence).encode(); evidence_path.write_bytes(raw)
            fixture.build['component_lab_export']['sha256'] = hashlib.sha256(raw).hexdigest()
            fixture.save()
            with self.source_boundaries(fixture):
                proof = receiver.validate_release(fixture.payload, fixture.receipt, fixture.binding, fixture.source)
            self.assertEqual(proof['component_matrix']['captures'], 4)
            self.assertEqual(reference.read_bytes(), old_reference)

    def receiver_fixture(self, root):
        fixture = self.release_fixture(root)
        archive = root/'plain-fixture.tar'
        with tarfile.open(archive, 'w', format=tarfile.USTAR_FORMAT) as tar:
            for path in sorted(fixture.payload.rglob('*')):
                info = tar.gettarinfo(str(path), path.relative_to(fixture.payload).as_posix())
                info.uid = info.gid = 0; info.uname = info.gname = ''
                if path.is_file():
                    with path.open('rb') as stream: tar.addfile(info, stream)
                else: tar.addfile(info)
        # The decompressor is stubbed below; this archive never invokes zstd.
        fixture.receipt['archive_sha256'] = hashlib.sha256(archive.read_bytes()).hexdigest()
        receipt = root/'supplied-receipt.json'; receipt.write_text(json.dumps(fixture.receipt))
        zipped = self.zip_fixture(root, [('artifact-receipt.json', receipt.read_bytes()),
                                         ('lucent-polish.tar.zst', archive.read_bytes())])
        _, api = self.api_fixture()
        fixture.binding.update(artifact_bytes=zipped.stat().st_size, artifact_zip_sha256=hashlib.sha256(zipped.read_bytes()).hexdigest())
        api['artifact.json'].update(size_in_bytes=fixture.binding['artifact_bytes'], digest='sha256:' + fixture.binding['artifact_zip_sha256'])
        metadata = root/'metadata'; metadata.mkdir()
        fixture.binding['metadata_sha256'] = {}
        for name, value in api.items():
            raw = json.dumps(value).encode(); (metadata/name).write_bytes(raw)
            fixture.binding['metadata_sha256'][name] = hashlib.sha256(raw).hexdigest()
        binding = root/'binding.json'; binding.write_text(json.dumps(fixture.binding))
        args = SimpleNamespace(binding=binding, binding_sha256=hashlib.sha256(binding.read_bytes()).hexdigest(),
                               metadata=metadata, archive=zipped, receipt=receipt, source=fixture.source,
                               output=root/'received', providers=None, providers_sha256=None)
        return fixture, args

    @contextlib.contextmanager
    def receiver_boundaries(self, fixture):
        outputs = StaticElfInspection().outputs()
        with (self.source_boundaries(fixture), patch.object(receiver, 'exact_source') as source_check,
              patch.object(receiver.shutil, 'disk_usage', return_value=SimpleNamespace(free=8*1024**3)),
              patch.object(receiver, 'decompress', side_effect=shutil.copyfile),
              patch.object(elf, 'readelf', side_effect=lambda fd, option: outputs[option])):
            yield source_check

    def test_owned_receiver_pipeline_retains_originals_and_all_acceptance_flags_false(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); fixture, args = self.receiver_fixture(root)
            input_digest = hashlib.sha256(args.archive.read_bytes()).hexdigest()
            with self.receiver_boundaries(fixture) as source_check:
                report = receiver.verify(args)
            self.assertEqual(source_check.call_count, 2)
            self.assertTrue(report['artifact_verified'])
            self.assertEqual(len(report['static_ELF_inspection']), 10)
            self.assertTrue(all(report[key] is False for key in receiver.FALSE_SCOPES))
            self.assertFalse(report['ELF_executed']); self.assertFalse(report['VM_contacted'])
            self.assertIn('all four jobs', report['missing_acceptance'][0])
            self.assertEqual((args.output/'originals/artifact.zip').read_bytes(), args.archive.read_bytes())
            self.assertEqual((args.output/'originals/artifact-receipt.json').read_bytes(), args.receipt.read_bytes())
            self.assertEqual(hashlib.sha256(args.archive.read_bytes()).hexdigest(), input_digest)
            self.assertEqual(json.loads((args.output/'receiver-report.json').read_text()), report)
            self.assertEqual(stat.S_IMODE(args.output.stat().st_mode), 0o700)
            self.assertTrue(all(stat.S_IMODE(path.stat().st_mode) == 0o600
                                for path in (args.output/'payload').rglob('*') if path.is_file()))

    def test_receiver_does_not_overwrite_existing_output_or_accept_untrusted_binding(self):
        for cause in ('output-exists', 'binding-hash', 'API-hash'):
            with self.subTest(cause=cause), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); fixture, args = self.receiver_fixture(root)
                if cause == 'output-exists': args.output.mkdir(); (args.output/'keep').write_bytes(b'keep')
                if cause == 'binding-hash': args.binding.write_bytes(b'{}')
                if cause == 'API-hash': (args.metadata/'run.json').write_bytes(b'{}')
                with self.receiver_boundaries(fixture), self.assertRaises(ValueError):
                    receiver.verify(args)
                if cause == 'output-exists': self.assertEqual((args.output/'keep').read_bytes(), b'keep')
                else: self.assertFalse(args.output.exists())

    def test_missing_binding_fields_refuse_before_creating_review_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); fixture, args = self.receiver_fixture(root)
            binding = json.loads(args.binding.read_text()); binding.pop('source_tree')
            args.binding.write_text(json.dumps(binding))
            args.binding_sha256 = hashlib.sha256(args.binding.read_bytes()).hexdigest()
            with self.assertRaisesRegex(ValueError, 'complete_explicit_binding'):
                receiver.verify(args)
            self.assertFalse(args.output.exists())

    def test_receiver_failure_retains_owned_raw_inputs_with_explicit_failure_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); fixture, args = self.receiver_fixture(root)
            args.receipt.write_bytes(args.receipt.read_bytes() + b'\n')
            with self.receiver_boundaries(fixture), self.assertRaisesRegex(ValueError, 'supplied_receipt'):
                receiver.verify(args)
            report = json.loads((args.output/'verification-failed.json').read_text())
            self.assertFalse(report['artifact_verified'])
            self.assertTrue(all(report[key] is False for key in receiver.FALSE_SCOPES))
            self.assertEqual((args.output/'originals/artifact.zip').read_bytes(), args.archive.read_bytes())


class StaticElfInspection(unittest.TestCase):
    def outputs(self):
        return {'--file-header': "  Class: ELF64\n  Data: 2's complement, little endian\n  Machine: Advanced Micro Devices X86-64\n  Type: DYN (Position-Independent Executable file)\n",
                '--program-headers': '[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]\n',
                '--dynamic': '0x0001 (NEEDED) Shared library: [libc.so.6]\n',
                '--version-info': "Version needs section '.gnu.version_r' contains 1 entries:\n  File: libc.so.6 Cnt: 1\n  Name: GLIBC_2.34 Flags: none Version: 2\n"}

    def reference(self):
        return {'aliases': {'libc.so.6': ['libc'], 'ld-linux-x86-64.so.2': ['loader']},
                'providers': {'libc': {'sha256': 'a'*64, 'soname': 'libc.so.6',
                    'defined_dynamic_symbols': [['fixture@@GLIBC_2.34', 'FUNC', 'GLOBAL', 'DEFAULT', 0]]},
                    'loader': {'sha256': 'b'*64, 'soname': 'ld-linux-x86-64.so.2', 'defined_dynamic_symbols': []}}}

    def test_static_architecture_interpreter_dependencies_and_versions(self):
        model = elf.parse(self.outputs())
        self.assertEqual(model['needed_sonames'], ['libc.so.6'])
        comparison = elf.compare_reference(model, self.reference())
        self.assertTrue(comparison['direct_sonames_and_versions_match_reference'])
        self.assertFalse(comparison['runtime_ABI_accepted'])
        self.assertFalse(comparison['symbol_resolution_checked'])
        self.assertFalse(comparison['recursive_provider_closure_checked'])

    def test_wrong_machine_interpreter_and_loader_overrides_are_refused(self):
        for key, old, new in [('--file-header', 'ELF64', 'ELF32'), ('--file-header', 'Advanced Micro Devices X86-64', 'AArch64'),
                              ('--program-headers', '/lib64/ld-linux-x86-64.so.2', '/foreign/loader'),
                              ('--dynamic', '(NEEDED)', '(RUNPATH)')]:
            outputs = self.outputs(); outputs[key] = outputs[key].replace(old, new)
            with self.subTest(key=key, new=new), self.assertRaises(ValueError):
                elf.parse(outputs)

    def test_missing_ambiguous_and_insufficient_cached_providers_are_reported(self):
        model = elf.parse(self.outputs())
        for mutate in (lambda value: value['aliases'].pop('libc.so.6'),
                       lambda value: value['aliases']['libc.so.6'].append('loader'),
                       lambda value: value['providers']['libc'].update(defined_dynamic_symbols=[])):
            reference = self.reference(); mutate(reference)
            result = elf.compare_reference(model, reference)
            self.assertFalse(result['direct_sonames_and_versions_match_reference'])
            self.assertTrue(result['mismatches'])
            self.assertFalse(result['runtime_ABI_accepted'])

    def test_inspection_uses_pinned_descriptor_and_never_executes_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'client'; path.write_bytes(b'owned non-ELF fixture')
            expected = hashlib.sha256(path.read_bytes()).hexdigest()
            outputs = self.outputs()
            with patch.object(elf, 'readelf', side_effect=lambda fd, option: outputs[option]) as inspect:
                result = elf.inspect(path, expected)
            self.assertEqual(inspect.call_count, 4)
            self.assertIsNone(result['reference_comparison'])
            self.assertFalse(result['ELF_executed'])
            self.assertFalse(result['runtime_ABI_accepted'])
            with self.assertRaisesRegex(ValueError, 'hash_mismatch'):
                elf.inspect(path, '0'*64)


if __name__ == '__main__':
    unittest.main()
