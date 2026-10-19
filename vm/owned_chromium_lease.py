"""Test-owned Chromium process leases. Import performs no native operations.

Chromium may move its original main PID to a registered application scope and
flatten its process title. These facts do not relax generic service ownership.
Only an immutable pre-exec wrapper receipt can grant this separate authority.
"""
import hashlib
import os
from pathlib import Path
import re
import signal
import stat
import time

from owned_application_units import process_snapshot, unit_snapshot

MAX_EXE = 512 * 1024 * 1024
CHROMIUM = '/usr/lib/chromium/chromium'


def require(value):
    if not value:
        raise ValueError('owned_chromium_lease_refused')


def identity(value):
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns]


def pinned_root_file(path, expected=None):
    path = Path(path)
    require(path.is_absolute() and path.resolve(strict=True) == path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_uid == 0
                and not stat.S_IMODE(before.st_mode) & 0o022 and 0 < before.st_size <= MAX_EXE)
        stamp = identity(before)
        require(identity(path.lstat()) == stamp)
        if expected is None:
            digest = hashlib.sha256(); count = 0
            while block := stream.read(1024 * 1024):
                count += len(block); require(count <= before.st_size)
                digest.update(block)
            require(count == before.st_size)
            result = {'path': str(path), 'identity': stamp, 'sha256': digest.hexdigest()}
        else:
            require(expected['path'] == str(path) and expected['identity'] == stamp)
            result = expected
        require(identity(os.fstat(stream.fileno())) == stamp == identity(path.lstat()))
        return result


def startup(argv):
    """Pin installed files and the explicit declared launch before exec."""
    require(type(argv) is list and 2 <= len(argv) <= 64 and argv[0] == '/usr/bin/chromium'
            and all(type(arg) is str and 0 < len(arg.encode()) <= 8192 and '\0' not in arg for arg in argv))
    for prefix in ('--user-data-dir=', '--class=', '--ozone-platform=',
                   '--remote-debugging-address=', '--remote-debugging-port='):
        require(len([arg for arg in argv if arg.startswith(prefix)]) == 1)
    profile = next(arg.split('=', 1)[1] for arg in argv if arg.startswith('--user-data-dir='))
    app_class = next(arg.split('=', 1)[1] for arg in argv if arg.startswith('--class='))
    require(Path(profile).is_absolute() and not any(c.isspace() for c in profile)
            and Path(profile).parent.resolve(strict=True) == Path(profile).parent
            and not os.path.lexists(profile) and re.fullmatch('[A-Za-z0-9]+', app_class))
    require(all(arg in argv for arg in ('--ozone-platform=wayland', '--remote-debugging-address=127.0.0.1',
                                      '--remote-debugging-port=0', '--new-window')))
    return {'argv': argv, 'profile': profile, 'class': app_class,
            'launcher': pinned_root_file('/usr/bin/chromium'), 'executable': pinned_root_file(CHROMIUM)}


def command_identity(pid, declared):
    with (Path('/proc') / str(pid) / 'cmdline').open('rb') as stream:
        raw = stream.read(65537)
    require(0 < len(raw) <= 65536 and raw.endswith(b'\0'))
    # Chromium's one-string process title is not a recoverable argv array.
    # Fixed profile/class flags contain no whitespace, so literal boundaries
    # are unambiguous. Never interpret shell syntax or split arbitrary text.
    values = raw[:-1].split(b'\0')
    required = [arg.encode() for arg in declared['argv'][1:]]
    prefixes = (b'--user-data-dir=', b'--class=', b'--ozone-platform=',
                b'--remote-debugging-address=', b'--remote-debugging-port=')
    if len(values) == 1:
        title = b' ' + values[0] + b' '
        require(values[0].startswith(CHROMIUM.encode() + b' '))
        for prefix in prefixes:
            require(title.count(b' ' + prefix) == 1)
        require(all(not any(c in arg for c in b'\t\n\r ') and title.count(b' ' + arg + b' ') == 1
                    for arg in required))
    else:
        require(values[0] == CHROMIUM.encode())
        require(all(sum(arg.startswith(prefix) for arg in values[1:]) == 1 for prefix in prefixes))
        require(all(values[1:].count(arg) == 1 for arg in required))
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw), 'flattened': len(values) == 1}


def scope_snapshot(group):
    """Query the existing scope named by actual kernel membership, never create it."""
    require(group.startswith('/') and '..' not in Path(group).parts)
    name = group.rsplit('/', 1)[1]
    require(name.endswith('.scope') and re.fullmatch('[A-Za-z0-9_.@\\-]+', name))
    import dbus
    bus = dbus.SessionBus(private=True)
    try:
        manager = dbus.Interface(bus.get_object('org.freedesktop.systemd1', '/org/freedesktop/systemd1'),
                                 'org.freedesktop.systemd1.Manager')
        try:
            path = manager.GetUnit(name, timeout=5)
        except dbus.DBusException as error:
            if error.get_dbus_name() == 'org.freedesktop.systemd1.NoSuchUnit':
                return {'load': 'not-found'}
            raise
        props = dbus.Interface(bus.get_object('org.freedesktop.systemd1', path), 'org.freedesktop.DBus.Properties')
        unit = lambda key: props.Get('org.freedesktop.systemd1.Unit', key, timeout=5)
        return {'id': str(unit('Id')), 'load': str(unit('LoadState')), 'active': str(unit('ActiveState')),
                'transient': bool(unit('Transient')), 'invocation': bytes(unit('InvocationID')).hex(),
                'group': str(props.Get('org.freedesktop.systemd1.Scope', 'ControlGroup', timeout=5))}
    finally:
        bus.close()


def observe(origin, declared, expected=None):
    original = origin['unit']; pid = origin['process']['pid']
    require(pid == original['main_pid'] and original['active'] == 'active' and original['transient'])
    name = original['group'].rsplit('/', 1)[1]
    value = unit_snapshot(name)
    require(value == original)
    pinned_root_file('/usr/bin/chromium', declared['launcher'])
    executable = pinned_root_file(CHROMIUM, declared['executable'])
    process = process_snapshot(pid, hashed=expected is None, max_executable_bytes=MAX_EXE)
    require(process['start'] == origin['process']['start'] and process['executable'] == executable['identity'])
    if expected is None:
        require(process['sha256'] == executable['sha256'])
    else:
        require(expected['process']['sha256'] == executable['sha256'])
        process['sha256'] = expected['process']['sha256']
    command = command_identity(pid, declared)
    scope = None
    if process['group'] != original['group']:
        scope = scope_snapshot(process['group'])
        require(scope.get('load') == 'loaded' and scope['active'] == 'active' and scope['transient']
                and scope['id'] == process['group'].rsplit('/', 1)[1] and scope['group'] == process['group']
                and re.fullmatch('[0-9a-f]{32}', scope['invocation']) and set(scope['invocation']) != {'0'})
    lease = {'unit': value, 'process': process, 'scope': scope, 'command': command,
             'declared': declared, 'origin': origin}
    require(expected is None or lease == expected)
    require(unit_snapshot(name) == value)
    again = process_snapshot(pid, max_executable_bytes=MAX_EXE)
    require({key: item for key, item in again.items() if key != 'sha256'} ==
            {key: item for key, item in process.items() if key != 'sha256'})
    require(command_identity(pid, declared) == command)
    if scope is not None:
        require(scope_snapshot(process['group']) == scope)
    return lease


def group_drained(group):
    require(group.startswith('/') and group != '/' and '..' not in Path(group).parts)
    try:
        with (Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.events').open('rb') as stream:
            raw = stream.read(4097)
    except FileNotFoundError:
        return True
    require(len(raw) <= 4096)
    rows = [row.split() for row in raw.splitlines()]
    require(all(len(row) == 2 for row in rows))
    values = [row[1] for row in rows if row[0] == b'populated']
    require(len(values) == 1 and values[0] in (b'0', b'1'))
    return values[0] == b'0'


def original_gone(process):
    try:
        with (Path('/proc') / str(process['pid']) / 'stat').open('rb') as stream:
            raw = stream.read(65537)
    except FileNotFoundError:
        return True
    require(len(raw) <= 65536)
    return raw.rsplit(b')', 1)[1].split()[19].decode('ascii') != process['start']


def retired(lease):
    """Observe both original registrations and recursive groups; never signal a scope."""
    origin = lease['origin']; old = origin['unit']
    value = unit_snapshot(old['group'].rsplit('/', 1)[1])
    if value['load'] != 'not-found':
        require(value['executable'] == old['executable'] and value['argv'] == old['argv']
                and value['invocation'] in ('', old['invocation']) and value['main_pid'] in (0, old['main_pid'])
                and value['group'] in ('', old['group']))
        if value['active'] not in ('inactive', 'failed'):
            require(value['active'] in ('active', 'deactivating'))
            return False
    scope = lease['scope']
    if scope is not None:
        current = scope_snapshot(scope['group'])
        if current['load'] != 'not-found':
            require(current['id'] == scope['id'] and current['invocation'] in ('', scope['invocation'])
                    and current['group'] in ('', scope['group']))
            if current['active'] not in ('inactive', 'failed'):
                require(current['active'] in ('active', 'deactivating'))
                return False
    return original_gone(lease['process']) and all(group_drained(group) for group in
        {old['group'], lease['process']['group']})


def retire(lease, seconds=15):
    require(type(seconds) in (int, float) and 0 < seconds <= 20)
    deadline = time.monotonic() + seconds
    def wait():
        while True:
            if retired(lease):
                return
            require(time.monotonic() < deadline)
            time.sleep(.1)
    if original_gone(lease['process']):
        wait()
        return
    try:
        descriptor = os.pidfd_open(lease['process']['pid'])
    except ProcessLookupError:
        require(original_gone(lease['process']))
        wait()
        return
    try:
        try:
            observe(lease['origin'], lease['declared'], lease)
            signal.pidfd_send_signal(descriptor, signal.SIGTERM)
        except (OSError, ValueError, AssertionError):
            require(original_gone(lease['process']))
        wait()
    finally:
        os.close(descriptor)
