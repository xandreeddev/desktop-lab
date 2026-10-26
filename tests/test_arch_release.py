"""Pure hosted-release evidence guards; no compiler, network or VM is used."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
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
        self.assertEqual(value['source_commit'], 'b2f54551dfbe3744762e04b06cc9abcdfd167eaa')
        self.assertEqual((value['planned_stages'], value['planned_tests']), (17, 840))

    def test_profile_preserves_nine_binary_and_support_example_rosters(self):
        profile = release.PROFILE
        self.assertEqual((len(profile['managed_executables']), len(profile['acceptance_executables']),
                          len(profile['support_files']), len(profile['example_sources'])), (6, 3, 29, 5))
        self.assertEqual(sum(profile['expected_test_counts'].values()), 840)
        self.assertEqual(profile['expected_test_counts']['lucent-wayland-tests'], 99)
        self.assertEqual(profile['expected_test_counts']['desktop-tests'], 393)
        self.assertEqual(len([name for name in profile['required_test_names']['desktop-tests']
                              if name.startswith('widget_keyboard_')]), 11)
        self.assertEqual(profile['rust_release'], '1.99.0')

    def context(self):
        return dict(GITHUB_ACTIONS='true', GITHUB_REPOSITORY='xandreeddev/desktop-lab',
                    GITHUB_ACTOR='xandreeddev', GITHUB_TRIGGERING_ACTOR='xandreeddev',
                    GITHUB_EVENT_NAME='push', GITHUB_REF='refs/heads/ci/lucent-arch-release-b2',
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


if __name__ == '__main__':
    unittest.main()
