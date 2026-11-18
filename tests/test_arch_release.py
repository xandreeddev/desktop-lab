"""Pure hosted-release evidence guards; no compiler, network or VM is used."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
import copy
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('lucent_arch_release_tests', ROOT / 'ci/arch_release.py')
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ArchReleaseEvidence(unittest.TestCase):
    def setUp(self):
        pinned = patch.dict(release.PROFILE, source_commit='a'*40, source_tree='b'*40)
        pinned.start()
        self.addCleanup(pinned.stop)

    def test_default_is_inert_with_unbound_source_and_dynamic_evidence(self):
        output = io.StringIO()
        with (patch.object(release.sys, 'argv', ['arch-release']), contextlib.redirect_stdout(output),
              patch.dict(release.PROFILE, source_commit=None, source_tree=None),
              patch.object(release.subprocess, 'run', side_effect=AssertionError('no subprocess')),
              patch.object(release.subprocess, 'check_output', side_effect=AssertionError('no subprocess'))):
            self.assertEqual(release.main(), 0)
        value = json.loads(output.getvalue())
        self.assertFalse(value['actions'])
        self.assertFalse(value['source_bound'])
        self.assertIsNone(value['source_commit'])
        self.assertIn('actual source-bound', value['planned_tests'])
        self.assertIn('workspace-tests', value['planned_test_gates'])

    def test_run_refuses_unbound_source_before_any_action(self):
        from argparse import Namespace
        with (patch.dict(release.PROFILE, source_commit=None, source_tree=None),
              patch.object(release.subprocess, 'run', side_effect=AssertionError('no subprocess')),
              patch.object(release.subprocess, 'check_output', side_effect=AssertionError('no subprocess'))):
            with self.assertRaisesRegex(RuntimeError, 'source_pin'):
                release.build(Namespace())

    def test_source_pin_is_exact_and_inspection_does_not_build(self):
        for commit, tree in [(None, None), ('main', 'a'*40), ('a'*40, None), ('a'*40, 'b'*39)]:
            with patch.dict(release.PROFILE, source_commit=commit, source_tree=tree):
                with self.assertRaisesRegex(RuntimeError, 'source_pin'):
                    release.source_pin()
        output = io.StringIO()
        with (patch.dict(release.PROFILE, source_commit='a'*40, source_tree='b'*40),
              patch.object(release.sys, 'argv', ['arch-release', '--require-source-pin']),
              patch.object(release, 'build', side_effect=AssertionError('no build')),
              contextlib.redirect_stdout(output)):
            self.assertEqual(release.main(), 0)
        self.assertEqual(json.loads(output.getvalue()), {'source_commit': 'a'*40, 'source_tree': 'b'*40})

    def test_even_pinned_build_is_hosted_only_before_any_subprocess(self):
        from argparse import Namespace
        with (patch.dict(release.PROFILE, source_commit='a'*40, source_tree='b'*40),
              patch.dict(os.environ, {}, clear=True),
              patch.object(release.subprocess, 'check_output', side_effect=AssertionError('no subprocess'))):
            with self.assertRaisesRegex(RuntimeError, 'owner_push'):
                release.build(Namespace(source_directory=Path('/fixture'), target_directory=Path('/target'), output=Path('/out')))

    def test_profile_preserves_full_release_roster_and_support_dependency_closure(self):
        profile = release.PROFILE
        self.assertEqual(set(profile['managed_executables']), {'lucent-desktop', 'lucent-cli', 'lucent-menu',
                                                            'lucent-lock', 'lucent-greeter', 'lucent-polkit'})
        self.assertEqual((len(profile['acceptance_executables']), len(profile['support_files']),
                          len(profile['example_sources'])), (3, 29, 5))
        self.assertEqual(len(set(profile['support_files'])), 29)
        self.assertTrue({'scripts/clipboard_store.py', 'scripts/lucent-compat.py', 'scripts/lucent-controls.py',
                         'scripts/lucent-menu.py', 'scripts/lucent-menu-routes.py', 'scripts/lucent-session.py',
                         'scripts/lucent_readiness.py', 'scripts/menu_model.py'} <= set(profile['support_files']))
        for name in profile['support_files'] + profile['example_sources'] + profile['license_files']:
            self.assertTrue((ROOT/name).is_file(), name)
        self.assertIn('lucent-session', profile['build_packages'])
        self.assertIn('lucent-polkit', profile['build_packages'])
        gates = {gate['name']: gate for gate in profile['test_gates']}
        self.assertEqual(gates['workspace-tests']['arguments'], ['--workspace', '--all-targets'])
        self.assertEqual(gates['workspace-doctests']['arguments'], ['--workspace', '--doc'])
        self.assertTrue(gates['workspace-tests']['full'])
        self.assertNotIn('expected_test_counts', profile)
        self.assertNotIn('producer_inputs', profile['component_lab'])
        self.assertNotIn('baseline_file_hashes', profile)
        self.assertTrue({'LICENSE', 'lucent/crates/lucent-services/assets/material/LICENSE',
                         'lucent/apps/lucent-desktop/assets/LICENSE-Lucid.txt',
                         'lucent/apps/lucent-desktop/assets/OFL-GoogleSansFlex.txt'} <= set(profile['license_files']))
        fallback = json.loads((ROOT/'lucent/crates/lucent-ui/assets/fallback-fonts.json').read_text())
        self.assertEqual({name for name in profile['license_files'] if name.startswith('lucent/crates/lucent-ui/assets/')},
                         {'lucent/crates/lucent-ui/assets/' + row['license_file'] for row in fallback['fonts'].values()})

    def test_license_copies_preserve_colliding_basenames_and_exact_source_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, out = root/'source', root/'output'
            source.mkdir(); out.mkdir()
            fixtures = {'LICENSE': b'repository notice', 'vendor/material/LICENSE': b'Material notice'}
            for name, raw in fixtures.items():
                path = source/name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(raw)
            with patch.dict(release.PROFILE, license_files=list(fixtures)):
                rows = release.copy_license_files(source, out, fixtures.__getitem__)
            self.assertEqual({row['path'] for row in rows.values()}, {'licenses/' + name for name in fixtures})
            for name, raw in fixtures.items():
                self.assertEqual((out/rows[name]['path']).read_bytes(), raw)
                self.assertEqual(rows[name]['sha256'], release.digest(source/name))
                self.assertEqual(rows[name]['bytes'], len(raw))
            other = root/'refused'; other.mkdir()
            with patch.dict(release.PROFILE, license_files=['LICENSE']):
                with self.assertRaisesRegex(RuntimeError, 'committed_license'):
                    release.copy_license_files(source, other, lambda name: b'different committed bytes')
            self.assertEqual(list(other.iterdir()), [])

    def test_actual_multi_harness_and_doctest_names_are_recorded_without_historical_counts(self):
        text = ('test first::one ... ok\ntest first::gpu ... ignored, requires Vulkan\n'
                'test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0s\n'
                'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
                'test crates/api/src/lib.rs - Example (line 42) ... ok\n'
                'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n')
        result = release.test_summary(text, ['one'], full=True)
        self.assertEqual((result['tests_passed'], result['harness_count']), (2, 3))
        self.assertEqual(result['ignored_test_names'], ['first::gpu'])
        self.assertEqual(result['passed_test_names'][-1], 'crates/api/src/lib.rs - Example (line 42)')
        for changed in (text.replace('first::gpu ... ignored, requires Vulkan', 'first::gpu ... ok'),
                        text.replace('... ok', '... FAILED', 1), text.replace('0 filtered out', '2 filtered out', 1)):
            with self.assertRaises(RuntimeError):
                release.test_summary(changed, full=True)

    def test_empty_test_selection_cannot_pass_but_empty_doctest_harness_can(self):
        text = 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
        with self.assertRaises(RuntimeError):
            release.test_summary(text)
        self.assertEqual(release.test_summary(text, full=True, allow_empty=True)['tests_passed'], 0)
        with self.assertRaises(RuntimeError):
            release.test_summary('', allow_empty=True)

    def test_one_harness_cannot_supply_another_harness_missing_test_records(self):
        text = ('test first::one ... ok\ntest first::two ... ok\n'
                'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
                'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n')
        with self.assertRaisesRegex(RuntimeError, 'individual_test_results'):
            release.test_summary(text, full=True)

    def test_pid_budget_and_reaping_init_are_required(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.resource_files(root)
            (root/'pids.max').write_text('max')
            with self.assertRaisesRegex(RuntimeError, 'PID'):
                release.resource_snapshot(root, root/'meminfo', root/'init')
            self.resource_files(root)
            (root/'init').write_text('sleep')
            with self.assertRaisesRegex(RuntimeError, 'reaping_init'):
                release.resource_snapshot(root, root/'meminfo', root/'init')

    def test_arch_png_bytes_may_differ_from_reviewed_catalog_but_inventory_must_match(self):
        with tempfile.TemporaryDirectory() as directory:
            source, catalog, matrix, manifest, expected = self.catalog_fixture(Path(directory))
            adapter = 'lucent GPU: AdapterInfo { name: "llvmpipe", device_type: Cpu, backend: Vulkan }\n'
            with patch.dict(release.PROFILE, component_lab=expected):
                original_reference = (source/'reference.json').read_bytes()
                (catalog/'images/one.png').write_bytes(b'new Arch software-driver pixels')
                manifest['stories'][0]['variants'][0]['image']['sha256'] = release.digest(catalog/'images/one.png')
                (catalog/'manifest.json').write_text(json.dumps(manifest))
                result = release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})
                self.assertEqual(result['captures']['images/one.png']['sha256'], release.digest(catalog/'images/one.png'))
                self.assertEqual((source/'reference.json').read_bytes(), original_reference)
                manifest['stories'][0]['variants'][0]['viewport']['width'] = 2
                (catalog/'manifest.json').write_text(json.dumps(manifest))
                with self.assertRaisesRegex(RuntimeError, 'reviewed_source_matrix'):
                    release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})



    def context(self):
        return dict(GITHUB_ACTIONS='true', GITHUB_REPOSITORY='xandreeddev/desktop-lab',
                    GITHUB_ACTOR='xandreeddev', GITHUB_TRIGGERING_ACTOR='xandreeddev',
                    GITHUB_EVENT_NAME='push', GITHUB_REF='refs/heads/ci/lucent-polish-coherent-release',
                    GITHUB_SHA='a'*40, LUCENT_WORKFLOW_SHA='a'*40, GITHUB_RUN_ID='123',
                    GITHUB_RUN_ATTEMPT='1', LUCENT_CI_IMAGE=release.PROFILE['container_image'])

    def test_context_requires_owner_repository_actor_and_rerun_actor(self):
        self.assertEqual(release.hosted_context(self.context())['GITHUB_ACTOR'], 'xandreeddev')
        for key, value in [('GITHUB_REPOSITORY', 'fork/desktop-lab'), ('GITHUB_ACTOR', 'other'),
                           ('GITHUB_TRIGGERING_ACTOR', 'other'), ('GITHUB_EVENT_NAME', 'pull_request'),
                           ('GITHUB_REF', 'refs/heads/main'), ('GITHUB_RUN_ID', '0'),
                           ('GITHUB_SHA', 'main'), ('LUCENT_CI_IMAGE', 'archlinux:latest')]:
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                release.hosted_context(dict(self.context(), **{key: value}))

    def resource_files(self, root, maximum=10*1024**3, current=1024**3, swap='0'):
        (root/'cpu.max').write_text('300000 100000\n')
        (root/'pids.max').write_text('1024\n')
        (root/'init').write_text('docker-init\n')
        (root/'memory.max').write_text(str(maximum))
        (root/'memory.current').write_text(str(current))
        (root/'memory.swap.max').write_text(swap)
        (root/'memory.stat').write_text('anon 268435456\nfile 805306368\n')
        (root/'meminfo').write_text('MemAvailable: 12582912 kB\n')

    def test_remote_budget_uses_effective_cgroup_memory_and_no_swap(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.resource_files(root)
            value = release.resource_snapshot(root, root/'meminfo', root/'init')
            self.assertEqual(value['effective_available_mib'], 9*1024)
            self.assertEqual(value['memory_swap_max_bytes'], 0)
            self.assertEqual(value['memory_stat_anon_bytes'], 268435456)
            self.assertEqual(value['memory_stat_file_bytes'], 805306368)
            self.assertFalse(value['local_host_policy_changed'])
            (root/'memory.swap.max').write_text('10737418240')
            with self.assertRaisesRegex(RuntimeError, 'swap'):
                release.resource_snapshot(root, root/'meminfo', root/'init')
            self.resource_files(root, current=10*1024**3 + 1)
            saturated = release.resource_snapshot(root, root/'meminfo', root/'init')
            self.assertEqual(saturated['effective_available_mib'], 0)
            self.assertEqual(saturated['memory_current_bytes'], 10*1024**3 + 1)

    def test_budget_refuses_unbounded_or_different_cpu_memory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.resource_files(root)
            for value in ('max 100000', '400000 100000', '300000 0'):
                (root/'cpu.max').write_text(value)
                with self.assertRaises(RuntimeError):
                    release.resource_snapshot(root, root/'meminfo', root/'init')
            self.resource_files(root)
            (root/'memory.max').write_text('max')
            with self.assertRaises(RuntimeError):
                release.resource_snapshot(root, root/'meminfo', root/'init')

    def test_summary_requires_actual_passed_names_and_exact_nonempty_count(self):
        text = 'test module::required ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
        value = release.test_summary(text, ['required'], True)
        self.assertEqual(value['tests_passed'], 1)
        self.assertEqual(value['required_test_names_verified'], ['required'])
        for changed in (text.replace('... ok', '... ignored'), text.replace('1 passed', '2 passed'),
                        text.replace('0 failed', '1 failed'), text.replace('0 filtered out', '1 filtered out')):
            with self.assertRaises(RuntimeError):
                release.test_summary(changed, ['required'], True)


    def test_names_from_other_or_ignored_tests_cannot_satisfy_requirement(self):
        text = ('test module::required ... ignored\ntest module::other ... ok\n'
                'test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0s\n')
        with self.assertRaisesRegex(RuntimeError, 'named'):
            release.test_summary(text, ['required'])

    def test_freshness_rejects_local_release_payload_but_keeps_registry_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'release/deps').mkdir(parents=True)
            (root/'release/deps/libserde-123.rlib').write_text('registry')
            self.assertTrue(release.release_freshness(root, ['lucent-api'], ['lucent_api'])['release_local_artifacts_absent'])
            (root/'release/deps/liblucent_api-123.rlib').write_text('old')
            with self.assertRaisesRegex(RuntimeError, 'residue'):
                release.release_freshness(root, ['lucent-api'], ['lucent_api'])

    def test_freshness_checks_extra_bin_targets_and_nested_fingerprints(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root/'release/.fingerprint/lucent-session-abc'
            path.mkdir(parents=True)
            (path/'old').write_text('old')
            with self.assertRaises(RuntimeError):
                release.release_freshness(root, ['lucent-session'], ['lucent-lock', 'lucent-greeter'])

    def test_actual_ffmpeg_header_hashes_cover_each_ffi_library(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('libavcodec', 'libavformat', 'libavutil', 'libswscale'):
                (root/name).mkdir()
                (root/name/'version.h').write_text('#define VERSION 1\n')
            result = release.ffmpeg_headers(root)
            self.assertEqual(len(result), 4)
            self.assertTrue(all(len(value) == 64 for value in result.values()))
            (root/'libavcodec/alias.h').symlink_to(root/'libavcodec/version.h')
            with self.assertRaises(RuntimeError):
                release.ffmpeg_headers(root)



    def test_success_envelope_is_exact_two_regular_root_files_and_refuses_reuse(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root/'lucent-polish.tar.zst'
            package.write_bytes(b'unit-fixture-not-a-release-archive')
            receipt = {'source_commit': release.PROFILE['source_commit'], 'archive': package.name,
                       'archive_sha256': release.digest(package)}
            destination = root/'envelope'
            release.envelope(package, receipt, destination)
            self.assertEqual({p.name for p in destination.iterdir()}, {package.name, 'artifact-receipt.json'})
            self.assertEqual(json.loads((destination/'artifact-receipt.json').read_text()), receipt)
            self.assertEqual(release.digest(destination/package.name), receipt['archive_sha256'])
            with self.assertRaisesRegex(RuntimeError, 'fresh'):
                release.envelope(package, receipt, destination)
            with self.assertRaisesRegex(RuntimeError, 'actual_archive'):
                release.envelope(package, dict(receipt, archive_sha256='0'*64), root/'wrong')
            self.assertFalse((root/'wrong').exists())

    def test_workflow_upload_inputs_are_single_directory_values(self):
        workflow = (ROOT/'.github/workflows/arch-release.yml').read_text()
        self.assertIn('path: ${{ runner.temp }}/arch-release-envelope\n', workflow)
        self.assertIn('path: ${{ runner.temp }}/arch-diagnostics\n', workflow)
        self.assertNotIn('path: |', workflow)
        self.assertIn('--memory 10g --memory-swap 10g', workflow)
        self.assertIn('ref: ${{ steps.source-pin.outputs.commit }}', workflow)
        self.assertIn('branches: [ci/lucent-polish-coherent-release]', workflow)
        self.assertIn('options: --init --cpus 3', workflow)
        self.assertIn('retention-days: 30', workflow)
        self.assertIn('retention-days: 14', workflow)
        self.assertNotIn('workflow_dispatch', workflow)

    def test_fresh_three_sample_admission_refuses_any_low_sample(self):
        with patch.object(release.time, 'sleep') as sleep:
            with patch.object(release, 'resource_snapshot', side_effect=[{'effective_available_mib': n} for n in [5000, 4500, 4800]]):
                self.assertEqual(release.admit_memory(), [5000, 4500, 4800])
                self.assertEqual(sleep.call_count, 3)
            with patch.object(release, 'resource_snapshot', side_effect=[{'effective_available_mib': n} for n in [5000, 4499, 6000]]):
                with self.assertRaisesRegex(RuntimeError, 'memory_admission'):
                    release.admit_memory()

    def test_refused_admission_sample_is_persisted_before_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'memory.log'
            samples = [dict(host_available_mib=8000, memory_current_bytes=current,
                            memory_max_bytes=10737418240, memory_stat_anon_bytes=100,
                            memory_stat_file_bytes=200, effective_available_mib=available)
                       for current, available in [(10, 7000), (20, 4499)]]
            with patch.object(release, 'resource_snapshot', side_effect=samples), patch.object(release.time, 'sleep'):
                with self.assertRaisesRegex(RuntimeError, 'memory_admission'):
                    release.admit_memory(lambda index, sample: release.log_memory_sample(path, 'capture', index, sample))
            saved = [json.loads(line) for line in path.read_text().splitlines()]
            self.assertEqual(saved, [{'phase': 'capture', 'sample_index': index, 'snapshot': sample}
                                     for index, sample in enumerate(samples)])
            self.assertEqual(saved[-1]['snapshot']['effective_available_mib'], 4499)

    def private_target_fixture(self, root):
        temporary = root/'runner'; temporary.mkdir()
        protected = [root/name for name in ('source', 'output', 'envelope', 'cargo-home')]
        for path in protected:
            path.mkdir(); (path/'preserve').write_bytes(path.name.encode())
        return temporary/'lucent-target-polish', temporary, protected

    def test_cleanup_removes_only_fresh_target_and_preserves_copies_and_registry(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target, temporary, protected = self.private_target_fixture(root)
            authority = release.PrivateCargoTarget(target, temporary, protected)
            (target/'release').mkdir(); (target/'release/temporary').write_bytes(b'compiled bytes')
            # Cargo output may contain a link; descriptor-safe rmtree must only
            # unlink it, leaving its unrelated source/registry target intact.
            (target/'registry-link').symlink_to(protected[-1], target_is_directory=True)
            out = protected[1]
            row = {'path': 'preserve', 'bytes': len(b'output'), 'sha256': release.digest(out/'preserve')}
            before = release.verify_copies(out, [row])
            receipt = authority.remove()
            self.assertEqual(before, release.verify_copies(out, [row]))
            self.assertFalse(target.exists())
            self.assertTrue(receipt['removed'])
            self.assertTrue(all((path/'preserve').read_bytes() == path.name.encode() for path in protected))
            with self.assertRaises(RuntimeError):
                authority.remove()

    def test_cleanup_refuses_replaced_root_directory_or_symlink(self):
        for replacement in ('directory', 'symlink'):
            with self.subTest(replacement=replacement), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                target, temporary, protected = self.private_target_fixture(root)
                authority = release.PrivateCargoTarget(target, temporary, protected)
                target.rename(temporary/'original-target')
                if replacement == 'directory':
                    target.mkdir(); (target/'foreign').write_bytes(b'foreign')
                else:
                    target.symlink_to(protected[0], target_is_directory=True)
                try:
                    with self.assertRaisesRegex(RuntimeError, 'identity'):
                        authority.remove()
                    self.assertTrue((temporary/'original-target').is_dir())
                    self.assertEqual((protected[0]/'preserve').read_bytes(), b'source')
                    if replacement == 'directory':
                        self.assertEqual((target/'foreign').read_bytes(), b'foreign')
                finally:
                    os.close(authority.parent_fd)

    def test_cleanup_authority_refuses_existing_wrong_named_or_protected_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target, temporary, protected = self.private_target_fixture(root)
            for candidate in (protected[0], temporary/'different-name'):
                with self.assertRaisesRegex(RuntimeError, 'fresh_exact'):
                    release.PrivateCargoTarget(candidate, temporary, protected)
            for forbidden in (target, temporary, target/'registry'):
                with self.assertRaisesRegex(RuntimeError, 'fresh_exact'):
                    release.PrivateCargoTarget(target, temporary, [*protected, forbidden])
                self.assertFalse(target.exists())
            target.mkdir()
            with self.assertRaisesRegex(RuntimeError, 'fresh_exact'):
                release.PrivateCargoTarget(target, temporary, protected)

    def test_changed_copied_payload_refuses_cleanup_eligibility(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            (out/'client').write_bytes(b'original')
            row = {'path': 'client', 'bytes': 8, 'sha256': release.digest(out/'client')}
            release.verify_copies(out, [row])
            (out/'client').write_bytes(b'modified')
            with self.assertRaisesRegex(RuntimeError, 'copied_release_bytes'):
                release.verify_copies(out, [row])

    def catalog_fixture(self, root):
        source, catalog = root/'source', root/'catalog'
        source.mkdir(); catalog.mkdir(); (catalog/'images').mkdir()
        (source/'producer.rs').write_text('committed producer fixture')
        png = catalog/'images/one.png'; png.write_bytes(b'fixture-pixels-checked-by-hash')
        matrix = {'stories': 1, 'captures': 1, 'palettes': 1,
                  'source_commit': release.PROFILE['source_commit'], 'rendering_inputs': 1}
        manifest = {'renderer': {'engine': 'lucent-render', 'scope': 'offscreen-native',
                     'source_commit': release.PROFILE['source_commit'],
                     'inputs': {'producer.rs': release.digest(source/'producer.rs')}},
                    'palettes': [{'id': 'fixture'}], 'stories': [{'id': 'one', 'source': {'path': 'producer.rs', 'symbol': 'view'},
                    'variants': [{'id': 'fixture', 'palette': 'fixture', 'state': 'normal', 'density': {'numerator': 1, 'denominator': 1},
                    'viewport': {'id': 'regular', 'width': 1, 'height': 1}, 'time_ms': 0, 'image': {'path': 'images/one.png',
                     'sha256': release.digest(png)}}]}]}
        (catalog/'manifest.json').write_text(json.dumps(manifest))
        (source/'reference.json').write_text(json.dumps(manifest))
        expected = dict(release.PROFILE['component_lab'], reference_manifest='reference.json')
        return source, catalog, matrix, manifest, expected

    def test_component_export_records_current_producers_and_truthful_software_scope(self):
        with tempfile.TemporaryDirectory() as directory:
            source, catalog, matrix, _, expected = self.catalog_fixture(Path(directory))
            with patch.dict(release.PROFILE, component_lab=expected):
                result = release.component_lab_evidence(source, catalog, matrix,
                    'lucent GPU: AdapterInfo { name: "llvmpipe", device_type: Cpu, backend: Vulkan }\n', {'sha256': 'a'*64})
            self.assertEqual(result['manifest_sha256'], release.digest(catalog/'manifest.json'))
            self.assertEqual(len(result['captures']), 1)
            self.assertEqual(result['producer_inputs']['producer.rs'], release.digest(source/'producer.rs'))
            self.assertTrue(result['software_Vulkan'])
            self.assertFalse(result['Wayland_compositor_tested'])
            self.assertFalse(result['VM_native_accepted'])
            self.assertFalse(result['cross_driver_pixel_identity_required'])

    def test_component_export_rejects_old_source_missing_producer_and_changed_or_extra_pixels(self):
        with tempfile.TemporaryDirectory() as directory:
            source, catalog, matrix, manifest, expected = self.catalog_fixture(Path(directory))
            adapter = 'lucent GPU: AdapterInfo { name: "llvmpipe", device_type: Cpu, backend: Vulkan }\n'
            with patch.dict(release.PROFILE, component_lab=expected):
                old = copy.deepcopy(manifest); old['renderer']['source_commit'] = '0'*40
                (catalog/'manifest.json').write_text(json.dumps(old))
                with self.assertRaisesRegex(RuntimeError, 'source'):
                    release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})
                missing = copy.deepcopy(manifest); missing['renderer']['inputs'] = {}
                (catalog/'manifest.json').write_text(json.dumps(missing))
                with self.assertRaisesRegex(RuntimeError, 'producer_inventory'):
                    release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})
                (catalog/'manifest.json').write_text(json.dumps(manifest))
                (catalog/'images/one.png').write_bytes(b'changed')
                with self.assertRaisesRegex(RuntimeError, 'capture_hash'):
                    release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})
                (catalog/'images/one.png').write_bytes(b'fixture-pixels-checked-by-hash')
                (catalog/'images/extra.png').write_bytes(b'extra')
                with self.assertRaisesRegex(RuntimeError, 'capture_inventory'):
                    release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})

    def test_component_export_rejects_native_gpu_claim_or_unbound_backend(self):
        with tempfile.TemporaryDirectory() as directory:
            source, catalog, matrix, _, expected = self.catalog_fixture(Path(directory))
            with patch.dict(release.PROFILE, component_lab=expected):
                for adapter in ['', 'lucent GPU: AdapterInfo { name: "RADV", device_type: IntegratedGpu, backend: Vulkan }\n']:
                    with self.assertRaisesRegex(RuntimeError, 'software_Vulkan'):
                        release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})


if __name__ == '__main__':
    unittest.main()
