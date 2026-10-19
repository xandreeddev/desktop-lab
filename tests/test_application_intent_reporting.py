"""Failure evidence only; no VM, browser, D-Bus or native input."""
from contextlib import nullcontext, redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
CheckFailure = type('CheckFailure', (Exception,), {})


def load():
    spec = importlib.util.spec_from_file_location('intent_reporting', ROOT/'vm/test-application-intent.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FailureEvidence(unittest.TestCase):
    def test_primary_exception_survives_separate_cleanup_failure(self):
        driver = load()
        primary = ValueError('private content')
        result = {'checks': {}, 'primary_failure': driver.failure_details(primary, 'physical_first_launch')}
        cleanup = {'browser_retirement': driver.failure_details(AssertionError('private identity'), 'browser_retirement')}
        with self.assertRaises(ValueError) as observed:
            try:
                raise primary
            finally:
                driver.finish_cleanup(result, cleanup)
        self.assertIs(observed.exception, primary)
        self.assertEqual(result['primary_failure'], {'phase': 'physical_first_launch', 'error_class': 'ValueError'})
        self.assertEqual(result['cleanup_failures'], cleanup)
        self.assertEqual(result['checks'], {})
        self.assertNotIn('private content', json.dumps(result))

    def test_cleanup_only_failure_still_fails_acceptance(self):
        driver = load()
        def require(condition, label):
            if not condition:
                raise CheckFailure(label)
        result = {}
        with patch.object(driver, 'require', require, create=True), self.assertRaisesRegex(
                CheckFailure, '^application_acceptance_cleanup_and_preservation$'):
            driver.finish_cleanup(result, {'browser_retirement': {'error_class': 'AssertionError'}})
        self.assertIn('browser_retirement', result['cleanup_failures'])

    def test_fixed_labels_survive_but_exception_payloads_do_not(self):
        driver = load()
        self.assertEqual(driver.failure_details(CheckFailure('owned_browser_retired'), 'browser_retirement')
                         ['fixed_reason'], 'owned_browser_retired')
        for error in (ValueError('secret text'), CheckFailure('secret text with spaces'), RuntimeError('secret text')):
            self.assertNotIn('secret text', json.dumps(driver.failure_details(error, 'native_suite')))
        with self.assertRaises(ValueError):
            driver.failure_details(ValueError(), 'unbounded private label')

    def test_failed_fixture_retains_private_stderr_operation_and_fixed_metadata(self):
        driver = load()
        raw = b'AssertionError: private fixture path and identity\n'
        failed = types.SimpleNamespace(returncode=1, stdout=b'', stderr=raw)
        with tempfile.TemporaryDirectory() as name, patch.object(driver.subprocess, 'run', return_value=failed):
            with self.assertRaises(driver.FixtureFailure) as observed:
                driver.fixture_command(['unused private argv'], 'guard', Path(name), 1)
            details = observed.exception.details
            self.assertEqual(details['operation'], 'guard')
            self.assertEqual(details['kind'], 'nonzero_exit')
            self.assertEqual(details['stderr_sha256'], hashlib.sha256(raw).hexdigest())
            path = Path(details['private_stderr'])
            self.assertEqual(path.read_bytes(), raw)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            self.assertNotIn('private fixture path and identity', json.dumps(details))
            self.assertNotIn('unused private argv', json.dumps(details))

    def test_stderr_retention_is_capped_and_hash_covers_full_observation(self):
        driver = load()
        raw = b'x'*70000
        failed = types.SimpleNamespace(returncode=7, stdout=b'', stderr=raw)
        with tempfile.TemporaryDirectory() as name, patch.object(driver.subprocess, 'run', return_value=failed):
            with self.assertRaises(driver.FixtureFailure) as observed:
                driver.fixture_command([], 'retire', Path(name), 2)
            details = observed.exception.details
            self.assertEqual(details['stderr_bytes'], 70000)
            self.assertEqual(details['stderr_retained_bytes'], 65536)
            self.assertTrue(details['stderr_truncated'])
            self.assertEqual(details['stderr_sha256'], hashlib.sha256(raw).hexdigest())
            self.assertEqual(Path(details['private_stderr']).stat().st_size, 65536)

    def test_lost_fixture_reply_retains_partial_stderr_without_retry(self):
        driver = load()
        failure = subprocess.TimeoutExpired(['private argv'], 30, stderr=b'private partial trace')
        with tempfile.TemporaryDirectory() as name, patch.object(driver.subprocess, 'run', side_effect=failure) as run:
            with self.assertRaises(driver.FixtureFailure) as observed:
                driver.fixture_command([], 'admission', Path(name), 3)
            run.assert_called_once()
            self.assertEqual(observed.exception.details['kind'], 'deadline')
            self.assertEqual(Path(observed.exception.details['private_stderr']).read_bytes(), b'private partial trace')

    def test_retention_error_does_not_replace_original_fixture_failure(self):
        driver = load()
        failed = types.SimpleNamespace(returncode=1, stdout=b'', stderr=b'private failure')
        with patch.object(driver.subprocess, 'run', return_value=failed), patch.object(
                driver.os, 'open', side_effect=PermissionError(13, 'private path')), self.assertRaises(driver.FixtureFailure) as observed:
            driver.fixture_command([], 'retire', Path('/unused'), 4)
        self.assertEqual(observed.exception.details['kind'], 'nonzero_exit')
        self.assertEqual(observed.exception.details['stderr_retention_error'],
                         {'error_class': 'PermissionError', 'errno': 13})

    def test_success_creates_no_failure_artifact(self):
        driver = load()
        success = types.SimpleNamespace(returncode=0, stdout=b'{"owned_clients":1}', stderr=b'')
        with tempfile.TemporaryDirectory() as name, patch.object(driver.subprocess, 'run', return_value=success):
            self.assertEqual(driver.fixture_command([], 'guard', Path(name), 1), '{"owned_clients":1}')
            self.assertEqual(list(Path(name).iterdir()), [])

    def test_private_report_born_0600_never_replaces_historical_receipt(self):
        driver = load()
        with tempfile.TemporaryDirectory() as name:
            path = Path(name)/'report.json'
            driver.write_private_report(path, {'passed': False, 'private_artifacts': {'guest_directory': '/owned'}})
            original = path.read_bytes()
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            with self.assertRaises(FileExistsError):
                driver.write_private_report(path, {'passed': True})
            self.assertEqual(path.read_bytes(), original)

    def test_main_prefers_primary_failure_and_omits_private_paths_from_stdout(self):
        driver = load()
        with tempfile.TemporaryDirectory() as name:
            path = Path(name)/'report.json'
            def suite(_args, result):
                result['primary_failure'] = {'phase': 'physical_first_launch', 'error_class': 'FixtureFailure',
                                             'fixed_reason': 'owned_fixture_operation_failed'}
                result['private_artifacts'] = {'guest_directory': '/private/guest/receipt'}
                result['cleanup_failures'] = {'browser_retirement': {'error_class': 'AssertionError'}}
                raise CheckFailure('application_acceptance_cleanup_and_preservation')
            output = io.StringIO()
            with patch.object(driver, 'load_helpers'), patch.object(driver, 'CheckFailure', CheckFailure, create=True), \
                 patch.object(driver, 'remote', None, create=True), patch.object(driver, 'suite', side_effect=suite), \
                 patch.object(driver.runpy, 'run_path', return_value={'stay_awake': lambda _: nullcontext()}), \
                 patch.object(sys, 'argv', ['test', '--release-manifest', '/unused', '--report', str(path)]), \
                 patch.object(driver.signal, 'signal'), redirect_stdout(output), self.assertRaises(SystemExit):
                driver.main()
            report = json.loads(path.read_text())
            self.assertFalse(report['passed'])
            self.assertEqual(report['checks'], {})
            self.assertEqual(report['failure'], 'owned_fixture_operation_failed')
            self.assertIn('browser_retirement', report['cleanup_failures'])
            self.assertIn('/private/guest/receipt', path.read_text())
            self.assertNotIn('/private/guest/receipt', output.getvalue())

    def test_existing_report_refuses_before_the_suite_can_mutate_anything(self):
        driver = load()
        with tempfile.TemporaryDirectory() as name:
            path = Path(name)/'existing.json'
            path.write_bytes(b'original failed receipt')
            with patch.object(driver, 'load_helpers'), patch.object(driver, 'suite') as suite, \
                 patch.object(sys, 'argv', ['test', '--release-manifest', '/unused', '--report', str(path)]), \
                 patch.object(driver.signal, 'signal'), self.assertRaisesRegex(ValueError, 'existing_private_report_refused'):
                driver.main()
            suite.assert_not_called()
            self.assertEqual(path.read_bytes(), b'original failed receipt')


if __name__ == '__main__':
    unittest.main()
