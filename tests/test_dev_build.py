"""Build planning and real temporary bundle storage, without Cargo or systemd."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from lucent_dev import build
from lucent_dev.model import BINARY_NAMES, SUPPORT_FILES, DevError, load_bundle, mutation_lease


class BuildTests(unittest.TestCase):
    def fixture(self, directory):
        root = Path(directory) / 'source'
        (root / 'lucent').mkdir(parents=True)
        (root / 'lucent/Cargo.toml').write_text('[workspace]\n')
        (root / 'lucent/source.rs').write_text('original')
        (root / 'scripts').mkdir()
        target = Path(directory) / 'target'
        for profile in ('release', 'debug'):
            (target / profile).mkdir(parents=True)
            for name in BINARY_NAMES:
                (target / profile / name).write_bytes(b'fixture executable\n')
                (target / profile / name).chmod(0o700)
        for name in SUPPORT_FILES:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('fixture support\n')
        return root, target, Path(directory) / 'bundles'

    def test_cargo_builds_exact_three_separate_packages_with_locked_jobs_one(self):
        command = build.cargo_command(Path('/source'), 'release')
        self.assertEqual([command[i + 1] for i, item in enumerate(command) if item == '-p'], list(BINARY_NAMES))
        self.assertIn('--locked', command)
        self.assertEqual(command[command.index('--jobs') + 1], '1')
        self.assertIn('--release', command)
        self.assertNotIn('--release', build.cargo_command(Path('/source'), 'debug'))

    def test_fresh_generators_check_without_rewriting_and_only_stale_regenerates(self):
        run = Mock(return_value=Mock(returncode=0))
        build.generate(Path('/source'), run)
        self.assertEqual(run.call_count, 2)
        self.assertTrue(all(call.args[0][-1] == '--check' for call in run.call_args_list))
        run = Mock(side_effect=[Mock(returncode=1), Mock(returncode=0), Mock(returncode=0)])
        build.generate(Path('/source'), run)
        self.assertEqual(run.call_count, 3)
        self.assertNotEqual(run.call_args_list[1].args[0][-1], '--check')

    def test_success_publishes_checked_private_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root, target, output = self.fixture(directory)
            run = Mock(return_value=Mock(returncode=0))
            with patch.object(build, 'source_info', return_value=('a' * 40, True)):
                result = build.publish_bundle(root, target, 'release', output, run)
            bundle = Path(result['bundle'])
            manifest = load_bundle(bundle)
            self.assertTrue(manifest['source_dirty'])
            self.assertEqual(manifest['source_commit'], 'a' * 40)
            self.assertEqual(set(manifest['binaries']), set(BINARY_NAMES))
            self.assertEqual(set(manifest['support_files']), set(SUPPORT_FILES))
            self.assertEqual(bundle.stat().st_mode & 0o777, 0o500)
            self.assertFalse(any(path.name.startswith('.building') for path in output.iterdir()))

    def test_compile_failure_never_publishes_or_deploys(self):
        with tempfile.TemporaryDirectory() as directory:
            root, target, output = self.fixture(directory)
            def run(command, **_):
                if 'build' in command:
                    raise subprocess.CalledProcessError(1, command)
                return Mock(returncode=0)
            with patch.object(build, 'source_info', return_value=('a' * 40, False)):
                with self.assertRaises(subprocess.CalledProcessError):
                    build.publish_bundle(root, target, 'release', output, run)
            self.assertEqual(list(output.iterdir()), [])

    def test_source_edit_during_compile_rejects_stale_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root, target, output = self.fixture(directory)
            def run(command, **_):
                if 'build' in command:
                    (root / 'lucent/source.rs').write_text('newer edit')
                return Mock(returncode=0)
            with patch.object(build, 'source_info', return_value=('a' * 40, False)):
                with self.assertRaisesRegex(DevError, 'Source changed'):
                    build.publish_bundle(root, target, 'release', output, run)
            self.assertEqual(list(output.iterdir()), [])

    def test_support_edit_during_copy_cannot_publish_mixed_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root, target, output = self.fixture(directory)
            original = build.shutil.copyfileobj
            def copy(reader, writer, size):
                original(reader, writer, size)
                (root / SUPPORT_FILES[-1]).write_text('new support')
            with patch.object(build, 'source_info', return_value=('a' * 40, False)), \
                    patch.object(build.shutil, 'copyfileobj', side_effect=copy):
                with self.assertRaisesRegex(DevError, 'Source changed'):
                    build.publish_bundle(root, target, 'release', output, Mock(return_value=Mock(returncode=0)))
            self.assertEqual(list(output.iterdir()), [])

    def test_bundle_corruption_and_manifest_symlink_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root, target, output = self.fixture(directory)
            with patch.object(build, 'source_info', return_value=('a' * 40, False)):
                result = build.publish_bundle(root, target, 'debug', output, Mock(return_value=Mock(returncode=0)))
            bundle = Path(result['bundle'])
            binary = bundle / BINARY_NAMES[0]
            binary.chmod(0o700)
            binary.write_bytes(b'corrupted')
            with self.assertRaises(DevError):
                load_bundle(bundle)
            bundle.chmod(0o700)
            manifest = bundle / 'manifest.json'
            manifest.unlink()
            manifest.symlink_to(root / SUPPORT_FILES[0])
            with self.assertRaises(OSError):
                load_bundle(bundle)

    def test_mutation_lease_is_exclusive_and_refuses_symlink(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'XDG_RUNTIME_DIR': directory}):
            with mutation_lease():
                with self.assertRaisesRegex(DevError, 'already active'):
                    with mutation_lease():
                        self.fail('second lease admitted')
            path = Path(directory) / 'lucent-dev.lock'
            path.unlink()
            path.symlink_to(Path(directory) / 'unrelated')
            with self.assertRaises(OSError):
                with mutation_lease():
                    self.fail('symlink admitted')

    def test_build_unit_has_resource_bounds_and_persistent_target(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'XDG_RUNTIME_DIR': directory}), \
                patch.object(build.shutil, 'which', return_value='/usr/bin/cargo'), \
                patch.object(build.subprocess, 'Popen') as popen, patch.object(build.os, 'pidfd_open', return_value=123):
            job = build.BuildJob(Path('/source'), Path('/persistent-target'), 'release', Path('/bundles'))
            command = popen.call_args.args[0]
            for setting in ('CPUQuota=100%', 'MemoryHigh=900M', 'MemoryMax=1400M', 'TasksMax=256', 'RuntimeMaxSec=1800'):
                self.assertIn('--property=' + setting, command)
            self.assertEqual(popen.call_args.kwargs['env']['CARGO_TARGET_DIR'], '/persistent-target')
            self.assertEqual(popen.call_args.kwargs['env']['CARGO_BUILD_JOBS'], '1')
            with patch.object(build.os, 'close'):
                job.close()

    def cancellation_job(self, directory, invocation='a' * 32):
        job = build.BuildJob.__new__(build.BuildJob)
        job.unit = 'lucent-dev-build-' + 'b' * 24 + '.service'
        job.control = Path(directory)
        job.process = Mock()
        job.process.poll.return_value = None
        (job.control / 'started.json').write_text(json.dumps({
            'unit': job.unit, 'invocation_id': 'a' * 32, 'pid': 123, 'birth': '456'}))
        active = {'LoadState': 'loaded', 'ActiveState': 'active', 'InvocationID': invocation,
                  'MainPID': '123', 'ControlPID': '0', 'Job': ''}
        job._state = Mock(side_effect=[active, active, {'LoadState': 'not-found'}])
        return job

    def test_cancel_revokes_admission_and_stops_only_matching_owned_unit(self):
        with tempfile.TemporaryDirectory() as directory:
            job = self.cancellation_job(directory)
            with patch.object(build, 'process_birth', return_value='456'), patch.object(build.subprocess, 'run') as run:
                job.cancel()
            self.assertTrue((job.control / 'cancelled').exists())
            job.process.terminate.assert_called_once()
            self.assertEqual(run.call_args.args[0], ['systemctl', '--user', 'stop', job.unit])
            self.assertNotIn('lucent.service', run.call_args.args[0])

    def test_cancel_does_not_stop_same_name_replacement_invocation(self):
        with tempfile.TemporaryDirectory() as directory:
            job = self.cancellation_job(directory, invocation='c' * 32)
            with patch.object(build.subprocess, 'run') as run, self.assertRaisesRegex(DevError, 'ownership changed'):
                job.cancel()
            run.assert_not_called()

    def test_cancelled_worker_never_starts_generators_or_cargo(self):
        with tempfile.TemporaryDirectory() as directory:
            root, target, output = self.fixture(directory)
            run = Mock()
            def cancelled(): raise DevError('cancelled')
            with self.assertRaisesRegex(DevError, 'cancelled'):
                build.publish_bundle(root, target, 'release', output, run, check_cancelled=cancelled)
            run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
