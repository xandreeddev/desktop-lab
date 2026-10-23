#!/usr/bin/env python3
"""Private default-handler bridge and independent configuration guardian.

The canonical application-intent fixture owns Chromium, its HTTP listener,
profile, documents and original process leases. This bridge forwards existing
activation authority by environment name; it never creates activation tokens.
Every browser window in that temporary profile is closed on teardown; all
content there, including user-added content, is disposable and not protected.
Private evidence may remain. Existing personal profiles, browser
processes and successors are never adopted. Notification wire IDs grant no
dismissal authority; fixtures expire naturally and replacements remain intact.
"""
import argparse
import contextlib
import fcntl
import hashlib
import html
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import default_uri_policy as p
from owned_application_units import (capture_unit_origin, assert_unit_origin, unit_snapshot,
                                     retire_original_unit, _retirement_drained)

NOTIFICATION_TTL_MS = 25000


def base_fixture():
    path = Path(__file__).with_name('fixture.py')
    spec = importlib.util.spec_from_file_location('canonical_intent_fixture', path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def context(run, run_id):
    run = p.directory(Path(run), True)
    p.require(re.fullmatch('[a-f0-9]{12}', run_id) is not None
              and run.name == 'lucent-application-intent.' + run_id, 'owned_default_uri_run')
    header = json.loads(p.read(run / 'default-header.json'))
    for name, expected in header['sources'].items():
        p.require(Path(name).name == name and hashlib.sha256(p.read(run / name)).hexdigest() == expected,
                  'frozen_default_uri_fixture_source')
    base = base_fixture()
    base.context(run, run_id)
    return run, header, base


def capture_print(function, *args):
    with contextlib.redirect_stdout(io.StringIO()) as output:
        result = function(*args)
    return json.loads(output.getvalue()) if output.getvalue().strip() else result


def guardian(run, run_id):
    path = run / 'default-guardian.json'
    original = json.loads(p.read(path))
    unit = 'lucent-default-uri-' + run_id + '.service'
    argv = ['/usr/bin/python3', '-I', str(run / 'default-uri.py'), 'watchdog', str(run), run_id]
    current = assert_unit_origin(unit_snapshot(unit), unit, argv, original)
    p.require(current is not None, 'original_default_uri_guardian_required')
    return original


def retire_guardian(run, run_id):
    p.require((run / 'default-finished.json').is_file(), 'settled_configuration_guardian_required')
    original = json.loads(p.read(run / 'default-guardian.json'))
    unit = 'lucent-default-uri-' + run_id + '.service'
    argv = ['/usr/bin/python3', '-I', str(run / 'default-uri.py'), 'watchdog', str(run), run_id]
    retire_original_unit(unit, argv, original)
    p.require(_retirement_drained(original), 'original_default_uri_guardian_not_drained')
    return {'retired': True}


def prepare(run, run_id):
    run, _, base = context(run, run_id)
    p.require(os.environ.get('XDG_CURRENT_DESKTOP', '').lower() == 'hyprland'
              and not os.environ.get('BROWSER'), 'known_native_default_handler_resolution')
    config = p.directory(Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config'))))
    data = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share')))
    apps = p.directory(data / 'applications')
    app_id = 'org.lucent.DefaultUri' + run_id + '.desktop'
    target = apps / app_id
    p.require(not os.path.lexists(target), 'new_owned_default_uri_entry_required')
    command = ' '.join(base.encoded(value) for value in (
        '/usr/bin/python3', '-I', str(run / 'default-uri.py'), 'open', str(run), run_id))
    entry = ('[Desktop Entry]\nType=Application\nName=Lucent owned default URI\n'
             'NoDisplay=true\nTerminal=false\nStartupNotify=false\n'
             'MimeType=x-scheme-handler/http;\nExec=' + command + ' %u\n').encode()
    mime = config / 'hyprland-mimeapps.list'
    old = p.read(mime) if os.path.lexists(mime) else b''
    p.plan(run, 'default-entry', target, entry)
    p.plan(run, 'default-mime', mime, p.mime_content(old, app_id))
    http = base.http_authority(run, run_id)
    p.require(len(http['targets']) == 1, 'one_owned_singular_http_document')
    p.private(run / 'default-plan.json', json.dumps({'app_id': app_id, 'http': http}).encode())
    p.private(run / 'default-events.json', b'[]\n')
    return {'prepared': True, 'app_id': app_id}


def publish(run, run_id):
    run, _, _ = context(run, run_id)
    guardian(run, run_id)
    p.require(not os.path.lexists(run / 'default-closing'), 'open_default_uri_fixture_required')
    # Both durable intents already exist. The independent guardian restores a
    # partial publication even if this process or its transport disappears.
    p.publish(p.lease(run, 'default-entry'))
    p.publish(p.lease(run, 'default-mime'))
    return {'published': True}


def admitted(run, run_id, config=True):
    run, header, base = context(run, run_id)
    guardian(run, run_id)
    if config:
        p.active(p.lease(run, 'default-entry'))
        p.active(p.lease(run, 'default-mime'))
        plan = json.loads(p.read(run / 'default-plan.json'))
        result = subprocess.run(['xdg-mime', 'query', 'default', 'x-scheme-handler/http'],
                                capture_output=True, timeout=5)
        p.require(result.returncode == 0 and result.stdout.decode().strip() == plan['app_id'],
                  'actual_owned_default_handler_selected')
        p.require(base.http_authority(run, run_id) == plan['http'], 'original_owned_http_listener')
    value = capture_print(base.admission, run, run_id,
                          {'desktop_sha256': header['desktop_sha256'], 'example_sha256': None})
    p.require(value['baseline']['desktop']['pid'] == header['desktop_pid'], 'fresh_exact_desktop_pid')
    return value


def launch(run, run_id, uri=None):
    run, _, base = context(run, run_id)
    admitted(run, run_id, uri is not None)
    p.require(not os.path.lexists(run / 'default-closing') and not os.path.lexists(run / 'closing'),
              'open_original_browser_fixture_required')
    environment = os.environ.copy()
    environment.pop('DESKTOP_STARTUP_ID', None)
    arguments = []
    if uri is None:
        p.require(base.events(run) == [], 'warm_original_browser_once')
        environment.pop('XDG_ACTIVATION_TOKEN', None)
    else:
        plan = json.loads(p.read(run / 'default-plan.json'))
        p.require(uri == plan['http']['targets'][0]['uri'], 'selected_owned_default_uri_required')
        with (run / 'events.lock').open('r+b') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            p.require(not os.path.lexists(run / 'default-closing') and not os.path.lexists(run / 'closing'),
                      'default_uri_bridge_retired')
            rows = json.loads(p.read(run / 'default-events.json'))
            p.require(len(rows) < 2, 'two_owned_native_uri_requests_only')
            rows.append({'selected_uri_matches': True,
                         'fresh_token_present': bool(environment.get('XDG_ACTIVATION_TOKEN')),
                         'legacy_startup_present': bool(os.environ.get('DESKTOP_STARTUP_ID'))})
            base.atomic(run / 'default-events.json', json.dumps(rows).encode())
        arguments = [uri]
    argv = ['systemd-run', '--user', '--quiet', '--collect', '--service-type=exec',
            '--expand-environment=no', '--slice=app.slice', '--property=ExitType=cgroup',
            '--property=RuntimeMaxSec=150', '--property=MemoryMax=768M',
            '--property=UnsetEnvironment=' + ('DESKTOP_STARTUP_ID' if environment.get('XDG_ACTIVATION_TOKEN')
                                             else 'XDG_ACTIVATION_TOKEN DESKTOP_STARTUP_ID')]
    for name in ('HOME', 'PATH', 'DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR',
                 'DBUS_SESSION_BUS_ADDRESS', 'HYPRLAND_INSTANCE_SIGNATURE', 'XDG_CURRENT_DESKTOP',
                 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_ACTIVATION_TOKEN'):
        if name in environment:
            argv.append('--setenv=' + name)
    argv += ['--', '/usr/bin/python3', str(run / 'fixture.py'), 'execute', str(run), run_id, 'open', *arguments]
    result = subprocess.run(argv, env=environment, capture_output=True, timeout=8)
    p.require(result.returncode == 0, 'owned_browser_request_admitted')
    return {'admitted': True}


def notify(run, run_id, sequence):
    run, header, _ = context(run, run_id)
    admitted(run, run_id)
    p.require(sequence in ('pointer', 'keyboard'), 'known_native_notification_interaction')
    path = run / ('notification-' + sequence + '.json')
    p.require(not path.exists(), 'new_owned_transient_notification')
    plan = json.loads(p.read(run / 'default-plan.json'))
    import dbus
    bus = dbus.SessionBus(private=True)
    try:
        owner = notification_owner(bus, header['desktop_pid'])
        proxy = dbus.Interface(bus.get_object(owner, '/org/freedesktop/Notifications'),
                               'org.freedesktop.Notifications')
        identifier = int(proxy.Notify('Lucent owned URI ' + run_id, 0, '', 'Owned URI ' + sequence,
            '<a href="' + html.escape(plan['http']['targets'][0]['uri'], quote=True) + '">Open owned document</a>',
            dbus.Array([], signature='s'), dbus.Dictionary({'transient': dbus.Boolean(True)}, signature='sv'),
            NOTIFICATION_TTL_MS, timeout=5))
    finally:
        bus.close()
    p.require(0 < identifier < 2**32, 'concrete_owned_notification_id')
    # These are observed routing facts, not authority over a mutable wire ID.
    p.private(path, json.dumps({'id': identifier, 'owner': owner, 'desktop_pid': header['desktop_pid'],
                                'expires_after': time.monotonic() + NOTIFICATION_TTL_MS / 1000}).encode())
    return {'id': identifier}


def notification_owner(bus, expected_pid):
    import dbus
    owner = str(bus.get_name_owner('org.freedesktop.Notifications'))
    peer = dbus.Interface(bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus'),
                          'org.freedesktop.DBus')
    p.require(re.fullmatch(r':[0-9]+\.[0-9]+', owner) is not None
              and int(peer.GetConnectionUnixProcessID(owner, timeout=5)) == expected_pid,
              'exact_original_notification_service_process')
    return owner


def inspection():
    result = subprocess.run([str(Path.home() / '.local/bin/lucent-cli'), 'inspect'],
                            capture_output=True, timeout=5)
    p.require(result.returncode == 0 and len(result.stdout) <= 4 * 1024 * 1024,
              'owned_desktop_inspector')
    return json.loads(result.stdout)


def observed_notes(run):
    records = []
    for sequence in ('pointer', 'keyboard'):
        path = run / ('notification-' + sequence + '.json')
        if path.exists():
            record = json.loads(p.read(path))
            observed = run / ('notification-' + sequence + '-observed.json')
            if observed.exists():
                record['observed_revision'] = json.loads(p.read(observed))['revision']
            records.append(record)
    return records


def observe_note(run, run_id, sequence):
    admitted(run, run_id)
    p.require(sequence in ('pointer', 'keyboard') and not (run / 'default-closing').exists(),
              'open_native_fixture_note_required')
    record = json.loads(p.read(run / ('notification-' + sequence + '.json')))
    plan = json.loads(p.read(run / 'default-plan.json'))
    uri = plan['http']['targets'][0]['uri']
    values = [hit['id'] for surface in inspection()['surfaces']
              if surface.get('role', surface.get('id')) == 'notifications' for hit in surface['hits']]
    expression = 'notification-body-' + str(record['id']) + r'-([0-9]+)-0/link/' + re.escape(uri)
    found = [re.fullmatch(expression, value) for value in values if re.fullmatch(expression, value)]
    p.require(len(found) == 1, 'one_observed_fixture_link_revision')
    revision = int(found[0].group(1))
    p.private(run / ('notification-' + sequence + '-observed.json'), json.dumps({'revision': revision}).encode())
    return {'revision': revision}


def wait_expiry(run, run_id):
    """Read only: never dismiss a mutable ID or remove a foreign replacement."""
    admitted(run, run_id, config=False)
    records = observed_notes(run)
    import dbus
    bus = dbus.SessionBus(private=True)
    try:
        started = time.monotonic()
        p.require(all(type(row['expires_after']) in (int, float)
                      and 0 <= row['expires_after'] <= started + NOTIFICATION_TTL_MS / 1000 + 1
                      for row in records), 'bounded_fixture_expiry_timestamp')
        after = max([row['expires_after'] for row in records], default=0)
        while True:
            for record in records:
                owner = notification_owner(bus, record['desktop_pid'])
                p.require(owner == record['owner'], 'notification_daemon_changed_during_expiry_wait')
            absent = p.notification_absence(inspection(), records)
            now = time.monotonic()
            if absent and now >= after:
                break
            p.require(now - started < 30, 'remaining_notification_preserved_after_expiry_window')
            time.sleep(.2)
    finally:
        bus.close()
    admitted(run, run_id, config=False)
    return {'absent_after_expiry_window': True}


def security():
    """Fixed read-only security scope; no configuration payload leaves here."""
    result = {}
    home = Path.home()
    paths = [Path('/etc/greetd/config.toml'), Path('/etc/greetd/lucent.lua'),
             Path('/etc/pam.d/greetd'), Path('/usr/local/lib/lucent/run-greeter'),
             Path('/usr/local/bin/lucent-greeter'), home / '.config/systemd/user/lucent.service',
             home / '.config/systemd/user/lucent-polkit.service']
    paths += [home / '.local/bin' / name for name in ('lucent-lock', 'lucent-greeter', 'lucent-polkit')]
    for path in paths:
        before = path.lstat()
        resolved = path.resolve(strict=True)
        fd = os.open(resolved, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as stream:
            value = os.fstat(stream.fileno())
            p.require(stat.S_ISREG(value.st_mode) and value.st_uid in (0, os.getuid())
                      and value.st_size <= 128 * 1024 * 1024, 'bounded_fixed_security_file')
            digest = hashlib.sha256()
            for block in iter(lambda: stream.read(65536), b''):
                digest.update(block)
            p.require(p.identity(value) == p.identity(os.fstat(stream.fileno())) == p.identity(resolved.lstat())
                      and p.identity(before) == p.identity(path.lstat()), 'fixed_security_file_changed')
        result[str(path)] = {'identity': p.identity(before), 'target_identity': p.identity(value),
                             'link': os.readlink(path) if path.is_symlink() else None,
                             'sha256': digest.hexdigest()}
    return result


def close_history(run, run_id):
    _, header, base = context(run, run_id)
    value = capture_print(base.admission, run, run_id,
                          {'desktop_sha256': header['desktop_sha256'], 'example_sha256': None})
    p.require(value['baseline']['desktop']['pid'] == header['desktop_pid'], 'same_desktop_before_history_restore')
    cli = str(Path.home() / '.local/bin/lucent-cli')
    def state():
        result = subprocess.run([cli, 'inspect'], capture_output=True, timeout=5)
        p.require(result.returncode == 0 and len(result.stdout) <= 4 * 1024 * 1024, 'owned_desktop_inspector')
        return json.loads(result.stdout)['client']
    current = state()
    p.require(current['notification_count'] == 0 and not current['launcher']
              and not current.get('widgets_editing') and not current.get('widget_grid')
              and not any(current.get(name, {}).get('open') for name in
                          ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'settings', 'displays',
                           'live_wallpaper', 'weather', 'application_actions')),
              'no_foreign_notification_or_panel_before_history_restore')
    clients = subprocess.run(['hyprctl', '-j', 'clients'], capture_output=True, timeout=5)
    p.require(clients.returncode == 0 and json.loads(clients.stdout) == [],
              'no_foreign_application_before_history_restore')
    if current['notification_history_open']:
        result = subprocess.run([cli, 'notifications', 'hide'], capture_output=True, timeout=5)
        p.require(result.returncode == 0, 'owned_notification_history_close')
    deadline = time.monotonic() + 5
    while state()['notification_history_open']:
        p.require(time.monotonic() < deadline, 'owned_notification_history_close_deadline')
        time.sleep(.1)


def watchdog(run, run_id):
    run, _, base = context(run, run_id)
    runtime = p.directory(Path(os.environ['XDG_RUNTIME_DIR']))
    fd = os.open(runtime / 'lucent-default-uri-acceptance.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'r+b') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        value = os.fstat(lock.fileno())
        p.require(stat.S_ISREG(value.st_mode) and stat.S_IMODE(value.st_mode) == 0o600
                  and value.st_uid == os.getuid() and value.st_nlink == 1
                  and p.identity(value) == p.identity((runtime / 'lucent-default-uri-acceptance.lock').lstat()),
                  'owned_default_uri_global_lease')
        unit = 'lucent-default-uri-' + run_id + '.service'
        argv = ['/usr/bin/python3', '-I', str(run / 'default-uri.py'), 'watchdog', str(run), run_id]
        origin = capture_unit_origin(unit, argv)
        p.private(run / 'default-guardian.json', json.dumps(origin).encode())
        p.private(run / 'default-ready.json', b'{"ready":true}\n')
        result = {'success': False, 'restoration': {}}
        try:
            deadline = time.monotonic() + 150
            while not (run / 'default-finish').exists() and time.monotonic() < deadline:
                time.sleep(.2)
        finally:
            if not (run / 'default-closing').exists():
                p.private(run / 'default-closing', b'closing\n')
            # Revoke queued browser work before retiring only captured origins.
            for operation, action in (
                ('browser_and_original_entry_retired', lambda: capture_print(base.retire, run, run_id)),
                ('notifications_absent_after_expiry_window', lambda: wait_expiry(run, run_id)),
                ('default_mime_restored', lambda: p.restore(p.lease(run, 'default-mime'))),
                ('default_entry_restored', lambda: p.restore(p.lease(run, 'default-entry'))),
                ('notification_history_closed', lambda: close_history(run, run_id)),
            ):
                try:
                    if operation == 'default_entry_restored':
                        p.require(result['restoration'].get('default_mime_restored') is True,
                                  'retain_owned_handler_when_foreign_mime_change_blocks_restoration')
                    if operation == 'notification_history_closed':
                        p.require(result['restoration'].get('notifications_absent_after_expiry_window') is True,
                                  'leave_history_untouched_when_notification_restoration_is_uncertain')
                    action()
                    result['restoration'][operation] = True
                except Exception:
                    result['restoration'][operation] = False
            result['success'] = all(result['restoration'].values())
            p.private(run / 'default-finished.json', json.dumps(result).encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('prepare', 'publish', 'admission', 'warm', 'open',
                                              'notify', 'observe-note', 'wait-expiry', 'watchdog', 'finish', 'state',
                                              'security', 'retire-guardian'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('run_id')
    parser.add_argument('value', nargs='?')
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    if args.operation == 'watchdog':
        watchdog(args.directory, args.run_id)
        return
    run, _, base = context(args.directory, args.run_id)
    if args.operation in ('prepare', 'publish', 'admission'):
        result = globals()[args.operation](run, args.run_id)
    elif args.operation in ('warm', 'open'):
        p.require((args.operation == 'warm') == (args.value is None), 'known_default_uri_request_shape')
        result = launch(run, args.run_id, args.value)
    elif args.operation == 'notify':
        result = notify(run, args.run_id, args.value)
    elif args.operation == 'observe-note':
        result = observe_note(run, args.run_id, args.value)
    elif args.operation == 'wait-expiry':
        result = wait_expiry(run, args.run_id)
    elif args.operation == 'state':
        admitted(run, args.run_id)
        result = {'bridge': json.loads(p.read(run / 'default-events.json')), 'events': base.events(run)}
    elif args.operation == 'security':
        result = security()
    elif args.operation == 'retire-guardian':
        result = retire_guardian(run, args.run_id)
    else:
        guardian(run, args.run_id)
        p.private(run / 'default-finish', b'finish\n')
        result = {'finishing': True}
    print(json.dumps(result))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(json.dumps({'failure': 'owned_default_uri_fixture_refused',
                          'error_class': type(error).__name__}), file=sys.stderr)
        raise SystemExit(1) from None
