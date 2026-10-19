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


class FocusEvidence(unittest.TestCase):
    def fixture(self):
        layer={'x':750,'y':380,'w':420,'h':320,'address':'private-layer-address','pid':999}
        surface={'id':'intent','instance':None,'width':420,'height':320,'buffer_width':420,
                 'buffer_height':320,'frames':3,'native_frames':3,'keyboard':'on-demand',
                 'private_text':'private field contents'}
        admitted={'example_state':{'surfaces':[surface]},'intent_layer':layer}
        clients=[{'class':'Owned','pid':123,'address':'0xabc','title':'private document'}]
        focused=dict(clients[0],title='private focused document')
        return layer,admitted,clients,focused

    def test_unchanged_exact_focus_predicate_and_submitted_example_facts(self):
        driver=load();layer,admitted,clients,focused=self.fixture()
        value=driver.focus_observation(focused,clients,admitted,'Owned',layer)
        self.assertTrue(value['matches_owned_browser'])
        self.assertEqual(value['submitted_example'],{'keyboard':'on-demand','surface_size':[420,320],
            'buffer_size':[420,320],'submitted':True,'native_submitted':True,
            'layer_geometry':{'x':750,'y':380,'w':420,'h':320},'layer_geometry_unchanged':True,
            'layer_matches_surface_size':True})
        for private in ('private document','private focused document','private field contents',
                        'private-layer-address','0xabc','999','123'):
            self.assertNotIn(private,json.dumps(value))

    def test_empty_foreign_and_cross_paired_active_window_do_not_satisfy_original_predicate(self):
        driver=load();layer,admitted,clients,focused=self.fixture()
        clients.append({'class':'Owned','pid':124,'address':'0xdef'})
        cases=[{},dict(focused,**{'class':'Other'}),dict(focused,pid=125),
               dict(focused,address='0xffff'),dict(focused,pid=123,address='0xdef')]
        for active in cases:
            with self.subTest(active=active):
                value=driver.focus_observation(active,clients,admitted,'Owned',layer)
                self.assertFalse(value['matches_owned_browser'])
        mixed=driver.focus_observation(cases[-1],clients,admitted,'Owned',layer)
        self.assertTrue(mixed['active_pid_matches_owned'] and mixed['active_address_matches_owned'])
        self.assertFalse(mixed['matches_owned_browser'])

    def test_predicate_false_is_recorded_before_timeout_cleanup_can_remove_the_example(self):
        driver=load();layer,admitted,clients,_=self.fixture();diagnostics={}
        guard=unittest.mock.Mock(return_value=(clients,admitted));read=unittest.mock.Mock(return_value={})
        self.assertFalse(driver.guarded_focus_observation(diagnostics,guard,read,'Owned',layer))
        self.assertEqual(diagnostics['phase'],'first_launch_focus_predicate')
        self.assertEqual(diagnostics['last_outcome'],'predicate_false')
        self.assertFalse(diagnostics['last_observation']['active_window_present'])
        self.assertEqual(diagnostics['last_observation']['submitted_example']['keyboard'],'on-demand')
        guard.assert_called_once_with();read.assert_called_once_with()
        admitted.clear()  # cleanup cannot erase the copied last geometry evidence
        self.assertTrue(diagnostics['last_observation']['submitted_example']['layer_geometry_unchanged'])

    def test_guard_failure_never_reads_focus_and_keeps_its_own_phase_without_payload(self):
        driver=load();layer,_,_,_=self.fixture();diagnostics={}
        guard=unittest.mock.Mock(side_effect=ValueError('private guard content'));read=unittest.mock.Mock()
        with self.assertRaises(ValueError):
            driver.guarded_focus_observation(diagnostics,guard,read,'Owned',layer)
        self.assertEqual(diagnostics['last_outcome'],'guard_exception')
        self.assertEqual(diagnostics['phase'],'first_launch_focus_guard')
        self.assertEqual(diagnostics['last_failure'],{'phase':'first_launch_focus_guard','error_class':'ValueError'})
        read.assert_not_called();self.assertNotIn('private guard content',json.dumps(diagnostics))

    def test_focus_read_failure_is_distinct_from_guard_failure_or_predicate_false(self):
        driver=load();layer,admitted,clients,_=self.fixture();diagnostics={}
        with self.assertRaises(ValueError):
            driver.guarded_focus_observation(diagnostics,lambda:(clients,admitted),
                unittest.mock.Mock(side_effect=ValueError('private focus payload')),'Owned',layer)
        self.assertEqual(diagnostics['last_outcome'],'observation_exception')
        self.assertEqual(diagnostics['phase'],'first_launch_focus_observation')
        self.assertNotIn('private focus payload',json.dumps(diagnostics))

    def test_invalid_or_nonfinite_submitted_geometry_cannot_emit_unbounded_diagnostics(self):
        driver=load();layer,admitted,clients,focused=self.fixture()
        for invalid in (float('nan'),float('inf'),32769,True,'private coordinate'):
            with self.subTest(value=invalid):
                changed=dict(admitted,intent_layer=dict(layer,x=invalid))
                with self.assertRaises(ValueError):
                    driver.focus_observation(focused,clients,changed,'Owned',layer)
        for focused in ([],None,'private raw activewindow'):
            with self.assertRaises(ValueError):
                driver.focus_observation(focused,clients,admitted,'Owned',layer)


class RequiredFocusCheckpoint(unittest.TestCase):
    def require(self, condition, label):
        if not condition:raise CheckFailure(label)

    def test_clean_false_continues_independent_checks_but_final_focus_gate_still_fails(self):
        driver=load();layer,admitted,clients,_=FocusEvidence().fixture();diagnostics={}
        guard=unittest.mock.Mock(return_value=(clients,admitted));independent=[]
        with patch.object(driver.time,'monotonic',side_effect=[0,35]),patch.object(driver.time,'sleep') as sleep:
            focused=driver.wait_for_owned_browser_focus(diagnostics,guard,lambda:{},'Owned',layer)
        self.assertFalse(focused);self.assertEqual(guard.call_count,2);sleep.assert_not_called()
        self.assertTrue(diagnostics['clean_focus_timeout'] and diagnostics['independent_checks_admitted'])
        checks={'fresh_token_delivered_to_first_owned_child':True,
                'fresh_token_child_delivery_and_fresh_browser_focus_observed':focused}
        independent.extend(['open','action','socket','cli'])
        checks['independent_document_checks']=True
        with patch.object(driver,'require',self.require,create=True),self.assertRaisesRegex(
                CheckFailure,'^fresh_owned_browser_focus_observed$'):
            driver.require_first_launch_focus(checks)
        self.assertEqual(independent,['open','action','socket','cli'])
        self.assertTrue(checks['independent_document_checks'])
        self.assertFalse(checks['fresh_token_child_delivery_and_fresh_browser_focus_observed'])

    def test_guard_error_aborts_immediately_without_read_retry_or_independent_admission(self):
        driver=load();layer,_,_,_=FocusEvidence().fixture();diagnostics={};continued=[]
        guard=unittest.mock.Mock(side_effect=ValueError('identity changed'));read=unittest.mock.Mock()
        with patch.object(driver.time,'monotonic',return_value=0),patch.object(driver.time,'sleep') as sleep, \
             self.assertRaises(ValueError):
            driver.wait_for_owned_browser_focus(diagnostics,guard,read,'Owned',layer)
            continued.append(True)
        guard.assert_called_once_with();read.assert_not_called();sleep.assert_not_called()
        self.assertEqual(continued,[]);self.assertNotIn('independent_checks_admitted',diagnostics)
        self.assertEqual(diagnostics['last_outcome'],'guard_exception')

    def test_focus_transport_error_is_fatal_rather_than_a_clean_false_timeout(self):
        driver=load();layer,admitted,clients,_=FocusEvidence().fixture();diagnostics={}
        read=unittest.mock.Mock(side_effect=OSError('private transport detail'))
        with patch.object(driver.time,'monotonic',return_value=0),patch.object(driver.time,'sleep') as sleep, \
             self.assertRaises(OSError):
            driver.wait_for_owned_browser_focus(diagnostics,lambda:(clients,admitted),read,'Owned',layer)
        read.assert_called_once_with();sleep.assert_not_called()
        self.assertNotIn('independent_checks_admitted',diagnostics)
        self.assertEqual(diagnostics['last_outcome'],'observation_exception')

    def test_fresh_complete_guard_failure_after_clean_timeout_also_aborts(self):
        driver=load();layer,admitted,clients,_=FocusEvidence().fixture();diagnostics={}
        guard=unittest.mock.Mock(side_effect=[(clients,admitted),ValueError('foreign input')])
        with patch.object(driver.time,'monotonic',side_effect=[0,35]),self.assertRaises(ValueError):
            driver.wait_for_owned_browser_focus(diagnostics,guard,lambda:{},'Owned',layer)
        self.assertEqual(guard.call_count,2)
        self.assertEqual(diagnostics['phase'],'first_launch_focus_continue_guard')
        self.assertEqual(diagnostics['last_outcome'],'guard_exception')
        self.assertNotIn('independent_checks_admitted',diagnostics)

    def test_genuine_focus_passes_the_original_final_criterion_without_extra_guard_or_command(self):
        driver=load();layer,admitted,clients,focused=FocusEvidence().fixture();diagnostics={}
        guard=unittest.mock.Mock(return_value=(clients,admitted))
        with patch.object(driver.time,'monotonic',return_value=0),patch.object(driver.time,'sleep') as sleep:
            self.assertTrue(driver.wait_for_owned_browser_focus(diagnostics,guard,lambda:focused,'Owned',layer))
        guard.assert_called_once_with();sleep.assert_not_called()
        with patch.object(driver,'require',self.require,create=True):
            driver.require_first_launch_focus({'fresh_token_child_delivery_and_fresh_browser_focus_observed':True})
        self.assertNotIn('clean_focus_timeout',diagnostics)

    def test_no_focus_cannot_produce_a_success_report_after_independent_checks_pass(self):
        driver=load();layer,admitted,clients,_=FocusEvidence().fixture()
        with tempfile.TemporaryDirectory() as name:
            path=Path(name)/'report.json';output=io.StringIO()
            def suite(_args,result):
                result['checks']['fresh_token_delivered_to_first_owned_child']=True
                diagnostics=result['first_launch_focus']={}
                focused=driver.wait_for_owned_browser_focus(diagnostics,lambda:(clients,admitted),lambda:{},'Owned',layer)
                result['checks']['fresh_token_child_delivery_and_fresh_browser_focus_observed']=focused
                result['checks']['independent_document_checks']=True
                driver.require_first_launch_focus(result['checks'])
            with patch.object(driver,'load_helpers'),patch.object(driver,'suite',side_effect=suite), \
                 patch.object(driver,'CheckFailure',CheckFailure,create=True), \
                 patch.object(driver,'require',self.require,create=True),patch.object(driver,'remote',None,create=True), \
                 patch.object(driver.runpy,'run_path',return_value={'stay_awake':lambda _:nullcontext()}), \
                 patch.object(driver.time,'monotonic',side_effect=[0,35]),patch.object(driver.signal,'signal'), \
                 patch.object(sys,'argv',['test','--release-manifest','/unused','--report',str(path)]), \
                 redirect_stdout(output),self.assertRaises(SystemExit):driver.main()
            result=json.loads(path.read_text());summary=json.loads(output.getvalue())
            self.assertFalse(result['passed']);self.assertFalse(summary['passed'])
            self.assertTrue(result['checks']['independent_document_checks'])
            self.assertFalse(result['checks']['fresh_token_child_delivery_and_fresh_browser_focus_observed'])
            self.assertEqual(result['failure'],'fresh_owned_browser_focus_observed')


if __name__ == '__main__':
    unittest.main()
