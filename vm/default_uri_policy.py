"""Owned temporary MIME/entry configuration leases. Import performs no IO.

Linux renameat2 exchanges retain the original inode for restoration. A changed
target, including an identical-content replacement, never grants overwrite or
deletion authority. Journals precede publication and also cover a lost reply.
"""
import configparser
import ctypes
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import stat


# Finite test-fixture budgets, independent of desktop authority or performance.
NOTIFICATION_CASES = ('pointer', 'keyboard')
NOTIFICATION_TTL_MS = 60000
NOTIFICATION_EXPIRY_SECONDS = 65
NOTIFICATION_POST_EXPIRY_SECONDS = 15
BRIDGE_COMMAND_SECONDS = 30
BRIDGE_EXPIRY_SECONDS = 70
NATIVE_INPUT_SECONDS = 180
CANONICAL_HTTP_SECONDS = 210
HTTP_DELIVERY_MARGIN_SECONDS = 30
BROWSER_RUNTIME_SECONDS = 240
GUARDIAN_OPEN_SECONDS = 210
GUARDIAN_CLEANUP_RESERVE_SECONDS = 120
GUARDIAN_STARTUP_MARGIN_SECONDS = 15
GUARDIAN_UNIT_RUNTIME_SECONDS = 345
FINISH_RECEIPT_SECONDS = 90


def require(value, reason):
    if not value:
        raise ValueError(reason)


def identity(value):
    # Renames can change ctime. Preserve every content/ownership/inode field.
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns]


def directory(path, private=False):
    path = Path(path)
    value = path.lstat()
    require(path.is_absolute() and path.resolve(strict=True) == path
            and stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and not stat.S_IMODE(value.st_mode) & 0o022
            and (not private or stat.S_IMODE(value.st_mode) == 0o700), 'owned_configuration_directory')
    return path


def snapshot(path):
    path = Path(path)
    directory(path.parent)
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    except FileNotFoundError:
        require(not os.path.lexists(path), 'configuration_link_is_not_absence')
        return None
    with os.fdopen(fd, 'rb') as stream:
        value = os.fstat(stream.fileno())
        require(stat.S_ISREG(value.st_mode) and value.st_uid == os.getuid()
                and value.st_nlink == 1 and value.st_size <= 256 * 1024
                and not stat.S_IMODE(value.st_mode) & 0o022, 'owned_configuration_file')
        raw = stream.read(256 * 1024 + 1)
        require(len(raw) == value.st_size and identity(value) == identity(os.fstat(stream.fileno()))
                == identity(path.lstat()), 'configuration_changed_during_read')
    return {'identity': identity(value), 'sha256': hashlib.sha256(raw).hexdigest(),
            'bytes': len(raw), 'mode': stat.S_IMODE(value.st_mode)}


def read(path):
    before = snapshot(path)
    require(before is not None, 'configuration_file_required')
    value = Path(path).read_bytes()
    require(snapshot(path) == before and hashlib.sha256(value).hexdigest() == before['sha256'],
            'configuration_read_changed')
    return value


def private(path, data, mode=0o600):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
    with os.fdopen(fd, 'wb') as stream:
        os.fchmod(stream.fileno(), mode)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    sync(Path(path).parent)


def sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def rename(source, target, flags):
    operation = ctypes.CDLL(None, use_errno=True).renameat2
    operation.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    operation.restype = ctypes.c_int
    if operation(-100, os.fsencode(source), -100, os.fsencode(target), flags):
        raise OSError(ctypes.get_errno(), 'owned_configuration_rename_failed')


def plan(run, name, target, content):
    run, target = directory(run, True), Path(target)
    require(re.fullmatch('[a-z][a-z-]{0,31}', name) is not None
            and target.is_absolute() and target.parent.stat().st_dev == run.stat().st_dev,
            'same_filesystem_owned_configuration_lease')
    before = snapshot(target)
    slot = run / (name + '.payload')
    private(slot, content, before['mode'] if before is not None else 0o600)
    value = {'target': str(target), 'slot': str(slot), 'original': before, 'candidate': snapshot(slot)}
    private(run / (name + '.lease.json'), (json.dumps(value, sort_keys=True) + '\n').encode())
    return value


def lease(run, name):
    run = directory(run, True)
    value = json.loads(read(run / (name + '.lease.json')))
    require(value['slot'] == str(run / (name + '.payload')) and Path(value['target']).is_absolute(),
            'exact_owned_configuration_lease')
    directory(Path(value['target']).parent)
    return value


def publish(value):
    target, slot = Path(value['target']), Path(value['slot'])
    require(snapshot(target) == value['original'] and snapshot(slot) == value['candidate'],
            'configuration_publication_preimage_changed')
    rename(slot, target, 1 if value['original'] is None else 2)
    sync(target.parent)
    sync(slot.parent)
    if snapshot(target) != value['candidate'] or snapshot(slot) != value['original']:
        # A successor that raced the preimage read is not ours to replace.
        # Put the displaced file back while our exact candidate still owns the
        # named target, and retain all private evidence for failed admission.
        if value['original'] is not None and snapshot(target) == value['candidate']:
            rename(slot, target, 2)
            sync(target.parent)
            sync(slot.parent)
        raise ValueError('raced_configuration_publication_preserved')


def active(value):
    require(snapshot(value['target']) == value['candidate'] and snapshot(value['slot']) == value['original'],
            'independent_configuration_change_preserved')


def restore(value):
    target, slot = Path(value['target']), Path(value['slot'])
    if snapshot(target) == value['original']:
        require(snapshot(slot) == value['candidate'], 'uncertain_configuration_restore_slot_preserved')
        return  # Never published, or this exact original inode is already back.
    active(value)
    if value['original'] is None:
        rename(target, slot, 1)
        if snapshot(slot) != value['candidate']:
            # A raced successor is retained; never overwrite another target.
            rename(slot, target, 1)
            raise ValueError('raced_configuration_removal_preserved')
    else:
        rename(slot, target, 2)
        if snapshot(slot) != value['candidate'] or snapshot(target) != value['original']:
            require(snapshot(target) == value['original'], 'uncertain_configuration_exchange_retained')
            rename(slot, target, 2)
            raise ValueError('raced_configuration_exchange_preserved')
    sync(target.parent)
    sync(slot.parent)
    require(snapshot(target) == value['original'], 'original_configuration_not_restored')


def mime_content(original, app_id):
    require(re.fullmatch(r'org\.lucent\.DefaultUri[A-Za-z0-9]{6,32}\.desktop', app_id) is not None,
            'owned_default_handler_identity')
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.optionxform = str
    parser.read_string(original.decode('utf-8'))
    section = 'Default Applications'
    if not parser.has_section(section):
        parser.add_section(section)
    parser.set(section, 'x-scheme-handler/http', app_id + ';')
    output = io.StringIO()
    parser.write(output, space_around_delimiters=False)
    return output.getvalue().encode()


def notification_absence(snapshot, records):
    """Read-only revision diagnostics; a wire ID never grants dismissal rights."""
    count = snapshot['client']['notification_count']
    require(type(count) is int and 0 <= count <= 4096, 'bounded_notification_count')
    known = {row['id']: row.get('observed_revision') for row in records}
    require(len(known) == len(records), 'distinct_observed_fixture_wire_ids')
    visible = []
    for surface in snapshot['surfaces']:
        if surface.get('role', surface.get('id')) != 'notifications':
            continue
        for hit in surface['hits']:
            match = re.fullmatch(r'notification-dismiss-([0-9]+)-([0-9]+)', hit['id'])
            if match:
                identifier, revision = map(int, match.groups())
                require(identifier in known, 'foreign_notification_preserved')
                require(known[identifier] is None or known[identifier] == revision,
                        'replaced_notification_revision_preserved')
                visible.append(identifier)
    require(count <= len(known), 'foreign_remaining_notification_preserved')
    return count == 0 and not visible


WARM_PHASES = ('context', 'original_event', 'original_birth', 'mapped_client', 'authority_before',
               'cdp_endpoint', 'blank_control', 'authority_after', 'mapped_client_after',
               'cdp_endpoint_after', 'ready')
WARM_ERROR_CLASSES = ('ValueError', 'AssertionError', 'FileNotFoundError', 'PermissionError',
                      'TimeoutError', 'TimeoutExpired', 'ConnectionError', 'ConnectionRefusedError',
                      'ConnectionResetError', 'JSONDecodeError', 'URLError', 'HTTPError', 'OSError',
                      'OtherError')


def warm_client(clients, pid, app_class):
    """One actual mapped window of the original temporary browser, or not ready."""
    require(type(clients) is list and len(clients) <= 32, 'bounded_warm_native_clients')
    if not clients:
        return None
    require(len(clients) == 1, 'sole_owned_warm_native_client_required')
    client = clients[0]
    require(type(pid) is int and pid > 1 and type(client.get('pid')) is int and client['pid'] == pid
            and client.get('class') == app_class and client.get('mapped') is True
            and client.get('hidden') is False and isinstance(client.get('address'), str)
            and re.fullmatch(r'0x[0-9a-fA-F]{1,32}', client['address']),
            'original_owned_mapped_warm_client_required')
    return {key: client[key] for key in ('pid', 'class', 'address')}


def warm_control(pages):
    """CDP has no page during startup; any nonblank/extra page blocks proof."""
    require(type(pages) is list and len(pages) <= 32, 'bounded_warm_cdp_pages')
    if not pages:
        return None
    require(len(pages) == 1 and pages[0].get('url') == 'about:blank'
            and isinstance(pages[0].get('id'), str)
            and re.fullmatch(r'[A-Za-z0-9-]{1,128}', pages[0]['id']),
            'sole_blank_warm_control_required')
    return dict(pages[0])


def warm_diagnostic(value):
    """Allow only fixed readiness labels; never relay native exception text."""
    require(type(value) is dict and set(value) <= {'ready', 'blocked', 'phase', 'error_class'}
            and type(value.get('ready')) is bool and type(value.get('blocked')) is bool
            and value.get('phase') in WARM_PHASES and not (value['ready'] and value['blocked'])
            and (not value['ready'] or value['phase'] == 'ready')
            and (value['phase'] != 'ready' or value['ready'])
            and (value['ready'] or value['blocked']
                 or value['phase'] in ('mapped_client', 'cdp_endpoint', 'blank_control'))
            and (('error_class' in value) == value['blocked'])
            and (not value['blocked'] or value['error_class'] in WARM_ERROR_CLASSES),
            'bounded_warm_readiness_reply_required')
    return dict(value)


BRIDGE_OPERATIONS = ('prepare', 'publish', 'admission', 'warm', 'open', 'warm-ready', 'browser-key-arm', 'browser-key-read',
                     'notify', 'observe-note', 'wait-expiry', 'watchdog', 'finish', 'state',
                     'security', 'retire-guardian')
BRIDGE_PHASES = ('arguments', 'context', 'dispatch', 'handler')
BRIDGE_ERROR_CLASSES = WARM_ERROR_CLASSES + ('KeyError', 'NameError', 'TypeError', 'AttributeError',
                                            'UnicodeDecodeError', 'DBusException')
BRIDGE_REASONS = ('unclassified_fixture_failure', 'owned_default_uri_run',
                  'frozen_default_uri_fixture_source', 'original_default_uri_guardian_required',
                  'open_default_uri_fixture_required', 'configuration_publication_preimage_changed',
                  'raced_configuration_publication_preserved', 'independent_configuration_change_preserved',
                  'owned_configuration_directory', 'owned_configuration_file',
                  'configuration_changed_during_read', 'configuration_file_required',
                  'configuration_link_is_not_absence', 'configuration_read_changed',
                  'exact_owned_configuration_lease', 'actual_owned_default_handler_selected',
                  'original_owned_http_listener', 'fresh_exact_desktop_pid',
                  'bounded_fixed_security_file', 'fixed_security_file_changed',
                  'owned_chromium_lease_refused', 'optional_browser_key_probe_required',
                  'exact_browser_key_request_required', 'one_original_fresh_key_document_required',
                  'original_key_capture_required', 'new_key_capture_required',
                  'exact_owned_key_target_endpoint_required', 'known_browser_key_case_required',
                  'fixed_browser_key_operation_required', 'bounded_browser_key_result_required',
                  'trusted_unmodified_key_a_required')


def browser_key_expression(operation, target, run_id, case):
    require(operation in ('arm', 'read'), 'fixed_browser_key_operation_required')
    require(type(run_id) is str and re.fullmatch('[a-f0-9]{12}', run_id) is not None
            and case in NOTIFICATION_CASES, 'known_browser_key_case_required')
    name = '__lucentOwnedKey_' + run_id + '_' + case
    guard = ('if(location.href!==' + json.dumps(target['url']) + '||document.title!=='
             + json.dumps(target['title']) + ')throw 1;const name=' + json.dumps(name) + ';')
    if operation == 'arm':
        body = ('if(Object.prototype.hasOwnProperty.call(globalThis,name))throw 1;'
                'const state=Object.seal({count:0,event:null});'
                'Object.defineProperty(globalThis,name,{value:state});'
                'window.addEventListener("keydown",event=>{if(event.code!=="KeyA")return;'
                'state.count=Math.min(2,state.count+1);if(state.count===1)state.event=Object.freeze({'
                'trusted:event.isTrusted,key:event.key==="a"?"a":"other",code:"KeyA",'
                'ctrl:event.ctrlKey,alt:event.altKey,meta:event.metaKey,shift:event.shiftKey,repeat:event.repeat});},true);'
                'return {armed:true,count:0};')
    else:
        body = ('const state=globalThis[name];if(!state)throw 1;'
                'return {count:state.count,event:state.event};')
    return '(()=>{' + guard + body + '})()'


def browser_key_observation(value, armed=False):
    if armed:
        require(type(value) is dict and set(value) == {'armed', 'count'}
                and value['armed'] is True and type(value['count']) is int and value['count'] == 0,
                'bounded_browser_key_result_required')
        return dict(value)
    require(type(value) is dict and set(value) == {'count', 'event'}
            and type(value['count']) is int and 0 <= value['count'] <= 2,
            'bounded_browser_key_result_required')
    event = value['event']
    if value['count'] == 0:
        require(event is None, 'bounded_browser_key_result_required')
        return {'count': 0, 'event': None}
    require(type(event) is dict and set(event) == {'trusted', 'key', 'code', 'ctrl', 'alt', 'meta', 'shift', 'repeat'}
            and event['key'] in ('a', 'other') and event['code'] == 'KeyA'
            and all(type(event[name]) is bool for name in ('trusted', 'ctrl', 'alt', 'meta', 'shift', 'repeat')),
            'bounded_browser_key_result_required')
    return {'count': value['count'], 'event': dict(event)}


def received_browser_key(value):
    value = browser_key_observation(value)
    if value['count'] == 0:
        return None
    event = value['event']
    require(value['count'] == 1 and event['trusted'] and event['key'] == 'a'
            and not any(event[key] for key in ('ctrl', 'alt', 'meta', 'shift', 'repeat')),
            'trusted_unmodified_key_a_required')
    return value


def bridge_timeout(operation):
    require(operation in BRIDGE_OPERATIONS, 'known_default_uri_bridge_operation')
    return BRIDGE_EXPIRY_SECONDS if operation == 'wait-expiry' else BRIDGE_COMMAND_SECONDS


def admit_notification_case(now, input_deadline, http_deadline):
    require(all(type(value) in (int, float) and math.isfinite(value)
                for value in (now, input_deadline, http_deadline)), 'finite_notification_case_budget_required')
    require(now >= 0 and now + NOTIFICATION_TTL_MS / 1000 + NOTIFICATION_POST_EXPIRY_SECONDS
            <= min(input_deadline, http_deadline), 'notification_case_budget_exhausted')
    return True


def bridge_error(error, operation, phase):
    """Fixed failure metadata only; exception text, argv and locals stay private."""
    name = type(error).__name__
    reason = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else None
    return {'failure': 'owned_default_uri_fixture_refused',
            'operation': operation if operation in BRIDGE_OPERATIONS else 'unknown',
            'phase': phase if phase in BRIDGE_PHASES else 'arguments',
            'error_class': name if name in BRIDGE_ERROR_CLASSES else 'OtherError',
            'fixed_reason': reason if reason in BRIDGE_REASONS else 'unclassified_fixture_failure'}


def bridge_error_packet(value, operation):
    """Accept one exact, operation-bound packet containing only public labels."""
    require(type(value) is dict and set(value) ==
            {'failure', 'operation', 'phase', 'error_class', 'fixed_reason'}
            and value['failure'] == 'owned_default_uri_fixture_refused'
            and operation in BRIDGE_OPERATIONS and value['operation'] == operation
            and value['phase'] in BRIDGE_PHASES and value['error_class'] in BRIDGE_ERROR_CLASSES
            and value['fixed_reason'] in BRIDGE_REASONS
            and re.fullmatch('[a-z][a-z0-9_]{0,95}', value['fixed_reason']) is not None,
            'bounded_default_uri_error_packet_required')
    return dict(value)
