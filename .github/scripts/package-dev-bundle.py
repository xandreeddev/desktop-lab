#!/usr/bin/env python3
"""Package one source-checked ordinary-client bundle for later VM qualification."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from lucent_dev.model import BINARY_NAMES, SUPPORT_FILES, load_bundle


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundles', type=Path, required=True)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert re.fullmatch('[0-9a-f]{40}', args.source_commit)
    candidates = list(args.bundles.resolve(strict=True).iterdir())
    assert len(candidates) == 1, 'Expected exactly one completed development bundle'
    bundle = candidates[0]
    manifest = load_bundle(bundle)
    assert manifest['source_commit'] == args.source_commit
    assert manifest['source_dirty'] is False and manifest['profile'] == 'release'
    assert re.fullmatch(args.source_commit[:12] + '-[0-9a-f]{24}', bundle.name)
    qualification = {
        'schema_version': 1,
        'source_commit': args.source_commit,
        'profile': 'release',
        'source_dirty': False,
        'bundle': bundle.name,
        'manifest_sha256': digest(bundle / 'manifest.json'),
        'binaries': {},
        'vm_qualified': False,
        'native_accepted': False,
        'scope': 'Three ordinary clients and seven theme files. Target VM ABI, '
                 'guarded deployment and actual native interaction remain separate gates.',
    }
    for name in BINARY_NAMES:
        path = bundle / name
        env = dict(os.environ, LC_ALL='C')
        dynamic = subprocess.check_output(['readelf', '-d', str(path)], env=env, text=True, timeout=15)
        versions = subprocess.check_output(['readelf', '--version-info', str(path)], env=env, text=True, timeout=15)
        dependencies = subprocess.check_output(['ldd', str(path)], env=env, text=True, timeout=15)
        assert 'not found' not in dependencies
        needed = re.findall(r'\(NEEDED\).*?\[([^\]]+)\]', dynamic)
        assert needed, name
        qualification['binaries'][name] = {
            **manifest['binaries'][name],
            'needed': needed,
            'glibc_requirements': sorted(set(re.findall(r'Name: (GLIBC_[A-Za-z0-9_.]+)', versions))),
            'all_builder_dependencies_resolved': True,
        }
        if name in ('lucent-cli', 'lucent-menu'):
            help_text = subprocess.check_output([str(path), '--help'], env=env, text=True, timeout=15)
            assert help_text.strip()
            qualification['binaries'][name]['inert_help_passed'] = True
    # Preserve the immutable payload's private modes; serialize only declared files.
    args.output.mkdir(mode=0o700, parents=True)
    archive = args.output / 'development-bundle.tar.gz'
    files = ('manifest.json', *BINARY_NAMES, *(str(Path('support') / name) for name in SUPPORT_FILES))
    directories = {Path('.')}
    for relative in files:
        directories.update(Path(relative).parents)

    def portable(info):
        info.uid = info.gid = 0
        info.uname = info.gname = ''
        return info

    with tarfile.open(archive, 'x:gz') as tar:
        for relative in sorted(directories, key=lambda value: (len(value.parts), str(value))):
            tar.add(bundle / relative, arcname=str(Path(bundle.name) / relative), recursive=False, filter=portable)
        for relative in files:
            tar.add(bundle / relative, arcname=str(Path(bundle.name) / relative), recursive=False, filter=portable)
    archive.chmod(0o600)
    with tarfile.open(archive, 'r:gz') as tar:
        regular = {member.name: member for member in tar.getmembers() if member.isfile()}
        assert set(regular) == {str(Path(bundle.name) / name) for name in files}
        for relative in files:
            member = regular[str(Path(bundle.name) / relative)]
            with tar.extractfile(member) as stream:
                actual = hashlib.file_digest(stream, 'sha256').hexdigest()
            assert actual == digest(bundle / relative), relative
    qualification['archive_payload_hashes_verified'] = True
    qualification['archive_sha256'] = digest(archive)
    qualification['archive_bytes'] = archive.stat().st_size
    assert load_bundle(bundle) == manifest
    with (args.output / 'qualification.json').open('x') as stream:
        json.dump(qualification, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'source_commit': args.source_commit, 'bundle': bundle.name,
                      'builder_checks_passed': True, 'vm_qualified': False,
                      'archive_sha256': qualification['archive_sha256']}))


if __name__ == '__main__':
    main()
