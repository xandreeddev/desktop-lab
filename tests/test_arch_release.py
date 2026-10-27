"""Pure hosted-release evidence guards; no compiler, network or VM is used."""
import contextlib
import importlib.util
import io
import json
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
        self.assertEqual(value['source_commit'], 'ba5b8a51f960e07507520a77534d1eccbce9e40c')
        self.assertEqual((value['planned_stages'], value['planned_tests']), (17, 880))

    def test_profile_preserves_nine_binary_and_support_example_rosters(self):
        profile = release.PROFILE
        self.assertEqual((len(profile['managed_executables']), len(profile['acceptance_executables']),
                          len(profile['support_files']), len(profile['example_sources'])), (6, 3, 29, 5))
        self.assertEqual(sum(profile['expected_test_counts'].values()), 880)
        self.assertEqual(profile['expected_test_counts']['lucent-wayland-tests'], 115)
        self.assertEqual(profile['expected_test_counts']['desktop-tests'], 404)
        self.assertEqual(len([name for name in profile['required_test_names']['desktop-tests']
                              if name.startswith('widget_keyboard_')]), 17)
        self.assertEqual(profile['rust_release'], '1.99.0')
        self.assertEqual(profile['baseline_manifest_sha256'], 'ba2a89b036ad00fde7fca82cce9a1647ab91247e2b59a90874bf87f587fb48b2')
        self.assertEqual(profile['baseline_manifest_kind'], 'localized-hosted-artifact-import')
        self.assertNotEqual(profile['baseline_remote_raw_manifest_sha256'], profile['baseline_manifest_sha256'])

    def context(self):
        return dict(GITHUB_ACTIONS='true', GITHUB_REPOSITORY='xandreeddev/desktop-lab',
                    GITHUB_ACTOR='xandreeddev', GITHUB_TRIGGERING_ACTOR='xandreeddev',
                    GITHUB_EVENT_NAME='push', GITHUB_REF='refs/heads/ci/lucent-arch-release-focus',
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
        (root/'meminfo').write_text('MemAvailable: 12582912 kB\n')

    def test_remote_budget_uses_effective_cgroup_memory_and_no_swap(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.resource_files(root)
            value = release.resource_snapshot(root, root/'meminfo')
            self.assertEqual(value['effective_available_mib'], 9*1024)
            self.assertEqual(value['memory_swap_max_bytes'], 0)
            self.assertFalse(value['local_host_policy_changed'])
            (root/'memory.swap.max').write_text('10737418240')
            with self.assertRaisesRegex(RuntimeError, 'swap'):
                release.resource_snapshot(root, root/'meminfo')

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
        value = release.test_summary(text, 1, ['required'], True)
        self.assertEqual(value['tests_passed'], 1)
        self.assertEqual(value['required_test_names_verified'], ['required'])
        for changed in (text.replace('... ok', '... ignored'), text.replace('1 passed', '2 passed'),
                        text.replace('0 failed', '1 failed'), text + text):
            with self.assertRaises(RuntimeError):
                release.test_summary(changed, 1, ['required'], True)

    def test_nocapture_caught_panic_can_have_separate_final_ok(self):
        text = ('test module::required ... \nthread panicked as expected\nok\n'
                'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0s\n')
        self.assertEqual(release.test_summary(text, 1, ['required'])['tests_passed'], 1)
        with self.assertRaises(RuntimeError):
            release.test_summary(text, 1, ['required'], True)

    def test_names_from_other_or_ignored_tests_cannot_satisfy_requirement(self):
        text = ('test module::required ... ignored\ntest module::other ... ok\n'
                'test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0s\n')
        with self.assertRaisesRegex(RuntimeError, 'named'):
            release.test_summary(text, 1, ['required'])

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
                          profile['expected_test_counts']['lucent-ui-tests']), (84, 121))
        self.assertIn('queued_sync_done_completes_before_a_following_real_leave_in_one_dispatch_batch',
                      profile['required_test_names']['lucent-wayland-tests'])
        self.assertIn('native_interaction_starts_inactive_without_discarding_component_focus',
                      profile['required_test_names']['lucent-wayland-tests'])
        self.assertIn('widget_keyboard_unavailable_request_is_visible_and_never_leaves_eligibility_armed',
                      profile['required_test_names']['desktop-tests'])
        self.assertEqual(profile['license_files'], {'lucent/crates/lucent-wayland/protocols/hyprland-focus-grab-v1.xml':
                          '0bc60c14d27f9079e0d1d9d706f91e67ad7da585f3b7f34ff7186a376c0285b4'})
        self.assertEqual((profile['component_lab']['captures'], profile['component_lab']['producer_inputs']), (480, 121))

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
        self.assertIn('ref: ba5b8a51f960e07507520a77534d1eccbce9e40c', workflow)

    def test_fresh_three_sample_admission_refuses_any_low_sample(self):
        with patch.object(release.time, 'sleep') as sleep:
            with patch.object(release, 'resource_snapshot', side_effect=[{'effective_available_mib': n} for n in [5000, 4500, 4800]]):
                self.assertEqual(release.admit_memory(), [5000, 4500, 4800])
                self.assertEqual(sleep.call_count, 3)
            with patch.object(release, 'resource_snapshot', side_effect=[{'effective_available_mib': n} for n in [5000, 4499, 6000]]):
                with self.assertRaisesRegex(RuntimeError, '4500'):
                    release.admit_memory()

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
        expected = dict(release.PROFILE['component_lab'], stories=1, captures=1, palettes=1, producer_inputs=1)
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
