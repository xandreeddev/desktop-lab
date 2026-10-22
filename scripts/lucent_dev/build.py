"""One bounded build unit and immutable, source-checked output bundles."""
import argparse
import json
import os
from pathlib import Path
import secrets
import re
import shutil
import subprocess
import sys
import tempfile
import time
from .model import (BINARY_NAMES, SUPPORT_FILES, DevError, file_digest,
                    load_bundle, private_directory)
from .watch import source_digest


def source_info(root):
    def git(*args):
        return subprocess.check_output(['git', '-C', str(root), *args], timeout=10, text=True).strip()
    if Path(git('rev-parse', '--show-toplevel')).resolve() != root:
        raise DevError('--root must identify a Git workspace root')
    return git('rev-parse', 'HEAD'), bool(git('status', '--porcelain', '--untracked-files=normal'))


def cargo_command(root, profile, cargo='cargo'):
    args = [cargo, 'build', '--manifest-path', str(root / 'lucent/Cargo.toml'), '--locked', '--jobs', '1']
    if profile == 'release':
        args.append('--release')
    for name in BINARY_NAMES:
        args.extend(['-p', name])
    return args


def generate(root, run=subprocess.run):
    for name in ('generate-design-tokens.py', 'generate-app-icons.py'):
        command = [sys.executable, str(root / 'scripts' / name)]
        if run([*command, '--check'], cwd=root, check=False).returncode:
            run(command, cwd=root, check=True)


def publish_bundle(root, target, profile, output, run=subprocess.run, check_cancelled=lambda: None):
    """Called inside the bounded unit, never from a watching thread."""
    root, target = Path(root).resolve(), Path(target).resolve()
    if profile not in ('release', 'debug') or not (root / 'lucent/Cargo.toml').is_file():
        raise DevError('Unsupported build profile or missing Lucent workspace')
    output = private_directory(output)
    check_cancelled()
    generate(root, run)
    source_commit, source_dirty = source_info(root)
    inputs = source_digest(root, (target, output))
    check_cancelled()
    run(cargo_command(root, profile, os.environ.get('LUCENT_DEV_CARGO', 'cargo')), cwd=root, check=True)
    check_cancelled()
    if source_digest(root, (target, output)) != inputs or source_info(root) != (source_commit, source_dirty):
        raise DevError('Source changed during build; this result will not be applied')
    tag = secrets.token_hex(12)
    temporary = output / ('.building-' + tag)
    temporary.mkdir(mode=0o700)
    manifest = {'format': 1, 'source_commit': source_commit, 'source_dirty': source_dirty,
                'profile': profile, 'binaries': {}, 'support_files': {}}
    try:
        for key, names, origin, destination in (
                ('binaries', BINARY_NAMES, target / profile, temporary),
                ('support_files', SUPPORT_FILES, root, temporary / 'support')):
            for name in names:
                source, copied = origin / name, destination / name
                expected = file_digest(source)
                copied.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                with source.open('rb') as reader, copied.open('xb') as writer:
                    shutil.copyfileobj(reader, writer, 1024 * 1024)
                    writer.flush()
                    os.fsync(writer.fileno())
                copied.chmod(0o500 if key == 'binaries' else 0o400)
                if file_digest(copied) != expected:
                    raise DevError('Build output changed during bundle copy')
                manifest[key][name] = expected
        (temporary / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        (temporary / 'manifest.json').chmod(0o400)
        load_bundle(temporary)
        if source_digest(root, (target, output)) != inputs or source_info(root) != (source_commit, source_dirty):
            raise DevError('Source changed during bundle copy; this result will not be applied')
        check_cancelled()
        for directory, _, _ in os.walk(temporary, topdown=False):
            Path(directory).chmod(0o500)
        destination = output / (source_commit[:12] + '-' + tag)
        temporary.rename(destination)
        return {'bundle': str(destination), 'inputs': inputs}
    finally:
        if temporary.exists():
            for directory, _, _ in os.walk(temporary):
                Path(directory).chmod(0o700)
            shutil.rmtree(temporary)


class BuildJob:
    def __init__(self, root, target, profile, output):
        self.unit = 'lucent-dev-build-' + secrets.token_hex(12) + '.service'
        runtime = private_directory(Path(os.environ['XDG_RUNTIME_DIR']), create=False)
        self.control = Path(tempfile.mkdtemp(prefix='lucent-dev-build-', dir=runtime))
        self.result = self.control / 'result.json'
        module_root = Path(__file__).resolve().parents[1]
        cargo = shutil.which('cargo')
        if not cargo:
            shutil.rmtree(self.control)
            raise DevError('Cargo is required on the build machine')
        env = dict(os.environ, PYTHONPATH=str(module_root), CARGO_TARGET_DIR=str(target),
                   CARGO_BUILD_JOBS='1', LUCENT_DEV_CARGO=cargo, LUCENT_DEV_BUILD_UNIT=self.unit)
        command = ['systemd-run', '--user', '--quiet', '--wait', '--pipe', '--collect',
                   '--service-type=exec', '--unit=' + self.unit, '--working-directory=' + str(root),
                   '--property=CPUQuota=100%', '--property=MemoryHigh=900M',
                   '--property=MemoryMax=1400M', '--property=TasksMax=256',
                   '--property=RuntimeMaxSec=1800', '--property=TimeoutStopSec=15',
                   '--property=KillMode=control-group', '--property=Restart=no']
        for name in ('PATH', 'HOME', 'PYTHONPATH', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS',
                     'LUCENT_DEV_CARGO', 'LUCENT_DEV_BUILD_UNIT', 'CARGO_HOME', 'RUSTUP_HOME', 'RUSTUP_TOOLCHAIN',
                     'CC', 'CXX', 'PKG_CONFIG_PATH', 'LIBCLANG_PATH'):
            if name in env:
                command.append('--setenv=' + name)
        command.extend(['--', sys.executable, '-m', 'lucent_dev.build', '--root', str(root),
                        '--target', str(target), '--profile', profile, '--output', str(output),
                        '--result', str(self.result)])
        try:
            self.process = subprocess.Popen(command, env=env, start_new_session=True)
            self.fd = os.pidfd_open(self.process.pid)
        except BaseException:
            if hasattr(self, 'process'):
                self.cancel()
            shutil.rmtree(self.control, ignore_errors=True)
            raise

    def fileno(self):
        return self.fd

    def finish(self):
        if self.process.wait() != 0:
            raise DevError('Build failed; the running shell was not changed')
        file_digest(self.result, 16384)
        result = json.loads(self.result.read_text())
        load_bundle(Path(result['bundle']))
        return result

    def cancel(self):
        # Revoke worker admission before ending its caller. A delayed start
        # cannot begin Cargo after cancellation, even before a receipt exists.
        try:
            fd = os.open(self.control / 'cancelled', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            os.close(fd)
        except FileExistsError:
            marker = (self.control / 'cancelled').lstat()
            if marker.st_uid != os.getuid() or marker.st_size or marker.st_mode & 0o077:
                raise DevError('Build cancellation marker changed')
        # End the sole StartTransientUnit caller first, then stop its exact unit.
        # Neither command targets lucent.service or application units.
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        deadline = time.monotonic() + 3
        while True:
            state = self._state()
            if self._terminal(state):
                return
            started = self.control / 'started.json'
            if started.exists():
                file_digest(started, 16384)
                owner = json.loads(started.read_text())
                if (owner.get('unit') != self.unit or owner.get('invocation_id') != state.get('InvocationID')
                        or str(owner.get('pid')) != state.get('MainPID')
                        or process_birth(owner['pid']) != owner.get('birth')):
                    raise DevError('Build unit ownership changed; refusing to stop a replacement')
                # Repeat identity immediately before the stop, never adopt a
                # successor invocation merely because its unit name matches.
                current = self._state()
                if self._terminal(current):
                    return
                if (current.get('InvocationID') != owner['invocation_id']
                        or current.get('MainPID') != str(owner['pid'])
                        or process_birth(owner['pid']) != owner['birth']):
                    raise DevError('Build unit changed before cancellation')
                subprocess.run(['systemctl', '--user', 'stop', self.unit], timeout=25, check=True)
                if not self._terminal(self._state()):
                    raise DevError('Owned build did not become quiescent')
                return
            if time.monotonic() >= deadline:
                raise DevError('Build startup ownership is uncertain; runtime limit remains in force')
            time.sleep(.05)

    def _state(self):
        value = subprocess.run(['systemctl', '--user', 'show', self.unit, '--no-pager',
                                '-p', 'LoadState', '-p', 'ActiveState', '-p', 'MainPID',
                                '-p', 'ControlPID', '-p', 'InvocationID', '-p', 'Job'],
                               timeout=5, check=False, capture_output=True, text=True)
        if len(value.stdout) > 16384:
            raise DevError('Build unit observation exceeded its bound')
        result = dict(line.split('=', 1) for line in value.stdout.splitlines() if '=' in line)
        if result.get('LoadState') not in ('loaded', 'not-found'):
            raise DevError('Could not observe the owned build unit')
        return result

    @staticmethod
    def _terminal(state):
        return state.get('LoadState') == 'not-found' or (
            state.get('ActiveState') in ('inactive', 'failed') and state.get('MainPID') == '0'
            and state.get('ControlPID') == '0' and state.get('Job') in ('', '0'))

    def close(self):
        if hasattr(self, 'fd'):
            os.close(self.fd)
        shutil.rmtree(self.control, ignore_errors=True)


def process_birth(pid):
    return (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()[19]


def worker():
    parser = argparse.ArgumentParser()
    for name in ('root', 'target', 'output', 'result'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--profile', choices=('release', 'debug'), required=True)
    args = parser.parse_args()
    unit, invocation = os.environ.get('LUCENT_DEV_BUILD_UNIT', ''), os.environ.get('INVOCATION_ID', '')
    if not re.fullmatch('lucent-dev-build-[0-9a-f]{24}\\.service', unit) or not re.fullmatch('[0-9a-f]{32}', invocation):
        raise DevError('This worker must run inside its owned bounded build unit')
    control = private_directory(args.result.parent, create=False)
    identity = control.stat().st_dev, control.stat().st_ino
    def check_cancelled():
        current = control.lstat()
        try:
            (control / 'cancelled').lstat()
            cancelled = True
        except FileNotFoundError:
            cancelled = False
        if (current.st_dev, current.st_ino) != identity or cancelled:
            raise DevError('Development build was cancelled')
    owner = {'unit': unit, 'invocation_id': invocation, 'pid': os.getpid(), 'birth': process_birth(os.getpid())}
    temporary = args.result.parent / 'started.tmp'
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as stream:
        json.dump(owner, stream)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.rename(args.result.parent / 'started.json')
    result = publish_bundle(args.root, args.target, args.profile, args.output, check_cancelled=check_cancelled)
    with os.fdopen(os.open(args.result, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as stream:
        json.dump(result, stream)


if __name__ == '__main__':
    worker()
