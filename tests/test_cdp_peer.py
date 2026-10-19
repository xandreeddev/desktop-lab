"""Protocol boundaries used by the real-browser clipboard acceptance peer."""

import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import time
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('owned_cdp_peer', ROOT / 'vm/cdp_peer.py')
cdp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cdp)


def frame(data, opcode=1, final=True):
    header = bytes(((0x80 if final else 0) | opcode, len(data))) if len(data) < 126 else (
        bytes(((0x80 if final else 0) | opcode, 126)) + struct.pack('!H', len(data)))
    return header + data


class Connection:
    def __init__(self, incoming=b''):
        self.incoming = bytearray(incoming)
        self.sent = bytearray()
        self.closed = False

    def recv(self, maximum):
        value = bytes(self.incoming[:min(maximum, 17)])
        del self.incoming[:len(value)]
        return value

    def sendall(self, data):
        self.sent.extend(data)

    def settimeout(self, _timeout):
        pass

    def close(self):
        self.closed = True


def client_frames(data):
    result = []
    offset = 0
    while offset < len(data):
        opcode, size = data[offset] & 15, data[offset + 1] & 127
        assert data[offset + 1] & 128
        offset += 2
        if size == 126:
            size = struct.unpack('!H', data[offset:offset + 2])[0]
            offset += 2
        elif size == 127:
            size = struct.unpack('!Q', data[offset:offset + 8])[0]
            offset += 8
        mask = data[offset:offset + 4]
        offset += 4
        payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(data[offset:offset + size]))
        offset += size
        result.append((opcode, payload))
    return result


class PeerTests(unittest.TestCase):
    def test_fragmented_unicode_response_with_ping_and_notifications(self):
        value = {'id': 1, 'result': {'result': {'value': '雪 e\u0301 👩\u200d💻'}}}
        data = json.dumps(value, ensure_ascii=False).encode()
        # Deliberately split inside a multibyte UTF-8 scalar.
        split = data.index('雪'.encode()) + 1
        connection = Connection(frame(b'{"method":"Runtime.contextCreated"}')
                                + frame(data[:split], final=False)
                                + frame(b'probe', opcode=9)
                                + frame(data[split:], opcode=0))
        peer = cdp.Peer(connection)
        result = peer.call('Runtime.evaluate', {'expression': 'owned fixture'})
        self.assertEqual(result, value['result'])
        sent = client_frames(connection.sent)
        self.assertEqual(json.loads(sent[0][1])['id'], 1)
        self.assertEqual(sent[1], (10, b'probe'))

    def test_peer_cannot_redirect_to_remote_or_other_browser_endpoint(self):
        for url in ('ws://example.com:9222/devtools/page/A',
                    'ws://127.0.0.1:9223/devtools/page/A',
                    'ws://user@127.0.0.1:9222/devtools/page/A',
                    'ws://127.0.0.1:9222/devtools/browser/A',
                    'ws://127.0.0.1:9222/devtools/page/B',
                    'ws://127.0.0.1:9222/devtools/page/A?redirect=1'):
            with self.subTest(url=url), patch.object(cdp.socket, 'create_connection') as connect:
                with self.assertRaises(cdp.PeerError):
                    cdp.Peer.connect(url, 9222, 'A')
                connect.assert_not_called()

    def test_upgrade_hash_and_buffered_first_frame(self):
        key = base64.b64encode(bytes(16)).decode()
        accept = base64.b64encode(hashlib.sha1(
            (key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
        upgrade = ('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n'
                   'Sec-WebSocket-Accept: ' + accept + '\r\n\r\n').encode()
        connection = Connection(upgrade + frame(b'{}'))
        with patch.object(cdp.socket, 'create_connection', return_value=connection), \
                patch.object(cdp.os, 'urandom', return_value=bytes(16)):
            peer = cdp.Peer.connect('ws://127.0.0.1:9222/devtools/page/A', 9222, 'A')
        self.assertEqual(peer.receive(time.monotonic() + 2), '{}')
        peer.close()
        self.assertTrue(connection.closed)

    def test_invalid_upgrade_closes_connection(self):
        connection = Connection(b'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n'
                                b'Connection: Upgrade\r\nSec-WebSocket-Accept: forged\r\n\r\n')
        with patch.object(cdp.socket, 'create_connection', return_value=connection):
            with self.assertRaisesRegex(cdp.PeerError, 'verified_uncompressed_upgrade_required'):
                cdp.Peer.connect('ws://127.0.0.1:9222/devtools/page/A', 9222, 'A')
        self.assertTrue(connection.closed)

    def test_masked_binary_or_invalid_continuation_is_rejected(self):
        for data in (b'\x81\x80', frame(b'xx', opcode=2), frame(b'xx', opcode=0),
                     frame(b'a', final=False) + frame(b'b'), frame(b'\xff')):
            with self.subTest(data=data), self.assertRaises(cdp.PeerError):
                cdp.Peer(Connection(data)).receive(time.monotonic() + 2)

    def test_overlarge_fragmented_message_is_rejected_before_body_read(self):
        connection = Connection(frame(b'a' * 65535, final=False)
                                + b'\x80\x7f' + struct.pack('!Q', cdp.Peer.LIMIT))
        with self.assertRaisesRegex(cdp.PeerError, 'bounded_message_required'):
            cdp.Peer(connection).receive(time.monotonic() + 2)

    def test_control_spam_cannot_extend_frame_bound(self):
        peer = cdp.Peer(Connection(frame(b'', opcode=10) * 257))
        with self.assertRaisesRegex(cdp.PeerError, 'bounded_fragment_count_required'):
            peer.receive(time.monotonic() + 2)

    def test_wrong_response_id_and_protocol_error_are_fixed_labels(self):
        for value, label in (({'id': 2, 'result': {}}, 'matching_response_required'),
                             ({'id': True, 'result': {}}, 'matching_response_required'),
                             ({'id': 1, 'error': {'message': 'private data'}}, 'successful_cdp_response_required'),
                             ({'id': 1, 'result': {'exceptionDetails': 'private data'}}, 'successful_dom_evaluation_required')):
            with self.subTest(label=label), self.assertRaisesRegex(cdp.PeerError, '^' + label + '$'):
                cdp.Peer(Connection(frame(json.dumps(value).encode()))).call('Runtime.evaluate', {})

    def test_unsolicited_event_flood_is_bounded(self):
        peer = cdp.Peer(Connection(frame(b'{"method":"event"}') * 65))
        with self.assertRaisesRegex(cdp.PeerError, 'bounded_peer_events_required'):
            peer.call('Runtime.evaluate', {})


if __name__ == '__main__':
    unittest.main()
