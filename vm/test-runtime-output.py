#!/usr/bin/env python3
"""Source-bound 4K resource-only output owner. Default is completely inert.

--begin requires a fresh reviewed 1080p resource binding and exact VM UUID.
Later requests use its new private receipt. No measurement is performed here;
the reviewed 9eb resource wrapper can sample the freshly bound 4K runtime.
Presentation is refused before transport because nested tracing recovery is absent.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import stat
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
SOURCE = '9eb41939d47ced5be2012bff41a1f2f5768d6ee7'
PINS = {
    'wrapper': ('.cache/daily-runtime-measurement-9eb41939-release/wrapper.py', 'dbf9dc96407897665f6902e10982ce66bb05b87f7c28e42d4b16942d47dc281a'),
    'runtime_output_proof.py': ('.cache/daily-runtime-measurement-9eb41939-release/guest-probe.py', 'bb64de9a4282d45ff71be651b54ec98973e32a424cc513a7a805c57c40d97b2b'),
    'units.py': ('.cache/amd-native-context-controller-btrfs-fix/qualified/units.py', '9a088150a982a844b775f2078731482bd0064a1a5985217a193336adc8000df2'),
    'preservation.py': ('.cache/amd-native-context-controller-btrfs-fix/qualified/preservation.py', 'ecd796f7bce24e4ea153633089595c7ba4d5604959ca5f0131367832b173e2a3'),
    'readiness.py': ('.cache/amd-native-context-controller-btrfs-fix/qualified/readiness.py', 'c0074ece3388723608430e98ffaec82811acea278c0160e9de014abfcef48baf'),
}
CANONICAL = ('vm/lab.py', 'vm/multi_output_guard.py', 'vm/multi_output_policy.py',
             'vm/owned_application_units.py')
NEW = ('runtime_output_policy.py', 'runtime_output_guard.py')


def require(value, label):
    if not value:
        raise ValueError(label)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path, maximum=4 * 1024 * 1024):
    path = Path(path)
    require(path.is_absolute() and path.resolve(strict=True) == path, 'canonical_local_source_required')
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        before = os.fstat(stream.fileno())
        identity = lambda value: (value.st_dev, value.st_ino, value.st_mode, value.st_uid,
                                  value.st_gid, value.st_size, value.st_mtime_ns, value.st_ctime_ns)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and before.st_size <= maximum, 'owned_bounded_local_source_required')
        raw = stream.read(maximum + 1)
        require(len(raw) <= maximum and identity(before) == identity(os.fstat(stream.fileno()))
                == identity(path.lstat()), 'local_source_changed')
        return raw


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def common_root():
    common = subprocess.check_output(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'],
                                     cwd=ROOT, text=True, timeout=5).strip()
    return Path(common).parent


def qualify(binding_path, binding_sha, vm_uuid, *, fresh):
    require(isinstance(vm_uuid, str) and re.fullmatch(r'[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}', vm_uuid),
            'exact_vm_uuid_required')
    main = common_root()
    sources = {}
    for name, (relative, expected) in PINS.items():
        raw = read(main / relative)
        require(sha(raw) == expected, 'frozen_resource_helper_changed')
        sources[name] = raw
    wrapper = module('frozen_9eb_resource_wrapper', main / PINS['wrapper'][0])
    wrapper.qualify()  # Local archive/source checks only; no native binaries.
    for relative in CANONICAL:
        raw = read(ROOT / relative)
        committed = subprocess.check_output(['git', 'show', SOURCE + ':' + relative], cwd=ROOT, timeout=5)
        require(raw == committed, 'canonical_runtime_output_helper_changed')
        if relative == 'vm/lab.py':
            require(raw == read(main / relative), 'canonical_private_transport_changed')
        sources[Path(relative).name] = raw
    for name in NEW:
        sources[name] = read(ROOT / 'vm' / name)
    raw = read(binding_path)
    require(sha(raw) == binding_sha, 'exact_reviewed_resource_binding_required')
    binding = json.loads(raw)
    require(binding['mode'] == 'resources' and binding['output_profile'] == '1080p60-1x'
            and len(binding['critical_user_files']) == 13,
            'fresh_original_resource_binding_required')
    if fresh:
        wrapper.candidate(wrapper.bound(binding))
    else:
        # The independent lease retains this exact initial runtime. Status and
        # cleanup must remain possible after its original 300s admission ages.
        wrapper.candidate(binding)
    return sources, binding, wrapper


STAGE = r'''
import hashlib,json,os,stat,sys
from pathlib import Path
packet=json.load(sys.stdin);tag=packet['bundle']['tag'];home=Path.home()
assert home.is_absolute() and home.resolve(strict=True)==home
def directory(path,private):
 try:path.mkdir(mode=0o700)
 except FileExistsError:pass
 value=path.lstat()
 assert stat.S_ISDIR(value.st_mode) and value.st_uid==os.getuid() and not stat.S_IMODE(value.st_mode)&0o022
 assert not private or stat.S_IMODE(value.st_mode)==0o700
 assert path.resolve(strict=True)==path
directory(home/'.cache',False);parent=home/'.cache/lucent-output-acceptance';directory(parent,True)
space=os.statvfs(parent);assert space.f_bavail*space.f_frsize>=256*1024*1024
root=parent/('lucent-output-4k.'+tag);root.mkdir(mode=0o700)
def write(name,raw):
 assert Path(name).name==name
 fd=os.open(root/name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
 with os.fdopen(fd,'wb') as stream:stream.write(raw);stream.flush();os.fsync(stream.fileno())
for name,text in packet['sources'].items():
 raw=text.encode();assert hashlib.sha256(raw).hexdigest()==packet['bundle']['sources'][name];write(name,raw)
raw=json.dumps(packet['bundle'],sort_keys=True).encode();write('bundle.json',raw)
fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW);os.fsync(fd);os.close(fd)
print(json.dumps({'directory':str(root),'bundle_sha256':hashlib.sha256(raw).hexdigest()}))
'''

ENTRY = r'''
import hashlib,os,stat,sys
from pathlib import Path
path=Path.home()/'desktop-lab/scripts/in-session.py';expected=sys.argv[1]
fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
with os.fdopen(fd,'rb') as stream:
 before=os.fstat(stream.fileno());assert stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_size<=1048576
 raw=stream.read(1048577);assert hashlib.sha256(raw).hexdigest()==expected
 identity=lambda v:(v.st_dev,v.st_ino,v.st_mode,v.st_uid,v.st_size,v.st_mtime_ns,v.st_ctime_ns)
 assert identity(before)==identity(os.fstat(stream.fileno()))==identity(path.lstat())
sys.argv=[str(path),*sys.argv[2:]]
exec(compile(raw,str(path),'exec'),{'__name__':'__main__','__file__':str(path)})
'''


def update(stream, path, expected, value):
    info = path.lstat()
    require((info.st_dev, info.st_ino, info.st_uid) == expected and info.st_nlink == 1,
            'original_host_output_receipt_required')
    stream.seek(0); json.dump(value, stream, sort_keys=True); stream.truncate(); stream.flush(); os.fsync(stream.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    operations = parser.add_mutually_exclusive_group()
    operations.add_argument('--begin', action='store_true')
    operations.add_argument('--apply', type=Path)
    operations.add_argument('--status', type=Path)
    operations.add_argument('--facts', type=Path)
    operations.add_argument('--finish', type=Path)
    parser.add_argument('--mode', default='resources')
    parser.add_argument('--binding', type=Path)
    parser.add_argument('--binding-sha256')
    parser.add_argument('--vm-uuid')
    args = parser.parse_args()
    require(args.mode == 'resources', 'presentation_requires_nested_restore_authority')
    if not any((args.begin, args.apply, args.status, args.facts, args.finish)):
        print(json.dumps({'guest_actions': False, 'resource_only': True, 'presentation_supported': False,
                          'measured': False, 'native_or_performance_accepted': False}))
        return 0
    operation = next((name for name in ('apply', 'status', 'facts', 'finish') if getattr(args, name)), 'begin')
    previous = None
    if not args.begin:
        previous = json.loads(read(getattr(args, operation)))
        require(previous['controller_sha256'] == sha(read(Path(__file__))), 'original_controller_source_required')
        args.binding = Path(previous['binding_path']); args.binding_sha256 = previous['binding_sha256']
        args.vm_uuid = previous['vm_uuid']
    sources, binding, wrapper = qualify(args.binding, args.binding_sha256, args.vm_uuid, fresh=args.begin)
    if previous:
        require(previous['source_sha256'] == {name: sha(raw) for name, raw in sources.items()},
                'original_output_helper_sources_required')
    lab = module('canonical_output_vm_transport', common_root() / 'vm/lab.py')
    inventory = lab.inventory()
    require(inventory['uri'] == 'qemu:///session' and inventory['user'] == 'omarchy'
            and inventory['vms']['lucent']['name'] == 'desktop-lab-lucent'
            and inventory['vms']['lucent']['port'] == 2243, 'fixed_lucent_test_vm_required')
    require(wrapper.command(['virsh', '-c', inventory['uri'], 'domuuid',
                             inventory['vms']['lucent']['name']], timeout=10).decode().strip() == args.vm_uuid,
            'original_test_vm_uuid_changed')
    transport = lab.ssh_args('lucent')
    require(transport.count('StrictHostKeyChecking=accept-new') == 1, 'known_original_vm_host_policy_required')
    transport = ['StrictHostKeyChecking=yes' if item == 'StrictHostKeyChecking=accept-new' else item for item in transport]
    def remote(*argv, data=None, timeout=20):
        return wrapper.command(transport + [shlex.join(argv)], timeout, data).decode()
    def session(*argv, timeout=20):
        return remote('/usr/bin/python3', '-I', '-c', ENTRY, binding['in_session_sha256'], *argv, timeout=timeout)
    lease_tag = secrets.token_hex(12) if args.begin else previous['tag']
    report = {'tag': lease_tag, 'vm_uuid': args.vm_uuid, 'source_commit': SOURCE, 'operation': operation,
              'controller_sha256': sha(read(Path(__file__))), 'binding_path': str(args.binding),
              'binding_sha256': args.binding_sha256, 'source_sha256': {name: sha(raw) for name, raw in sources.items()},
              'success': False, 'measured': False, 'native_or_performance_accepted': False}
    path = common_root() / 'reports/local' / ('native-resource-output-4k-' + lease_tag + '-' + secrets.token_hex(6) + '.json')
    require(path.parent.resolve(strict=True) == path.parent, 'canonical_private_report_parent_required')
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w+') as stream:
        info = os.fstat(stream.fileno()); stamp = (info.st_dev, info.st_ino, info.st_uid)
        update(stream, path, stamp, report)
        try:
            if args.begin:
                staged = {name: raw for name, raw in sources.items() if name not in ('wrapper', 'lab.py')}
                bundle = {'tag': lease_tag, 'mode': 'resources', 'binding': binding, 'vm_uuid': args.vm_uuid,
                          'sources': {name: sha(raw) for name, raw in staged.items()}}
                digest = sha(json.dumps(bundle, sort_keys=True).encode())
                report['bundle_sha256'] = digest
                update(stream, path, stamp, report)
                value = json.loads(remote('/usr/bin/python3', '-I', '-c', STAGE,
                    data=json.dumps({'bundle': bundle, 'sources': {name: raw.decode() for name, raw in staged.items()}}).encode()))
                require(value['bundle_sha256'] == digest and value['directory'] ==
                    '/home/omarchy/.cache/lucent-output-acceptance/lucent-output-4k.' + lease_tag,
                    'exact_new_private_output_scratch_required')
                report['directory'] = value['directory']
                update(stream, path, stamp, report)
                session('systemd-run', '--user', '--collect', '--unit=lucent-output-4k-' + lease_tag,
                    '--property=RuntimeMaxSec=1200', '--property=TimeoutStopSec=135',
                    '/usr/bin/python3', '-I', value['directory'] + '/runtime_output_guard.py',
                    'serve', value['directory'], digest)
            else:
                report.update(directory=previous['directory'], bundle_sha256=previous['bundle_sha256'])
            deadline = time.monotonic() + (150 if args.begin else 1)
            while True:
                observed = json.loads(session('/usr/bin/python3', '-I', report['directory'] + '/runtime_output_guard.py',
                    'status' if args.begin else operation, report['directory'], report['bundle_sha256'], timeout=250))
                if not args.begin or observed.get('ready') or 'finished' in observed:
                    break
                require(time.monotonic() < deadline, 'bounded_independent_output_guard_startup')
                time.sleep(.25)
            if operation == 'finish':
                # The acknowledgement precedes independent cleanup. Only its
                # durable finished receipt establishes actual restoration.
                deadline = time.monotonic() + 135
                while 'finished' not in observed:
                    require(time.monotonic() < deadline, 'bounded_independent_output_restore')
                    time.sleep(.2)
                    observed = json.loads(session('/usr/bin/python3', '-I', report['directory'] + '/runtime_output_guard.py',
                        'status', report['directory'], report['bundle_sha256'], timeout=20))
            report['observed'] = observed
            report['success'] = (observed.get('ready') is True and
                (observed.get('reply', {}).get('success') is True or operation in ('begin', 'status')
                 and observed.get('status', {}).get('live_guardian') is True)
                or operation in ('finish', 'status') and observed.get('finished', {}).get('cleanup_success') is True)
        except BaseException as error:
            report['failure_class'] = type(error).__name__
            report['mutation_reply_may_be_lost'] = operation in ('begin', 'apply', 'finish')
            # No mutating retries. The finite independent guardian owns cleanup.
        update(stream, path, stamp, report)
    print(json.dumps({'success': report['success'], 'report': str(path), 'measured': False}))
    return 0 if report['success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
