#!/usr/bin/env python3
"""Bounded readelf-only inspection helpers; never execute an inspected ELF."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import resource
import selectors
import stat
import subprocess
import time

READELF = '/usr/bin/readelf'
MAX_FILE = 512*1024*1024
MAX_OUTPUT = 16*1024*1024
MAX_PAYLOAD = 64*1024*1024
SONAME = re.compile(r'[A-Za-z0-9_+.-]{1,128}')
HEX = re.compile(r'[0-9a-f]{64}')
VERSION = re.compile(r'[A-Za-z0-9_.+\-]{1,256}')


class Refused(ValueError):
    def __init__(self, reason):
        self.reason = reason
        super().__init__(reason)


def need(value, reason):
    if not value:
        raise Refused(reason)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def identity(value):
    return (value.st_dev,value.st_ino,value.st_mode,value.st_uid,value.st_gid,
            value.st_size,value.st_mtime_ns,value.st_ctime_ns)


def open_pinned(path, expected, bound):
    need(isinstance(path,Path) and path.is_absolute() and path.resolve(strict=True)==path,
         'explicit_canonical_regular_file_required')
    need(type(expected) is str and HEX.fullmatch(expected), 'explicit_sha256_required')
    fd = os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    try:
        before = os.fstat(fd)
        need(stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and 0<before.st_size<=bound,
             'owned_bounded_regular_file_required')
        with os.fdopen(os.dup(fd),'rb') as stream:
            actual = hashlib.file_digest(stream,'sha256').hexdigest()
        os.lseek(fd,0,os.SEEK_SET)
        need(actual==expected,'pinned_file_hash_mismatch')
        need(identity(os.fstat(fd))==identity(before) and identity(path.lstat())==identity(before),
             'file_changed_during_static_inspection')
        return fd,before
    except BaseException:
        os.close(fd)
        raise


def read_json(path, expected, bound=MAX_PAYLOAD):
    fd,before=open_pinned(path,expected,bound)
    try:
        with os.fdopen(os.dup(fd),'rb') as stream:raw=stream.read(bound+1)
        need(len(raw)<=bound and identity(os.fstat(fd))==identity(before)
             and identity(path.lstat())==identity(before),'file_changed_during_static_inspection')
        return json.loads(raw)
    finally:
        os.close(fd)


def child_limits():
    resource.setrlimit(resource.RLIMIT_AS,(256*1024*1024,256*1024*1024))
    resource.setrlimit(resource.RLIMIT_CPU,(10,10))
    resource.setrlimit(resource.RLIMIT_CORE,(0,0))
    resource.setrlimit(resource.RLIMIT_FSIZE,(0,0))


def readelf(fd, option):
    need(option in ('--file-header','--program-headers','--dynamic','--dyn-syms','--version-info','--relocs'),
         'fixed_readelf_operation_required')
    process=subprocess.Popen([READELF,'--wide',option,'/proc/self/fd/'+str(fd)],pass_fds=(fd,),
        stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
        env={'LC_ALL':'C'},preexec_fn=child_limits)
    output,error=bytearray(),bytearray()
    selector=selectors.DefaultSelector()
    selector.register(process.stdout,selectors.EVENT_READ,output)
    selector.register(process.stderr,selectors.EVENT_READ,error)
    deadline=time.monotonic()+15
    try:
        while selector.get_map():
            need(time.monotonic()<deadline,'readelf_deadline_exceeded')
            for key,_ in selector.select(min(.2,max(0,deadline-time.monotonic()))):
                raw=os.read(key.fd,65536)
                if not raw:
                    selector.unregister(key.fileobj)
                    continue
                key.data.extend(raw)
                need(len(output)<=MAX_OUTPUT and len(error)<=16384,'bounded_readelf_output_required')
        need(process.wait(timeout=1)==0 and not error,'readelf_refused_or_warned')
        return output.decode('utf-8',errors='strict')
    finally:
        selector.close()
        if process.poll() is None:
            process.kill()  # Only the exact child created above, never an ELF/application.
        process.wait(timeout=2)
        process.stdout.close();process.stderr.close()


def parse_versions(raw):
    requirements={};current=None;expected_records=0;seen_records=0;expected_section=None
    in_needs=False
    for line in raw.splitlines():
        section=re.match(r"Version needs section .* contains ([0-9]+) entries:",line)
        if section:
            need(expected_section is None,'one_version_needs_section_required')
            expected_section=int(section[1]);in_needs=True;continue
        if not in_needs:continue
        if line.startswith('Version ') and ' section ' in line:
            in_needs=False;continue
        file=re.search(r'\bFile: (\S+)\s+Cnt: ([0-9]+)',line)
        if file:
            need(current is None or seen_records==expected_records,'complete_version_provider_record_required')
            current=file[1];need(SONAME.fullmatch(current),'version_provider_soname_required')
            expected_records=int(file[2]);seen_records=0;continue
        name=re.search(r'\bName: (\S+)\s+Flags: (\S+)\s+Version: ([0-9]+)',line)
        if name:
            need(current is not None and VERSION.fullmatch(name[1]) and name[2] in ('none','WEAK'),
                 'recognized_version_requirement_required')
            index=int(name[3]);need(1<index<2**15 and index not in requirements,'unique_version_requirement_index_required')
            requirements[index]={'provider':current,'version':name[1],'weak':name[2]=='WEAK'};seen_records+=1
    need(current is None or seen_records==expected_records,'complete_version_provider_record_required')
    if expected_section is not None:
        providers=len({row['provider'] for row in requirements.values()})
        need(providers==expected_section,'complete_version_needs_section_required')
    need(len(requirements)<=4096,'bounded_version_requirements_required')
    return requirements


def parse(outputs):
    header, program, dynamic = (outputs[name] for name in ('--file-header', '--program-headers', '--dynamic'))
    fields = {match[1]: match[2].strip() for match in re.finditer(
        r'^\s*(Class|Data|Machine|Type):\s*(.*)$', header, re.M)}
    need(fields.get('Class') == 'ELF64' and fields.get('Data') == "2's complement, little endian"
         and fields.get('Machine') == 'Advanced Micro Devices X86-64'
         and fields.get('Type', '').split()[:1] in (['DYN'], ['EXEC']), 'expected_ELF64_amd64_required')
    interpreters = re.findall(r'\[Requesting program interpreter: ([^\]]+)\]', program)
    need(interpreters == ['/lib64/ld-linux-x86-64.so.2'], 'expected_interpreter_required')
    needed = re.findall(r'\(NEEDED\).*?Shared library: \[([^\]]+)\]', dynamic)
    need(0 < len(needed) <= 128 and len(set(needed)) == len(needed)
         and all(SONAME.fullmatch(name) for name in needed), 'bounded_needed_libraries_required')
    need(not re.search(r'\((?:RPATH|RUNPATH|FILTER|AUXILIARY|AUDIT|DEPAUDIT|SYMBOLIC)\)', dynamic),
         'unreviewed_loader_override_forbidden')
    return {'needed_sonames': needed, 'interpreter': interpreters[0],
            'version_requirements': list(parse_versions(outputs['--version-info']).values())}


def compare_reference(model, reference):
    """Compare only direct SONAME/version needs to a separately pinned snapshot.

    This intentionally does not resolve symbols, recursive dependencies, loader
    environment, struct layouts or the target's current installed state.
    """
    need(isinstance(reference, dict) and isinstance(reference.get('providers'), dict)
         and isinstance(reference.get('aliases'), dict)
         and 0 < len(reference['providers']) <= 384 and 0 < len(reference['aliases']) <= 768,
         'bounded_provider_reference_required')
    missing = []
    names = set(model['needed_sonames']) | {'ld-linux-x86-64.so.2'}
    names.update(row['provider'] for row in model['version_requirements'])
    selected = {}
    for name in sorted(names):
        keys = reference['aliases'].get(name, [])
        if not isinstance(keys, list) or len(keys) != 1 or keys[0] not in reference['providers']:
            missing.append({'soname': name, 'reason': 'missing_or_ambiguous_provider'})
            continue
        row = reference['providers'][keys[0]]
        need(isinstance(row, dict) and HEX.fullmatch(row.get('sha256', ''))
             and row.get('soname') in (None, name), 'identified_provider_reference_required')
        exports = row.get('defined_dynamic_symbols')
        need(isinstance(exports, list) and len(exports) <= 65536, 'bounded_provider_exports_required')
        versions = set()
        for export in exports:
            need(isinstance(export, list) and len(export) == 5 and isinstance(export[0], str)
                 and len(export[0]) <= 8192, 'known_provider_export_required')
            if '@' in export[0]:
                version = export[0].rsplit('@', 1)[-1]
                need(VERSION.fullmatch(version), 'known_provider_version_required')
                versions.add(version)
        selected[name] = versions
    for row in model['version_requirements']:
        if not row['weak'] and row['version'] not in selected.get(row['provider'], set()):
            missing.append({'soname': row['provider'], 'version': row['version'], 'reason': 'missing_required_version'})
    return {'direct_sonames_and_versions_match_reference': not missing, 'mismatches': missing,
            'symbol_resolution_checked': False, 'recursive_provider_closure_checked': False,
            'target_installed_state_checked': False, 'runtime_ABI_accepted': False}


def inspect(path, expected, reference=None):
    fd, before = open_pinned(path, expected, MAX_FILE)
    try:
        outputs = {option: readelf(fd, option) for option in
                   ('--file-header', '--program-headers', '--dynamic', '--version-info')}
        model = parse(outputs)
        need(identity(os.fstat(fd)) == identity(before) and identity(path.lstat()) == identity(before),
             'file_changed_during_static_inspection')
        return dict(model, file_sha256=expected, file_bytes=before.st_size,
                    readelf_outputs_sha256={key: digest(value.encode()) for key, value in outputs.items()},
                    reference_comparison=compare_reference(model, reference) if reference is not None else None,
                    runtime_ABI_accepted=False, ELF_executed=False)
    finally:
        os.close(fd)


