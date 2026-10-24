"""Cold fixture tests use only local temporary files and injected native boundaries."""
import argparse
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import cold_default_uri_policy as c
import default_uri_policy as p
from application_intent_browser import pages


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


warm_fixture = load('cold_test_warm_fixture', 'vm/fixtures/default-uri-fixture.py')
with patch.dict(sys.modules, {'default_uri_warm': warm_fixture}):
    guest = load('cold_test_guest', 'vm/fixtures/cold-default-uri-fixture.py')
driver = load('cold_test_controller', 'vm/test-cold-default-uri-notification.py')


def facts(run):
    origin = {'unit': {'group': '/app.slice/run-owned.service'},
              'process': {'pid': 321, 'start': '12345'}}
    event = {'operation': 'open', 'uris': ['http://127.0.0.1:1234/owned'],
             'fresh_token_present': True, 'legacy_startup_present': False,
             'origin': origin, 'cgroup': origin['unit']['group']}
    declared = {'profile': str(run / 'profile'), 'class': 'LucentOwned',
                'executable': {'identity': ['chromium-file']}}
    http = {'targets': [{'uri': event['uris'][0], 'title': 'Owned document'}]}
    value = {'declared': declared, 'http': http}
    lease = {'origin': origin, 'declared': declared, 'process': {'pid': 321, 'start': '12345'}}
    endpoint = {'port': 1234, 'browser': '/devtools/browser/owned'}
    target = {'id': 'target-owned', 'url': event['uris'][0], 'title': 'Owned document'}
    client = {'pid': 321, 'class': 'LucentOwned', 'mapped': True,
              'hidden': False, 'address': '0x1234'}
    base = types.SimpleNamespace(
        chromium=types.SimpleNamespace(original_gone=Mock(return_value=False),
                                       observe=Mock(return_value=lease), retire=Mock()),
        events=Mock(return_value=[event]), browser_endpoint=Mock(return_value=endpoint),
        browser_pages=Mock(return_value=[target]), event_argv=Mock(return_value=['original-wrapper']),
        retire=Mock(), pages=pages)
    return base, value, event, lease, endpoint, target, client


class ColdPolicy(unittest.TestCase):
    def test_cold_document_has_no_invented_blank_control(self):
        expected = [{'uri': 'http://127.0.0.1/owned', 'title': 'Owned'}]
        value = {'id': 'id1', 'url': expected[0]['uri'], 'title': 'Owned'}
        self.assertIsNone(c.document([], expected))
        self.assertEqual(c.document([value], expected), value)
        for values in ([dict(value, url='about:blank')], [value, dict(value, id='id2')],
                       [dict(value, title='Foreign')], [dict(value, id='bad/id')]):
            with self.subTest(values=values), self.assertRaises(ValueError):
                c.document(values, expected)

    def test_first_event_requires_native_authority_and_exact_single_uri(self):
        with tempfile.TemporaryDirectory() as directory:
            _, value, event, *_ = facts(Path(directory))
        uri = value['http']['targets'][0]['uri']
        self.assertEqual(c.first_event([event], uri), event)
        for change in ({'uris': []}, {'uris': [uri, uri]}, {'fresh_token_present': False},
                       {'legacy_startup_present': True}, {'cgroup': '/foreign.scope'}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                c.first_event([dict(event, **change)], uri)
        with self.assertRaises(ValueError):
            c.first_event([event, event], uri)

    def test_case_receipts_require_distinct_births_and_complete_first_cleanup(self):
        def receipt(case, suffix):
            return {'case': case, 'success': True, 'cleanup': {'owned': True},
                    'cold_browser': {'lease': {'declared': {'profile': '/tmp/' + suffix, 'class': suffix},
                        'origin': {'original': suffix}, 'process': {'pid': 10, 'start': suffix}}}}
        first, second = receipt('pointer', 'one'), receipt('keyboard', 'two')
        c.case_receipts([first, second])
        for field in ('profile', 'class'):
            changed = copy.deepcopy(second)
            changed['cold_browser']['lease']['declared'][field] = first['cold_browser']['lease']['declared'][field]
            with self.assertRaises(ValueError):
                c.case_receipts([first, changed])
        bad = copy.deepcopy(second)
        bad['cold_browser']['lease']['process'] = first['cold_browser']['lease']['process']
        with self.assertRaises(ValueError):
            c.case_receipts([first, bad])
        first['cleanup']['owned'] = False
        with self.assertRaises(ValueError):
            c.case_receipts([first, second])

    def test_only_expiry_bridge_has_long_timeout(self):
        self.assertEqual(c.bridge_timeout('wait-expiry'), 70)
        for operation in c.OPERATIONS:
            if operation != 'wait-expiry':
                self.assertEqual(c.bridge_timeout(operation), 30)
        with self.assertRaises(ValueError):
            c.bridge_timeout('warm')

    def test_fixed_errors_never_echo_unknown_secret_or_accept_other_operation(self):
        error = ValueError('looks_like_a_private_token')
        packet = c.error_packet('cold-ready', error)
        self.assertEqual(packet['fixed_reason'], 'unclassified_fixture_failure')
        self.assertNotIn(error.args[0], json.dumps(packet))
        self.assertEqual(c.validate_error(packet, 'cold-ready'), packet)
        for changed in (dict(packet, operation='open'), dict(packet, fixed_reason=error.args[0]),
                        dict(packet, phase='secret'), dict(packet, raw='private')):
            with self.assertRaises(ValueError):
                c.validate_error(changed, 'cold-ready')


class ColdPreparation(unittest.TestCase):
    def test_uri_startup_is_declared_before_profile_and_canonical_manifest_not_rewritten(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            manifest = {'browser_startup': {'argv': ['about:blank']}}
            original = copy.deepcopy(manifest)
            http = {'targets': [{'uri': 'http://127.0.0.1:1234/owned'}]}
            base = types.SimpleNamespace(context=Mock(return_value=(run, manifest)),
                http_authority=Mock(return_value=http), events=Mock(return_value=[]),
                browser_argv=Mock(return_value=['/usr/bin/chromium', 'exact-owned-uri']),
                chromium=types.SimpleNamespace(startup=Mock(return_value={'cold': True})))
            with patch.object(guest, 'context', return_value=(run, {}, base)), \
                 patch.object(guest.warm, 'prepare', return_value={'prepared': True}):
                guest.prepare(run, 'a' * 12)
            base.browser_argv.assert_called_once_with(run, manifest, [http['targets'][0]['uri']])
            base.chromium.startup.assert_called_once_with(['/usr/bin/chromium', 'exact-owned-uri'])
            self.assertEqual(manifest, original)
            self.assertEqual(json.loads((run / 'cold-startup.json').read_text())['declared'], {'cold': True})
            self.assertFalse((run / 'profile').exists())

    def test_single_cold_dispatch_preserves_real_token_and_refuses_second_or_queued_request(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, *_ = facts(run)
            base.events.return_value = []
            base.atomic = lambda path, raw: path.write_bytes(raw)
            (run / 'events.lock').write_bytes(b'')
            (run / 'default-events.json').write_text('[]')
            with patch.object(guest, 'startup', return_value=(run, base, value)), \
                 patch.object(guest.warm, 'admitted'), \
                 patch.dict(os.environ, {'XDG_ACTIVATION_TOKEN': 'opaque-test-only'}, clear=True), \
                 patch.object(guest.subprocess, 'run', return_value=types.SimpleNamespace(returncode=0)) as native:
                guest.launch(run, 'a' * 12, value['http']['targets'][0]['uri'])
                self.assertIn('--setenv=XDG_ACTIVATION_TOKEN', native.call_args.args[0])
                self.assertEqual(native.call_args.args[0][-1], value['http']['targets'][0]['uri'])
                with self.assertRaises(ValueError):
                    guest.launch(run, 'a' * 12, value['http']['targets'][0]['uri'])
                self.assertEqual(native.call_count, 1)
            self.assertEqual(json.loads((run / 'default-events.json').read_text()), [
                {'selected_uri_matches': True, 'fresh_token_present': True, 'legacy_startup_present': False}])


class ColdReadiness(unittest.TestCase):
    def scenario(self, run):
        base, value, event, lease, endpoint, target, client = facts(run)
        return base, value, event, lease, endpoint, target, client

    def test_missing_mapping_waits_without_writing_a_lease(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, event, *_ = self.scenario(run)
            with patch.object(guest, 'original', return_value=(run, base, value, event)), \
                 patch.object(guest.warm, 'admitted'), patch.object(guest.warm, 'warm_native_clients', return_value=[]):
                self.assertEqual(guest.ready(run, 'a' * 12), {'ready': False})
            base.chromium.observe.assert_not_called()
            self.assertFalse((run / 'cold-browser.json').exists())

    def test_ready_brackets_same_birth_mapping_endpoint_and_exact_document(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, event, lease, endpoint, target, client = self.scenario(run)
            with patch.object(guest, 'original', return_value=(run, base, value, event)), \
                 patch.object(guest.warm, 'admitted'), \
                 patch.object(guest.warm, 'warm_native_clients', return_value=[client]):
                result = guest.ready(run, 'a' * 12)
                self.assertTrue(result['ready'])
                self.assertEqual(result['identity'], {'lease': lease, 'endpoint': endpoint, 'target': target})
                with self.assertRaises(ValueError):
                    guest.ready(run, 'a' * 12)
            self.assertEqual(base.chromium.observe.call_count, 3)
            self.assertEqual(base.chromium.observe.call_args.args[-1], lease)

    def test_changed_identity_or_unexpected_page_never_persists_an_immutable_admission(self):
        for change in ('scope', 'endpoint', 'page', 'client', 'gone'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                run = Path(directory)
                base, value, event, lease, endpoint, target, client = self.scenario(run)
                clients = Mock(return_value=[client])
                if change == 'scope':
                    base.chromium.observe.side_effect = [lease, ValueError('changed')]
                elif change == 'endpoint':
                    base.browser_endpoint.side_effect = [endpoint, dict(endpoint, port=1235)]
                elif change == 'page':
                    base.browser_pages.return_value = [dict(target, title='Foreign')]
                elif change == 'client':
                    clients.side_effect = [[client], [dict(client, pid=322)]]
                else:
                    base.chromium.original_gone.return_value = True
                with patch.object(guest, 'original', return_value=(run, base, value, event)), \
                     patch.object(guest.warm, 'admitted'), patch.object(guest.warm, 'warm_native_clients', clients):
                    with self.assertRaises(ValueError):
                        guest.ready(run, 'a' * 12)
                self.assertFalse((run / 'cold-browser.json').exists())

    def test_missing_devtools_waits_without_mistaking_it_for_changed_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, event, _, _, _, client = self.scenario(run)
            base.browser_endpoint.side_effect = FileNotFoundError()
            with patch.object(guest, 'original', return_value=(run, base, value, event)), \
                 patch.object(guest.warm, 'admitted'), patch.object(guest.warm, 'warm_native_clients', return_value=[client]):
                self.assertEqual(guest.ready(run, 'a' * 12), {'ready': False})
            self.assertFalse((run / 'cold-browser.json').exists())

    def test_endpoint_change_during_last_page_or_authority_read_is_rejected(self):
        for phase in ('ready', 'admitted'):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                run = Path(directory)
                base, value, event, lease, endpoint, target, client = self.scenario(run)
                captured = {'lease': lease, 'endpoint': endpoint, 'target': target}
                if phase == 'admitted':
                    (run / 'cold-browser.json').write_text(json.dumps(captured))
                base.browser_endpoint.side_effect = ([endpoint, endpoint, dict(endpoint, port=9999)]
                                                     if phase == 'ready' else [endpoint, dict(endpoint, port=9999)])
                with patch.object(guest, 'original', return_value=(run, base, value, event)), \
                     patch.object(guest.warm, 'admitted'), \
                     patch.object(guest.warm, 'warm_native_clients', return_value=[client]):
                    with self.assertRaises(ValueError):
                        (guest.ready if phase == 'ready' else guest.browser_authority)(run, 'a' * 12)
                if phase == 'ready':
                    self.assertFalse((run / 'cold-browser.json').exists())


class ColdRetirement(unittest.TestCase):
    def scenario(self, run):
        base, value, event, lease, *_ = facts(run)
        (run / 'events.lock').write_bytes(b'')
        order = []
        base.chromium.retire.side_effect = lambda _: order.append('cold')
        base.retire.side_effect = lambda *_: (order.append('canonical'), print(json.dumps({
            'owned_browser_units_retired': True, 'owned_entry_retired': True})))
        return base, value, event, lease, order

    def test_exec_complete_before_mapping_or_cdp_is_retired_before_canonical_unit_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, event, lease, order = self.scenario(run)
            current = {'pid': 321, 'start': '12345', 'group': '/new-owned.scope', 'executable': ['chromium-file']}
            with patch.object(guest, 'context', return_value=(run, {}, base)), \
                 patch.object(guest, 'original', return_value=(run, base, value, event)), \
                 patch.object(guest, 'process_snapshot', return_value=current), \
                 patch.object(guest, 'retire_original_unit') as generic:
                guest.retire(run, 'a' * 12)
                guest.retire(run, 'a' * 12)
            self.assertEqual(order, ['cold', 'canonical', 'cold', 'canonical'])
            generic.assert_not_called()
            self.assertFalse((run / 'cold-browser.json').exists())
            self.assertEqual(json.loads((run / 'cold-retirement.json').read_text()), lease)
            self.assertEqual((run / 'closing').read_bytes(), b'retiring\n')

    def test_original_wrapper_failure_uses_guarded_unit_then_remains_idempotent(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, event, _, order = self.scenario(run)
            current = {'start': '12345', 'group': event['cgroup'], 'executable': ['python-or-launcher']}
            with patch.object(guest, 'context', return_value=(run, {}, base)), \
                 patch.object(guest, 'original', return_value=(run, base, value, event)), \
                 patch.object(guest, 'process_snapshot', return_value=current), \
                 patch.object(guest, 'retire_original_unit', side_effect=lambda *a, **k: order.append('wrapper')) as generic, \
                 patch.object(guest, '_retirement_drained', return_value=True):
                guest.retire(run, 'a' * 12)
                guest.retire(run, 'a' * 12)
            self.assertEqual(order, ['wrapper', 'canonical', 'canonical'])
            generic.assert_called_once()
            base.chromium.observe.assert_not_called()
            base.chromium.retire.assert_not_called()

    def test_gone_without_recorded_scope_lease_is_not_a_successful_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, value, event, _, order = self.scenario(run)
            base.chromium.original_gone.return_value = True
            with patch.object(guest, 'context', return_value=(run, {}, base)), \
                 patch.object(guest, 'original', return_value=(run, base, value, event)), \
                 patch.object(guest, 'retire_original_unit') as generic:
                with self.assertRaises(ValueError):
                    guest.retire(run, 'a' * 12)
            self.assertEqual(order, [])
            generic.assert_not_called()
            base.chromium.retire.assert_not_called()

    def test_unknown_moved_wrapper_or_wrong_birth_never_grants_scope_authority(self):
        for change in ({'group': '/foreign.scope'}, {'start': 'replacement'}):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                run = Path(directory)
                base, value, event, _, order = self.scenario(run)
                current = dict({'start': '12345', 'group': event['cgroup'], 'executable': ['python']}, **change)
                with patch.object(guest, 'context', return_value=(run, {}, base)), \
                     patch.object(guest, 'original', return_value=(run, base, value, event)), \
                     patch.object(guest, 'process_snapshot', return_value=current), \
                     patch.object(guest, 'retire_original_unit') as generic:
                    with self.assertRaises(ValueError):
                        guest.retire(run, 'a' * 12)
                self.assertEqual(order, [])
                generic.assert_not_called()

    def test_no_event_revokes_queued_work_before_canonical_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, _, _, _, order = self.scenario(run)
            base.events.return_value = []
            with patch.object(guest, 'context', return_value=(run, {}, base)):
                guest.retire(run, 'a' * 12)
            self.assertEqual(order, ['canonical'])
            self.assertEqual((run / 'default-closing').read_bytes(), b'closing\n')
            base.chromium.observe.assert_not_called()

    def test_independent_guardian_retires_cold_scope_first_and_still_restores_mime_if_it_fails(self):
        for fail in (False, True):
            with self.subTest(fail=fail), tempfile.TemporaryDirectory() as directory:
                run = Path(directory)
                (run / 'events.lock').write_bytes(b'')
                order = []
                def retirement(*_):
                    order.append('cold-retire')
                    if fail:
                        raise ValueError('refused')
                with patch.object(guest, 'context', return_value=(run, {}, None)), \
                     patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(run)}), \
                     patch.object(guest, 'capture_unit_origin', return_value={'original': True}), \
                     patch.object(guest.time, 'monotonic', side_effect=[0, 220]), \
                     patch.object(guest, 'retire', side_effect=retirement), \
                     patch.object(guest.warm, 'wait_expiry', side_effect=lambda *_: order.append('expiry')), \
                     patch.object(guest.p, 'lease', side_effect=lambda _, name: name), \
                     patch.object(guest.p, 'restore', side_effect=lambda name: order.append(name)), \
                     patch.object(guest.warm, 'close_history', side_effect=lambda *_: order.append('history')):
                    guest.watchdog(run, 'a' * 12)
                self.assertEqual(order, ['cold-retire', 'expiry', 'default-mime', 'default-entry', 'history'])
                result = json.loads((run / 'default-finished.json').read_text())
                self.assertIs(result['success'], not fail)
                self.assertIs(result['restoration']['browser_and_original_entry_retired'], not fail)
                self.assertTrue(result['restoration']['default_mime_restored'])


class ColdDocumentKey(unittest.TestCase):
    def test_only_exact_cold_document_is_observed_without_cdp_focus_commands(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            base, _, _, lease, endpoint, target, _ = facts(run)
            captured = {'lease': lease, 'endpoint': endpoint, 'target': target}
            base.browser_http = Mock(return_value=json.dumps([dict(target, type='page',
                webSocketDebuggerUrl='ws://127.0.0.1:1234/devtools/page/target-owned')]).encode())
            request = {'identity': captured, 'before': [], 'targets': [target], 'case': 'pointer'}
            peer = Mock()
            peer.call.return_value = {'result': {'value': {'armed': True, 'count': 0}}}
            transport = types.SimpleNamespace(Peer=types.SimpleNamespace(connect=Mock(return_value=peer)))
            with patch.object(guest, 'context', return_value=(run, {'cold_case': 'pointer'}, base)), \
                 patch.object(guest.warm, 'admitted'), \
                 patch.object(guest, 'browser_authority', return_value=captured), \
                 patch.dict(sys.modules, {'cdp_peer': transport}):
                self.assertEqual(guest.browser_key(run, 'a' * 12, request, True), {'armed': True, 'count': 0})
                for bad in (dict(request, before=[{'url': 'about:blank'}]), dict(request, case='keyboard'),
                            dict(request, targets=[dict(target, id='foreign')])):
                    with self.assertRaises(ValueError):
                        guest.browser_key(run, 'a' * 12, bad, True)
            self.assertEqual(peer.call.call_count, 1)
            self.assertEqual(peer.call.call_args.args[0], 'Runtime.evaluate')
            self.assertIs(peer.call.call_args.args[1]['userGesture'], False)
            expression = peer.call.call_args.args[1]['expression']
            self.assertIn('event.isTrusted', expression)
            self.assertNotIn('focus(', expression)
            self.assertEqual(driver.assert_browser_key_handoff, driver.warm.assert_browser_key_handoff)


class ColdController(unittest.TestCase):
    def test_default_and_incomplete_run_never_reach_native_transport(self):
        with patch.object(sys, 'argv', ['cold']), patch.object(driver, 'run') as run, \
             patch.object(driver, 'qualify') as qualify, contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(driver.main(), 0)
            self.assertIs(json.loads(output.getvalue())['guest_actions'], False)
            qualify.assert_not_called()
            run.assert_not_called()
        with patch.object(driver, 'MANIFEST_SHA', None), patch.object(driver.subprocess, 'check_output') as command:
            with self.assertRaisesRegex(ValueError, 'cold_runtime_manifest_unbound'):
                driver.qualify(argparse.Namespace())
            command.assert_not_called()

    def test_bound_qualification_still_requires_fresh_pid_exact_uuid_and_all_source_guards(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / 'manifest.json'
            raw = json.dumps({'source_commit': driver.SOURCE, 'passed': True,
                              'source_working_tree_changed': False}).encode()
            manifest.write_bytes(raw)
            digest = hashlib.sha256(raw).hexdigest()
            args = argparse.Namespace(manifest_sha=digest, release_manifest=manifest,
                expected_desktop_pid=321, vm_uuid='a4691892-fa44-41d9-98cf-8f0dc4131ee3')
            def source(command, **_):
                if command[1] == 'show':
                    return (ROOT / command[2].split(':', 1)[1]).read_bytes()
                return str(ROOT / '.git')
            with patch.object(driver, 'MANIFEST_SHA', digest), \
                 patch.object(driver.subprocess, 'check_output', side_effect=source) as observed:
                self.assertEqual(driver.qualify(args), json.loads(raw))
                self.assertEqual(sum(call.args[0][1] == 'show' for call in observed.call_args_list), 18)
                for field, value in (('expected_desktop_pid', None), ('expected_desktop_pid', 1),
                                     ('vm_uuid', None), ('vm_uuid', 'other'), ('manifest_sha', '0' * 64)):
                    bad = argparse.Namespace(**dict(vars(args), **{field: value}))
                    observed.reset_mock()
                    with self.assertRaises(ValueError):
                        driver.qualify(bad)
                    observed.assert_not_called()

    def test_bound_wrong_source_archive_fails_without_native_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / 'manifest.json'
            raw = json.dumps({'source_commit': driver.REUSE_SOURCE, 'passed': True,
                              'source_working_tree_changed': False}).encode()
            manifest.write_bytes(raw)
            digest = hashlib.sha256(raw).hexdigest()
            args = argparse.Namespace(manifest_sha=digest, release_manifest=manifest,
                expected_desktop_pid=321, vm_uuid='a4691892-fa44-41d9-98cf-8f0dc4131ee3')
            with patch.object(driver, 'MANIFEST_SHA', digest), \
                 patch.object(driver.subprocess, 'check_output') as observed:
                with self.assertRaisesRegex(ValueError, 'exact_built_runtime_source_required'):
                    driver.qualify(args)
                observed.assert_not_called()

    def test_second_profile_is_not_allocated_if_first_feature_or_cleanup_failed(self):
        for failed in ({'success': False}, {'success': True, 'case': 'pointer', 'cleanup': {'owned': False}}):
            with self.subTest(failed=failed), patch.object(driver, 'run_case', return_value=failed) as execute:
                if failed['success']:
                    with self.assertRaises(ValueError):
                        driver.run(object(), {})
                else:
                    self.assertEqual(driver.run(object(), {}), 1)
                self.assertEqual(execute.call_count, 1)
                self.assertEqual(execute.call_args.args[-1], 'pointer')

    def test_both_routes_use_separate_successful_case_lifetimes(self):
        def receipt(case):
            return {'case': case, 'success': True, 'cleanup': {'owned': True}, 'cold_browser': {'lease': {
                'declared': {'profile': '/tmp/' + case, 'class': case}, 'origin': {'case': case},
                'process': {'pid': 321, 'start': case}}}}
        with patch.object(driver, 'run_case', side_effect=lambda _, __, case: receipt(case)) as execute:
            self.assertEqual(driver.run(object(), {}), 0)
        self.assertEqual([call.args[-1] for call in execute.call_args_list], ['pointer', 'keyboard'])

    def test_bridge_failure_is_bound_sanitized_and_never_errors_as_success(self):
        packet = c.error_packet('cold-ready', ValueError('secret'))
        result = types.SimpleNamespace(returncode=1, stdout=b'', stderr=json.dumps(packet).encode())
        with patch.object(driver.subprocess, 'run', return_value=result) as command:
            with self.assertRaises(driver.BridgeFailure) as raised:
                driver.bridge_command(['fixed-transport'], 'cold-ready')
            self.assertEqual(raised.exception.details, packet)
            self.assertEqual(command.call_args.kwargs['timeout'], 30)
        for result in (types.SimpleNamespace(returncode=0, stdout=json.dumps(packet).encode(), stderr=b''),
                       types.SimpleNamespace(returncode=1, stdout=b'', stderr=b'secret raw exception'),
                       types.SimpleNamespace(returncode=0, stdout=b'{}', stderr=b'noise')):
            with patch.object(driver.subprocess, 'run', return_value=result):
                with self.assertRaises(driver.BridgeFailure) as raised:
                    driver.bridge_command(['fixed-transport'], 'cold-ready')
                self.assertNotIn('secret', json.dumps(raised.exception.details))

    def test_actual_guest_dispatch_and_failure_envelope_have_no_warm_operation(self):
        with patch.object(sys, 'argv', ['fixture', 'cold-ready', '/tmp/owned', 'a' * 12]), \
             patch.object(guest, 'context', return_value=(Path('/tmp/owned'), {}, None)), \
             patch.object(guest, 'ready', side_effect=ValueError('secret_lowercase_token')), \
             contextlib.redirect_stderr(io.StringIO()) as output:
            self.assertEqual(guest.main(), 1)
        packet = json.loads(output.getvalue())
        self.assertEqual(c.validate_error(packet, 'cold-ready')['fixed_reason'], 'unclassified_fixture_failure')
        self.assertNotIn('secret_lowercase_token', output.getvalue())
        self.assertNotIn('warm', c.OPERATIONS)
        self.assertNotIn('warm-ready', c.OPERATIONS)

    def test_guest_dispatch_cannot_create_the_other_routes_notification(self):
        with patch.object(sys, 'argv', ['fixture', 'notify', '/tmp/owned', 'a' * 12, 'keyboard']), \
             patch.object(guest, 'context', return_value=(Path('/tmp/owned'), {'cold_case': 'pointer'}, None)), \
             patch.object(guest.warm, 'notify') as notify, \
             contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(guest.main(), 1)
            notify.assert_not_called()


if __name__ == '__main__':
    unittest.main()
