#!/usr/bin/env python3
"""Private real-Chromium peer for native clipboard interoperability acceptance.

CDP prepares/selects the fixture textarea and reads its DOM. It never accesses a
clipboard API or synthesizes a copy/paste event; those use real VM keyboard input.
An independently owned watchdog retains the browser lease across normal scope
migration; original-service expiry alone is not proof of scope drainage.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time
import urllib.request

sys.dont_write_bytecode = True
from cdp_peer import Peer
from owned_application_units import (assert_unit_origin, capture_unit_origin,
                                     retire_original_unit, unit_snapshot)
import owned_chromium_lease as chromium

SOURCE_TEXT = 'browser one\n雪 e\u0301 👩\u200d💻\nlast line\n'
DESTINATION_TEXT = 'notes one\n\nnotes two\n'
FIXTURE_MIME_PRIORITY = ('text/plain;charset=utf-8', 'UTF8_STRING', 'text/plain')
TEXT_ALIASES = ('text/plain;charset=utf-8', 'text/plain;charset=UTF-8',
                'text/plain', 'UTF8_STRING', 'STRING', 'TEXT')
EVENT_KEYS = ('trusted_copy', 'untrusted_copy', 'trusted_paste', 'untrusted_paste',
              'ctrl_c_down', 'ctrl_c_up', 'ctrl_v_down', 'ctrl_v_up')
OPERATIONS = ('initialize', 'absent', 'serve', 'origin', 'admit', 'clients', 'dom', 'clipboard-observe', 'fixture-clipboard', 'retire', 'cleanup', 'watchdog')


def failure_details(error, operation):
    """Fixed metadata only; never expose text, argv, headers or exception payload."""
    classes = ('AssertionError', 'ValueError', 'OSError', 'FileNotFoundError', 'PermissionError',
               'TimeoutError', 'TimeoutExpired', 'ConnectionRefusedError', 'ConnectionResetError',
               'HTTPError', 'URLError', 'JSONDecodeError', 'UnicodeDecodeError', 'PeerError')
    reasons = {'owned_chromium_lease_refused', 'unit_observation_changed', 'unit_observation_deadline',
               'unit_exec_start_shape', 'websocket_upgrade_required', 'verified_uncompressed_upgrade_required',
               'unique_upgrade_headers_required', 'bounded_upgrade_required', 'upgrade_closed',
               'owned_loopback_page_endpoint_required', 'owned_port_required', 'owned_target_required',
               'peer_closed', 'peer_deadline', 'successful_cdp_response_required',
               'successful_dom_evaluation_required', 'matching_response_required'}
    result = {'operation': operation if operation in OPERATIONS else 'unknown',
              'error_class': type(error).__name__ if type(error).__name__ in classes else 'OtherError',
              'fixed_reason': 'unclassified_peer_failure', 'trace_frames': []}
    if len(error.args) == 1 and type(error.args[0]) is str and len(error.args[0]) <= 128:
        reason = error.args[0]
        if reason in reasons: result['fixed_reason'] = reason
        elif reason == 'Owned GUI clipboard peer identity failed': result['fixed_reason'] = 'owned_peer_identity_refused'
    status = getattr(error, 'upgrade_status', None)
    if type(status) is dict and set(status) == {'http_version', 'status_code'} \
            and type(status['http_version']) is str and re.fullmatch(r'[0-9]{1,2}(?:\.[0-9]{1,2})?', status['http_version']) \
            and type(status['status_code']) is int and 100 <= status['status_code'] <= 599:
        result['upgrade_status'] = dict(status)
    trace = error.__traceback__; observed = 0
    while trace is not None and observed < 64 and len(result['trace_frames']) < 12:
        code = trace.tb_frame.f_code; source = Path(code.co_filename).name
        if source == 'fixture.py': source = 'gui-clipboard-peer.py'
        if source in ('gui-clipboard-peer.py', 'owned_chromium_lease.py', 'owned_application_units.py', 'cdp_peer.py'):
            result['trace_frames'].append({'source': source,
                'function': code.co_name if re.fullmatch('[A-Za-z_][A-Za-z0-9_]{0,95}', code.co_name) else 'module',
                'line': trace.tb_lineno if 0 < trace.tb_lineno <= 1000000 else 0})
        trace = trace.tb_next; observed += 1
    result['trace_truncated'] = trace is not None
    return result


def require(value):
    if not value:
        raise ValueError('Owned GUI clipboard peer identity failed')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular(path, limit=262144):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and 0 <= before.st_size <= limit)
        identity = stable_file_identity(before)
        require(stable_file_identity(path.lstat()) == identity)
        data = stream.read(limit + 1)
        require(len(data) <= limit and stable_file_identity(os.fstat(stream.fileno())) == identity
                and stable_file_identity(path.lstat()) == identity)
        return data


def stable_file_identity(value):
    # Cold reads can change only atime; all identity/content metadata stays pinned.
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns]


def private(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)


def context(directory, run_id, initialized=True):
    directory = Path(directory)
    require(directory.is_absolute() and directory.resolve(strict=True) == directory
            and re.fullmatch(r'[0-9a-f]{24}', run_id))
    value = directory.lstat()
    require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and stat.S_IMODE(value.st_mode) == 0o700)
    if not initialized:
        return directory, None
    manifest = json.loads(regular(directory / 'manifest.json'))
    require(manifest['run_id'] == run_id)
    for name, expected in manifest['helpers'].items():
        require(name in ('fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'owned_chromium_lease.py')
                and digest(regular(directory / name)) == expected)
    require(digest(regular(directory / 'document.html')) == manifest['document_sha256'])
    return directory, manifest


def argv(directory, run_id):
    return ['/usr/bin/python3', str(directory / 'fixture.py'), 'serve', str(directory), run_id]


def unit_name(run_id):
    return 'lucent-gui-clipboard-' + run_id + '.service'


def watchdog_name(run_id):
    return 'lucent-gui-clipboard-watchdog-' + run_id + '.service'


def watchdog_argv(directory, run_id):
    return ['/usr/bin/python3', str(directory / 'fixture.py'), 'watchdog', str(directory), run_id]


def chromium_argv(directory, manifest):
    return ['/usr/bin/chromium', '--user-data-dir=' + str(directory / 'profile'),
            '--class=' + manifest['class'], '--ozone-platform=wayland',
            '--no-first-run', '--no-default-browser-check', '--disable-sync',
            '--disable-extensions', '--disable-background-networking', '--password-store=basic',
            '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
            '--new-window', manifest['uri']]


def initialize(directory, run_id):
    directory, _ = context(directory, run_id, False)
    require(unit_snapshot(unit_name(run_id))['load'] == 'not-found')
    require(unit_snapshot(watchdog_name(run_id))['load'] == 'not-found')
    require(Path('/usr/bin/chromium').is_file() and os.access('/usr/bin/chromium', os.X_OK))
    title = 'Lucent Clipboard ' + run_id
    document = ('<!doctype html><meta charset="utf-8"><title>' + title + '</title>'
                '<label for="clipboard">Owned clipboard acceptance</label>'
                '<textarea id="clipboard" autofocus style="display:block;width:80vw;'
                'height:65vh;font:24px monospace"></textarea>'
                '<script>const clipboardEvents=Object.seal(' + json.dumps(dict.fromkeys(EVENT_KEYS, 0)) + ');'
                'for(const type of ["copy","paste"])document.addEventListener(type,event=>{'
                'const key=(event.isTrusted?"trusted_":"untrusted_")+type;'
                'clipboardEvents[key]=Math.min(4096,clipboardEvents[key]+1);});'
                'for(const type of ["keydown","keyup"])document.addEventListener(type,event=>{'
                'if(!event.isTrusted||!event.ctrlKey||event.altKey||event.metaKey||event.shiftKey)return;'
                'const key=event.code==="KeyC"?"ctrl_c_":event.code==="KeyV"?"ctrl_v_":null;'
                'if(key!==null){const counter=key+(type==="keydown"?"down":"up");'
                'clipboardEvents[counter]=Math.min(4096,clipboardEvents[counter]+1);}});</script>').encode()
    manifest = {'run_id': run_id, 'title': title, 'class': 'LucentClipboard' + run_id,
                'uri': (directory / 'document.html').as_uri(),
                'helpers': {name: digest(regular(directory / name))
                            for name in ('fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'owned_chromium_lease.py')},
                'document_sha256': digest(document)}
    manifest['startup'] = chromium.startup(chromium_argv(directory, manifest))
    private(directory / 'document.html', document)
    private(directory / 'manifest.json', json.dumps(manifest).encode())
    print(json.dumps({'class': manifest['class'], 'source_sha256': digest(SOURCE_TEXT.encode())}))


def absent(directory, run_id):
    context(directory, run_id)
    require(unit_snapshot(unit_name(run_id))['load'] == 'not-found')
    print(json.dumps({'owned_unit_name_absent': True}))


def owned_unit(directory, run_id, captured):
    require(captured == json.loads(regular(directory / 'admission.json')))
    directory, manifest = context(directory, run_id)
    require(captured['lease'] == browser_lease(directory, run_id))
    require(browser_endpoint(directory) == captured['endpoint'])
    return captured


def serve(directory, run_id):
    directory, manifest = context(directory, run_id)
    origin = capture_unit_origin(unit_name(run_id), argv(directory, run_id))
    require(not os.path.lexists(directory / 'profile'))
    private(directory / 'origin.json', json.dumps(origin).encode())
    require(unit_snapshot(watchdog_name(run_id))['load'] == 'not-found')
    subprocess.run(['systemd-run', '--user', '--quiet', '--collect', '--property=Type=exec',
                    '--property=RuntimeMaxSec=240s', '--property=TimeoutStopSec=10s',
                    '--unit='+watchdog_name(run_id), *watchdog_argv(directory, run_id)], check=True, timeout=10)
    deadline = time.monotonic()+10
    while not os.path.lexists(directory / 'watchdog-origin.json'):
        require(time.monotonic()<deadline)
        time.sleep(.1)
    watched = json.loads(regular(directory / 'watchdog-origin.json'))
    assert_unit_origin(unit_snapshot(watchdog_name(run_id)), watchdog_name(run_id),
                       watchdog_argv(directory, run_id), watched)
    command = chromium_argv(directory, manifest)
    os.execv(command[0], command)


def origin(directory, run_id):
    directory, _ = context(directory, run_id)
    receipt = json.loads(regular(directory / 'origin.json'))
    value, process = receipt['unit'], receipt['process']
    require(value['argv'] == argv(directory, run_id) and value['executable'] == '/usr/bin/python3'
            and value['transient'] and value['active'] in ('active', 'activating')
            and re.fullmatch('[0-9a-f]{32}', value['invocation']) and set(value['invocation']) != {'0'}
            and type(process['pid']) is int and process['pid'] > 1 and process['pid'] == value['main_pid']
            and type(process['start']) is str and process['start'].isdigit()
            and process['group'] == value['group'] and value['group'].endswith('/'+unit_name(run_id)))
    # Returning immutable pre-exec evidence grants no input or signal authority.
    # Those still need the current process lease (and full page admission for input).
    print(json.dumps(receipt))


def browser_lease(directory, run_id, retiring=False):
    directory, manifest = context(directory, run_id)
    receipt = json.loads(regular(directory / 'origin.json'))
    path = directory / 'browser-lease.json'
    if os.path.lexists(path):
        captured = json.loads(regular(path))
        require(captured['origin'] == receipt and captured['declared'] == manifest['startup'])
        if chromium.original_gone(captured['process']):
            require(retiring and chromium.retired(captured))
            return captured
        return chromium.observe(receipt, manifest['startup'], captured)
    captured = chromium.observe(receipt, manifest['startup'])
    # Arch Chromium registers its application scope during normal startup.
    # Wait for that observed migration before freezing this fixture's lease;
    # an already frozen lease is never updated to follow another scope.
    require(captured['scope'] is not None)
    try:
        private(path, json.dumps(captured).encode())
    except FileExistsError:
        require(json.loads(regular(path)) == captured)
    return chromium.observe(receipt, manifest['startup'], captured)


def admit(directory, run_id, emit=True):
    directory, manifest = context(directory, run_id)
    if (directory / 'admission.json').exists():
        captured = json.loads(regular(directory / 'admission.json'))
        owned_unit(directory, run_id, captured)
    else:
        lease = browser_lease(directory, run_id)
        # The port and exact owned page exist only after Chromium has exec'd;
        # never freeze an intermediate Python/shell executable as the browser.
        peer, control = page(directory, manifest, capture=True)
        peer.close()
        captured = {'lease':lease, **control}
        try:
            private(directory / 'admission.json', json.dumps(captured).encode())
        except FileExistsError:
            captured = json.loads(regular(directory / 'admission.json'))
        owned_unit(directory, run_id, captured)
    if emit:
        print(json.dumps(captured))
    return captured


def clients(directory, run_id, request):
    directory, manifest = context(directory, run_id)
    values = request['clients']
    require(isinstance(values, list) and len(values) <= 1)
    lease = owned_unit(directory, run_id, request['identity'])
    for value in values:
        require(value.get('class') == manifest['class'] and type(value.get('pid')) is int
                and value['pid'] == lease['lease']['process']['pid'] and value.get('workspace', {}).get('name') ==
                'lucent-clipboard-browser-' + run_id)
        process = Path('/proc') / str(value['pid'])
        require(process.stat().st_uid == os.getuid())
        groups = [line.split(':', 2)[2] for line in regular(process / 'cgroup').decode().splitlines()
                  if line.startswith('0::')]
        require(groups == [lease['lease']['process']['group']])
    print(json.dumps({'owned_clients': len(values)}))


def browser_endpoint(directory):
    path = directory / 'profile/DevToolsActivePort'
    before = stable_file_identity(path.lstat())
    data = regular(path, 4096)
    require(stable_file_identity(path.lstat()) == before)
    lines = data.decode('ascii').splitlines()
    require(len(lines) == 2 and lines[0].isdecimal())
    port = int(lines[0])
    require(1024 <= port <= 65535 and re.fullmatch(r'/devtools/browser/[A-Za-z0-9-]{1,128}', lines[1]))
    return {'port':port,'browser':lines[1],'file':before,'sha256':digest(data)}


def page(directory, manifest, expected=None, capture=False):
    endpoint = browser_endpoint(directory)
    require(expected is None or endpoint == expected['endpoint'])
    port = endpoint['port']
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *_args, **_kwargs):
            return None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open('http://127.0.0.1:' + str(port) + '/json/list', timeout=3) as response:
        require(response.status == 200 and response.geturl() == 'http://127.0.0.1:' + str(port) + '/json/list')
        data = response.read(262145)
        require(len(data) <= 262144)
    values = json.loads(data)
    require(isinstance(values, list) and len(values) <= 16)
    pages = [value for value in values if value.get('type') == 'page']
    require(len(pages) == 1 and pages[0].get('url') == manifest['uri']
            and pages[0].get('title') == manifest['title'])
    target = pages[0]
    control = {key:target[key] for key in ('id','url','title','webSocketDebuggerUrl')}
    require(expected is None or control == expected['control'])
    require(browser_endpoint(directory) == endpoint)
    peer = Peer.connect(target['webSocketDebuggerUrl'], port, target['id'])
    return (peer, {'endpoint':endpoint,'control':control}) if capture else peer


def dom(directory, run_id, operation, request):
    directory, manifest = context(directory, run_id)
    owned_unit(directory, run_id, request['identity'])
    require(operation in ('seed', 'clear', 'read'))
    # No navigator.clipboard, Input.dispatchKeyEvent or document.execCommand.
    prepare = ('e.value=' + json.dumps(SOURCE_TEXT) + ';e.focus();e.select();') if operation == 'seed' else (
        'e.value="replacement sentinel";e.focus();e.select();' if operation == 'clear' else '')
    expression = ('(()=>{if(location.href!==' + json.dumps(manifest['uri'])
                  + '||document.title!==' + json.dumps(manifest['title']) + ')throw 1;'
                  'const e=document.getElementById("clipboard");'
                  'if(!(e instanceof HTMLTextAreaElement))throw 1;' + prepare
                  + 'return {value:e.value,focused:document.hasFocus()&&document.activeElement===e,'
                  'start:e.selectionStart,end:e.selectionEnd,events:{...clipboardEvents}};})()')
    peer = page(directory, manifest, expected=request['identity'])
    try:
        result = peer.call('Runtime.evaluate', {'expression': expression, 'returnByValue': True})
    finally:
        peer.close()
    value = result.get('result', {}).get('value')
    require(isinstance(value, dict) and isinstance(value.get('value'), str)
            and len(value['value'].encode()) <= 65536)
    events = value.get('events')
    require(type(events) is dict and set(events) == set(EVENT_KEYS)
            and all(type(count) is int and 0 <= count <= 4096 for count in events.values()))
    print(json.dumps({'sha256': digest(value['value'].encode()), 'focused': value['focused'],
                      'selection_complete': value['start'] == 0 and value['end'] == len(value['value'].encode('utf-16-le')) // 2,
                      'bytes': len(value['value'].encode()), 'events': events}))


class ClipboardObservationFailure(ValueError):
    def __init__(self, reason):
        require(reason in ('read_deadline', 'stdout_limit', 'stderr_limit'))
        self.reason = reason
        super().__init__('bounded_clipboard_observation_failed')


def clipboard_failure(error):
    if isinstance(error, ClipboardObservationFailure):
        return error.reason
    if isinstance(error, subprocess.TimeoutExpired):
        return 'read_deadline'
    if isinstance(error, OSError):
        return 'reader_process_unavailable'
    return 'reader_refused'


def bounded_clipboard_read(command, limit, deadline):
    """Read-only child with bounded bytes/time; never put payloads in errors."""
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    selector = selectors.DefaultSelector()
    output = [bytearray(), bytearray()]
    try:
        for index, stream in enumerate((child.stdout, child.stderr)):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, index)
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise ClipboardObservationFailure('read_deadline')
            for key, _mask in selector.select(.05):
                data = os.read(key.fd, 4096)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                output[key.data].extend(data)
                if len(output[key.data]) > (limit if key.data == 0 else 4096):
                    raise ClipboardObservationFailure('stdout_limit' if key.data == 0 else 'stderr_limit')
        return child.wait(timeout=max(.01, deadline - time.monotonic())), bytes(output[0]), bytes(output[1])
    finally:
        selector.close()
        if child.poll() is None:
            child.kill()
            child.wait(timeout=1)
        child.stdout.close()
        child.stderr.close()


def fixture_payload_status(data):
    """Classify only public fixture equality; unknown/original bytes stay private."""
    for name, fixture in (('source', SOURCE_TEXT), ('destination', DESTINATION_TEXT)):
        for suffix, candidate in (('', fixture), ('_crlf', fixture.replace('\n', '\r\n')),
                                  ('_without_final_lf', fixture.removesuffix('\n'))):
            if data == candidate.encode():
                return 'fixture_' + name + suffix
    if not data:
        return 'empty'
    try:
        data.decode('utf-8')
    except UnicodeDecodeError:
        return 'other_non_utf8'
    return 'other'


def clipboard_observation(read=bounded_clipboard_read):
    """Diagnose MIME compatibility without accepting offers or exposing user data."""
    result = {'list_status': 'unavailable', 'list_failure': None, 'list_exit_status': None,
              'entry_count': None, 'duplicate_entry_count': None, 'known_text_aliases': [],
              'other_mime_count': 0, 'payloads': {}, 'payload_failures': {},
              'strict_text_only': None, 'all_read_alias_payloads_identical': None,
              'inherited_reader_compatibility': 'unavailable'}
    deadline = time.monotonic() + 10
    try:
        code, data, error = read(['wl-paste', '--list-types'], 16384, deadline)
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        result['list_failure'] = clipboard_failure(error)
        return result
    if type(code) is int and -255 <= code <= 255:
        result['list_exit_status'] = code
    if code == 1 and b'Nothing is copied' in error:
        result['list_status'] = 'empty'
        result['inherited_reader_compatibility'] = 'supported'
        return result
    if code != 0:
        result['list_failure'] = 'reader_nonzero_exit'
        return result
    try:
        types = data.decode('utf-8').splitlines()
    except UnicodeDecodeError:
        result['list_failure'] = 'types_non_utf8'
        return result
    result['entry_count'] = len(types)
    result['duplicate_entry_count'] = len(types) - len(set(types))
    # Duplicate advertised MIME entries are legal observations. The inherited
    # reader uses a set for its admission and must not be diagnosed differently.
    if len(types) > 64:
        result['list_failure'] = 'too_many_type_entries'
        return result
    if not all(0 < len(value) <= 256 for value in types):
        result['list_failure'] = 'invalid_mime_length'
        return result
    result['list_status'] = 'available'
    result['known_text_aliases'] = [value for value in TEXT_ALIASES if value in types]
    result['other_mime_count'] = sum(value not in TEXT_ALIASES for value in types)
    result['strict_text_only'] = bool(types) and result['other_mime_count'] == 0
    payloads = []
    for mime in result['known_text_aliases']:
        try:
            code, data, _error = read(['wl-paste', '--no-newline', '--type', mime], 65536, deadline)
            if code != 0:
                result['payloads'][mime] = 'unavailable'
                result['payload_failures'][mime] = 'reader_nonzero_exit'
                continue
        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
            result['payloads'][mime] = 'unavailable'
            result['payload_failures'][mime] = clipboard_failure(error)
            continue
        result['payloads'][mime] = fixture_payload_status(data)
        payloads.append(data)
    if payloads:
        result['all_read_alias_payloads_identical'] = all(value == payloads[0] for value in payloads)
    if not result['strict_text_only']:
        result['inherited_reader_compatibility'] = 'nontext_or_sensitive_mime'
    elif any(status == 'other_non_utf8' for status in result['payloads'].values()):
        result['inherited_reader_compatibility'] = 'non_utf8_clipboard'
    elif result['payload_failures']:
        result['inherited_reader_compatibility'] = 'read_unavailable'
    elif not result['all_read_alias_payloads_identical']:
        result['inherited_reader_compatibility'] = 'different_mime_payloads'
    else:
        result['inherited_reader_compatibility'] = 'supported'
    return result


def fixture_clipboard_offer(read=bounded_clipboard_read):
    """Read one native-supported UTF-8 flavor, accepting only compiled fixtures."""
    deadline = time.monotonic() + 10
    code, data, _error = read(['wl-paste', '--list-types'], 16384, deadline)
    require(code == 0 and len(data) <= 16384)
    offered = data.decode('utf-8').splitlines()
    require(0 < len(offered) <= 64 and all(0 < len(mime) <= 256 for mime in offered))
    # Match the native backend's preference; other/duplicate MIME entries and
    # legacy aliases confer no authority and are never requested by this test.
    mime = next((value for value in FIXTURE_MIME_PRIORITY if value in offered), None)
    require(mime is not None)
    code, data, _error = read(['wl-paste', '--no-newline', '--type', mime], 65536, deadline)
    require(code == 0 and len(data) <= 65536)
    fixture = next((name for name, text in (('source', SOURCE_TEXT), ('destination', DESTINATION_TEXT))
                    if data == text.encode('utf-8')), None)
    require(fixture is not None)
    return {'fixture': fixture, 'sha256': digest(data), 'empty': False, 'mime': mime}


def fixture_clipboard(directory, run_id, request):
    directory, _manifest = context(directory, run_id)
    owned_unit(directory, run_id, request['identity'])
    observation = fixture_clipboard_offer()
    owned_unit(directory, run_id, request['identity'])
    print(json.dumps(observation))


def clipboard_observe(directory, run_id, request):
    directory, _manifest = context(directory, run_id)
    owned_unit(directory, run_id, request['identity'])
    observation = clipboard_observation()
    owned_unit(directory, run_id, request['identity'])
    print(json.dumps(observation))


def retire_owned(directory, run_id, request):
    directory, _ = context(directory, run_id)
    if request.get('identity') is not None:
        captured = request['identity']
        require(captured == json.loads(regular(directory / 'admission.json')))
        lease = browser_lease(directory, run_id, retiring=True)
        require(lease == captured['lease'])
    else:
        receipt = request['origin']
        require(receipt == json.loads(regular(directory / 'origin.json')))
        lease = browser_lease(directory, run_id, retiring=True)
        require(receipt == lease['origin'])
    chromium.retire(lease)
    watched = json.loads(regular(directory / 'watchdog-origin.json'))
    retire_original_unit(watchdog_name(run_id), watchdog_argv(directory, run_id), watched)


def watchdog(directory, run_id):
    directory, _ = context(directory, run_id)
    signal.alarm(235)
    receipt = capture_unit_origin(watchdog_name(run_id), watchdog_argv(directory, run_id))
    private(directory / 'watchdog-origin.json', json.dumps(receipt).encode())
    started = time.monotonic()
    while True:
        try:
            lease = browser_lease(directory, run_id)
            break
        except Exception:
            require(time.monotonic()-started < 35)
            time.sleep(.15)
    time.sleep(max(0, 180-(time.monotonic()-started)))
    chromium.retire(lease)


def retire(directory, run_id, request):
    retire_owned(directory, run_id, request)
    print(json.dumps({'owned_browser_unit_retired': True}))


def cleanup(directory, run_id, request):
    directory, _ = context(directory, run_id)
    retire_owned(directory, run_id, request)
    require(set(path.name for path in directory.iterdir()) <= {
        'fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'manifest.json',
        'document.html', 'profile', 'origin.json', 'admission.json',
        'owned_chromium_lease.py', 'watchdog-origin.json', 'browser-lease.json'})
    shutil.rmtree(directory)
    print(json.dumps({'owned_private_profile_removed': True}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=OPERATIONS)
    parser.add_argument('directory')
    parser.add_argument('run_id')
    parser.add_argument('extra', nargs='?')
    args = parser.parse_args()
    if args.operation in ('clients', 'retire', 'cleanup'):
        globals()[args.operation](args.directory, args.run_id, json.load(sys.stdin))
    elif args.operation == 'fixture-clipboard':
        fixture_clipboard(args.directory, args.run_id, json.load(sys.stdin))
    elif args.operation == 'clipboard-observe':
        clipboard_observe(args.directory, args.run_id, json.load(sys.stdin))
    elif args.operation == 'dom':
        dom(args.directory, args.run_id, args.extra, json.load(sys.stdin))
    else:
        globals()[args.operation](args.directory, args.run_id)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # No browser/profile contents or unsolicited protocol errors in output.
        print(json.dumps(failure_details(error, sys.argv[1] if len(sys.argv) > 1 else None),
                         sort_keys=True), file=sys.stderr)
        raise SystemExit(1)
