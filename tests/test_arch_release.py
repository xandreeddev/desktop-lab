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
    def test_default_is_inert_and_has_exact_source_and_planned_counts(self):
        output = io.StringIO()
        with (patch.object(release.sys, 'argv', ['arch-release']), contextlib.redirect_stdout(output),
              patch.object(release.subprocess, 'run', side_effect=AssertionError('no subprocess')),
              patch.object(release.subprocess, 'check_output', side_effect=AssertionError('no subprocess'))):
            self.assertEqual(release.main(), 0)
        value = json.loads(output.getvalue())
        self.assertFalse(value['actions'])
        self.assertEqual(value['source_commit'], '1c1dfae474929fab60d6c300326e6959d1fae77c')
        self.assertEqual((value['planned_stages'], value['planned_tests']), (21, 952))

    def test_profile_preserves_nine_binary_and_support_example_rosters(self):
        profile = release.PROFILE
        self.assertEqual((len(profile['managed_executables']), len(profile['acceptance_executables']),
                          len(profile['support_files']), len(profile['example_sources'])), (6, 3, 29, 5))
        self.assertEqual(sum(profile['expected_test_counts'].values()), 952)
        self.assertEqual(profile['expected_test_counts']['lucent-wayland-tests'], 120)
        self.assertEqual(profile['expected_test_counts']['desktop-tests'], 414)
        self.assertEqual(len([name for name in profile['required_test_names']['desktop-tests']
                              if name.rsplit('::', 1)[-1].startswith('widget_keyboard_')]), 17)
        self.assertEqual(profile['rust_release'], '1.99.0')
        self.assertEqual(profile['baseline_manifest_sha256'], 'b25ac061d3fe334299f61d7d120d489d03975036ed477fc797caca4ba7ab0a06')
        self.assertEqual(profile['baseline_manifest_kind'], 'localized-hosted-artifact-import')
        self.assertNotEqual(profile['baseline_remote_raw_manifest_sha256'], profile['baseline_manifest_sha256'])

    def context(self):
        return dict(GITHUB_ACTIONS='true', GITHUB_REPOSITORY='xandreeddev/desktop-lab',
                    GITHUB_ACTOR='xandreeddev', GITHUB_TRIGGERING_ACTOR='xandreeddev',
                    GITHUB_EVENT_NAME='push', GITHUB_REF='refs/heads/ci/lucent-arch-release-video-focus',
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
        (root/'memory.max').write_text(str(maximum))
        (root/'memory.current').write_text(str(current))
        (root/'memory.swap.max').write_text(swap)
        (root/'memory.stat').write_text('anon 268435456\nfile 805306368\n')
        (root/'meminfo').write_text('MemAvailable: 12582912 kB\n')

    def test_remote_budget_uses_effective_cgroup_memory_and_no_swap(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.resource_files(root)
            value = release.resource_snapshot(root, root/'meminfo')
            self.assertEqual(value['effective_available_mib'], 9*1024)
            self.assertEqual(value['memory_swap_max_bytes'], 0)
            self.assertEqual(value['memory_stat_anon_bytes'], 268435456)
            self.assertEqual(value['memory_stat_file_bytes'], 805306368)
            self.assertFalse(value['local_host_policy_changed'])
            (root/'memory.swap.max').write_text('10737418240')
            with self.assertRaisesRegex(RuntimeError, 'swap'):
                release.resource_snapshot(root, root/'meminfo')
            self.resource_files(root, current=10*1024**3 + 1)
            saturated = release.resource_snapshot(root, root/'meminfo')
            self.assertEqual(saturated['effective_available_mib'], 0)
            self.assertEqual(saturated['memory_current_bytes'], 10*1024**3 + 1)

    def test_budget_refuses_unbounded_or_different_cpu_memory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.resource_files(root)
            for value in ('max 100000', '400000 100000', '300000 0'):
                (root/'cpu.max').write_text(value)
                with self.assertRaises(RuntimeError):
                    release.resource_snapshot(root, root/'meminfo')
            self.resource_files(root)
            (root/'memory.max').write_text('max')
            with self.assertRaises(RuntimeError):
                release.resource_snapshot(root, root/'meminfo')

    def test_summary_requires_actual_passed_names_and_exact_nonempty_count(self):
        text = 'test module::required ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
        value = release.test_summary(text, 1, ['module::required'], True)
        self.assertEqual(value['tests_passed'], 1)
        self.assertEqual(value['required_test_names_verified'], ['module::required'])
        for changed in (text.replace('... ok', '... ignored'), text.replace('1 passed', '2 passed'),
                        text.replace('0 failed', '1 failed'), text + text):
            with self.assertRaises(RuntimeError):
                release.test_summary(changed, 1, ['module::required'], True)

    def test_nocapture_caught_panic_can_have_separate_final_ok(self):
        text = ('test module::required ... \nthread panicked as expected\nok\n'
                'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0s\n')
        self.assertEqual(release.test_summary(text, 1, ['module::required'])['tests_passed'], 1)
        with self.assertRaises(RuntimeError):
            release.test_summary(text, 1, ['module::required'], True)

    def test_names_from_other_or_ignored_tests_cannot_satisfy_requirement(self):
        text = ('test module::required ... ignored\ntest module::other ... ok\n'
                'test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0s\n')
        with self.assertRaisesRegex(RuntimeError, 'named'):
            release.test_summary(text, 1, ['module::required'])

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

    def test_all_new_focus_gate_inventories_and_license_are_explicit(self):
        profile = release.PROFILE
        self.assertEqual((profile['expected_test_counts']['lucent-api-tests'],
                          profile['expected_test_counts']['lucent-ui-tests']), (86, 122))
        self.assertIn('queued_sync_done_completes_before_a_following_real_leave_in_one_dispatch_batch',
                      {name.rsplit('::',1)[-1] for name in profile['required_test_names']['lucent-wayland-tests']})
        self.assertIn('native_interaction_starts_inactive_without_discarding_component_focus',
                      {name.rsplit('::',1)[-1] for name in profile['required_test_names']['lucent-wayland-tests']})
        self.assertIn('widget_keyboard_unavailable_request_is_visible_and_never_leaves_eligibility_armed',
                      {name.rsplit('::',1)[-1] for name in profile['required_test_names']['desktop-tests']})
        self.assertEqual(profile['license_files'], {'lucent/crates/lucent-wayland/protocols/hyprland-focus-grab-v1.xml':
                          '0bc60c14d27f9079e0d1d9d706f91e67ad7da585f3b7f34ff7186a376c0285b4'})
        self.assertEqual((profile['component_lab']['captures'], profile['component_lab']['producer_inputs']), (480, 134))

    def test_control_flush_regressions_all_require_individually_successful_log_records(self):
        selected = ['control_flush_attempts_success_once_without_an_ownership_receipt', 'control_flush_defers_would_block_without_retrying_or_waiting', 'control_flush_preserves_fatal_io_errors', 'control_flush_preserves_protocol_failure', 'control_flush_sends_complete_acquisition_before_peer_dispatch']
        required = release.PROFILE['required_test_names']['lucent-wayland-tests']
        self.assertTrue(set(selected) <= {name.rsplit('::',1)[-1] for name in required})
        self.assertEqual(sum(len(v) for v in release.PROFILE['required_test_names'].values()), 952)
        self.assertEqual(len(required), len(set(required)))
        text = ''.join('test ' + name + ' ... ok\n' for name in required)
        text += 'test result: ok. 120 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
        self.assertEqual(release.test_summary(text, 120, required, True)['tests_passed'], 120)
        for name in selected:
            with self.subTest(name=name), self.assertRaisesRegex(RuntimeError, 'named'):
                release.test_summary(text.replace(name + ' ... ok', name + ' ... ignored'), 120, required, True)

    def test_every_combined_stage_has_an_exact_full_name_inventory(self):
        profile = release.PROFILE
        self.assertEqual(len(profile['expected_stage_order']), 21)
        self.assertEqual(len(set(profile['expected_stage_order'])), 21)
        self.assertEqual((profile['expected_payload_origins'], profile['expected_memory_rows']), (45, 8))
        self.assertEqual(profile['compiled_inventory_provenance']['inventory_artifact_id'], 12712792584)
        for stage, count in profile['expected_test_counts'].items():
            names = profile['required_test_names'][stage]
            self.assertEqual(len(names), count)
            self.assertEqual(len(set(names)), count)
        self.assertEqual({stage: profile['expected_test_counts'][stage] for stage in (
            'video-frame-tests', 'video-plane-vulkan-tests', 'video-scene-vulkan-tests',
            'native-video-example-tests', 'video-tests', 'video-allocation-example-tests')}, {
                'video-frame-tests': 7, 'video-plane-vulkan-tests': 3, 'video-scene-vulkan-tests': 3,
                'native-video-example-tests': 1, 'video-tests': 77, 'video-allocation-example-tests': 21})
        self.assertIn('lucent-video-frame', profile['expected_workspace_package_targets'])
        self.assertEqual(len(profile['expected_workspace_package_targets']), 34)

    def test_same_leaf_different_namespace_extra_or_duplicate_names_do_not_qualify(self):
        summary = 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
        for prefix in ['test wrong::required ... ok\n',
                       'test module::required ... ok\ntest module::required ... ok\n',
                       'test module::required ... ok\ntest module::extra ... ok\n']:
            with self.subTest(prefix=prefix), self.assertRaises(RuntimeError):
                release.test_summary(prefix + summary, 1, ['module::required'])

    def test_stage_completion_refuses_reordering_missing_or_relabelled_gates(self):
        rows = [dict(name=name, exit=0, **({'tests_passed': release.PROFILE['expected_test_counts'][name]}
                if name in release.PROFILE['expected_test_counts'] else {}))
                for name in release.PROFILE['expected_stage_order']]
        release.completed_stage_inventory(rows)
        swapped = copy.deepcopy(rows); swapped[4], swapped[5] = swapped[5], swapped[4]
        renamed = copy.deepcopy(rows); renamed[4]['name'] = 'unreviewed-gate'
        failed = copy.deepcopy(rows); failed[4]['exit'] = 1
        wrong_count = copy.deepcopy(rows); wrong_count[4]['tests_passed'] += 1
        for changed in (rows[:-1], swapped, renamed, failed, wrong_count):
            with self.assertRaisesRegex(RuntimeError, '21_stages'):
                release.completed_stage_inventory(changed)

    def test_workspace_inventory_requires_neutral_package_and_every_actual_target(self):
        source = Path('/fixture/source')
        metadata = {'workspace_members': ['neutral', 'app'], 'packages': [
            {'id': 'neutral', 'name': 'lucent-video-frame', 'manifest_path': str(source/'lucent/crates/lucent-video-frame/Cargo.toml'),
             'targets': [{'name': 'lucent_video_frame', 'kind': ['lib']}]},
            {'id': 'app', 'name': 'app', 'manifest_path': str(source/'lucent/apps/app/Cargo.toml'),
             'targets': [{'name': 'app', 'kind': ['bin']}, {'name': 'extra', 'kind': ['bin']}]}]}
        expected = {'lucent-video-frame': ['lucent_video_frame'], 'app': ['app', 'extra']}
        with patch.dict(release.PROFILE, expected_workspace_package_targets=expected):
            packages, targets = release.workspace_inventory(metadata, source)
            self.assertEqual(packages, ['app', 'lucent-video-frame'])
            self.assertEqual(targets, ['app', 'extra', 'lucent_video_frame'])
            missing = copy.deepcopy(metadata); missing['packages'].pop(0)
            target = copy.deepcopy(metadata); target['packages'][1]['targets'].pop()
            outside = copy.deepcopy(metadata); outside['packages'][0]['manifest_path'] = '/outside/Cargo.toml'
            for changed in (missing, target, outside):
                with self.assertRaises(RuntimeError):
                    release.workspace_inventory(changed, source)

    def test_exact_eight_memory_rows_preserve_admission_and_cleanup_observations(self):
        phases = [('initial-admission', i) for i in range(3)] + [
            ('target-cleanup-before', 0), ('target-cleanup-after', 0)] + [
            ('component-lab-admission', i) for i in range(3)]
        rows = [dict(phase=phase, sample_index=index, snapshot={'fixture': True}) for phase, index in phases]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'memory.log'
            path.write_text(''.join(json.dumps(row)+'\n' for row in rows))
            self.assertEqual(release.completed_memory_rows(path), 8)
            for changed in (rows[:-1], rows + rows[:1], list(reversed(rows))):
                path.write_text(''.join(json.dumps(row)+'\n' for row in changed))
                with self.assertRaisesRegex(RuntimeError, 'eight'):
                    release.completed_memory_rows(path)

    def test_success_envelope_is_exact_two_regular_root_files_and_refuses_reuse(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root/'lucent-focus.tar.zst'
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
        self.assertIn('ref: 1c1dfae474929fab60d6c300326e6959d1fae77c', workflow)

    def test_only_the_dedicated_tools_push_skips_unrelated_standard_builds(self):
        workflow = (ROOT/'.github/workflows/check.yml').read_text()
        self.assertIn('branches-ignore: [ci/lucent-arch-release-video-focus]', workflow)
        self.assertIn('  pull_request:', workflow)
        self.assertNotIn('branches-ignore: [main]', workflow)
        build = (ROOT/'ci/arch_release.py').read_text()
        self.assertNotIn('stream_upload_cpu_stage_diagnostic_1080p', build)
        self.assertIn("run('video-frame-tests'", build)
        self.assertIn("run('native-video-example-tests'", build)

    def test_fresh_three_sample_admission_refuses_any_low_sample(self):
        with patch.object(release.time, 'sleep') as sleep:
            with patch.object(release, 'resource_snapshot', side_effect=[{'effective_available_mib': n} for n in [5000, 4500, 4800]]):
                self.assertEqual(release.admit_memory(), [5000, 4500, 4800])
                self.assertEqual(sleep.call_count, 3)
            with patch.object(release, 'resource_snapshot', side_effect=[{'effective_available_mib': n} for n in [5000, 4499, 6000]]):
                with self.assertRaisesRegex(RuntimeError, '4500'):
                    release.admit_memory()

    def test_refused_admission_sample_is_persisted_before_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'memory.log'
            samples = [dict(host_available_mib=8000, memory_current_bytes=current,
                            memory_max_bytes=10737418240, memory_stat_anon_bytes=100,
                            memory_stat_file_bytes=200, effective_available_mib=available)
                       for current, available in [(10, 7000), (20, 4499)]]
            with patch.object(release, 'resource_snapshot', side_effect=samples), patch.object(release.time, 'sleep'):
                with self.assertRaisesRegex(RuntimeError, '4500'):
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
        return temporary/'lucent-target-video-focus', temporary, protected

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
                    'palettes': [{}], 'stories': [{'variants': [{'image': {'path': 'images/one.png',
                     'sha256': release.digest(png)}}]}]}
        (catalog/'manifest.json').write_text(json.dumps(manifest))
        expected = dict(release.PROFILE['component_lab'], stories=1, captures=1, palettes=1, producer_inputs=1,
                        producer_sha256=manifest['renderer']['inputs'].copy())
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

    def test_component_export_refuses_replaced_producer_at_the_same_count(self):
        with tempfile.TemporaryDirectory() as directory:
            source, catalog, matrix, manifest, expected = self.catalog_fixture(Path(directory))
            (source/'other.rs').write_text('other actual source')
            manifest['renderer']['inputs'] = {'other.rs': release.digest(source/'other.rs')}
            (catalog/'manifest.json').write_text(json.dumps(manifest))
            with patch.dict(release.PROFILE, component_lab=expected), self.assertRaisesRegex(RuntimeError, 'producer_inventory'):
                release.component_lab_evidence(source, catalog, matrix,
                    'lucent GPU: AdapterInfo { name: "llvmpipe", device_type: Cpu, backend: Vulkan }\n', {'sha256': 'a'*64})

    def test_component_export_rejects_native_gpu_claim_or_unbound_backend(self):
        with tempfile.TemporaryDirectory() as directory:
            source, catalog, matrix, _, expected = self.catalog_fixture(Path(directory))
            with patch.dict(release.PROFILE, component_lab=expected):
                for adapter in ['', 'lucent GPU: AdapterInfo { name: "RADV", device_type: IntegratedGpu, backend: Vulkan }\n']:
                    with self.assertRaisesRegex(RuntimeError, 'software_Vulkan'):
                        release.component_lab_evidence(source, catalog, matrix, adapter, {'sha256': 'a'*64})


if __name__ == '__main__':
    unittest.main()
