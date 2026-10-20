"""Disk-backed private scratch for the two owned Chromium acceptance suites.

This allocates new directories only. Captured lease retirement and cleanup remain
the callers' responsibility; it never searches, removes or reuses old fixtures.
"""
import hashlib
import json
from pathlib import PurePosixPath
import re


PREFIXES = ('lucent-application-intent', 'lucent-gui-clipboard')
MIN_FREE_BYTES = 1024**3

# Fixed source, supplied to Python -c. The selected prefix travels on stdin.
PROGRAM = r'''
import ctypes,json,os,re,secrets,stat,sys
from pathlib import Path

PREFIXES=('lucent-application-intent','lucent-gui-clipboard')
MIN_FREE_BYTES=1024**3
RAM_FILESYSTEMS=(0x01021994,0x858458f6)  # Linux TMPFS_MAGIC, RAMFS_MAGIC.

def need(value,reason):
    if not value: raise ValueError(reason)

def same(left,right):
    return (left.st_dev,left.st_ino,left.st_mode,left.st_uid,left.st_gid)==(
        right.st_dev,right.st_ino,right.st_mode,right.st_uid,right.st_gid)

def owned(value,private=False):
    need(stat.S_ISDIR(value.st_mode) and value.st_uid==os.getuid()
         and not stat.S_IMODE(value.st_mode)&0o022
         and (not private or stat.S_IMODE(value.st_mode)==0o700),
         'owned_safe_fixture_parent_required')

def filesystem_type(fd):
    # Linux/glibc statfs on the supported 64-bit guest. f_type is the leading
    # machine-word field; the retained buffer exceeds this ABI's full structure.
    need(sys.platform=='linux' and ctypes.sizeof(ctypes.c_void_p)==8,
         'supported_fixture_filesystem_probe_required')
    value=ctypes.create_string_buffer(256)
    libc=ctypes.CDLL(None,use_errno=True)
    libc.fstatfs.argtypes=(ctypes.c_int,ctypes.c_void_p)
    libc.fstatfs.restype=ctypes.c_int
    need(libc.fstatfs(fd,value)==0,'fixture_filesystem_probe_failed')
    return ctypes.c_long.from_buffer(value).value

def disk(fd):
    kind=filesystem_type(fd)
    need(kind not in RAM_FILESYSTEMS,'disk_backed_fixture_storage_required')
    space=os.fstatvfs(fd)
    available=space.f_bavail*space.f_frsize
    need(available>=MIN_FREE_BYTES,'fixture_storage_reserve_required')
    return kind,available

def child(parent,name,private):
    try: os.mkdir(name,0o700,dir_fd=parent)
    except FileExistsError: pass
    fd=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent)
    try:
        value=os.fstat(fd)
        owned(value,private)
        need(same(value,os.stat(name,dir_fd=parent,follow_symlinks=False)),
             'fixture_parent_changed')
        return fd
    except BaseException:
        os.close(fd)
        raise

def allocate(prefix):
    need(prefix in PREFIXES,'known_fixture_prefix_required')
    home=Path.home()
    need(home.is_absolute() and home.resolve(strict=True)==home
         and not any(character.isspace() for character in str(home)),
         'canonical_fixture_home_required')
    root=os.open(home,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    opened=[root]
    try:
        owned(os.fstat(root))
        need(same(os.fstat(root),home.lstat()),'fixture_home_changed')
        cache=child(root,'.cache',False);opened.append(cache)
        disk(cache)
        parent=child(cache,'lucent-acceptance',True);opened.append(parent)
        kind,available=disk(parent)
        for _ in range(16):
            name=prefix+'.'+secrets.token_hex(6)
            try: os.mkdir(name,0o700,dir_fd=parent)
            except FileExistsError: continue
            break
        else: raise ValueError('fixture_directory_collision_bound')
        fd=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent)
        opened.append(fd)
        owned(os.fstat(fd),True)
        path=home/'.cache'/'lucent-acceptance'/name
        need(path.resolve(strict=True)==path and same(os.fstat(root),home.lstat())
             and same(os.fstat(cache),(home/'.cache').lstat())
             and same(os.fstat(parent),path.parent.lstat())
             and same(os.fstat(fd),path.lstat()),'fixture_directory_changed')
        os.fsync(fd);os.fsync(parent)
        return {'path':str(path),'filesystem_type':kind,'available_bytes':available}
    finally:
        for fd in reversed(opened): os.close(fd)

def main():
    raw=sys.stdin.buffer.read(257)
    need(len(raw)<=256,'fixture_allocation_request_bound')
    packet=json.loads(raw)
    need(type(packet) is dict and set(packet)=={'prefix'},'fixture_allocation_request_required')
    print(json.dumps(allocate(packet['prefix']),sort_keys=True))

if __name__=='__main__': main()
'''
PROGRAM_SHA256 = hashlib.sha256(PROGRAM.encode()).hexdigest()


def allocate(remote, prefix):
    """Use the caller's existing bounded transport; never invoke a shell script."""
    if prefix not in PREFIXES:
        raise ValueError('known_fixture_prefix_required')
    raw = remote('/usr/bin/python3', '-I', '-c', PROGRAM,
                 data=json.dumps({'prefix': prefix}).encode(), timeout=15)
    if not isinstance(raw, (str, bytes)) or len(raw) > 4096:
        raise ValueError('bounded_fixture_allocation_reply_required')
    reply = json.loads(raw)
    path = PurePosixPath(reply['path'])
    if (not path.is_absolute() or path.parent.name != 'lucent-acceptance'
            or path.parent.parent.name != '.cache'
            or re.fullmatch(re.escape(prefix) + r'\.[0-9a-f]{12}', path.name) is None
            or '..' in path.parts or any(character.isspace() for character in str(path))
            or type(reply.get('available_bytes')) is not int or reply['available_bytes'] < MIN_FREE_BYTES
            or type(reply.get('filesystem_type')) is not int
            or reply['filesystem_type'] in (0x01021994, 0x858458f6)):
        raise ValueError('private_disk_fixture_directory_required')
    return str(path)
