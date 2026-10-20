#!/usr/bin/env python3
"""Owned real-Chromium desktop entry for native launch/open/intent acceptance.

All browser data, files and recording stay in one private run directory. Tokens
are recorded only as presence booleans. Retirement validates the original
systemd application unit and never searches for or kills a general browser PID.
"""
import argparse
import fcntl
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import stat
import struct
import sys
import tempfile
import time
import urllib.parse
import urllib.request

sys.dont_write_bytecode = True
from owned_application_units import (assert_unit_origin, capture_unit_origin,
                                     owned_argv, process_snapshot, retire_original_unit, retire_unit,
                                     unit_snapshot, _retirement_drained as retirement_drained)
from application_intent_policy import http_open_count, owned_layers, socket_peer, unbound_pointer
from application_intent_browser import fresh_documents, pages
import owned_chromium_lease as chromium


OPERATIONS = ('initialize', 'execute', 'state', 'retire', 'cleanup', 'watchdog', 'guard',
              'admission', 'example-command', 'initial-absence', 'start-example',
              'retire-example', 'retire-watchdog', 'browser-snapshot', 'browser-close', 'http-info')


def failure_details(error, operation):
    """Bounded fixed diagnostics: no exception text, argv, locals or source lines."""
    classes = ('AssertionError', 'ValueError', 'OSError', 'FileNotFoundError', 'PermissionError',
               'TimeoutError', 'TimeoutExpired', 'ConnectionError', 'ConnectionRefusedError',
               'ConnectionResetError', 'BrokenPipeError', 'HTTPError', 'URLError',
               'JSONDecodeError', 'UnicodeDecodeError', 'DBusException')
    reasons = {'owned_chromium_lease_refused': 'owned_chromium_lease_refused',
               'Owned application intent fixture identity failed': 'owned_fixture_identity_refused',
               'Owned browser page identity failed': 'owned_browser_page_identity_refused',
               'unit_observation_changed': 'unit_observation_changed'}
    details = {'operation': operation if operation in OPERATIONS else 'unknown',
               'error_class': type(error).__name__ if type(error).__name__ in classes else 'OtherError',
               'fixed_reason': 'unclassified_fixture_failure', 'trace_frames': []}
    if len(error.args) == 1 and type(error.args[0]) is str and len(error.args[0]) <= 128:
        details['fixed_reason'] = reasons.get(error.args[0], details['fixed_reason'])
    sources = {'application-intent-fixture.py', 'owned_chromium_lease.py',
               'owned_application_units.py', 'application_intent_policy.py', 'application_intent_browser.py'}
    trace = error.__traceback__; observed = 0
    while trace is not None and observed < 64 and len(details['trace_frames']) < 12:
        code = trace.tb_frame.f_code; source = Path(code.co_filename).name
        if source == 'fixture.py':
            source = 'application-intent-fixture.py'
        if source in sources:
            function = code.co_name
            details['trace_frames'].append({'source': source,
                'function': function if re.fullmatch('[A-Za-z_][A-Za-z0-9_]{0,95}', function) else 'module',
                'line': trace.tb_lineno if 0 < trace.tb_lineno <= 1000000 else 0})
        trace = trace.tb_next; observed += 1
    details['trace_truncated'] = trace is not None
    return details


def require(condition):
    if not condition:
        raise ValueError('Owned application intent fixture identity failed')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def artifact_identity(value):
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns]


def regular(path, limit=131072):
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'rb') as stream:
        value = os.fstat(stream.fileno())
        require(stat.S_ISREG(value.st_mode) and value.st_uid == os.getuid()
                and value.st_size <= limit)
        data = stream.read(limit + 1)
        def identity(meta):
            return (meta.st_dev, meta.st_ino, meta.st_mode, meta.st_uid, meta.st_gid,
                    meta.st_size, meta.st_mtime_ns, meta.st_ctime_ns)
        require(len(data) <= limit and identity(value) == identity(os.fstat(stream.fileno()))
                == identity(path.lstat()))
        return data


def private(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
        value = os.fstat(stream.fileno())
        require(value.st_nlink == 1 and value.st_size == len(data))
        created = artifact_identity(value)
        require(artifact_identity(path.lstat()) == created)
    parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)
    return created


def atomic(path, data):
    descriptor, name = tempfile.mkstemp(prefix='.intent-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
        parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    finally:
        Path(name).unlink(missing_ok=True)


def context(directory, run_id, initialized=True):
    directory = Path(directory)
    require(directory.is_absolute() and directory.resolve() == directory)
    value = directory.lstat()
    require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and stat.S_IMODE(value.st_mode) == 0o700
            and re.fullmatch(r'[A-Za-z0-9]{6,32}', run_id))
    manifest = None
    if initialized:
        manifest = json.loads(regular(directory / 'manifest.json'))
        require(manifest['run_id'] == run_id
                and manifest['script_sha256'] == digest(regular(directory / 'fixture.py'))
                and manifest['units_sha256'] == digest(regular(directory / 'owned_application_units.py'))
                and manifest['policy_sha256'] == digest(regular(directory / 'application_intent_policy.py'))
                and manifest['browser_policy_sha256'] == digest(regular(directory / 'application_intent_browser.py')))
        require(manifest['chromium_lease_sha256'] == digest(regular(directory / 'owned_chromium_lease.py')))
    return directory, manifest


def encoded(value):
    # Exec quoting and desktop-entry escaping are two independent layers.
    value = (value.replace('\\', '\\\\').replace('"', '\\"')
             .replace('$', '\\$').replace('`', '\\`').replace('%', '%%'))
    return '"' + value.replace('\\', '\\\\') + '"'


def original_receipt(directory, name, value=None):
    require(name in ('example-origin.json', 'example-admission.json', 'watchdog-origin.json',
                     'browser-admission.json', 'chromium-lease.json', 'entry-origin.json', 'http-origin.json'))
    path = directory / name
    if value is not None and not os.path.lexists(path):
        private(path, json.dumps(value, sort_keys=True).encode())
    original = json.loads(regular(path))
    require(value is None or original == value)
    return original


def initialize(directory, run_id, example_sha256, http_mode=False, field_code='%U'):
    http_open_count(http_mode, field_code)
    directory, _ = context(directory, run_id, False)
    require(re.fullmatch('[a-f0-9]{64}', example_sha256) is not None)
    require(Path('/usr/bin/chromium').is_file() and os.access('/usr/bin/chromium', os.X_OK))
    data_home = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share')))
    applications = (data_home / 'applications').resolve(strict=True)
    require(data_home.is_absolute() and applications.is_dir())
    app_id = 'org.lucent.ApplicationIntent' + run_id + '.desktop'
    entry_path = applications / app_id
    require(not entry_path.exists() and not entry_path.is_symlink())
    settings = Path.home() / '.local/state/lucent/desktop.json'
    settings_data = regular(settings, 4 * 1024 * 1024)
    files = []
    for index, name in enumerate(('a b%雪$(unused);.html', 'second file.html')):
        title = 'Lucent Intent ' + str(index) + ' ' + run_id
        data = ('<!doctype html><meta charset="utf-8"><title>' + html.escape(title)
                + '</title><p>' + html.escape(title) + '</p>').encode()
        files.append({'name': name, 'title': title, 'sha256': digest(data),
                      'uri': (directory / name).as_uri(), 'data': data})
    command = ' '.join(encoded(value) for value in (
        '/usr/bin/python3', str(directory / 'fixture.py'), 'execute', str(directory), run_id))
    entry = ('[Desktop Entry]\nType=Application\nName=Lucent intent ' + run_id.lower()
             + '\nIcon=web-browser\nTerminal=false\nNoDisplay=false\n'
             + 'StartupWMClass=LucentIntent' + run_id + '\nExec=' + command + ' open ' + field_code + '\n'
             + 'Actions=Probe;\n\n[Desktop Action Probe]\nName=Open owned documents\nExec='
             + command + ' action ' + ' '.join(encoded(item['uri']) for item in files) + '\n').encode()
    manifest = {'run_id': run_id, 'example_sha256': example_sha256,
                'script_sha256': digest(regular(directory / 'fixture.py')),
                'units_sha256': digest(regular(directory / 'owned_application_units.py')),
                'policy_sha256': digest(regular(directory / 'application_intent_policy.py')),
                'browser_policy_sha256': digest(regular(directory / 'application_intent_browser.py')),
                'chromium_lease_sha256': digest(regular(directory / 'owned_chromium_lease.py')),
                'app_id': app_id, 'name': 'Lucent intent ' + run_id.lower(),
                'class': 'LucentIntent' + run_id, 'entry': str(entry_path),
                'entry_sha256': digest(entry), 'settings': str(settings),
                'settings_sha256': digest(settings_data),
                'files': [{key: value for key, value in item.items() if key != 'data'}
                          for item in files]}
    if http_mode:
        manifest['http_mode'] = True
    if field_code != '%U':
        manifest['http_field_code'] = field_code
    manifest['browser_startup'] = chromium.startup(browser_argv(directory, manifest, []))
    private(directory / 'manifest.json', json.dumps(manifest).encode())
    private(directory / 'events.json', b'[]\n')
    private(directory / 'events.lock', b'')
    for item in files:
        private(directory / item['name'], item['data'])
    created = private(entry_path, entry)
    original_receipt(directory, 'entry-origin.json',
                     {'path': str(entry_path), 'identity': created, 'sha256': digest(entry)})
    response = {key: manifest[key] for key in ('app_id', 'name', 'class', 'files')}
    if field_code != '%U':
        response['http_field_code'] = field_code
    print(json.dumps(response))


def http_enabled(manifest):
    enabled = manifest.get('http_mode', False)
    require(type(enabled) is bool)
    http_open_count(enabled, manifest.get('http_field_code', '%U'))
    return enabled


def http_documents(manifest, port):
    """Fixed explicit URI targets; no provider, default-handler or arbitrary URL."""
    run_id = manifest['run_id']
    require(re.fullmatch('[A-Za-z0-9]{6,32}', run_id)
            and type(port) is int and 1024 <= port <= 65535
            and len(manifest['files']) == 2)
    routes = ('first%20%25U%E9%9B%AA?encoded=%2520&field=%25u#first',
              'second%20document?literal=%24%28unused%29%3B#second')
    count = http_open_count(http_enabled(manifest), manifest.get('http_field_code', '%U'))
    return [{'uri': f'http://127.0.0.1:{port}/{run_id}/{route}', 'title': item['title']}
            for route, item in zip(routes[:count], manifest['files'][:count])]


def http_socket_identity(value):
    require(stat.S_ISSOCK(value.st_mode) and value.st_uid == os.getuid())
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid]


def http_tcp_listener(raw, listener):
    """Join the exact open socket inode to its loopback LISTEN row."""
    require(type(raw) is bytes and len(raw) <= 262144)
    lines = raw.decode('ascii').splitlines()
    require(lines and 'local_address' in lines[0] and 'inode' in lines[0])
    rows = [line.split() for line in lines[1:]]
    require(all(10 <= len(row) <= 32 for row in rows))
    matching = [row for row in rows if row[9] == str(listener['identity'][1])]
    require(len(matching) == 1)
    row = matching[0]
    require(row[1] == '0100007F:' + format(listener['port'], '04X')
            and row[2] == '00000000:0000' and row[3] == '0A'
            and row[7] == str(os.getuid()))


def http_listener_snapshot(origin, listener):
    require(set(listener) == {'fd', 'port', 'identity'}
            and type(listener['fd']) is int and 3 <= listener['fd'] <= 65535
            and type(listener['port']) is int and 1024 <= listener['port'] <= 65535
            and type(listener['identity']) is list and len(listener['identity']) == 5
            and all(type(value) is int and value >= 0 for value in listener['identity']))
    pid = origin['process']['pid']
    require(type(pid) is int and pid > 1)
    path = Path('/proc') / str(pid) / 'fd' / str(listener['fd'])
    before = http_socket_identity(path.stat())
    require(before == listener['identity'] and os.readlink(path) == f'socket:[{before[1]}]')
    with (Path('/proc') / str(pid) / 'net/tcp').open('rb') as stream:
        raw = stream.read(262145)
    http_tcp_listener(raw, listener)
    require(http_socket_identity(path.stat()) == before)


def http_authority(directory, run_id, expected=None):
    directory, manifest = context(directory, run_id)
    require(http_enabled(manifest))
    captured = original_receipt(directory, 'http-origin.json')
    require(set(captured) == {'origin', 'listener', 'targets'}
            and (expected is None or expected == captured))
    origin = original_receipt(directory, 'watchdog-origin.json')
    require(captured['origin'] == origin)
    unit = 'lucent-application-intent-watchdog-' + run_id + '.service'
    argv = owned_argv('lucent-application-intent-watchdog-', str(directory / 'fixture.py'), run_id)
    def live_origin():
        current = assert_unit_origin(unit_snapshot(unit), unit, argv, origin)
        require(current is not None and current['executable'] == origin['process']['executable'])
    live_origin()
    http_listener_snapshot(origin, captured['listener'])
    require(captured['targets'] == http_documents(manifest, captured['listener']['port']))
    live_origin()
    return captured


def request_documents(directory, run_id, manifest, operation):
    require(operation in ('open', 'action'))
    if http_enabled(manifest) and operation == 'open':
        return http_authority(directory, run_id)['targets']
    documents = manifest['files']
    require(type(documents) is list and len(documents) == 2)
    return documents


def http_response(request, routes, port):
    """A closed, fixed-route fixture response, never a general file server."""
    status, body, head = '400 Bad Request', b'', False
    if type(request) is bytes and len(request) <= 8192 and request.endswith(b'\r\n\r\n'):
        try:
            lines = request[:-4].decode('ascii').split('\r\n')
            method, target, version = lines[0].split(' ')
            headers = [line.split(':', 1) for line in lines[1:]]
            valid = (len(lines) <= 33 and len(lines[0]) <= 4096 and version == 'HTTP/1.1'
                     and all(len(row) == 2 and re.fullmatch('[!#$%&\'*+.^_`|~A-Za-z0-9-]+', row[0])
                             and not any(ord(c) < 32 or ord(c) == 127 for c in row[1].strip(' \t'))
                             for row in headers))
            values = [(row[0].lower(), row[1].strip(' \t')) for row in headers if len(row) == 2]
            valid = (valid and [value for key, value in values if key == 'host'] == [f'127.0.0.1:{port}']
                     and not any(key == 'transfer-encoding' or key == 'content-length' and value != '0'
                                 for key, value in values))
            if valid:
                if method not in ('GET', 'HEAD'):
                    status = '405 Method Not Allowed'
                elif target not in routes:
                    status = '404 Not Found'
                else:
                    status, body = '200 OK', routes[target]
                    head = method == 'HEAD'
        except (ValueError, UnicodeDecodeError):
            pass
    headers = (f'HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n'
               f'Content-Length: {len(body)}\r\nConnection: close\r\nCache-Control: no-store\r\n'
               "Content-Security-Policy: default-src 'none'; base-uri 'none'; form-action 'none'\r\n"
               'X-Content-Type-Options: nosniff\r\n\r\n').encode('ascii')
    return headers + (b'' if head else body)


def http_connection(connection, routes, port):
    deadline, request = time.monotonic() + 1, bytearray()
    try:
        while not request.endswith(b'\r\n\r\n'):
            remaining = deadline - time.monotonic()
            if remaining <= 0 or len(request) > 8192:
                return
            connection.settimeout(min(remaining, .25))
            try:
                chunk = connection.recv(min(4096, 8193 - len(request)))
            except socket.timeout:
                continue
            if not chunk:
                return
            request.extend(chunk)
        connection.settimeout(.5)
        connection.sendall(http_response(bytes(request), routes, port))
    except (OSError, ValueError):
        return


def http_wait(directory, run_id, manifest, origin, seconds=210):
    """Serve only during the watchdog's existing wait, with no extra worker."""
    deadline = time.monotonic() + seconds
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.bind(('127.0.0.1', 0))
        server.listen(4)
        host, port = server.getsockname()
        require(host == '127.0.0.1')
        documents = http_documents(manifest, port)
        routes = {}
        for document, item in zip(documents, manifest['files']):
            require(Path(item['name']).name == item['name'])
            body = regular(directory / item['name'], 8192)
            require(digest(body) == item['sha256'])
            uri = urllib.parse.urlsplit(document['uri'])
            routes[uri.path + '?' + uri.query] = body
        listener = {'fd': server.fileno(), 'port': port,
                    'identity': http_socket_identity(os.fstat(server.fileno()))}
        http_listener_snapshot(origin, listener)
        original_receipt(directory, 'http-origin.json',
                         {'origin': origin, 'listener': listener, 'targets': documents})
        while time.monotonic() < deadline:
            server.settimeout(min(.25, max(.001, deadline - time.monotonic())))
            try:
                connection, address = server.accept()
            except socket.timeout:
                continue
            with connection:
                if address[0] == '127.0.0.1':
                    http_connection(connection, routes, port)


def events(directory):
    value = json.loads(regular(directory / 'events.json'))
    require(isinstance(value, list) and len(value) <= 32)
    return value


def start_example(directory, run_id):
    directory, manifest = context(directory, run_id)
    binary = directory / 'application-intent'
    require(digest(regular(binary, 64 * 1024 * 1024)) == manifest['example_sha256'])
    unit = 'lucent-application-intent-' + run_id + '.service'
    argv = owned_argv('lucent-application-intent-', str(binary), run_id)
    descriptor = os.open(directory / 'events.lock', os.O_RDWR | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'r+b') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        require(not os.path.lexists(directory / 'closing'))
        original_receipt(directory, 'example-origin.json', capture_unit_origin(unit, argv))
    arguments = [str(binary), '--app', manifest['app_id'], '--action', 'Probe']
    require(len(manifest['files']) == 2)
    documents = request_documents(directory, run_id, manifest, 'open')
    for item in documents:
        if http_enabled(manifest):
            arguments += ['--target', item['uri']]
            continue
        require(Path(item['name']).name == item['name'])
        arguments += ['--target', str(directory / item['name'])]
    signal.alarm(0)
    os.execv(arguments[0], arguments)


def execute(directory, run_id, arguments):
    directory, manifest = context(directory, run_id)
    require(arguments and arguments[0] in ('open', 'action'))
    operation, *uris = arguments
    allowed = [item['uri'] for item in request_documents(directory, run_id, manifest, operation)]
    if http_enabled(manifest):
        require(uris == allowed or operation == 'open' and uris == [])
    else:
        require(len(uris) <= 2 and all(uri in allowed for uri in uris)
                and (operation != 'action' or uris == allowed))
    require(digest(regular(Path(manifest['entry']))) == manifest['entry_sha256'])
    for item in manifest['files']:
        require(digest(regular(directory / item['name'])) == item['sha256'])
    group = Path('/proc/self/cgroup').read_text()
    groups = [line.split(':', 2)[2] for line in group.splitlines() if line.startswith('0::')]
    require(len(groups) == 1 and '/app.slice/' in groups[0] and groups[0].endswith('.service'))
    record = {'operation': operation, 'uris': uris,
              'fresh_token_present': bool(os.environ.get('XDG_ACTIVATION_TOKEN')),
              'legacy_startup_present': bool(os.environ.get('DESKTOP_STARTUP_ID')),
              'cgroup': groups[0]}
    unit = groups[0].rsplit('/', 1)[1]
    require(re.fullmatch(r'run-[A-Za-z0-9_-]+\.service', unit))
    record['origin'] = capture_unit_origin(unit, event_argv(directory, run_id, record))
    descriptor = os.open(directory / 'events.lock', os.O_RDWR | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'r+b') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        require(not os.path.lexists(directory / 'closing'))
        recorded = events(directory)
        require(len(recorded) < 32)
        recorded.append(record)
        atomic(directory / 'events.json', json.dumps(recorded).encode())
    # A real browser consumes the files through its normal Wayland client and
    # file-URI path. Separate profile/class ensure no existing browser is reused.
    arguments = browser_argv(directory, manifest, uris)
    signal.alarm(0)
    os.execv(arguments[0], arguments)


def browser_argv(directory, manifest, uris):
    return ['/usr/bin/chromium', '--user-data-dir=' + str(directory / 'profile'),
                 '--class=' + manifest['class'], '--ozone-platform=wayland',
                 '--no-first-run', '--no-default-browser-check', '--disable-sync',
                 '--disable-extensions', '--disable-background-networking',
                 '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
                 '--new-window', *(uris or ['about:blank'])]


def event_argv(directory, run_id, event):
    require(event['operation'] in ('open', 'action') and len(event['uris']) <= 2)
    return ['/usr/bin/python3', str(directory / 'fixture.py'), 'execute',
            str(directory), run_id, event['operation'], *event['uris']]


def owned_clients(directory, run_id, clients):
    directory, manifest = context(directory, run_id)
    require(isinstance(clients, list) and len(clients) <= 32)
    recorded = events(directory)
    for client in clients:
        require(client['class'] == manifest['class'])
        pid = client['pid']
        require(isinstance(pid, int) and 1 < pid < 2**31)
        require(Path('/proc', str(pid)).stat().st_uid == os.getuid())
        matching = [event for event in recorded if event['origin']['process']['pid'] == pid]
        require(len(matching) == 1)
        event = matching[0]
        lease = browser_lease(directory, run_id, event)
        require(lease['process']['pid'] == pid)
    print(json.dumps({'owned_clients': len(clients)}))


def admission(directory, run_id, request):
    """Observe real process/unit/layer authority; no input or compositor mutation."""
    directory, manifest = context(directory, run_id)
    require(request['example_sha256'] is None or request['example_sha256'] == manifest['example_sha256'])
    require(set(request) == {'desktop_sha256', 'example_sha256'})
    require(re.fullmatch('[a-f0-9]{64}', request['desktop_sha256']) is not None)
    def command(*args):
        result = subprocess.run(args, capture_output=True, text=True, timeout=5)
        require(result.returncode == 0 and len(result.stdout) <= 4 * 1024 * 1024)
        return result.stdout
    require(json.loads(command('hyprctl', '-j', 'locked')) == {'locked': False})
    require(command('omarchy-shell', 'lock', 'isLocked').strip() == 'false')
    pids = command('pgrep', '-x', 'lucent-desktop').split()
    require(len(pids) == 1 and pids[0].isdigit())
    desktop = process_snapshot(int(pids[0]), hashed=True)
    require(desktop['sha256'] == request['desktop_sha256'])
    service = unit_snapshot('lucent.service')
    require(service['load'] == 'loaded' and service['active'] == 'active'
            and service['group'] == desktop['group'] and service['main_pid'] > 1
            and service['executable'] == '/usr/bin/python3'
            and service['argv'] == ['/usr/bin/python3', str(Path.home() / '.local/lib/lucent/session.py')])
    supervisor = process_snapshot(service['main_pid'])
    require(supervisor['group'] == desktop['group'])
    signature = os.environ.get('HYPRLAND_INSTANCE_SIGNATURE')
    require(signature and '/' not in signature)
    instances = [item for item in json.loads(command('hyprctl', '-j', 'instances'))
                 if item.get('instance') == signature]
    require(len(instances) == 1)
    compositor = process_snapshot(instances[0]['pid'])
    def socket_identity(path):
        value = path.lstat()
        require(stat.S_ISSOCK(value.st_mode) and value.st_uid == os.getuid())
        return [value.st_dev, value.st_ino, value.st_mode, value.st_uid]
    runtime = Path(os.environ['XDG_RUNTIME_DIR'])
    baseline = {'desktop': desktop, 'service': service, 'supervisor': supervisor,
                'compositor': compositor, 'signature': signature,
                'compositor_socket': socket_identity(runtime / 'hypr' / signature / '.socket.sock')}
    example = None
    if request['example_sha256'] is not None:
        require(re.fullmatch('[a-f0-9]{64}', request['example_sha256']) is not None)
        unit = 'lucent-application-intent-' + run_id + '.service'
        value = unit_snapshot(unit)
        argv = owned_argv('lucent-application-intent-', str(directory / 'application-intent'), run_id)
        origin = original_receipt(directory, 'example-origin.json')
        assert_unit_origin(value, unit, argv, origin)
        require(value['active'] == 'active' and value['main_pid'] > 1)
        process = process_snapshot(value['main_pid'], hashed=True)
        require(process['sha256'] == request['example_sha256'] and process['group'] == value['group'])
        example = {'unit': value, 'process': process,
                   'socket': socket_identity(runtime / 'application-intent.sock')}
        require(stat.S_IMODE(example['socket'][2]) == 0o600)
    state = json.loads(example_command(directory, run_id, 'inspect', example)) if example else None
    layers = json.loads(command('hyprctl', '-j', 'layers'))
    layer = owned_layers(layers, desktop['pid'], example['process']['pid'] if example else None)
    unbound_pointer(json.loads(command('hyprctl', '-j', 'binds')))
    # Native observations may take time: prove the same process lifetimes still
    # exist immediately before returning the admission snapshot.
    require(process_snapshot(desktop['pid'], hashed=True) == desktop)
    if example:
        require(process_snapshot(example['process']['pid'], hashed=True) == example['process'])
        original_receipt(directory, 'example-admission.json', example)
    print(json.dumps({'baseline': baseline, 'example': example, 'intent_layer': layer, 'example_state': state}))


def example_command(directory, run_id, command, expected):
    context(directory, run_id)
    require(command in ('inspect', 'open'))
    unit = 'lucent-application-intent-' + run_id + '.service'
    require(unit_snapshot(unit) == expected['unit'])
    require(process_snapshot(expected['process']['pid'], hashed=True) == expected['process'])
    path = Path(os.environ['XDG_RUNTIME_DIR']) / 'application-intent.sock'
    def identity():
        value = path.lstat()
        require(stat.S_ISSOCK(value.st_mode) and value.st_uid == os.getuid()
                and stat.S_IMODE(value.st_mode) == 0o600)
        return [value.st_dev, value.st_ino, value.st_mode, value.st_uid]
    require(identity() == expected['socket'])
    deadline = time.monotonic() + 4
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(4)
        connection.connect(str(path))
        socket_peer(struct.unpack('3i', connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)),
                    expected['process']['pid'], os.getuid())
        require(identity() == expected['socket'])
        connection.sendall((command + '\n').encode())
        response = bytearray()
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0)
            connection.settimeout(remaining)
            data = connection.recv(65536)
            if not data:
                break
            response.extend(data)
            require(len(response) <= 4 * 1024 * 1024)
    require(not response.startswith(b'error:') and identity() == expected['socket'])
    require(process_snapshot(expected['process']['pid'], hashed=True) == expected['process'])
    return response.decode()


def retire_example_socket(original):
    """Only the admitted, drained example's exact refused transport can retire."""
    expected_socket = original['socket']
    require(type(expected_socket) is list and len(expected_socket) == 4
            and all(type(value) is int for value in expected_socket)
            and expected_socket[0] > 0 and expected_socket[1] > 0
            and stat.S_ISSOCK(expected_socket[2]) and stat.S_IMODE(expected_socket[2]) == 0o600
            and expected_socket[3] == os.getuid())
    require(retirement_drained(original))
    runtime = Path(os.environ['XDG_RUNTIME_DIR'])
    require(runtime.is_absolute() and runtime.resolve(strict=True) == runtime
            and runtime.stat().st_uid == os.getuid())
    path = runtime / 'application-intent.sock'
    def observed():
        value = path.lstat()
        require(stat.S_ISSOCK(value.st_mode) and value.st_uid == os.getuid()
                and stat.S_IMODE(value.st_mode) == 0o600)
        stamp = [value.st_dev, value.st_ino, value.st_mode, value.st_uid]
        require(stamp == expected_socket)
        return stamp
    if not os.path.lexists(path):
        return
    expected = observed()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(1)
        try:
            connection.connect(str(path))
        except ConnectionRefusedError:
            pass
        except FileNotFoundError:
            require(not os.path.lexists(path))
            return
        else:
            require(False)  # A live listener never grants stale-socket removal.
    require(observed() == expected and retirement_drained(original))
    path.unlink()


def retire_example(directory, run_id, captured=None):
    directory, _ = context(directory, run_id)
    binary = str(directory / 'application-intent')
    unit = 'lucent-application-intent-' + run_id + '.service'
    argv = owned_argv('lucent-application-intent-', binary, run_id)
    value = unit_snapshot(unit)
    admission = directory / 'example-admission.json'
    if os.path.lexists(admission):
        origin = original_receipt(directory, 'example-origin.json')
        original = original_receipt(directory, 'example-admission.json')
        require(captured is None or original == captured)
        require(original['unit']['invocation'] == origin['unit']['invocation']
                and all(original['process'][key] == origin['process'][key]
                        for key in ('pid', 'start', 'group')))
        path = Path(os.environ['XDG_RUNTIME_DIR']) / 'application-intent.sock'
        if os.path.lexists(path):
            value = path.lstat()
            require([value.st_dev, value.st_ino, value.st_mode, value.st_uid] == original['socket'])
        value = unit_snapshot(unit)
        if value['load'] != 'not-found':
            assert_unit_origin(value, unit, argv, origin)
            retire_unit(unit, argv, origin['unit']['group'], captured=original)
        retire_example_socket(original)
        return {'retired': True, 'socket_admitted': True, 'socket_retired': True}
    else:
        require(captured is None)
        require(not os.path.lexists(Path(os.environ['XDG_RUNTIME_DIR']) / 'application-intent.sock'))
        # Lost StartUnit reply or failed-before-socket startup: the original
        # wrapper's durable invocation/birth is enough to retire only its unit.
        if value['load'] != 'not-found':
            origin = original_receipt(directory, 'example-origin.json')
            retire_original_unit(unit, argv, origin)
            require(retirement_drained(origin))
        elif os.path.lexists(directory / 'example-origin.json'):
            require(retirement_drained(original_receipt(directory, 'example-origin.json')))
        return {'retired': True, 'socket_admitted': False, 'socket_retired': False}


def retire_entry(directory, run_id, manifest):
    """Keep private evidence, but retire only the exact published desktop entry."""
    expected = original_receipt(directory, 'entry-origin.json')
    path = Path(manifest['entry'])
    data_home = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share')))
    require(manifest['app_id'] == 'org.lucent.ApplicationIntent' + run_id + '.desktop'
            and path == (data_home / 'applications').resolve(strict=True) / manifest['app_id']
            and expected['path'] == str(path) and expected['sha256'] == manifest['entry_sha256'])
    if not os.path.lexists(path):
        return
    require(artifact_identity(path.lstat()) == expected['identity']
            and path.lstat().st_nlink == 1
            and digest(regular(path)) == expected['sha256'])
    require(artifact_identity(path.lstat()) == expected['identity'])
    path.unlink()


def retire_watchdog(directory, run_id):
    directory, manifest = context(directory, run_id)
    unit = 'lucent-application-intent-watchdog-' + run_id + '.service'
    if not http_enabled(manifest):
        if unit_snapshot(unit)['load'] != 'not-found':
            original = original_receipt(directory, 'watchdog-origin.json')
            argv = owned_argv('lucent-application-intent-watchdog-', str(directory / 'fixture.py'), run_id)
            retire_original_unit(unit, argv, original)
        return None
    origin_path = directory / 'watchdog-origin.json'
    ready_path = directory / 'http-origin.json'
    origin = original_receipt(directory, 'watchdog-origin.json') if os.path.lexists(origin_path) else None
    if os.path.lexists(ready_path):
        require(origin is not None and original_receipt(directory, 'http-origin.json')['origin'] == origin)
    if unit_snapshot(unit)['load'] != 'not-found':
        require(origin is not None)
        argv = owned_argv('lucent-application-intent-watchdog-', str(directory / 'fixture.py'), run_id)
        retire_original_unit(unit, argv, origin)
    # Capture precedes bind/publication. A drained original cgroup owns no
    # surviving listener even if its ready reply/publication was lost.
    require(origin is None and not os.path.lexists(ready_path) or origin is not None and retirement_drained(origin))
    return {'http_server_retired': True}


def retire(directory, run_id):
    directory, manifest = context(directory, run_id)
    # This lock also guards each wrapper's final admission before recording.
    # Already-recorded children will be swept; queued wrappers cannot start a
    # browser after this point, even when systemd accepted their job earlier.
    descriptor = os.open(directory / 'events.lock', os.O_RDWR | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'r+b') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        marker = directory / 'closing'
        if os.path.lexists(marker):
            require(regular(marker) == b'retiring\n')
        else:
            private(marker, b'retiring\n')
    failed = False
    try:
        retire_example(directory, run_id)
    except Exception:
        failed = True
    recorded = events(directory)
    for group in sorted({event['cgroup'] for event in recorded}):
        unit = group.rsplit('/', 1)[1]
        require(re.fullmatch(r'run-[A-Za-z0-9_-]+\.service', unit))
        matching = [event for event in recorded if event['cgroup'] == group]
        require(len(matching) == 1)
        event = matching[0]
        try:
            if event == recorded[0] and event['operation'] == 'open' and event['uris'] == []:
                chromium.retire(browser_lease(directory, run_id, event, retiring=True))
            else:
                retire_original_unit(unit, event_argv(directory, run_id, event), event['origin'],
                                     max_executable_bytes=512 * 1024 * 1024)
                require(retirement_drained(event['origin']))
        except Exception:
            failed = True
    require(not failed)
    retire_entry(directory, run_id, manifest)
    print(json.dumps({'owned_browser_units_retired': True, 'owned_entry_retired': True}))


def cleanup(directory, run_id):
    directory, manifest = context(directory, run_id)
    retire(directory, run_id)
    require(digest(regular(Path(manifest['settings']), 4 * 1024 * 1024)) == manifest['settings_sha256'])
    entry = Path(manifest['entry'])
    require(not os.path.lexists(entry))
    for item in manifest['files']:
        require(digest(regular(directory / item['name'])) == item['sha256'])
    require(set(path.name for path in directory.iterdir()) <= {
        'fixture.py', 'manifest.json', 'events.json', 'events.lock', 'closing', 'profile', 'application_intent_policy.py',
        'application-intent', 'application-intent.sha256', 'owned_application_units.py',
        'example-origin.json', 'example-admission.json', 'watchdog-origin.json', 'browser-admission.json',
        'application_intent_browser.py', 'owned_chromium_lease.py', 'chromium-lease.json', 'entry-origin.json',
        'http-origin.json',
        *[item['name'] for item in manifest['files']]})
    profile = directory / 'profile'
    if profile.exists():
        value = profile.lstat()
        require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid() and not profile.is_symlink())
    shutil.rmtree(directory)


def browser_lease(directory, run_id, event, retiring=False):
    directory, manifest = context(directory, run_id)
    recorded = events(directory)
    require(recorded and event == recorded[0] and event['operation'] == 'open' and event['uris'] == [])
    path = directory / 'chromium-lease.json'
    if os.path.lexists(path):
        captured = original_receipt(directory, 'chromium-lease.json')
        require(captured['origin'] == event['origin'] and captured['declared'] == manifest['browser_startup'])
        # An already retired original is verified by the retirement path, never
        # replaced by a new process or another application scope.
        if chromium.original_gone(captured['process']):
            require(retiring and chromium.retired(captured))
            return captured
        return chromium.observe(event['origin'], manifest['browser_startup'], captured)
    captured = chromium.observe(event['origin'], manifest['browser_startup'])
    return original_receipt(directory, 'chromium-lease.json', captured)


def browser_authority(directory, run_id, expected=None):
    directory, manifest = context(directory, run_id)
    recorded = events(directory)
    require(recorded and recorded[0]['operation'] == 'open' and recorded[0]['uris'] == [])
    event = recorded[0]
    unit = event['cgroup'].rsplit('/', 1)[1]
    require(re.fullmatch(r'run-[A-Za-z0-9_-]+\.service', unit))
    lease = browser_lease(directory, run_id, event)
    path = directory / 'browser-admission.json'
    if os.path.lexists(path):
        captured = original_receipt(directory, 'browser-admission.json')
        require(expected is None or expected == captured)
        require(captured['lease'] == lease)
        return captured
    require(expected is None)
    endpoint = browser_endpoint(directory)
    current_pages = browser_pages(directory)
    require(browser_endpoint(directory) == endpoint)
    require(len(current_pages) == 1 and current_pages[0]['url'] == 'about:blank')
    require(browser_lease(directory, run_id, event) == lease)
    captured = {'unit': lease['unit'], 'process': lease['process'], 'lease': lease,
                'control': current_pages[0], 'endpoint': endpoint}
    original_receipt(directory, 'browser-admission.json', captured)
    return captured


def browser_endpoint(directory):
    path = directory / 'profile/DevToolsActivePort'
    def identity(value):
        return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
                value.st_size, value.st_mtime_ns, value.st_ctime_ns]
    before = identity(path.lstat())
    data = regular(path, 4096)
    require(identity(path.lstat()) == before)
    lines = data.decode('ascii').splitlines()
    require(len(lines) == 2 and lines[0].isdecimal()
            and re.fullmatch(r'/devtools/browser/[A-Za-z0-9-]{1,128}', lines[1]))
    port = int(lines[0])
    require(1024 <= port <= 65535)
    return {'port': port, 'browser': lines[1], 'file': before, 'sha256': digest(data)}


def browser_http(directory, suffix):
    endpoint = browser_endpoint(directory)
    if os.path.lexists(directory / 'browser-admission.json'):
        require(original_receipt(directory, 'browser-admission.json')['endpoint'] == endpoint)
    port = endpoint['port']
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *_args, **_kwargs):
            return None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    url = 'http://127.0.0.1:' + str(port) + suffix
    with opener.open(url, timeout=3) as response:
        require(response.status == 200 and response.geturl() == url)
        raw = response.read(262145)
        require(len(raw) <= 262144 and browser_endpoint(directory) == endpoint)
        return raw


def browser_pages(directory):
    return pages(json.loads(browser_http(directory, '/json/list')))


def browser_snapshot(directory, run_id, request):
    directory, _ = context(directory, run_id)
    captured = browser_authority(directory, run_id, request.get('identity'))
    current = browser_pages(directory)
    browser_authority(directory, run_id, captured)
    print(json.dumps({'identity': captured, 'pages': current}))


def browser_close(directory, run_id, request):
    directory, manifest = context(directory, run_id)
    captured = browser_authority(directory, run_id, request['identity'])
    require(request['before'] == [captured['control']])
    current = browser_pages(directory)
    documents = request_documents(directory, run_id, manifest, request.get('operation', 'open'))
    fresh = fresh_documents(request['before'], current, documents, captured['control'])
    require(fresh is not None and fresh == request['targets'])
    remaining = list(fresh)
    for target in fresh:
        browser_authority(directory, run_id, captured)
        require(browser_pages(directory) == sorted([captured['control'], *remaining], key=lambda item: item['id']))
        # The exact fresh target ID was observed with its expected literal URL
        # and title. Never close the original blank target or a profile-wide set.
        require(re.fullmatch(r'[A-Za-z0-9-]{1,128}', target['id']))
        browser_http(directory, '/json/close/' + target['id'])
        remaining.remove(target)
        deadline = time.monotonic() + 3
        while browser_pages(directory) != sorted([captured['control'], *remaining], key=lambda item: item['id']):
            require(time.monotonic() < deadline)
            browser_authority(directory, run_id, captured)
            time.sleep(.05)
    browser_authority(directory, run_id, captured)
    require(browser_pages(directory) == [captured['control']])
    print(json.dumps({'fresh_owned_targets_retired': True}))


def initial_absence(directory, run_id):
    context(directory, run_id)
    for prefix in ('lucent-application-intent-', 'lucent-application-intent-watchdog-'):
        require(unit_snapshot(prefix + run_id + '.service')['load'] == 'not-found')
    print(json.dumps({'absent': True}))


def watchdog(directory, run_id):
    directory, manifest = context(directory, run_id)
    unit = 'lucent-application-intent-watchdog-' + run_id + '.service'
    argv = owned_argv('lucent-application-intent-watchdog-', str(directory / 'fixture.py'), run_id)
    origin = original_receipt(directory, 'watchdog-origin.json', capture_unit_origin(unit, argv))
    signal.alarm(295)
    try:
        if http_enabled(manifest):
            http_wait(directory, run_id, manifest, origin)
        else:
            time.sleep(210)
    finally:
        # Independent expiry has the same immutable receipts as host cleanup.
        # A server failure still runs retirement; no successor is adopted.
        for attempt in range(6):
            try:
                retire(directory, run_id)
                break
            except Exception:
                if attempt == 5:
                    raise
                time.sleep(5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=OPERATIONS)
    parser.add_argument('directory')
    parser.add_argument('run_id')
    parser.add_argument('arguments', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    signal.alarm(55)
    if args.operation == 'initialize' and len(args.arguments) in (1, 2, 3):
        require(len(args.arguments) == 1 or args.arguments[1] == 'http')
        initialize(args.directory, args.run_id, args.arguments[0], len(args.arguments) >= 2,
                   args.arguments[2] if len(args.arguments) == 3 else '%U')
    elif args.operation == 'initial-absence' and not args.arguments:
        initial_absence(args.directory, args.run_id)
    elif args.operation == 'start-example' and not args.arguments:
        start_example(args.directory, args.run_id)
    elif args.operation == 'retire-example' and len(args.arguments) in (0, 1):
        print(json.dumps(retire_example(args.directory, args.run_id,
                         json.loads(args.arguments[0]) if args.arguments else None)))
    elif args.operation == 'retire-watchdog' and not args.arguments:
        result = retire_watchdog(args.directory, args.run_id)
        print(json.dumps({'retired': True, **(result or {})}))
    elif args.operation == 'http-info' and not args.arguments:
        directory, manifest = context(args.directory, args.run_id)
        require(http_enabled(manifest))
        if not os.path.lexists(directory / 'http-origin.json'):
            print(json.dumps({'ready': False}))
        else:
            print(json.dumps({'ready': True, 'identity': http_authority(directory, args.run_id)}))
    elif args.operation == 'execute':
        execute(args.directory, args.run_id, args.arguments)
    elif args.operation == 'state' and not args.arguments:
        directory, _ = context(args.directory, args.run_id)
        print(json.dumps({'events': events(directory)}))
    elif args.operation == 'guard' and len(args.arguments) == 1:
        owned_clients(args.directory, args.run_id, json.loads(args.arguments[0]))
    elif args.operation == 'admission' and len(args.arguments) == 1:
        admission(args.directory, args.run_id, json.loads(args.arguments[0]))
    elif args.operation == 'example-command' and len(args.arguments) == 2:
        print(example_command(args.directory, args.run_id, args.arguments[0], json.loads(args.arguments[1])))
    elif args.operation in ('browser-snapshot', 'browser-close') and len(args.arguments) == 1:
        globals()[args.operation.replace('-', '_')](args.directory, args.run_id, json.loads(args.arguments[0]))
    elif args.operation == 'retire' and not args.arguments:
        retire(args.directory, args.run_id)
    elif args.operation == 'watchdog' and not args.arguments:
        watchdog(args.directory, args.run_id)
    elif args.operation == 'cleanup' and not args.arguments:
        cleanup(args.directory, args.run_id)
    else:
        raise ValueError('Unexpected owned fixture arguments')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(json.dumps(failure_details(error, sys.argv[1] if len(sys.argv) > 1 else None),
                         sort_keys=True), file=sys.stderr)
        raise SystemExit(1) from None
