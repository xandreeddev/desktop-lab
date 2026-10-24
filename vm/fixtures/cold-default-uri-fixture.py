#!/usr/bin/env python3
"""One URI-created, disposable Chromium birth; never warms or adopts a browser.

Staged as default-uri.py beside the unchanged warm bridge and canonical fixture.
Only this fixture's cold declaration/admission/retirement are different. MIME
leases, actual xdg-open dispatch, notification expiry and fixed Exec encoding
come from the maintained bridge. Every window in this new profile is disposable.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import cold_default_uri_policy as c
import default_uri_policy as p
import default_uri_warm as warm
from owned_application_units import (capture_unit_origin, process_snapshot,
                                     retire_original_unit, _retirement_drained)


def context(run, run_id):
    run, header, base = warm.context(run, run_id)
    c.require(header.get('cold_case') in p.NOTIFICATION_CASES
              and header.get('assert_browser_key_handoff') is True)
    return run, header, base


def prepare(run, run_id):
    run, _, base = context(run, run_id)
    result = warm.prepare(run, run_id)
    _, manifest = base.context(run, run_id)
    http = base.http_authority(run, run_id)
    c.require(len(http['targets']) == 1 and base.events(run) == [])
    declared = base.chromium.startup(base.browser_argv(run, manifest, [http['targets'][0]['uri']]))
    # The canonical manifest deliberately remains its original blank-startup
    # declaration. Only the separate, immutable cold journal owns URI startup.
    p.private(run / 'cold-startup.json', json.dumps({'http': http, 'declared': declared}).encode())
    return result


def startup(run, run_id):
    run, _, base = context(run, run_id)
    value = json.loads(p.read(run / 'cold-startup.json'))
    _, manifest = base.context(run, run_id)
    c.require(value['http']['targets'] == base.http_documents(manifest, value['http']['listener']['port'])
              and value['declared']['argv'] == base.browser_argv(run, manifest, [value['http']['targets'][0]['uri']]))
    return run, base, value


def launch(run, run_id, uri):
    run, base, declared = startup(run, run_id)
    warm.admitted(run, run_id)
    c.require(uri == declared['http']['targets'][0]['uri']
              and bool(os.environ.get('XDG_ACTIVATION_TOKEN'))
              and not os.environ.get('DESKTOP_STARTUP_ID'))
    with (run / 'events.lock').open('r+b') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        c.require(not os.path.lexists(run / 'default-closing') and not os.path.lexists(run / 'closing')
                  and base.events(run) == [] and not os.path.lexists(run / 'profile')
                  and not os.path.lexists(run / 'cold-request.json')
                  and json.loads(p.read(run / 'default-events.json')) == [])
        # An immutable request intent denies even a second queued native open.
        p.private(run / 'cold-request.json', json.dumps({'uri': uri}).encode())
        base.atomic(run / 'default-events.json', json.dumps([{
            'selected_uri_matches': True, 'fresh_token_present': True,
            'legacy_startup_present': False}]).encode())
    environment = os.environ.copy()
    environment.pop('DESKTOP_STARTUP_ID', None)
    argv = ['systemd-run', '--user', '--quiet', '--collect', '--service-type=exec',
            '--expand-environment=no', '--slice=app.slice', '--property=ExitType=cgroup',
            '--property=RuntimeMaxSec=' + str(p.BROWSER_RUNTIME_SECONDS), '--property=MemoryMax=768M',
            '--property=UnsetEnvironment=DESKTOP_STARTUP_ID']
    for name in ('HOME', 'PATH', 'DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR',
                 'DBUS_SESSION_BUS_ADDRESS', 'HYPRLAND_INSTANCE_SIGNATURE', 'XDG_CURRENT_DESKTOP',
                 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_ACTIVATION_TOKEN'):
        if name in environment:
            argv.append('--setenv=' + name)
    argv += ['--', '/usr/bin/python3', str(run / 'fixture.py'), 'execute', str(run), run_id, 'open', uri]
    result = subprocess.run(argv, env=environment, capture_output=True, timeout=8)
    c.require(result.returncode == 0)
    return {'admitted': True}


def original(run, run_id):
    run, base, value = startup(run, run_id)
    event = c.first_event(base.events(run), value['http']['targets'][0]['uri'])
    return run, base, value, event


def browser_authority(run, run_id, expected=None):
    run, base, value, event = original(run, run_id)
    captured = json.loads(p.read(run / 'cold-browser.json'))
    c.require(expected is None or expected == captured)
    c.require(captured['lease']['origin'] == event['origin']
              and captured['lease']['declared'] == value['declared'])
    base.chromium.observe(event['origin'], value['declared'], captured['lease'])
    c.require(base.browser_endpoint(run) == captured['endpoint']
              and c.document(base.browser_pages(run), value['http']['targets']) == captured['target'])
    base.chromium.observe(event['origin'], value['declared'], captured['lease'])
    c.require(base.browser_endpoint(run) == captured['endpoint'])
    return captured


def ready(run, run_id):
    """Only missing startup facts wait; original identity changes are failures."""
    from urllib.error import URLError
    run, base, value, event = original(run, run_id)
    warm.admitted(run, run_id)
    c.require(not os.path.lexists(run / 'cold-browser.json'))
    c.require(not base.chromium.original_gone(event['origin']['process']))
    clients = warm.warm_native_clients()
    mapped = p.warm_client(clients, event['origin']['process']['pid'], value['declared']['class'])
    if mapped is None:
        return {'ready': False}
    lease = base.chromium.observe(event['origin'], value['declared'])
    try:
        endpoint = base.browser_endpoint(run)
        target = c.document(base.browser_pages(run), value['http']['targets'])
    except (FileNotFoundError, ConnectionError, TimeoutError, URLError):
        return {'ready': False}
    if target is None:
        return {'ready': False}
    base.chromium.observe(event['origin'], value['declared'], lease)
    c.require(p.warm_client(warm.warm_native_clients(), lease['process']['pid'], value['declared']['class']) == mapped
              and base.browser_endpoint(run) == endpoint
              and c.document(base.browser_pages(run), value['http']['targets']) == target)
    c.require(base.browser_endpoint(run) == endpoint)
    base.chromium.observe(event['origin'], value['declared'], lease)
    captured = {'lease': lease, 'endpoint': endpoint, 'target': target}
    p.private(run / 'cold-browser.json', json.dumps(captured).encode())
    return {'ready': True, 'identity': captured}


def guard(run, run_id, clients):
    captured = browser_authority(run, run_id)
    c.require(p.warm_client(clients, captured['lease']['process']['pid'],
                            captured['lease']['declared']['class']) is not None)
    return {'owned_clients': len(clients)}


def revoke(run):
    with (run / 'events.lock').open('r+b') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        for name, content in (('default-closing', b'closing\n'), ('closing', b'retiring\n')):
            path = run / name
            if os.path.lexists(path):
                c.require(p.read(path) == content)
            else:
                p.private(path, content)


def retire(run, run_id):
    """Independent cleanup works before CDP/map readiness, without adoption.

    A still-original wrapper uses the original unit guard. An exec-complete
    original Chromium can grant its exact moved-scope lease before it maps.
    If its birth is already gone before a scope lease exists, evidence is
    insufficient: refuse, never search for or signal a replacement scope.
    """
    run, _, base = context(run, run_id)
    revoke(run)
    events = base.events(run)
    if events:
        _, base, value, event = original(run, run_id)
        path = run / 'cold-retirement.json'
        wrapper = run / 'cold-wrapper-retired.json'
        if wrapper.exists():
            c.require(json.loads(p.read(wrapper)) == event['origin']
                      and _retirement_drained(event['origin']))
        elif path.exists():
            lease = json.loads(p.read(path))
            c.require(lease['origin'] == event['origin'] and lease['declared'] == value['declared'])
            base.chromium.retire(lease)
        elif (run / 'cold-browser.json').exists():
            lease = json.loads(p.read(run / 'cold-browser.json'))['lease']
            c.require(lease['origin'] == event['origin'] and lease['declared'] == value['declared'])
            p.private(path, json.dumps(lease).encode())
            base.chromium.retire(lease)
        else:
            c.require(not base.chromium.original_gone(event['origin']['process']))
            process = process_snapshot(event['origin']['process']['pid'])
            c.require(process['start'] == event['origin']['process']['start'])
            if process['executable'] == value['declared']['executable']['identity']:
                lease = base.chromium.observe(event['origin'], value['declared'])
                p.private(path, json.dumps(lease).encode())
                base.chromium.retire(lease)
            else:
                # Python/installed launcher's original service only. The
                # maintained unit helper independently brackets final birth,
                # cgroup, executable and invocation immediately before stop.
                c.require(process['group'] == event['origin']['unit']['group'])
                unit = event['cgroup'].rsplit('/', 1)[1]
                retire_original_unit(unit, base.event_argv(run, run_id, event), event['origin'],
                                     max_executable_bytes=512 * 1024 * 1024)
                c.require(_retirement_drained(event['origin']))
                p.private(run / 'cold-wrapper-retired.json', json.dumps(event['origin']).encode())
    # Cold scope is retired FIRST. Generic canonical retirement can now
    # confirm its recorded wrapper is drained and remove only its own entry.
    result = warm.capture_print(base.retire, run, run_id)
    c.require(result['owned_browser_units_retired'] and result['owned_entry_retired'])
    return result


def watchdog(run, run_id):
    run, _, _ = context(run, run_id)
    runtime = p.directory(Path(os.environ['XDG_RUNTIME_DIR']))
    path = runtime / 'lucent-default-uri-acceptance.lock'
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'r+b') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        value = os.fstat(lock.fileno())
        c.require(stat.S_ISREG(value.st_mode) and stat.S_IMODE(value.st_mode) == 0o600
                  and value.st_uid == os.getuid() and value.st_nlink == 1
                  and p.identity(value) == p.identity(path.lstat()))
        unit = 'lucent-default-uri-' + run_id + '.service'
        argv = ['/usr/bin/python3', '-I', str(run / 'default-uri.py'), 'watchdog', str(run), run_id]
        p.private(run / 'default-guardian.json', json.dumps(capture_unit_origin(unit, argv)).encode())
        p.private(run / 'default-ready.json', b'{"ready":true}\n')
        result = {'success': False, 'restoration': {}}
        try:
            deadline = time.monotonic() + p.GUARDIAN_OPEN_SECONDS
            while not (run / 'default-finish').exists() and time.monotonic() < deadline:
                time.sleep(.2)
        finally:
            revoke(run)
            for name, action in (
                ('browser_and_original_entry_retired', lambda: retire(run, run_id)),
                ('notifications_absent_after_expiry_window', lambda: warm.wait_expiry(run, run_id)),
                ('default_mime_restored', lambda: p.restore(p.lease(run, 'default-mime'))),
                ('default_entry_restored', lambda: p.restore(p.lease(run, 'default-entry'))),
                ('notification_history_closed', lambda: warm.close_history(run, run_id)),
            ):
                try:
                    if name == 'default_entry_restored':
                        c.require(result['restoration'].get('default_mime_restored') is True)
                    if name == 'notification_history_closed':
                        c.require(result['restoration'].get('notifications_absent_after_expiry_window') is True)
                    action()
                    result['restoration'][name] = True
                except Exception:
                    result['restoration'][name] = False
            result['success'] = all(result['restoration'].values())
            p.private(run / 'default-finished.json', json.dumps(result).encode())


def browser_key(run, run_id, request, arm):
    run, header, base = context(run, run_id)
    warm.admitted(run, run_id)
    c.require(type(request) is dict and set(request) == {'identity', 'before', 'targets', 'case'}
              and request['case'] == header['cold_case'] and request['before'] == [])
    captured = browser_authority(run, run_id, request['identity'])
    c.require(request['targets'] == [captured['target']])
    receipt = run / ('browser-key-' + request['case'] + '.json')
    if arm:
        c.require(not os.path.lexists(receipt))
    else:
        c.require(json.loads(p.read(receipt)) == request)
    endpoint = captured['endpoint']
    values = json.loads(base.browser_http(run, '/json/list'))
    selected = [row for row in values if row.get('type') == 'page']
    c.require(base.pages(values) == [captured['target']] and len(selected) == 1
              and all(selected[0].get(key) == value for key, value in captured['target'].items())
              and base.browser_endpoint(run) == endpoint)
    from cdp_peer import Peer
    peer = Peer.connect(selected[0]['webSocketDebuggerUrl'], endpoint['port'], captured['target']['id'])
    try:
        result = peer.call('Runtime.evaluate', {
            'expression': p.browser_key_expression('arm' if arm else 'read', captured['target'], run_id, request['case']),
            'returnByValue': True, 'userGesture': False})
    finally:
        peer.close()
    value = p.browser_key_observation(result.get('result', {}).get('value'), armed=arm)
    c.require(browser_authority(run, run_id, captured) == captured)
    if arm:
        p.private(receipt, json.dumps(request).encode())
    return value


def dispatch(diagnostic):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=c.OPERATIONS)
    parser.add_argument('directory', type=Path)
    parser.add_argument('run_id')
    parser.add_argument('value', nargs='?')
    args = parser.parse_args()
    diagnostic.update(operation=args.operation, phase='context')
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    run, header, base = context(args.directory, args.run_id)
    diagnostic['phase'] = 'handler'
    if args.operation == 'watchdog':
        watchdog(run, args.run_id)
        return
    handlers = {'prepare': prepare, 'publish': warm.publish, 'admission': warm.admitted,
                'cold-ready': ready, 'cold-retire': retire, 'wait-expiry': warm.wait_expiry,
                'retire-guardian': warm.retire_guardian}
    if args.operation in handlers:
        value = handlers[args.operation](run, args.run_id)
    elif args.operation == 'open':
        value = launch(run, args.run_id, args.value)
    elif args.operation == 'cold-snapshot':
        captured = browser_authority(run, args.run_id, json.loads(args.value))
        value = {'identity': captured, 'pages': [captured['target']]}
    elif args.operation == 'cold-guard':
        value = guard(run, args.run_id, json.loads(args.value))
    elif args.operation in ('notify', 'observe-note'):
        c.require(args.value == header['cold_case'])
        value = (warm.notify if args.operation == 'notify' else warm.observe_note)(run, args.run_id, args.value)
    elif args.operation in ('browser-key-arm', 'browser-key-read'):
        c.require(type(args.value) is str and len(args.value.encode()) <= 16384)
        value = browser_key(run, args.run_id, json.loads(args.value), args.operation == 'browser-key-arm')
    elif args.operation == 'security':
        value = warm.security()
    elif args.operation == 'state':
        warm.admitted(run, args.run_id)
        value = {'bridge': json.loads(p.read(run / 'default-events.json')), 'events': base.events(run)}
    else:
        warm.guardian(run, args.run_id)
        p.private(run / 'default-finish', b'finish\n')
        value = {'finishing': True}
    print(json.dumps(value))


def main():
    diagnostic = {'operation': 'unknown', 'phase': 'arguments'}
    try:
        dispatch(diagnostic)
    except Exception as error:
        print(json.dumps(c.error_packet(diagnostic['operation'], error, diagnostic['phase'])), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
