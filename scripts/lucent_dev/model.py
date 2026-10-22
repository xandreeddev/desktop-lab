"""The small, checked bundle shared by build and deployment."""
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat

BINARY_NAMES = ('lucent-desktop', 'lucent-cli', 'lucent-menu')
SUPPORT_FILES = ('configs/lucent/palettes.json', *(
    f'configs/lucent/themes/{theme}/{name}'
    for theme in ('dark', 'light') for name in ('colors.toml', 'gtk.css', 'hyprlock.conf')))
MAX_BINARY = 512 * 1024 * 1024
MAX_SUPPORT = 4 * 1024 * 1024


class DevError(RuntimeError):
    pass


def file_digest(path, limit=MAX_BINARY):
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_uid != os.getuid() or before.st_mode & 0o022:
            raise DevError(f'Expected an owned regular non-writable-by-others file: {path}')
        if not 0 < before.st_size <= limit:
            raise DevError(f'File exceeds the supported size: {path}')
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        after = os.fstat(stream.fileno())
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise DevError(f'File changed while reading: {path}')
    return {'sha256': digest, 'bytes': before.st_size}


def private_directory(path, *, create=True):
    path = Path(path).absolute()
    if path.resolve() != path:
        raise DevError(f'Symlinked private directories are unsupported: {path}')
    if create:
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    value = path.lstat()
    if (path.resolve() != path or not stat.S_ISDIR(value.st_mode)
            or value.st_uid != os.getuid() or value.st_mode & 0o077):
        raise DevError(f'Expected a private owned directory: {path}')
    return path


def validate_manifest(value):
    if not isinstance(value, dict) or value.get('format') != 1 or type(value['format']) is not int:
        raise DevError('Unsupported development bundle format')
    if not re.fullmatch('[0-9a-f]{40}', str(value.get('source_commit', ''))):
        raise DevError('Bundle needs a full Git source identity')
    if type(value.get('source_dirty')) is not bool or value.get('profile') not in ('release', 'debug'):
        raise DevError('Invalid bundle source/profile metadata')
    for key, expected, limit in (
            ('binaries', BINARY_NAMES, MAX_BINARY), ('support_files', SUPPORT_FILES, MAX_SUPPORT)):
        entries = value.get(key)
        if not isinstance(entries, dict) or set(entries) != set(expected):
            raise DevError(f'Bundle must contain the exact supported {key}')
        for entry in entries.values():
            if (not isinstance(entry, dict) or set(entry) != {'sha256', 'bytes'}
                    or not re.fullmatch('[0-9a-f]{64}', str(entry.get('sha256', '')))
                    or type(entry.get('bytes')) is not int or not 0 < entry['bytes'] <= limit):
                raise DevError(f'Invalid {key} fingerprint')
    return value


def load_bundle(bundle):
    bundle = Path(bundle).absolute()
    value = bundle.lstat()
    if (bundle.resolve() != bundle or not stat.S_ISDIR(value.st_mode)
            or value.st_uid != os.getuid() or value.st_mode & 0o077):
        raise DevError('Bundle must be a canonical private owned directory')
    with os.fdopen(os.open(bundle / 'manifest.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_mode & 0o022 or not 0 < info.st_size <= 128 * 1024):
            raise DevError('Unsafe development manifest')
        raw = stream.read(128 * 1024 + 1)
        after = os.fstat(stream.fileno())
        if len(raw) != info.st_size or (info.st_dev, info.st_ino, info.st_mode, info.st_uid,
                info.st_size, info.st_mtime_ns, info.st_ctime_ns) != (after.st_dev, after.st_ino,
                after.st_mode, after.st_uid, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise DevError('Development manifest changed while reading')
    manifest = validate_manifest(json.loads(raw))
    for key, prefix, limit in (('binaries', bundle, MAX_BINARY),
                               ('support_files', bundle / 'support', MAX_SUPPORT)):
        for relative, expected in manifest[key].items():
            path = prefix / relative
            if path.resolve() != path or file_digest(path, limit) != expected:
                raise DevError(f'Bundle payload does not match its manifest: {relative}')
            if key == 'binaries' and not path.stat().st_mode & stat.S_IXUSR:
                raise DevError(f'Bundle executable is not executable: {relative}')
    return manifest


@contextmanager
def mutation_lease():
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR', ''))
    if not runtime.is_absolute():
        raise DevError('XDG_RUNTIME_DIR must identify the current user runtime directory')
    if not runtime.exists():
        raise DevError('The user runtime directory does not exist')
    private_directory(runtime, create=False)
    fd = os.open(runtime / 'lucent-dev.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or info.st_mode & 0o077:
            raise DevError('Unsafe development lock file')
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise DevError('Another Lucent development command is already active') from error
        yield
    finally:
        os.close(fd)
