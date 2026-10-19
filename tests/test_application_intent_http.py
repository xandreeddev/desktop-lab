"""Owned HTTP URI fixture policy; mocks/tempfiles only, never bind a socket."""
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import types
import unittest
from unittest.mock import MagicMock, Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
spec = importlib.util.spec_from_file_location('intent_http_fixture', ROOT / 'vm/fixtures/application-intent-fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)

MANIFEST = {'run_id': 'abcdef', 'http_mode': True,
            'files': [{'name': 'one.html', 'title': 'Owned first', 'uri': 'file:///owned/one.html'},
                      {'name': 'two.html', 'title': 'Owned second', 'uri': 'file:///owned/two.html'}]}
ORIGIN = {'unit': {'invocation': 'a' * 32},
          'process': {'pid': 123, 'start': '42', 'group': '/owned.service', 'executable': [1, 2, 3]}}
SOCKET = types.SimpleNamespace(st_dev=10, st_ino=456, st_mode=stat.S_IFSOCK | 0o777,
                              st_uid=os.getuid(), st_gid=os.getgid())
LISTENER = {'fd': 7, 'port': 12345,
            'identity': [10, 456, SOCKET.st_mode, os.getuid(), os.getgid()]}


def tcp_row(*, local='0100007F:3039', remote='00000000:0000', state='0A', uid=None, inode=456):
    uid = os.getuid() if uid is None else uid
    return f'0: {local} {remote} {state} 00000000:00000000 00:00000000 00000000 {uid} 0 {inode}'


def tcp_bytes(*rows):
    return ('sl local_address rem_address st tx_queue rx_queue uid inode\n' + '\n'.join(rows) + '\n').encode()


def request(target='/owned', *, method='GET', headers='Host: 127.0.0.1:12345', tail=''):
    return f'{method} {target} HTTP/1.1\r\n{headers}\r\n\r\n{tail}'.encode()


class FixedHTTPProtocol(unittest.TestCase):
    def response(self, raw):
        return fixture.http_response(raw, {'/owned': b'<title>Owned</title>'}, 12345)

    def test_only_exact_owned_route_has_content_and_no_external_resource_authority(self):
        response = self.response(request())
        self.assertTrue(response.startswith(b'HTTP/1.1 200 OK\r\n'))
        self.assertTrue(response.endswith(b'<title>Owned</title>'))
        self.assertIn(b"Content-Security-Policy: default-src 'none'; base-uri 'none'; form-action 'none'", response)
        self.assertIn(b'Connection: close\r\n', response)
        self.assertIn(b'Cache-Control: no-store\r\n', response)
        for target in ('/', '/owned/../one.html', '/owned#fragment', 'http://foreign/owned'):
            with self.subTest(target=target):
                rejected = self.response(request(target))
                self.assertTrue(rejected.startswith(b'HTTP/1.1 404 Not Found'))
                self.assertEqual(rejected.split(b'\r\n\r\n')[1], b'')

    def test_head_has_same_length_but_never_sends_the_body(self):
        get, head = self.response(request()), self.response(request(method='HEAD'))
        self.assertEqual(get.split(b'\r\n\r\n')[0], head.split(b'\r\n\r\n')[0])
        self.assertEqual(head.split(b'\r\n\r\n')[1], b'')

    def test_foreign_duplicate_missing_hosts_body_transfer_and_malformed_headers_are_rejected(self):
        for raw in (request(headers='Host: foreign'), request(headers='X-Test: none'),
                    request(headers='Host: 127.0.0.1:12345\r\nHost: 127.0.0.1:12345'),
                    request(headers='Host: 127.0.0.1:12345\r\nTransfer-Encoding: chunked'),
                    request(headers='Host: 127.0.0.1:12345\r\nContent-Length: 1'),
                    request(tail='body'), request(headers='Host: 127.0.0.1:12345\r\nMalformed'),
                    request(headers='Host: 127.0.0.1:12345\r\n Bad: folded'),
                    request(headers='Host: 127.0.0.1:12345\r\nBad: \x00'),
                    request(headers='Host: 127.0.0.1:12345\r\n' + 'X: a\r\n' * 33),
                    b'\xff\r\n\r\n', b'x' * 8193, None):
            with self.subTest(raw=type(raw).__name__):
                self.assertTrue(self.response(raw).startswith(b'HTTP/1.1 400 Bad Request'))
        self.assertTrue(self.response(request(method='POST')).startswith(b'HTTP/1.1 405 Method Not Allowed'))

    def test_slow_partial_headers_have_one_absolute_deadline(self):
        connection = Mock()
        connection.recv.return_value = b'G'
        with patch.object(fixture.time, 'monotonic', side_effect=[0, 0, .9, 1.01]):
            fixture.http_connection(connection, {'/owned': b'owned'}, 12345)
        self.assertEqual(connection.recv.call_count, 2)
        connection.sendall.assert_not_called()
        self.assertTrue(all(0 < call.args[0] <= .25 for call in connection.settimeout.call_args_list))

    def test_complete_request_is_bounded_and_response_send_has_a_timeout(self):
        connection = Mock()
        raw = request()
        connection.recv.side_effect = [raw[:8], raw[8:]]
        fixture.http_connection(connection, {'/owned': b'owned'}, 12345)
        connection.sendall.assert_called_once()
        self.assertTrue(connection.sendall.call_args.args[0].endswith(b'owned'))
        self.assertTrue(all(0 < call.args[0] <= 4096 for call in connection.recv.call_args_list))
        self.assertEqual(connection.settimeout.call_args.args, (.5,))

    def test_oversized_partial_headers_and_broken_transport_never_send_content(self):
        for chunks in ([b'x' * 4096, b'x' * 4096, b'x'], [ConnectionResetError()]):
            connection = Mock()
            connection.recv.side_effect = chunks
            fixture.http_connection(connection, {'/owned': b'owned'}, 12345)
            connection.sendall.assert_not_called()


class ListenerAuthority(unittest.TestCase):
    def test_tcp_join_requires_exact_inode_ipv4_loopback_port_uid_and_listen_state(self):
        fixture.http_tcp_listener(tcp_bytes(tcp_row()), LISTENER)
        for rows in ([tcp_row(local='00000000:3039')], [tcp_row(local='0100007F:303A')],
                     [tcp_row(remote='0100007F:0001')], [tcp_row(state='01')],
                     [tcp_row(uid=os.getuid() + 1)], [tcp_row(inode=457)],
                     [tcp_row(), tcp_row()], ['malformed'], []):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                fixture.http_tcp_listener(tcp_bytes(*rows), LISTENER)
        with self.assertRaises(ValueError):
            fixture.http_tcp_listener(b'x' * 262145, LISTENER)

    def test_fd_metadata_and_kernel_socket_link_are_bracketed_around_the_tcp_observation(self):
        with patch.object(Path, 'stat', return_value=SOCKET), \
             patch.object(Path, 'open', return_value=io.BytesIO(tcp_bytes(tcp_row()))), \
             patch.object(fixture.os, 'readlink', return_value='socket:[456]'):
            fixture.http_listener_snapshot(ORIGIN, LISTENER)
        changed = types.SimpleNamespace(**{**vars(SOCKET), 'st_ino': 457})
        with patch.object(Path, 'stat', side_effect=[SOCKET, changed]), \
             patch.object(Path, 'open', return_value=io.BytesIO(tcp_bytes(tcp_row()))), \
             patch.object(fixture.os, 'readlink', return_value='socket:[456]'), self.assertRaises(ValueError):
            fixture.http_listener_snapshot(ORIGIN, LISTENER)
        with patch.object(Path, 'stat', return_value=SOCKET), \
             patch.object(Path, 'open') as contacted, \
             patch.object(fixture.os, 'readlink', return_value='socket:[457]'), self.assertRaises(ValueError):
            fixture.http_listener_snapshot(ORIGIN, LISTENER)
        contacted.assert_not_called()

    def captured(self):
        return {'origin': ORIGIN, 'listener': LISTENER, 'targets': fixture.http_documents(MANIFEST, 12345)}

    def patches(self, captured, current=ORIGIN['process']):
        stack = self.enterContext
        stack(patch.object(fixture, 'context', return_value=(Path('/owned'), MANIFEST)))
        stack(patch.object(fixture, 'original_receipt', side_effect=lambda _, name:
                          captured if name == 'http-origin.json' else ORIGIN))
        stack(patch.object(fixture, 'owned_argv', return_value=['owned']))
        stack(patch.object(fixture, 'unit_snapshot', return_value={'active': 'active'}))
        live = stack(patch.object(fixture, 'assert_unit_origin', return_value=current))
        listener = stack(patch.object(fixture, 'http_listener_snapshot'))
        return live, listener

    def test_live_original_is_rechecked_before_and_after_listener_proof(self):
        captured = self.captured()
        live, listener = self.patches(captured)
        self.assertEqual(fixture.http_authority('/owned', 'abcdef', captured), captured)
        self.assertEqual(live.call_count, 2)
        listener.assert_called_once_with(ORIGIN, LISTENER)

    def test_inactive_replaced_process_or_changed_receipt_cannot_authorize_a_listener(self):
        for current in (None, dict(ORIGIN['process'], executable=[9, 9])):
            with self.subTest(current=current):
                live, listener = self.patches(self.captured(), current)
                with self.assertRaises(ValueError):
                    fixture.http_authority('/owned', 'abcdef')
                listener.assert_not_called()
        captured = self.captured()
        live, listener = self.patches(captured)
        with self.assertRaises(ValueError):
            fixture.http_authority('/owned', 'abcdef', dict(captured, targets=[]))
        listener.assert_not_called()
        live.assert_not_called()

    def test_foreign_target_or_successor_unit_is_never_adopted(self):
        captured = copy.deepcopy(self.captured())
        captured['targets'][0]['uri'] = 'http://foreign/'
        self.patches(captured)
        with self.assertRaises(ValueError):
            fixture.http_authority('/owned', 'abcdef')
        with patch.object(fixture, 'assert_unit_origin', side_effect=AssertionError('successor')):
            with self.assertRaises(AssertionError):
                fixture.http_authority('/owned', 'abcdef')


class ModeAndRetirement(unittest.TestCase):
    def test_target_uris_are_fixed_loopback_encoded_values_and_ports_are_checked(self):
        documents = fixture.http_documents(MANIFEST, 12345)
        self.assertEqual([item['title'] for item in documents], ['Owned first', 'Owned second'])
        self.assertTrue(all(item['uri'].startswith('http://127.0.0.1:12345/abcdef/') for item in documents))
        self.assertIn('%25U%E9%9B%AA?encoded=%2520&field=%25u#first', documents[0]['uri'])
        for port in (True, 0, 1023, 65536, 12345.0, '12345'):
            with self.subTest(port=port), self.assertRaises(ValueError):
                fixture.http_documents(MANIFEST, port)

    def test_default_files_and_declared_action_do_not_use_the_http_target_set(self):
        default = {'files': MANIFEST['files']}
        with patch.object(fixture, 'http_authority') as http:
            self.assertEqual(fixture.request_documents('/owned', 'abcdef', default, 'open'), default['files'])
            self.assertEqual(fixture.request_documents('/owned', 'abcdef', MANIFEST, 'action'), default['files'])
        http.assert_not_called()
        with patch.object(fixture, 'http_authority', return_value={'targets': ['owned-http']}):
            self.assertEqual(fixture.request_documents('/owned', 'abcdef', MANIFEST, 'open'), ['owned-http'])

    def test_http_wrapper_rejects_partial_duplicate_or_foreign_uris_before_reading_or_launching(self):
        documents = fixture.http_documents(MANIFEST, 12345)
        uris = [item['uri'] for item in documents]
        with patch.object(fixture, 'context', return_value=(Path('/owned'), MANIFEST)), \
             patch.object(fixture, 'request_documents', return_value=documents), \
             patch.object(fixture, 'regular') as read, patch.object(fixture.os, 'execv') as execute:
            for values in ([uris[0]], [uris[0], uris[0]], ['http://foreign/'], [*uris, uris[0]]):
                with self.subTest(values=values), self.assertRaises(ValueError):
                    fixture.execute('/owned', 'abcdef', ['open', *values])
        read.assert_not_called()
        execute.assert_not_called()

    def test_absent_unit_still_requires_original_recursive_cgroup_drain(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            fixture.original_receipt(directory, 'watchdog-origin.json', ORIGIN)
            fixture.original_receipt(directory, 'http-origin.json', {'origin': ORIGIN})
            with patch.object(fixture, 'context', return_value=(directory, MANIFEST)), \
                 patch.object(fixture, 'unit_snapshot', return_value={'load': 'not-found'}), \
                 patch.object(fixture, 'retirement_drained', return_value=False), self.assertRaises(ValueError):
                fixture.retire_watchdog(directory, 'abcdef')
            with patch.object(fixture, 'context', return_value=(directory, MANIFEST)), \
                 patch.object(fixture, 'unit_snapshot', return_value={'load': 'not-found'}), \
                 patch.object(fixture, 'retirement_drained', return_value=True):
                self.assertEqual(fixture.retire_watchdog(directory, 'abcdef'), {'http_server_retired': True})

    def test_live_watchdog_without_original_receipt_never_grants_stop_authority(self):
        with tempfile.TemporaryDirectory() as name, \
             patch.object(fixture, 'context', return_value=(Path(name), MANIFEST)), \
             patch.object(fixture, 'unit_snapshot', return_value={'load': 'loaded'}), \
             patch.object(fixture, 'retire_original_unit') as stopped, self.assertRaises(ValueError):
            fixture.retire_watchdog(name, 'abcdef')
        stopped.assert_not_called()

    def test_server_failure_closes_listener_and_runs_original_watchdog_retirement(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            manifest = copy.deepcopy(MANIFEST)
            for item in manifest['files']:
                body = ('<title>' + item['title'] + '</title>').encode()
                fixture.private(directory / item['name'], body)
                item['sha256'] = fixture.digest(body)
            server = MagicMock()
            server.__enter__.return_value = server
            server.getsockname.return_value = ('127.0.0.1', 12345)
            server.fileno.return_value = 60000
            server.accept.side_effect = OSError('fixture listener failure')
            closed = []
            server.bind.side_effect = lambda *_: self.assertTrue((directory / 'watchdog-origin.json').is_file())
            server.__exit__.side_effect = lambda *_: closed.append(True)
            def retirement(*_):
                self.assertEqual(closed, [True])
                ready = fixture.original_receipt(directory, 'http-origin.json')
                self.assertEqual(ready['origin'], ORIGIN)
                self.assertEqual(ready['listener'], dict(LISTENER, fd=60000))
            original_fstat = os.fstat
            with patch.object(fixture, 'context', return_value=(directory, manifest)), \
                 patch.object(fixture, 'owned_argv', return_value=['owned']), \
                 patch.object(fixture, 'capture_unit_origin', return_value=ORIGIN), \
                 patch.object(fixture.os, 'fstat', side_effect=lambda fd:
                              SOCKET if fd == 60000 else original_fstat(fd)), \
                 patch.object(fixture, 'http_listener_snapshot'), \
                 patch.object(fixture.socket, 'socket', return_value=server), \
                 patch.object(fixture.signal, 'alarm'), \
                 patch.object(fixture, 'retire', side_effect=retirement) as retired, \
                 self.assertRaises(OSError):
                fixture.watchdog(directory, 'abcdef')
            server.bind.assert_called_once_with(('127.0.0.1', 0))
            server.listen.assert_called_once_with(4)
            retired.assert_called_once()

    def test_default_watchdog_still_waits_without_opening_any_socket(self):
        with patch.object(fixture, 'context', return_value=(Path('/owned'), {'files': []})), \
             patch.object(fixture, 'owned_argv', return_value=['owned']), \
             patch.object(fixture, 'capture_unit_origin', return_value=ORIGIN), \
             patch.object(fixture, 'original_receipt', return_value=ORIGIN), \
             patch.object(fixture.signal, 'alarm'), patch.object(fixture.time, 'sleep') as sleep, \
             patch.object(fixture, 'retire'), patch.object(fixture.socket, 'socket') as socket:
            fixture.watchdog('/owned', 'abcdef')
        sleep.assert_called_once_with(210)
        socket.assert_not_called()

    def test_readiness_failure_before_example_start_still_retires_watchdog_and_listener(self):
        spec = importlib.util.spec_from_file_location('intent_http_driver', ROOT / 'vm/test-application-intent.py')
        driver = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(driver)
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            (directory / 'reports/local').mkdir(parents=True)
            binary = directory / 'example'
            binary.write_bytes(b'owned example')
            manifest = directory / 'release.json'
            manifest.write_text(json.dumps({'source_commit': 'a' * 40, 'binaries': {
                key: {'sha256': 'b' * 64} for key in ('application-intent', 'lucent-desktop', 'lucent-cli')}}))
            monitor = {'id': 1, 'name': 'Virtual-1', 'width': 1920, 'height': 1080, 'x': 0, 'y': 0,
                       'scale': 1, 'transform': 0, 'dpmsStatus': True}
            client = {'launcher': False, 'query': '', 'settings_save_status': 'saved',
                      'wallpaper_busy': False, 'wallpaper': 'preserved'}
            observed = []
            def session(*args):
                if args[:2] == ('omarchy-shell', 'lock'):
                    return 'false'
                if args[:3] == ('hyprctl', '-j', 'clients'):
                    return '[]'
                if args[:3] == ('hyprctl', '-j', 'layers'):
                    return '{}'
                if args[:3] == ('hyprctl', '-j', 'monitors'):
                    return json.dumps([monitor])
                if args[:3] == ('hyprctl', '-j', 'activeworkspace'):
                    return '{"id":1,"name":"1","monitorID":1}'
                if args[:3] == ('hyprctl', '-j', 'cursorpos'):
                    return '{"x":960,"y":540}'
                return 'true'
            def remote(*args):
                if args[0] == 'sha256sum' or args[0] == 'python3' and args[-1] != 'preferences':
                    return 'b' * 64
                if args[0] == 'mktemp':
                    return '/tmp/lucent-application-intent.abcdef'
                return '{}'
            def require(condition, label):
                if not condition:
                    raise AssertionError(label)
            def command(_argv, operation, *_args):
                observed.append(operation)
                if operation == 'initialize':
                    return json.dumps({'app_id': 'owned.desktop', 'files': MANIFEST['files']})
                if operation == 'initial-absence':
                    return '{"absent":true}'
                if operation == 'admission':
                    return '{"baseline":{"cohort":"original"}}'
                if operation == 'http-info':
                    raise driver.FixtureFailure({'operation': operation, 'kind': 'deadline'})
                if operation == 'retire-example':
                    return '{"retired":true,"socket_admitted":false,"socket_retired":false}'
                if operation == 'retire':
                    return '{"owned_browser_units_retired":true,"owned_entry_retired":true}'
                if operation == 'retire-watchdog':
                    return '{"retired":true,"http_server_retired":true}'
                if operation == 'guard':
                    return '{"owned_clients":0}'
                self.fail('Unexpected fixture operation: ' + operation)
            lab = types.SimpleNamespace(ROOT=directory, inventory=lambda: {'vms': {'lucent': {'name': 'owned'}}},
                                        ssh_args=lambda *_: ['unused-ssh'])
            replacements = {'lab': lab, 'remote': remote, 'session': session, 'cli': Mock(),
                            'inspect': lambda: {'client': client}, 'require': require,
                            'eventually': lambda predicate, _label, **_kwargs: require(predicate(), 'test_predicate'),
                            'a': {'keyboard_preflight': Mock(), 'PREFERENCES': 'preferences'},
                            'h': {'run': Mock()}}
            for key, value in replacements.items():
                self.enterContext(patch.object(driver, key, value, create=True))
            results = {'checks': {}}
            with patch.object(driver, 'checked_binary', return_value=binary), \
                 patch.object(driver, 'fixture_command', side_effect=command), \
                 self.assertRaises(driver.FixtureFailure):
                driver.suite(types.SimpleNamespace(release_manifest=str(manifest), example_binary=None,
                                                  http_uris=True), results)
            self.assertNotIn('example-command', observed)
            self.assertLess(observed.index('retire-example'), observed.index('retire-watchdog'))
            self.assertTrue(results['cleanup']['example_retired'])
            self.assertTrue(results['cleanup']['watchdog_retired'])
            self.assertTrue(results['cleanup']['http_server_retired'])
            self.assertTrue(results['cleanup']['output_workspace_and_cursor_preserved'])
            self.assertEqual(results['primary_failure']['phase'], 'loopback_http_server_admission')
            self.assertNotIn('cleanup_failures', results)


if __name__ == '__main__':
    unittest.main()
