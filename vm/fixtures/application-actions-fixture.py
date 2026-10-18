#!/usr/bin/env python3
"""Owned desktop entries, literal-argv recorder and bounded native action fixture.

This helper never discovers, launches or replaces another application. All
mutable files are identified by a private run manifest and exact content hashes.
The server owns one unique reverse-DNS name without replacement or queuing.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import tempfile


APPLICATION = 'org.freedesktop.Application'
CONTROL = 'org.lucent.ApplicationActionsFixture'
LITERAL = ['two words', '$HOME', '$(unused)', 'semi;colon', '%', '']


def require(value):
    if not value:
        raise ValueError('Owned fixture identity check failed')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def private_file(path, data, *, exclusive=False):
    flags = os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW
    flags |= os.O_EXCL if exclusive else os.O_TRUNC
    descriptor = os.open(path, flags, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)


def atomic_file(path, data):
    descriptor, temporary = tempfile.mkstemp(prefix='.lucent-actions-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(data)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def regular(path):
    value = path.lstat()
    require(stat.S_ISREG(value.st_mode) and value.st_uid == os.getuid())
    return path.read_bytes()


def context(directory, run_id, *, initialized=True):
    directory = Path(directory)
    require(directory.is_absolute() and directory.resolve() == directory)
    value = directory.lstat()
    require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and stat.S_IMODE(value.st_mode) == 0o700)
    require(re.fullmatch(r'[A-Za-z0-9]{6,32}', run_id))
    manifest = None
    if initialized:
        manifest = json.loads(regular(directory / 'manifest.json'))
        require(manifest['run_id'] == run_id
                and manifest['script'] == digest(regular(directory / 'fixture.py')))
    return directory, manifest


def encoded(value):
    # Apply Exec quoting, then desktop-entry string unescaping. Dollars and
    # backticks remain literal even in another conforming launcher; %% is the
    # Exec field code for one literal percent character.
    value = (value.replace('\\', '\\\\').replace('"', '\\"')
             .replace('$', '\\$').replace('`', '\\`').replace('%', '%%'))
    return '"' + value.replace('\\', '\\\\') + '"'


def identity(run_id, kind):
    stem = 'org.lucent.ApplicationActions' + kind.title() + run_id
    return stem + '.desktop', stem, 'Lucent action ' + kind + ' ' + run_id.lower()


def entry(directory, run_id, kind, variant='original'):
    app_id, _, name = identity(run_id, kind)

    def command(event, action, version, *arguments):
        values = ['/usr/bin/python3', str(directory / 'fixture.py'), 'record',
                  str(directory), run_id, event, action, version, *arguments]
        return ' '.join(encoded(value) for value in values)

    actions = [('Literal', 'Literal fixture action'), ('Stale', 'Revocable fixture action')]
    if kind == 'native':
        actions = [('Native', 'Native fixture action'), ('Denied', 'Denied fixture action')]
    elif variant == 'removed-action':
        actions = actions[:1]
    elif variant == 'changed':
        actions[1] = ('Stale', 'Changed fixture action')
    version = 'v2' if variant == 'changed' else 'v1'
    lines = ['[Desktop Entry]', 'Type=Application', 'Name=' + name,
             'Icon=application-x-executable', 'Terminal=false', 'NoDisplay=false',
             'DBusActivatable=' + ('true' if kind == 'native' else 'false'),
             'Exec=' + command('primary-exec', '-', 'v1'), 'Path=' + str(directory),
             'Actions=' + ''.join(action + ';' for action, _ in actions)]
    for action, label in actions:
        event = 'action-fallback' if kind == 'native' else 'action-exec'
        arguments = LITERAL if action == 'Literal' else []
        lines += ['', '[Desktop Action ' + action + ']', 'Name=' + label,
                  'Exec=' + command(event, action, version, *arguments)]
    return app_id, ('\n'.join(lines) + '\n').encode()


def initialize(directory, run_id):
    directory, _ = context(directory, run_id, initialized=False)
    script = regular(directory / 'fixture.py')
    data_home = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share')))
    require(data_home.is_absolute())
    applications = (data_home / 'applications').resolve(strict=True)
    require(applications.is_dir())
    settings = Path.home() / '.local/state/lucent/desktop.json'
    settings_bytes = regular(settings)
    manifest = {'run_id': run_id, 'script': digest(script), 'entries': {},
                'settings': digest(settings_bytes), 'settings_path': str(settings)}
    for kind in ('command', 'native'):
        app_id, content = entry(directory, run_id, kind)
        path = applications / app_id
        require(not path.exists() and not path.is_symlink())
        manifest['entries'][kind] = {'path': str(path), 'sha256': digest(content)}
    private_file(directory / 'settings.backup', settings_bytes, exclusive=True)
    private_file(directory / 'events.json', b'[]\n', exclusive=True)
    private_file(directory / 'events.lock', b'', exclusive=True)
    private_file(directory / 'manifest.json', json.dumps(manifest).encode(), exclusive=True)
    for kind in ('command', 'native'):
        _, content = entry(directory, run_id, kind)
        private_file(Path(manifest['entries'][kind]['path']), content, exclusive=True)
    print(json.dumps({kind: {'id': identity(run_id, kind)[0],
                            'name': identity(run_id, kind)[2]}
                      for kind in ('command', 'native')}))


def append_event(directory, run_id, event):
    directory, _ = context(directory, run_id)
    lock = os.open(directory / 'events.lock', os.O_RDWR | os.O_NOFOLLOW)
    with os.fdopen(lock, 'r+b') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        data = regular(directory / 'events.json')
        require(len(data) <= 131072)
        events = json.loads(data)
        require(isinstance(events, list) and len(events) < 128)
        events.append(event)
        atomic_file(directory / 'events.json', json.dumps(events).encode())


def record(directory, run_id, arguments):
    require(len(arguments) >= 3)
    event, action, version, *values = arguments
    require(event in ('primary-exec', 'action-exec', 'action-fallback'))
    require(action in ('-', 'Literal', 'Stale', 'Native', 'Denied')
            and version in ('v1', 'v2'))
    require(values == (LITERAL if action == 'Literal' else []))
    append_event(directory, run_id, {'kind': event, 'action': action,
                 'version': version, 'argv': values,
                 'pid': os.getpid(),
                 'started': Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()[19],
                 'cgroup': Path('/proc/self/cgroup').read_text()})


def mutate(directory, run_id, variant):
    require(variant in ('original', 'changed', 'removed-action', 'removed-entry'))
    directory, manifest = context(directory, run_id)
    owned = manifest['entries']['command']
    path = Path(owned['path'])
    if owned['sha256'] is None:
        require(not path.exists() and not path.is_symlink())
    else:
        require(digest(regular(path)) == owned['sha256'])
    if variant == 'removed-entry':
        path.unlink()
        owned['sha256'] = None
    else:
        _, content = entry(directory, run_id, 'command', variant)
        atomic_file(path, content)
        owned['sha256'] = digest(content)
    atomic_file(directory / 'manifest.json', json.dumps(manifest).encode())


def state(directory, run_id):
    directory, _ = context(directory, run_id)
    events = json.loads(regular(directory / 'events.json'))
    require(isinstance(events, list) and len(events) <= 128)
    print(json.dumps({'run_id': run_id, 'events': events}))


def cleanup(directory, run_id):
    directory, manifest = context(directory, run_id)
    # Validate everything before deleting any entry. Independent changes retain
    # both entries and the settings backup for inspection; never overwrite them.
    settings_preserved = digest(regular(Path(manifest['settings_path']))) == manifest['settings']
    require(digest(regular(directory / 'settings.backup')) == manifest['settings'])
    # The recorders normally exit immediately. Refuse to remove their helper
    # while an exact recorded process is still alive; PID reuse is harmless.
    for event in json.loads(regular(directory / 'events.json')):
        if 'pid' in event:
            process = Path('/proc') / str(event['pid']) / 'stat'
            if process.exists():
                try:
                    started = process.read_text().rsplit(')', 1)[1].split()[19]
                except FileNotFoundError:
                    continue
                require(started != event['started'])
    paths = []
    for owned in manifest['entries'].values():
        path = Path(owned['path'])
        if not path.exists() and not path.is_symlink():
            continue  # Includes an interrupted exclusive installation.
        if owned['sha256'] is None:
            require(not path.exists() and not path.is_symlink())
        else:
            require(digest(regular(path)) == owned['sha256'])
            paths.append(path)
    for path in paths:
        path.unlink()
    print(json.dumps({'entries_removed': True, 'settings_preserved': settings_preserved}))


def native(directory, run_id, operation):
    # Lazy imports let the literal recorder stay a short-lived stdlib process.
    import dbus
    import dbus.service
    from dbus.mainloop.glib import DBusGMainLoop
    from gi.repository import GLib

    directory, _ = context(directory, run_id)
    _, bus_name, _ = identity(run_id, 'native')
    object_path = '/' + bus_name.replace('.', '/')
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus(private=True)
    if operation != 'serve':
        # Calling a unique owner prevents accidental activation of any service.
        owner = bus.get_name_owner(bus_name)
        api = dbus.Interface(bus.get_object(owner, '/Control'), CONTROL)
        value = json.loads(api.State(timeout=3))
        require(value['run_id'] == run_id)
        if operation == 'native-state':
            print(json.dumps(value))
        elif operation == 'quit':
            api.Quit(run_id, timeout=3)
        bus.close()
        return

    loop = GLib.MainLoop()

    class Application(dbus.service.Object):
        def __init__(self):
            self.name = dbus.service.BusName(bus_name, bus=bus, do_not_queue=True,
                                             allow_replacement=False, replace_existing=False)
            super().__init__(bus, object_path)

        @dbus.service.method(APPLICATION, in_signature='a{sv}', out_signature='')
        def Activate(self, platform):
            append_event(directory, run_id, {'kind': 'activate', 'empty_platform': not platform})

        @dbus.service.method(APPLICATION, in_signature='sava{sv}', out_signature='')
        def ActivateAction(self, action, parameters, platform):
            require(str(action) in ('Native', 'Denied'))
            append_event(directory, run_id, {'kind': 'activate-action', 'action': str(action),
                         'empty_parameters': not parameters, 'empty_platform': not platform})
            if str(action) == 'Denied':
                raise dbus.exceptions.DBusException('Owned fixture denial',
                    name='org.freedesktop.DBus.Error.AccessDenied')

        @dbus.service.method(APPLICATION, in_signature='asa{sv}', out_signature='')
        def Open(self, uris, platform):
            append_event(directory, run_id, {'kind': 'open'})
            raise dbus.exceptions.DBusException('Open is outside this fixture',
                name='org.freedesktop.DBus.Error.NotSupported')

    class Controls(dbus.service.Object):
        @dbus.service.method(CONTROL, in_signature='', out_signature='s')
        def State(self):
            return json.dumps({'run_id': run_id, 'owner': str(bus.get_unique_name()),
                               'pid': os.getpid(), 'ready': True})

        @dbus.service.method(CONTROL, in_signature='s', out_signature='')
        def Quit(self, supplied):
            require(str(supplied) == run_id)
            GLib.idle_add(loop.quit)

    application = Application()
    controls = Controls(bus, '/Control')
    GLib.timeout_add_seconds(290, loop.quit)
    loop.run()
    del controls, application
    bus.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('initialize', 'record', 'mutate', 'state',
                                             'cleanup', 'serve', 'native-state', 'quit'))
    parser.add_argument('directory')
    parser.add_argument('run_id')
    parser.add_argument('arguments', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    operation, directory, run_id = args.operation, args.directory, args.run_id
    # App Exec recorders are launched in independent application units. Give
    # every short-lived operation its own bound, including lock acquisition, so
    # failed acceptance cannot leave a recorder waiting indefinitely.
    if operation != 'serve':
        signal.alarm(15)
    if operation == 'initialize' and not args.arguments:
        initialize(directory, run_id)
    elif operation == 'record':
        record(directory, run_id, args.arguments)
    elif operation == 'mutate' and len(args.arguments) == 1:
        mutate(directory, run_id, args.arguments[0])
    elif operation == 'state' and not args.arguments:
        state(directory, run_id)
    elif operation == 'cleanup' and not args.arguments:
        cleanup(directory, run_id)
    elif operation in ('serve', 'native-state', 'quit') and not args.arguments:
        native(directory, run_id, operation)
    else:
        raise ValueError('Unexpected fixture arguments')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('Owned application-actions fixture failed') from None
