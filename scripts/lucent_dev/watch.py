"""Linux source notifications; no background polling or build threads."""
import ctypes
import hashlib
import os
from pathlib import Path
import struct
import stat
import time
from .model import DevError

ROOTS = ('lucent', 'design', 'configs/lucent')
GENERATORS = ('scripts/generate-design-tokens.py', 'scripts/generate-app-icons.py')
EXCLUDED = {'.git', '.cache', 'target', 'node_modules', '__pycache__', 'site', 'docs', 'reports'}
MASK = 0x2 | 0x4 | 0x8 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400 | 0x800
DIRECTORY = 0x40000000
OVERFLOW = 0x4000
IGNORED = 0x8000


def included(relative):
    relative = Path(relative)
    if EXCLUDED.intersection(relative.parts) or relative.parts[:4] == ('lucent', 'tests', 'visual', 'baselines'):
        return False
    text = relative.as_posix()
    return text in GENERATORS or any(text == root or text.startswith(root + '/') for root in ROOTS)


def source_paths(root, ignored=()):
    root = Path(root)
    ignored = tuple(Path(path).resolve() for path in ignored)
    for relative in ROOTS:
        start = root / relative
        if start.resolve() != start:
            raise DevError(f'Symlinked development directories are unsupported: {start}')
        if not start.is_dir():
            continue
        for directory, dirs, files in os.walk(start, followlinks=False):
            for name in dirs:
                if included((Path(directory) / name).relative_to(root)) and (Path(directory) / name).is_symlink():
                    raise DevError(f'Symlinked development directories are unsupported: {Path(directory) / name}')
            dirs[:] = sorted(name for name in dirs if included((Path(directory) / name).relative_to(root))
                              and not (Path(directory) / name).is_symlink()
                              and not any((Path(directory) / name).resolve().is_relative_to(path) for path in ignored))
            for name in sorted(files):
                path = Path(directory) / name
                if included(path.relative_to(root)) and not any(path.resolve().is_relative_to(p) for p in ignored):
                    yield path
    for name in GENERATORS:
        if (root / name).exists():
            yield root / name


def source_digest(root, ignored=()):
    digest = hashlib.sha256()
    try:
        for path in sorted(source_paths(root, ignored)):
            if path.resolve() != path:
                raise DevError(f'Symlinked development inputs are unsupported: {path}')
            with os.fdopen(os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW), 'rb') as stream:
                before = os.fstat(stream.fileno())
                if not stat.S_ISREG(before.st_mode) or before.st_size > 512 * 1024 * 1024:
                    raise DevError('Development inputs must be regular files of at most 512 MiB')
                value = hashlib.file_digest(stream, 'sha256').digest()
            after = path.stat()
            if (before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                    after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                raise DevError('Development inputs changed while reading')
            digest.update(path.relative_to(root).as_posix().encode() + b'\0' + value)
    except FileNotFoundError as error:
        raise DevError('Development inputs changed while reading') from error
    return digest.hexdigest()


class Debounce:
    def __init__(self, seconds=.35):
        self.seconds = seconds
        self.dirty = True
        self.deadline = 0.

    def changed(self, now=None):
        self.dirty = True
        self.deadline = (time.monotonic() if now is None else now) + self.seconds

    def ready(self, now):
        return self.dirty and now >= self.deadline

    def begin(self):
        self.dirty = False


class Watcher:
    def __init__(self, root, ignored=()):
        self.root = Path(root).resolve()
        self.ignored = tuple(Path(path).resolve() for path in ignored)
        self.libc = ctypes.CDLL(None, use_errno=True)
        self.libc.inotify_init1.argtypes = [ctypes.c_int]
        self.libc.inotify_init1.restype = ctypes.c_int
        self.libc.inotify_add_watch.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_uint32]
        self.libc.inotify_add_watch.restype = ctypes.c_int
        self.libc.inotify_rm_watch.argtypes = [ctypes.c_int, ctypes.c_int]
        self.libc.inotify_rm_watch.restype = ctypes.c_int
        self.fd = self.libc.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
        if self.fd < 0:
            raise OSError(ctypes.get_errno(), 'inotify_init1')
        self.paths = {}
        try:
            self.rescan()
        except BaseException:
            self.close()
            raise

    def fileno(self):
        return self.fd

    def rescan(self):
        if (self.root / 'scripts').resolve() != self.root / 'scripts':
            raise DevError('Symlinked development directories are unsupported')
        wanted = {self.root, self.root / 'scripts', self.root / 'configs'}
        for relative in ROOTS:
            start = self.root / relative
            if start.resolve() != start:
                raise DevError(f'Symlinked development directories are unsupported: {start}')
            if start.is_dir():
                for directory, dirs, _ in os.walk(start, followlinks=False):
                    wanted.add(Path(directory))
                    for name in dirs:
                        if included((Path(directory) / name).relative_to(self.root)) and (Path(directory) / name).is_symlink():
                            raise DevError('Symlinked development directories are unsupported')
                    dirs[:] = [name for name in dirs if included((Path(directory) / name).relative_to(self.root))
                               and not (Path(directory) / name).is_symlink()
                               and not any((Path(directory) / name).resolve().is_relative_to(p) for p in self.ignored)]
        wanted = {path for path in wanted if path.is_dir() and not path.is_symlink()}
        if len(wanted) > 32768:
            raise DevError('Development watch exceeds 32768 directories')
        for wd, path in list(self.paths.items()):
            if path not in wanted:
                self.libc.inotify_rm_watch(self.fd, wd)
                del self.paths[wd]
        for path in sorted(wanted - set(self.paths.values())):
            wd = self.libc.inotify_add_watch(self.fd, os.fsencode(path), MASK)
            if wd < 0:
                raise OSError(ctypes.get_errno(), f'Cannot watch source directory {path}')
            self.paths[wd] = path

    def read(self):
        changed = set()
        rescan = False
        while True:
            try:
                raw = os.read(self.fd, 65536)
            except BlockingIOError:
                break
            if not raw:
                break
            offset = 0
            while offset < len(raw):
                wd, mask, _, size = struct.unpack_from('iIII', raw, offset)
                name = os.fsdecode(raw[offset + 16:offset + 16 + size].split(b'\0', 1)[0])
                offset += 16 + size
                if mask & OVERFLOW:
                    changed.add('source watch overflow: full rebuild required')
                    rescan = True
                elif mask & IGNORED:
                    self.paths.pop(wd, None)
                    rescan = True
                elif wd in self.paths:
                    path = self.paths[wd] / name
                    relative = path.relative_to(self.root)
                    if included(relative) and not any(path.resolve().is_relative_to(p) for p in self.ignored):
                        changed.add(relative.as_posix())
                    if mask & DIRECTORY or not name:
                        rescan = True
        if rescan:
            self.rescan()
        return changed

    def close(self):
        if self.fd >= 0:
            os.close(self.fd)
            self.fd = -1

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
