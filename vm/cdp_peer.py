"""Small bounded CDP transport for an explicitly owned loopback test browser.

This is acceptance tooling, never a shell dependency. The caller must establish
the profile, process and target identity before connecting. No browser-wide
endpoint, proxy, remote host, binary message or unsolicited command is accepted.
"""

import base64
import hashlib
import json
import os
import re
import socket
import struct
import time
from urllib.parse import urlsplit


class PeerError(Exception):
    """Fixed labels only; peer data must not enter diagnostics."""


def require(value, label):
    if not value:
        raise PeerError(label)


class Peer:
    LIMIT = 262144

    def __init__(self, connection, buffered=b''):
        self.connection = connection
        self.buffered = bytearray(buffered)
        self.next_id = 0

    @classmethod
    def connect(cls, url, port, target):
        require(type(port) is int and 1024 <= port <= 65535, 'owned_port_required')
        require(isinstance(target, str) and re.fullmatch(r'[A-Za-z0-9-]{1,128}', target),
                'owned_target_required')
        value = urlsplit(url)
        require(value.scheme == 'ws' and value.hostname == '127.0.0.1'
                and value.port == port and value.netloc == '127.0.0.1:' + str(port)
                and value.path == '/devtools/page/' + target and not value.query
                and not value.fragment, 'owned_loopback_page_endpoint_required')
        connection = socket.create_connection(('127.0.0.1', port), timeout=3)
        try:
            key = base64.b64encode(os.urandom(16)).decode('ascii')
            connection.sendall(('GET ' + value.path + ' HTTP/1.1\r\nHost: ' + value.netloc
                                + '\r\nUpgrade: websocket\r\nConnection: Upgrade'
                                + '\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: '
                                + key + '\r\n\r\n').encode('ascii'))
            raw = bytearray()
            deadline = time.monotonic() + 3
            while b'\r\n\r\n' not in raw:
                require(len(raw) <= 16384 and time.monotonic() < deadline, 'bounded_upgrade_required')
                connection.settimeout(max(.001, deadline - time.monotonic()))
                block = connection.recv(4096)
                require(block, 'upgrade_closed')
                raw.extend(block)
            header, buffered = bytes(raw).split(b'\r\n\r\n', 1)
            require(len(header) <= 16384, 'bounded_upgrade_required')
            lines = header.decode('ascii').split('\r\n')
            require(lines[0] == 'HTTP/1.1 101 Switching Protocols', 'websocket_upgrade_required')
            headers = {}
            for line in lines[1:]:
                name, separator, contents = line.partition(':')
                require(separator and name.lower() not in headers, 'unique_upgrade_headers_required')
                headers[name.lower()] = contents.strip()
            expected = base64.b64encode(hashlib.sha1(
                (key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode('ascii')).digest()).decode('ascii')
            require(headers.get('sec-websocket-accept') == expected
                    and headers.get('upgrade', '').lower() == 'websocket'
                    and 'upgrade' in headers.get('connection', '').lower().split(', ')
                    and 'sec-websocket-extensions' not in headers,
                    'verified_uncompressed_upgrade_required')
            return cls(connection, buffered)
        except BaseException:
            connection.close()
            raise

    def close(self):
        self.connection.close()

    def exact(self, length, deadline):
        require(length <= self.LIMIT, 'bounded_frame_required')
        while len(self.buffered) < length:
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'peer_deadline')
            self.connection.settimeout(remaining)
            block = self.connection.recv(min(65536, length - len(self.buffered)))
            require(block, 'peer_closed')
            self.buffered.extend(block)
        result = bytes(self.buffered[:length])
        del self.buffered[:length]
        return result

    def send(self, payload, opcode=1):
        require(len(payload) <= self.LIMIT and opcode in (1, 10), 'bounded_client_message_required')
        if opcode == 10:
            require(len(payload) <= 125, 'bounded_control_required')
        mask = os.urandom(4)
        length = len(payload)
        header = bytes((0x80 | opcode, 0x80 | length)) if length < 126 else (
            bytes((0x80 | opcode, 0xfe)) + struct.pack('!H', length) if length <= 65535 else
            bytes((0x80 | opcode, 0xff)) + struct.pack('!Q', length))
        masked = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        self.connection.sendall(header + mask + masked)

    def receive(self, deadline):
        data = bytearray()
        fragmented = False
        for _ in range(256):
            first, second = self.exact(2, deadline)
            final, opcode = bool(first & 0x80), first & 15
            require(not first & 0x70 and not second & 0x80, 'unextended_unmasked_server_frame_required')
            length = second & 127
            if length == 126:
                length = struct.unpack('!H', self.exact(2, deadline))[0]
                require(length >= 126, 'canonical_frame_length_required')
            elif length == 127:
                length = struct.unpack('!Q', self.exact(8, deadline))[0]
                require(length >= 65536, 'canonical_frame_length_required')
            if opcode >= 8:
                require(final and length <= 125 and opcode in (8, 9, 10), 'valid_control_required')
                control = self.exact(length, deadline)
                require(opcode != 8, 'peer_closed')
                if opcode == 9:
                    self.send(control, 10)
                continue
            require((opcode == 1 and not fragmented) or (opcode == 0 and fragmented),
                    'text_fragment_sequence_required')
            require(length <= self.LIMIT - len(data), 'bounded_message_required')
            data.extend(self.exact(length, deadline))
            if final:
                try:
                    return data.decode('utf-8')
                except UnicodeDecodeError:
                    raise PeerError('utf8_message_required') from None
            fragmented = True
        raise PeerError('bounded_fragment_count_required')

    def call(self, method, parameters):
        require(method == 'Runtime.evaluate', 'only_owned_dom_evaluation_supported')
        self.next_id += 1
        payload = json.dumps({'id': self.next_id, 'method': method, 'params': parameters}).encode()
        deadline = time.monotonic() + 4
        self.connection.settimeout(4)
        self.send(payload)
        for _ in range(64):
            try:
                value = json.loads(self.receive(deadline))
            except (ValueError, RecursionError):
                raise PeerError('json_response_required') from None
            require(isinstance(value, dict), 'object_response_required')
            if 'id' not in value:
                continue
            require(type(value['id']) is int and value['id'] == self.next_id,
                    'matching_response_required')
            require('error' not in value and isinstance(value.get('result'), dict), 'successful_cdp_response_required')
            result = value['result']
            require('exceptionDetails' not in result, 'successful_dom_evaluation_required')
            return result
        raise PeerError('bounded_peer_events_required')
