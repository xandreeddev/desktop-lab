"""Private bounded clipboard history. No rendering or command execution."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import time

ROOT = Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state')))
if not ROOT.is_absolute():
    ROOT = Path.home() / '.local/state'
ROOT = ROOT / 'lucent/clipboard'
LIMIT = 100
BYTES = 2 * 1024 * 1024
PNG = b'\x89PNG\r\n\x1a\n'


class HistoryError(ValueError):
    """Saved history cannot be used; only an explicit reset may discard it."""


def _identifier(mime, data):
    return hashlib.sha256(mime.encode() + b'\0' + data).hexdigest()


def _validate(items):
    if not isinstance(items, list) or len(items) > LIMIT:
        raise HistoryError('Invalid clipboard history')
    identifiers = set()
    for item in items:
        if not isinstance(item, dict):
            raise HistoryError('Invalid clipboard history')
        kind = item.get('type')
        fields = {'type', 'id', 'capturedAt', 'text' if kind == 'text' else 'path'}
        identifier = item.get('id')
        if (kind not in ('text', 'image') or set(item) != fields
                or not isinstance(identifier, str)
                or re.fullmatch(r'[0-9a-f]{64}', identifier) is None
                or identifier in identifiers
                or type(item.get('capturedAt')) is not int
                or not 0 <= item['capturedAt'] <= 2**63 - 1):
            raise HistoryError('Invalid clipboard history')
        identifiers.add(identifier)
        if kind == 'text':
            text = item['text']
            if (not isinstance(text, str) or not text
                    or _identifier('text', text.encode('utf-8')) != identifier):
                raise HistoryError('Invalid clipboard history')
        elif item['path'] != str(ROOT / (identifier + '.png')):
            raise HistoryError('Invalid clipboard image reference')


def _read_bytes(path):
    # A damaged symlink/FIFO must not read another file or block the picker.
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise HistoryError('Invalid clipboard file')
        data = stream.read(BYTES + 1)
    if len(data) > BYTES:
        raise HistoryError('Clipboard file exceeds its byte limit')
    return data


def read():
    try:
        data = _read_bytes(ROOT / 'history.json')
    except FileNotFoundError:
        return []
    except (OSError, HistoryError):
        raise HistoryError('Saved clipboard history could not be read') from None
    try:
        items = json.loads(data.decode('utf-8'))
        _validate(items)
        return items
    except (ValueError, UnicodeError, RecursionError):
        # Never put saved content or parser excerpts into UI messages/logs.
        raise HistoryError('Saved clipboard history could not be read') from None


def image_data(item):
    """Revalidate the selected image after the picker releases its surface."""
    try:
        _validate([item])
        if item['type'] != 'image':
            raise HistoryError('Not an image')
        data = _read_bytes(ROOT / (item['id'] + '.png'))
        if not data.startswith(PNG) or _identifier('image/png', data) != item['id']:
            raise HistoryError('Invalid clipboard image')
        return data
    except (OSError, ValueError, UnicodeError):
        raise HistoryError('Saved clipboard image is unavailable') from None


def _encode(items):
    # Admission and writing use the same bytes, including JSON escapes.
    return json.dumps(items, separators=(',', ':'), ensure_ascii=True).encode('utf-8')


def _replace(path, data):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='wb', dir=ROOT, delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _write_history(data):
    if len(data) > BYTES:
        raise HistoryError('Clipboard history exceeds its byte limit')
    _replace(ROOT / 'history.json', data)


def write(items):
    _validate(items)
    _write_history(_encode(items))


def prune(items):
    live = {item['id'] + '.png' for item in items if item['type'] == 'image'}
    for path in ROOT.glob('*.png'):
        if re.fullmatch(r'[0-9a-f]{64}\.png', path.name) and path.name not in live:
            path.unlink(missing_ok=True)


def _prepare():
    ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
    ROOT.chmod(0o700)


def capture(mime, data, sensitive=False):
    if sensitive or not data or len(data) > BYTES:
        return
    if mime == 'text':
        try:
            text = data.decode('utf-8')
        except UnicodeDecodeError:
            return
        item = {'type': 'text', 'text': text}
    elif mime == 'image/png':
        if not data.startswith(PNG):
            return
        item = {'type': 'image'}
    else:
        raise ValueError('Unsupported clipboard format')
    identifier = _identifier(mime, data)
    item.update(id=identifier, capturedAt=int(time.time() * 1000))
    if item['type'] == 'image':
        item['path'] = str(ROOT / (identifier + '.png'))
    # An oversize offer must not evict entries or truncate its content.
    if len(_encode([item])) > BYTES:
        return
    _prepare()
    with (ROOT / 'history.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        previous = read()
        items = ([item] + [old for old in previous if old['id'] != identifier])[:LIMIT]
        encoded = _encode(items)
        while len(encoded) > BYTES:
            items.pop()
            encoded = _encode(items)
        created_image = None
        if item['type'] == 'image':
            path = ROOT / (identifier + '.png')
            if path.exists() or path.is_symlink():
                image_data(item)
            else:
                _replace(path, data)
                created_image = path
        try:
            _write_history(encoded)
        except Exception:
            if created_image is not None:
                created_image.unlink(missing_ok=True)
            raise
        prune(items)


def clear():
    # Recovery deliberately does not read or validate the damaged old history.
    _prepare()
    with (ROOT / 'history.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        write([])
        prune([])
