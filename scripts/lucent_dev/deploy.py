"""Opt-in reload of an already-running user shell, under the CLI's exclusive lease.

Each publication is atomic; the ten-file transaction is not globally atomic.
Durable receipts/backups support guarded recovery, not recovery from SIGKILL or
power loss. Secure clients, user settings and session configuration are not owned
by this transaction and are never restored over an independent change.
"""
from contextlib import contextmanager
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import stat
import struct
import subprocess
import time
import uuid

from lucent_readiness import desktop_rendered

LIMIT = 512 * 1024 * 1024
SERVICE = 'lucent.service'
CGROUP_ROOT = Path('/sys/fs/cgroup')


class DeployError(RuntimeError):
    """Reload failed; an optional private journal explains recovery state."""


class Deferred(DeployError):
    """The session is temporarily unsafe to reload; retain the newest bundle."""


class Interrupted(DeployError):
    """A catchable user interruption, rethrown only after guarded recovery."""
    def __init__(self, signum):
        self.signum = signum
        self.journal = None
        self.recovery_phase = None
        super().__init__(f'Reload interrupted by signal {signum}')


def require(value, message, kind=DeployError):
    if not value:
        raise kind(message)


def _stamp(value):
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid,
            value.st_gid, value.st_size, value.st_mtime_ns, value.st_ctime_ns]


def _directory(path):
    value = path.lstat()
    require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and not value.st_mode & 0o022 and path.resolve(strict=True) == path,
            f'Expected an owned, non-writable-by-others directory: {path}')


def _file(path, limit=LIMIT):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and not before.st_mode & 0o6022 and before.st_size <= limit,
                f'Expected a bounded owned regular file: {path}')
        digest = hashlib.sha256()
        count = 0
        while block := stream.read(1024 * 1024):
            count += len(block)
            require(count <= limit, 'File grew while being checked')
            digest.update(block)
        require(count == before.st_size and _stamp(before) ==
                _stamp(os.fstat(stream.fileno())) == _stamp(path.lstat()),
                f'File changed while being checked: {path}')
    return {'identity': _stamp(before), 'sha256': digest.hexdigest(), 'bytes': count}


def _elf(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        header = stream.read(20)
    require(len(header) == 20 and header[:7] == b'\x7fELF\x02\x01\x01'
            and int.from_bytes(header[16:18], 'little') in (2, 3),
            f'Expected a native executable ELF file: {path}')
    import platform
    machine = {'x86_64': 62, 'aarch64': 183}.get(platform.machine())
    require(machine is not None and int.from_bytes(header[18:20], 'little') == machine,
            'Bundle executable architecture does not match this machine')


def _sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def _copy(source, target, expected, mode):
    require(_file(source, expected['bytes']) == expected, f'Source changed before staging: {source}')
    src = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        dst = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
        with os.fdopen(dst, 'wb') as output, os.fdopen(src, 'rb', closefd=False) as incoming:
            count = 0
            for block in iter(lambda: incoming.read(1024 * 1024), b''):
                count += len(block)
                require(count <= expected['bytes'], 'Source grew during staging')
                output.write(block)
            output.flush()
            os.fchmod(output.fileno(), mode)
            os.fsync(output.fileno())
    finally:
        os.close(src)
    facts = _file(target, expected['bytes'])
    require(_file(source, expected['bytes']) == expected and facts['sha256'] == expected['sha256']
            and facts['bytes'] == expected['bytes'], 'Staged copy changed')
    _sync(target.parent)
    return facts


def _rename_exchange(left, right):
    libc = ctypes.CDLL(None, use_errno=True)
    rename = getattr(libc, 'renameat2', None)
    require(rename is not None, 'This Linux system does not provide checked rename exchange')
    rename.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    rename.restype = ctypes.c_int
    if rename(-100, os.fsencode(left), -100, os.fsencode(right), 2) != 0:
        number = ctypes.get_errno()
        raise OSError(number, os.strerror(number))


def _moved(actual, expected):
    # rename changes ctime; other inode metadata and all bytes must agree.
    return (actual['identity'][:-1] == expected['identity'][:-1]
            and all(actual[key] == expected[key] for key in ('sha256', 'bytes')))


def _publish(temporary, target, expected_current, expected_temporary):
    """Retain both entries and validate the one actually displaced by Linux.

    This detects a target rename between checking and exchanging. On conflict
    exchange back only while both observed entries still match; never discard
    an unexpected entry. It is not an adversarial, global filesystem CAS.
    """
    require(_file(target) == expected_current and _file(temporary) == expected_temporary,
            'Publication inputs changed')
    _rename_exchange(temporary, target)
    displaced_stamp = _stamp(temporary.lstat())
    installed = _file(target)
    require(_moved(installed, expected_temporary), 'Published entry changed; retain both files for recovery')
    try:
        displaced = _file(temporary)
    except (OSError, DeployError):
        displaced = None
    if displaced is not None and _moved(displaced, expected_current):
        _sync(target.parent)
        return installed, displaced
    require(_file(target) == installed and _stamp(temporary.lstat()) == displaced_stamp,
            'Publication conflict changed again; retain both files for recovery')
    _rename_exchange(temporary, target)
    require(_stamp(target.lstat())[:-1] == displaced_stamp[:-1]
            and _moved(_file(temporary), expected_temporary),
            'Conflict exchange-back is uncertain; retain both files for recovery')
    _sync(target.parent)
    raise DeployError('Independent target replacement was preserved; inspect the recovery journal')


def _remove_owned(path, expected):
    require(_file(path) == expected, 'Temporary entry changed; retained for manual recovery')
    path.unlink()
    _sync(path.parent)


def _journal(directory, data):
    temporary = directory / ('.journal-' + uuid.uuid4().hex)
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(data, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, directory / 'journal.json')
    _sync(directory)


@contextmanager
def _interrupts():
    previous = {}
    def interrupted(signum, _frame):
        for number in previous:
            signal.signal(number, signal.SIG_IGN)
        raise Interrupted(signum)
    try:
        for number in (signal.SIGINT, signal.SIGTERM):
            previous[number] = signal.signal(number, interrupted)
        yield
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)


@contextmanager
def _publication():
    """Do not deliver catchable interruption between rename and its receipt."""
    previous = signal.pthread_sigmask(signal.SIG_BLOCK, (signal.SIGINT, signal.SIGTERM))
    try:
        yield
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)


class _Session:
    def __init__(self):
        require(os.getuid() != 0, 'Run dev reload as the existing desktop user, without sudo')
        self.home = Path.home()
        value = os.environ.get('XDG_RUNTIME_DIR')
        require(value, 'Run inside the existing Lucent session (XDG_RUNTIME_DIR is missing)')
        self.runtime = Path(value)
        _directory(self.home)
        _directory(self.runtime)

    def command(self, *args, timeout=15):
        try:
            result = subprocess.run(args, capture_output=True, timeout=timeout, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise DeployError(f'{args[0]} is unavailable or timed out; use the running Lucent user session') from error
        require(result.returncode == 0 and len(result.stdout) <= 4 * 1024 * 1024,
                f'{args[0]} failed; check the existing Lucent user session')
        return result.stdout.decode()

    def service(self):
        names = ('ActiveState', 'SubState', 'MainPID', 'ControlPID', 'ControlGroup', 'InvocationID',
                 'NRestarts', 'ExecMainStartTimestampMonotonic')
        raw = self.command('systemctl', '--user', 'show', SERVICE,
                           *['--property=' + name for name in names])
        value = dict(line.split('=', 1) for line in raw.splitlines())
        require(set(value) == set(names), 'Cannot inspect the existing Lucent user service')
        return value

    def process(self, pid, hashed=False):
        root = Path('/proc') / str(pid)
        require(pid > 1 and root.stat().st_uid == os.getuid(), 'Lucent process is not owned by this user')
        birth = (root / 'stat').read_text().rsplit(')', 1)[1].split()[19]
        group = (root / 'cgroup').read_text()
        require(group.startswith('0::/') and group.count('\n') == 1, 'A unified owned service cgroup is required')
        executable = (root / 'exe').stat()
        require(stat.S_ISREG(executable.st_mode) and executable.st_size <= LIMIT,
                'Lucent process executable exceeds the supported bound')
        result = {'pid': pid, 'birth': birth, 'group': group[3:].strip(),
                  'executable': [executable.st_dev, executable.st_ino]}
        if hashed:
            with (root / 'exe').open('rb') as stream:
                digest = hashlib.sha256()
                count = 0
                while block := stream.read(1024 * 1024):
                    count += len(block)
                    require(count <= LIMIT, 'Lucent process executable grew beyond its bound')
                    digest.update(block)
                require(count == executable.st_size, 'Lucent executable changed during hashing')
                result['sha256'] = digest.hexdigest()
        require(birth == (root / 'stat').read_text().rsplit(')', 1)[1].split()[19]
                and result['executable'] == [(root / 'exe').stat().st_dev, (root / 'exe').stat().st_ino],
                'Lucent process changed while being inspected')
        return result

    def inspect(self, hashed=True):
        path = self.runtime / 'lucent.sock'
        before = path.lstat()
        require(stat.S_ISSOCK(before.st_mode) and before.st_uid == os.getuid(), 'Existing owned Lucent socket required')
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(2)
            client.connect(str(path))
            pid, uid, _ = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            require(uid == os.getuid(), 'Lucent inspector peer has the wrong owner')
            peer = self.process(pid, hashed=hashed)
            client.sendall(b'inspect\n')
            raw = bytearray()
            while block := client.recv(65536):
                raw.extend(block)
                require(len(raw) <= 4 * 1024 * 1024, 'Lucent inspector response exceeds its bound')
        require(_stamp(before) == _stamp(path.lstat()), 'Lucent inspector socket changed')
        require((Path('/proc') / str(pid) / 'exe').resolve() == self.home / '.local/bin/lucent-desktop',
                'Inspector peer is not the installed ordinary desktop')
        return json.loads(raw), peer

    def admission(self):
        require(self.command('omarchy-shell', 'lock', 'isLocked').strip() == 'false',
                'Unlock the screen before reloading', Deferred)
        layers = self.command('hyprctl', '-j', 'layers')
        require('lucent-authorization-' not in layers, 'Finish the authorization dialog before reloading', Deferred)
        state, peer = self.inspect()
        client = state['client']
        require(client.get('settings_writable') is True and client.get('settings_save_status') == 'saved'
                and not client.get('settings_save_error'), 'Wait for settings to save before reloading', Deferred)
        display = client.get('displays', {})
        transaction = display.get('transaction') or {}
        require(not display.get('busy') and transaction.get('phase') not in ('preparing', 'confirmation'),
                'Keep or revert the display change before reloading', Deferred)
        service = self.service()
        require(service['ActiveState'] == 'active' and int(service['MainPID']) > 1
                and re.fullmatch('[a-f0-9]{32}', service['InvocationID']),
                'An already-active Lucent user service is required; this command does not activate it')
        group = service['ControlGroup']
        require(group.startswith('/') and group.endswith('/' + SERVICE)
                and '..' not in Path(group).parts and peer['group'] == group,
                'Inspector peer does not belong to the existing Lucent service')
        supervisor = self.process(int(service['MainPID']))
        require(supervisor['group'] == group, 'Lucent supervisor cgroup changed')
        argv = (Path('/proc') / service['MainPID'] / 'cmdline').read_bytes().split(b'\0')[:-1]
        require(argv == [b'/usr/bin/python3', os.fsencode(self.home / '.local/lib/lucent/session.py')],
                'Existing Lucent supervisor is not the installed supported supervisor')
        require(desktop_rendered(state), 'The existing Lucent shell has not rendered its desktop')
        return {'service': service, 'peer': peer}

    def protected(self):
        paths = ['.local/state/lucent/desktop.json', '.config/systemd/user/lucent.service',
                 '.local/lib/lucent/session.py', '.local/lib/lucent/lucent_readiness.py',
                 '.local/bin/lucent-lock', '.local/bin/lucent-greeter', '.local/bin/lucent-polkit']
        return {name: _file(self.home / name) for name in paths}

    def preserved(self, before):
        after = self.protected()
        # Ordinary settings saves can replace the inode while keeping all bytes.
        for name, old in before.items():
            current = after[name]
            require(current['sha256'] == old['sha256'] and current['bytes'] == old['bytes']
                    and current['identity'][2:5] == old['identity'][2:5],
                    f'Protected file changed; it will not be overwritten: {name}')

    def quiescent(self, group):
        current = self.service()
        require(current['ActiveState'] in ('inactive', 'failed')
                and current['MainPID'] == current['ControlPID'] == '0', 'Lucent service has not stopped')
        require(self.drained(group), 'Lucent service still owns running children')

    def drained(self, group):
        require(group.startswith('/') and group.endswith('/' + SERVICE)
                and '..' not in Path(group).parts, 'Invalid captured Lucent cgroup')
        events = CGROUP_ROOT / group.lstrip('/') / 'cgroup.events'
        return not events.exists() or 'populated 0' in events.read_text().splitlines()

    def stop(self, expected, *, drain_group=None):
        # systemd can clear ControlGroup while an owned restart is pending.
        # The captured live invocation's group remains the drain authority.
        group = expected['ControlGroup'] if drain_group is None else drain_group
        require(group.startswith('/') and group.endswith('/' + SERVICE)
                and '..' not in Path(group).parts, 'A nonempty captured Lucent cgroup is required before stopping')
        require(self.service() == expected, 'Lucent service changed before stop; refusing to stop a new invocation')
        try:
            self.command('systemctl', '--user', 'stop', SERVICE, timeout=25)
        finally:
            self.quiescent(group)

    def start(self):
        self.command('systemctl', '--user', 'start', SERVICE, timeout=20)
        return self.service()

    def ready(self, sha, started, timeout):
        deadline = time.monotonic() + timeout
        service = self.service()
        peer = None
        while time.monotonic() < deadline:
            require(self.service() == service and service['ActiveState'] == 'active',
                    'Lucent service changed or restarted during readiness')
            try:
                state, observed = self.inspect(hashed=peer is None)
            except (OSError, ValueError):
                time.sleep(.2)
                continue
            require(observed.get('sha256', sha) == sha and observed['group'] == service['ControlGroup'],
                    'The new inspector peer is not the requested desktop binary')
            if peer is None:
                peer = observed
            require(all(observed[key] == peer[key] for key in observed), 'Desktop process changed during readiness')
            require(not state['client'].get('theme_error'), 'Desktop reported a theme error')
            if desktop_rendered(state) and time.monotonic() - started >= 35:
                return {'service': service, 'peer': peer}
            time.sleep(.2)
        raise DeployError('Desktop did not render and survive beyond its startup deadline')


def preflight():
    """Read-only admission; ordinary running application windows are allowed."""
    try:
        session = _Session()
        snapshot = session.admission()
        session.protected()
        return {'ready': True, 'desktop_pid': snapshot['peer']['pid'],
                'desktop_sha256': snapshot['peer']['sha256'],
                'service_invocation': snapshot['service']['InvocationID']}
    except (OSError, ValueError, KeyError) as error:
        raise DeployError('Cannot inspect the installed Lucent session; run doctor inside that session') from error


def _apply(bundle, manifest, session, ready_timeout):
    from .model import BINARY_NAMES, SUPPORT_FILES
    before = session.admission()
    protected = session.protected()
    entries = []
    for name in BINARY_NAMES:
        entries.append((bundle / name, session.home / '.local/bin' / name, manifest['binaries'][name]))
    for name in SUPPORT_FILES:
        entries.append((bundle / 'support' / name,
                        session.home / '.local/lib/lucent' / name.removeprefix('configs/lucent/'),
                        manifest['support_files'][name]))
    root = session.home / '.local/state/lucent/dev-reload'
    _directory(root.parent)
    root.mkdir(mode=0o700, exist_ok=True)
    _directory(root)
    require(stat.S_IMODE(root.stat().st_mode) == 0o700, 'Dev reload state directory must have mode0700')
    transaction = root / uuid.uuid4().hex
    transaction.mkdir(mode=0o700)
    journal = {'format': 1, 'phase': 'staging', 'source_commit': manifest['source_commit'],
               'source_dirty': manifest['source_dirty'], 'profile': manifest['profile'], 'files': [],
               'rollback_complete': False, 'error': None,
               'candidate_start_attempt_us': None, 'candidate_invocations': []}
    stopped = stop_attempted = False

    def candidate_authority(current):
        # A watchdog restart advances NRestarts. A same-cohort manual successor
        # does not gain authority merely by using the same service name/cgroup.
        session.preserved(protected)
        for name, facts in protected.items():
            if name.endswith(('/session.py', '/lucent.service')):
                require(_file(session.home / name) == facts, 'Supervisor or unit changed; no rollback stop')
        for item in journal['files']:
            require(_file(Path(item['target'])) == (item['installed'] or item['original']),
                    'Independent file edit; no rollback stop or overwrite')
        pending = (current['ActiveState'] == 'activating' and current.get('SubState') == 'auto-restart'
                   and current['MainPID'] == current['ControlPID'] == '0')
        group = before['service']['ControlGroup']
        same_group = current['ControlGroup'] == group
        retired_group = pending and current['ControlGroup'] == '' and session.drained(group)
        require((current['ActiveState'] == 'active' and int(current['MainPID']) > 1 or pending)
                and (same_group or retired_group)
                and re.fullmatch('[a-f0-9]{32}', current['InvocationID']),
                'Candidate service identity changed; no rollback stop')
        pair = {'invocation': current['InvocationID'], 'restarts': int(current['NRestarts'])}
        previous = journal['candidate_invocations']
        if previous:
            require(pair == previous[-1] or (pair['invocation'] != previous[-1]['invocation']
                    and pair['restarts'] > previous[-1]['restarts']),
                    'Independent service successor; no rollback stop')
        else:
            attempt = journal['candidate_start_attempt_us']
            require(attempt is not None and attempt <= int(current['ExecMainStartTimestampMonotonic'])
                    <= time.monotonic_ns() // 1000,
                    'First start reply is uncertain; inspect the recovery journal')
        if not previous or pair != previous[-1]:
            previous.append(pair)
            _journal(transaction, journal)

    def recovery_record():
        # An exhausted/full filesystem must not prevent trying the already
        # authorized in-memory rollback using its durable previous-file copies.
        try:
            _journal(transaction, journal)
        except OSError:
            journal['recovery_journal_write_failed'] = True

    with _interrupts():
        try:
            for index, (source, target, expected) in enumerate(entries):
                _directory(target.parent)
                original = _file(target, LIMIT if index < len(BINARY_NAMES) else 4 * 1024 * 1024)
                candidate = _file(source, expected['bytes'])
                require(all(candidate[key] == expected[key] for key in ('sha256', 'bytes')), 'Bundle changed before staging')
                if index < len(BINARY_NAMES):
                    _elf(source)
                    require(original['identity'][2] & 0o111, 'Installed ordinary binary is not executable')
                backup = transaction / f'backup-{index}'
                temporary = target.parent / ('.lucent-dev-' + transaction.name + '-' + str(index))
                item = {'target': str(target), 'backup': str(backup), 'temporary': str(temporary),
                        'original': original, 'staged': None, 'installed': None, 'backup_facts': None}
                journal['files'].append(item)
                _journal(transaction, journal)
                item['backup_facts'] = _copy(target, backup, original, stat.S_IMODE(original['identity'][2]))
                staged = _copy(source, temporary, candidate, stat.S_IMODE(original['identity'][2]))
                item['staged'] = staged
                _journal(transaction, journal)
            require(session.admission() == before, 'Lucent changed during staging; rebuild/retry the reload')
            session.preserved(protected)
            journal['phase'] = 'stopping'
            _journal(transaction, journal)
            stop_attempted = True
            session.stop(before['service'])
            stopped = True
            for item in journal['files']:
                session.quiescent(before['service']['ControlGroup'])
                target, temporary = Path(item['target']), Path(item['temporary'])
                require(_file(target) == item['original'] and _file(temporary) == item['staged'],
                        'Installed file changed; refusing to overwrite an independent edit')
                with _publication():
                    item['installed'], item['displaced'] = _publish(temporary, target, item['original'], item['staged'])
                    _journal(transaction, journal)
            journal['phase'] = 'starting'
            journal['candidate_start_attempt_us'] = time.monotonic_ns() // 1000
            _journal(transaction, journal)
            session.preserved(protected)
            candidate_authority(session.start())
            started = time.monotonic()
            ready = session.ready(manifest['binaries']['lucent-desktop']['sha256'], started, ready_timeout)
            session.preserved(protected)
            for item in journal['files']:
                _remove_owned(Path(item['temporary']), item['displaced'])
            journal['phase'] = 'complete'
            _journal(transaction, journal)
            return {'success': True, 'source_commit': manifest['source_commit'],
                    'source_dirty': manifest['source_dirty'], 'profile': manifest['profile'],
                    'journal': str(transaction / 'journal.json'), 'desktop_pid': ready['peer']['pid']}
        except BaseException as error:
            for number in (signal.SIGINT, signal.SIGTERM):
                signal.signal(number, signal.SIG_IGN)
            journal['error'] = type(error).__name__
            journal['error_message'] = str(error)[:2048]
            journal['failure_phase'] = journal['phase']
            journal['phase'] = 'rolling-back'
            recovery_record()
            try:
                journal['rollback_step'] = 'inspect-service'
                current = session.service()
                if stop_attempted and not stopped and current['ActiveState'] in ('inactive', 'failed'):
                    require(current['InvocationID'] in ('', '0' * 32, before['service']['InvocationID'])
                            and current['ExecMainStartTimestampMonotonic'] == before['service']['ExecMainStartTimestampMonotonic'],
                            'Original stop reply is uncertain; an independent successor was observed')
                    session.quiescent(before['service']['ControlGroup'])
                    stopped = True
                if stopped and current['ActiveState'] not in ('inactive', 'failed'):
                    candidate_authority(current)
                    journal['rollback_step'] = 'stop-candidate'
                    session.stop(current, drain_group=before['service']['ControlGroup'])
                elif stop_attempted and not stopped:
                    require(current == before['service'], 'Stop completion is uncertain; inspect the recovery journal')
                if stopped:
                    journal['rollback_step'] = 'prove-stopped'
                    session.quiescent(before['service']['ControlGroup'])
                for item in reversed(journal['files']):
                    journal['rollback_step'] = 'restore-files'
                    journal['rollback_target'] = item['target']
                    target = Path(item['target'])
                    if item['installed'] is None:
                        require(_file(target) == item['original'], 'Uncertain replacement; keep the journal for manual recovery')
                        continue
                    require(stopped and _file(target) == item['installed'], 'Independent target edit; rollback will not overwrite it')
                    restore = target.parent / ('.lucent-restore-' + transaction.name)
                    _copy(Path(item['backup']), restore, item['backup_facts'],
                          stat.S_IMODE(item['original']['identity'][2]))
                    require(_file(restore)['sha256'] == item['original']['sha256']
                            and _file(target) == item['installed'], 'Restore or current target changed')
                    with _publication():
                        item['restored'], old_candidate = _publish(restore, target, item['installed'], _file(restore))
                        recovery_record()
                        _remove_owned(restore, old_candidate)
                journal['rollback_step'] = 'verify-restored-cohort'
                for item in journal['files']:
                    require(_file(Path(item['target'])) == item.get('restored', item['original']),
                            'Restored cohort changed; old shell will not be started')
                    temporary = Path(item['temporary'])
                    if item['staged'] is not None and temporary.exists():
                        _remove_owned(temporary, item.get('displaced', item['staged']))
                if stopped:
                    journal['rollback_step'] = 'start-original'
                    session.preserved(protected)
                    session.start()
                    started = time.monotonic()
                    journal['rollback_step'] = 'original-readiness'
                    session.ready(before['peer']['sha256'], started, ready_timeout)
                if stop_attempted:
                    journal['rollback_step'] = 'verify-protected'
                    session.preserved(protected)
                journal['rollback_complete'] = True
                journal['rollback_step'] = 'complete'
                journal['phase'] = 'rolled-back'
            except BaseException as recovery:
                journal['phase'] = 'recovery-required'
                journal['rollback_error'] = type(recovery).__name__
                journal['rollback_error_message'] = str(recovery)[:2048]
            recovery_record()
            if isinstance(error, Interrupted):
                error.journal = str(transaction / 'journal.json')
                error.recovery_phase = journal['phase']
                error.args = (f'Reload interrupted; {journal["phase"]}. Recovery journal: {error.journal}',)
                raise error
            if isinstance(error, Deferred) and journal['rollback_complete']:
                raise
            raise DeployError(f'Reload failed ({error}); {journal["phase"]}. Recovery journal: {transaction / "journal.json"}') from error


def apply_bundle(bundle_dir: Path, *, ready_timeout=45):
    """Apply under the CLI-owned exclusive lock; retain all recovery receipts."""
    from .model import load_bundle
    require(type(ready_timeout) in (int, float) and 35 <= ready_timeout <= 180,
            'Readiness timeout must be between35 and180seconds')
    try:
        bundle = Path(bundle_dir).absolute()
        manifest = load_bundle(bundle)
        return _apply(bundle, manifest, _Session(), ready_timeout)
    except (OSError, ValueError, KeyError) as error:
        raise DeployError(f'Cannot load or inspect this reload bundle/session: {error}') from error
