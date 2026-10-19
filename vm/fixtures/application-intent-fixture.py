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
import stat
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from owned_application_units import (assert_unit_identity, owned_argv, retire_unit, unit_snapshot)


def require(condition):
    if not condition:
        raise ValueError('Owned application intent fixture identity failed')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular(path, limit=131072):
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'rb') as stream:
        value = os.fstat(stream.fileno())
        require(stat.S_ISREG(value.st_mode) and value.st_uid == os.getuid()
                and value.st_size <= limit)
        data = stream.read(limit + 1)
        require(len(data) <= limit)
        return data


def private(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data)


def atomic(path, data):
    descriptor, name = tempfile.mkstemp(prefix='.intent-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(data)
        os.replace(name, path)
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
                and manifest['units_sha256'] == digest(regular(directory / 'owned_application_units.py')))
    return directory, manifest


def encoded(value):
    # Exec quoting and desktop-entry escaping are two independent layers.
    value = (value.replace('\\', '\\\\').replace('"', '\\"')
             .replace('$', '\\$').replace('`', '\\`').replace('%', '%%'))
    return '"' + value.replace('\\', '\\\\') + '"'


def initialize(directory, run_id):
    directory, _ = context(directory, run_id, False)
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
             + 'StartupWMClass=LucentIntent' + run_id + '\nExec=' + command + ' open %U\n'
             + 'Actions=Probe;\n\n[Desktop Action Probe]\nName=Open owned document\nExec='
             + command + ' action ' + encoded(files[0]['uri']) + '\n').encode()
    manifest = {'run_id': run_id, 'script_sha256': digest(regular(directory / 'fixture.py')),
                'units_sha256': digest(regular(directory / 'owned_application_units.py')),
                'app_id': app_id, 'name': 'Lucent intent ' + run_id.lower(),
                'class': 'LucentIntent' + run_id, 'entry': str(entry_path),
                'entry_sha256': digest(entry), 'settings': str(settings),
                'settings_sha256': digest(settings_data),
                'files': [{key: value for key, value in item.items() if key != 'data'}
                          for item in files]}
    private(directory / 'manifest.json', json.dumps(manifest).encode())
    private(directory / 'events.json', b'[]\n')
    private(directory / 'events.lock', b'')
    for item in files:
        private(directory / item['name'], item['data'])
    private(entry_path, entry)
    print(json.dumps({key: manifest[key] for key in ('app_id', 'name', 'class', 'files')}))


def events(directory):
    value = json.loads(regular(directory / 'events.json'))
    require(isinstance(value, list) and len(value) <= 32)
    return value


def execute(directory, run_id, arguments):
    directory, manifest = context(directory, run_id)
    require(arguments and arguments[0] in ('open', 'action'))
    operation, *uris = arguments
    require(len(uris) <= 2 and all(uri in [item['uri'] for item in manifest['files']] for uri in uris)
            and (operation != 'action' or uris == [manifest['files'][0]['uri']]))
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
    arguments = ['/usr/bin/chromium', '--user-data-dir=' + str(directory / 'profile'),
                 '--class=' + manifest['class'], '--ozone-platform=wayland',
                 '--no-first-run', '--no-default-browser-check', '--disable-sync',
                 '--disable-extensions', '--disable-background-networking',
                 '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
                 '--new-window', *uris]
    signal.alarm(0)
    os.execv(arguments[0], arguments)


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
        groups = [line.split(':', 2)[2] for line in regular(Path('/proc', str(pid), 'cgroup')).decode().splitlines()
                  if line.startswith('0::')]
        require(len(groups) == 1)
        matching = [event for event in recorded if event['cgroup'] == groups[0]]
        require(len(matching) == 1)
        event = matching[0]
        unit = groups[0].rsplit('/', 1)[1]
        require(re.fullmatch(r'run-[A-Za-z0-9_-]+\.service', unit))
        value = unit_snapshot(unit)
        assert_unit_identity(value, unit, event_argv(directory, run_id, event), groups[0])
        require(value['active'] not in ('inactive', 'failed'))
    print(json.dumps({'owned_clients': len(clients)}))


def retire_example(directory, run_id):
    binary = str(directory / 'application-intent')
    retire_unit('lucent-application-intent-' + run_id + '.service',
                owned_argv('lucent-application-intent-', binary, run_id))


def retire(directory, run_id):
    directory, _ = context(directory, run_id)
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
    retire_example(directory, run_id)
    recorded = events(directory)
    for group in sorted({event['cgroup'] for event in recorded}):
        unit = group.rsplit('/', 1)[1]
        require(re.fullmatch(r'run-[A-Za-z0-9_-]+\.service', unit))
        matching = [event for event in recorded if event['cgroup'] == group]
        require(len(matching) == 1)
        retire_unit(unit, event_argv(directory, run_id, matching[0]), group)
    print(json.dumps({'owned_browser_units_retired': True}))


def cleanup(directory, run_id):
    directory, manifest = context(directory, run_id)
    retire(directory, run_id)
    require(digest(regular(Path(manifest['settings']), 4 * 1024 * 1024)) == manifest['settings_sha256'])
    entry = Path(manifest['entry'])
    require(digest(regular(entry)) == manifest['entry_sha256'])
    for item in manifest['files']:
        require(digest(regular(directory / item['name'])) == item['sha256'])
    require(set(path.name for path in directory.iterdir()) <= {
        'fixture.py', 'manifest.json', 'events.json', 'events.lock', 'closing', 'profile',
        'application-intent', 'application-intent.sha256', 'owned_application_units.py', *[item['name'] for item in manifest['files']]})
    profile = directory / 'profile'
    if profile.exists():
        value = profile.lstat()
        require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid() and not profile.is_symlink())
    entry.unlink()
    shutil.rmtree(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('initialize', 'execute', 'state', 'retire', 'cleanup', 'watchdog', 'guard'))
    parser.add_argument('directory')
    parser.add_argument('run_id')
    parser.add_argument('arguments', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    signal.alarm(55)
    if args.operation == 'initialize' and not args.arguments:
        initialize(args.directory, args.run_id)
    elif args.operation == 'execute':
        execute(args.directory, args.run_id, args.arguments)
    elif args.operation == 'state' and not args.arguments:
        directory, _ = context(args.directory, args.run_id)
        print(json.dumps({'events': events(directory)}))
    elif args.operation == 'guard' and len(args.arguments) == 1:
        owned_clients(args.directory, args.run_id, json.loads(args.arguments[0]))
    elif args.operation == 'retire' and not args.arguments:
        retire(args.directory, args.run_id)
    elif args.operation == 'watchdog' and not args.arguments:
        context(args.directory, args.run_id)
        signal.alarm(295)
        time.sleep(210)
        # Keep attempting retirement when the host disappears or one bus call
        # fails. No new request is admitted after the exact example is stopped.
        for attempt in range(6):
            try:
                retire(args.directory, args.run_id)
                break
            except Exception:
                if attempt == 5:
                    raise
                time.sleep(5)
    elif args.operation == 'cleanup' and not args.arguments:
        cleanup(args.directory, args.run_id)
    else:
        raise ValueError('Unexpected owned fixture arguments')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('Owned application intent fixture failed') from None
