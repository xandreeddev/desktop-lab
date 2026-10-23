"""Owned temporary MIME/entry configuration leases. Import performs no IO.

Linux renameat2 exchanges retain the original inode for restoration. A changed
target, including an identical-content replacement, never grants overwrite or
deletion authority. Journals precede publication and also cover a lost reply.
"""
import configparser
import ctypes
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat


def require(value, reason):
    if not value:
        raise ValueError(reason)


def identity(value):
    # Renames can change ctime. Preserve every content/ownership/inode field.
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns]


def directory(path, private=False):
    path = Path(path)
    value = path.lstat()
    require(path.is_absolute() and path.resolve(strict=True) == path
            and stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and not stat.S_IMODE(value.st_mode) & 0o022
            and (not private or stat.S_IMODE(value.st_mode) == 0o700), 'owned_configuration_directory')
    return path


def snapshot(path):
    path = Path(path)
    directory(path.parent)
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    except FileNotFoundError:
        require(not os.path.lexists(path), 'configuration_link_is_not_absence')
        return None
    with os.fdopen(fd, 'rb') as stream:
        value = os.fstat(stream.fileno())
        require(stat.S_ISREG(value.st_mode) and value.st_uid == os.getuid()
                and value.st_nlink == 1 and value.st_size <= 256 * 1024
                and not stat.S_IMODE(value.st_mode) & 0o022, 'owned_configuration_file')
        raw = stream.read(256 * 1024 + 1)
        require(len(raw) == value.st_size and identity(value) == identity(os.fstat(stream.fileno()))
                == identity(path.lstat()), 'configuration_changed_during_read')
    return {'identity': identity(value), 'sha256': hashlib.sha256(raw).hexdigest(),
            'bytes': len(raw), 'mode': stat.S_IMODE(value.st_mode)}


def read(path):
    before = snapshot(path)
    require(before is not None, 'configuration_file_required')
    value = Path(path).read_bytes()
    require(snapshot(path) == before and hashlib.sha256(value).hexdigest() == before['sha256'],
            'configuration_read_changed')
    return value


def private(path, data, mode=0o600):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
    with os.fdopen(fd, 'wb') as stream:
        os.fchmod(stream.fileno(), mode)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    sync(Path(path).parent)


def sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def rename(source, target, flags):
    operation = ctypes.CDLL(None, use_errno=True).renameat2
    operation.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    operation.restype = ctypes.c_int
    if operation(-100, os.fsencode(source), -100, os.fsencode(target), flags):
        raise OSError(ctypes.get_errno(), 'owned_configuration_rename_failed')


def plan(run, name, target, content):
    run, target = directory(run, True), Path(target)
    require(re.fullmatch('[a-z][a-z-]{0,31}', name) is not None
            and target.is_absolute() and target.parent.stat().st_dev == run.stat().st_dev,
            'same_filesystem_owned_configuration_lease')
    before = snapshot(target)
    slot = run / (name + '.payload')
    private(slot, content, before['mode'] if before is not None else 0o600)
    value = {'target': str(target), 'slot': str(slot), 'original': before, 'candidate': snapshot(slot)}
    private(run / (name + '.lease.json'), (json.dumps(value, sort_keys=True) + '\n').encode())
    return value


def lease(run, name):
    run = directory(run, True)
    value = json.loads(read(run / (name + '.lease.json')))
    require(value['slot'] == str(run / (name + '.payload')) and Path(value['target']).is_absolute(),
            'exact_owned_configuration_lease')
    directory(Path(value['target']).parent)
    return value


def publish(value):
    target, slot = Path(value['target']), Path(value['slot'])
    require(snapshot(target) == value['original'] and snapshot(slot) == value['candidate'],
            'configuration_publication_preimage_changed')
    rename(slot, target, 1 if value['original'] is None else 2)
    sync(target.parent)
    sync(slot.parent)
    if snapshot(target) != value['candidate'] or snapshot(slot) != value['original']:
        # A successor that raced the preimage read is not ours to replace.
        # Put the displaced file back while our exact candidate still owns the
        # named target, and retain all private evidence for failed admission.
        if value['original'] is not None and snapshot(target) == value['candidate']:
            rename(slot, target, 2)
            sync(target.parent)
            sync(slot.parent)
        raise ValueError('raced_configuration_publication_preserved')


def active(value):
    require(snapshot(value['target']) == value['candidate'] and snapshot(value['slot']) == value['original'],
            'independent_configuration_change_preserved')


def restore(value):
    target, slot = Path(value['target']), Path(value['slot'])
    if snapshot(target) == value['original']:
        require(snapshot(slot) == value['candidate'], 'uncertain_configuration_restore_slot_preserved')
        return  # Never published, or this exact original inode is already back.
    active(value)
    if value['original'] is None:
        rename(target, slot, 1)
        if snapshot(slot) != value['candidate']:
            # A raced successor is retained; never overwrite another target.
            rename(slot, target, 1)
            raise ValueError('raced_configuration_removal_preserved')
    else:
        rename(slot, target, 2)
        if snapshot(slot) != value['candidate'] or snapshot(target) != value['original']:
            require(snapshot(target) == value['original'], 'uncertain_configuration_exchange_retained')
            rename(slot, target, 2)
            raise ValueError('raced_configuration_exchange_preserved')
    sync(target.parent)
    sync(slot.parent)
    require(snapshot(target) == value['original'], 'original_configuration_not_restored')


def mime_content(original, app_id):
    require(re.fullmatch(r'org\.lucent\.DefaultUri[A-Za-z0-9]{6,32}\.desktop', app_id) is not None,
            'owned_default_handler_identity')
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.optionxform = str
    parser.read_string(original.decode('utf-8'))
    section = 'Default Applications'
    if not parser.has_section(section):
        parser.add_section(section)
    parser.set(section, 'x-scheme-handler/http', app_id + ';')
    output = io.StringIO()
    parser.write(output, space_around_delimiters=False)
    return output.getvalue().encode()


def notification_absence(snapshot, records):
    """Read-only revision diagnostics; a wire ID never grants dismissal rights."""
    count = snapshot['client']['notification_count']
    require(type(count) is int and 0 <= count <= 4096, 'bounded_notification_count')
    known = {row['id']: row.get('observed_revision') for row in records}
    require(len(known) == len(records), 'distinct_observed_fixture_wire_ids')
    visible = []
    for surface in snapshot['surfaces']:
        if surface.get('role', surface.get('id')) != 'notifications':
            continue
        for hit in surface['hits']:
            match = re.fullmatch(r'notification-dismiss-([0-9]+)-([0-9]+)', hit['id'])
            if match:
                identifier, revision = map(int, match.groups())
                require(identifier in known, 'foreign_notification_preserved')
                require(known[identifier] is None or known[identifier] == revision,
                        'replaced_notification_revision_preserved')
                visible.append(identifier)
    require(count <= len(known), 'foreign_remaining_notification_preserved')
    return count == 0 and not visible
