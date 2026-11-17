#!/usr/bin/env python3
"""Build in the bounded hosted Arch cgroup and qualify its reference imports."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from lucent_dev.build import publish_bundle


def reference_imports(qualification, reference):
    assert set(qualification['binaries']) == set(reference['needed'])
    highest = (0,)
    for name, expected in reference['needed'].items():
        binary = qualification['binaries'][name]
        assert set(binary['needed']) == set(expected), f'{name}: target library imports changed'
        for version in binary['glibc_requirements']:
            match = re.fullmatch(r'GLIBC_(\d+(?:\.\d+)+)', version)
            if match:
                number = tuple(map(int, match[1].split('.')))
                assert number <= tuple(reference['maximum_glibc']), version
                highest = max(highest, number)
            else:
                assert version in reference['allowed_glibc_abi_names'], version
    return {'reference_source_commit': reference['reference_source_commit'],
            'needed_libraries_match_reference': True,
            'maximum_required_glibc': '.'.join(map(str, highest)),
            'vm_qualified': False, 'native_accepted': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', type=Path, required=True)
    parser.add_argument('--bundles', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert os.environ.get('GITHUB_ACTIONS') == 'true'
    assert os.environ.get('GITHUB_REPOSITORY') == 'xandreeddev/desktop-lab'
    cgroup = Path('/sys/fs/cgroup')
    assert (cgroup / 'cpu.max').read_text().strip() == '100000 100000'
    assert (cgroup / 'memory.max').read_text().strip() == '4294967296'
    assert (cgroup / 'memory.swap.max').read_text().strip() == '0'
    assert (cgroup / 'pids.max').read_text().strip() == '512'
    container_pid1 = Path('/proc/1/comm').read_text().strip()
    assert container_pid1 == 'docker-init'
    os.umask(0o077)
    os.environ['CARGO_TARGET_DIR'] = str(args.target.resolve())
    result = publish_bundle(ROOT, args.target, 'release', args.bundles)
    subprocess.run([
        sys.executable, str(ROOT / '.github/scripts/package-dev-bundle.py'),
        '--bundles', str(args.bundles), '--source-commit', os.environ['GITHUB_SHA'],
        '--output', str(args.output),
    ], check=True)
    qualification = args.output / 'qualification.json'
    value = json.loads(qualification.read_text())
    reference = json.loads((ROOT / '.github/arch-dev-abi.json').read_text())
    value['arch_reference_imports'] = reference_imports(value, reference)
    value['builder_platform'] = 'Arch Linux'
    value['container_pid1'] = container_pid1
    value['container_image'] = os.environ['LUCENT_CI_IMAGE']
    value['resource_limits'] = {'cpu_cores': 1, 'memory_max_bytes': 4294967296,
                                'swap_max_bytes': 0, 'pids_max': 512}
    value['packages'] = subprocess.check_output(
        ['pacman', '-Q', 'glibc', 'ffmpeg', 'rustup'], text=True).strip().splitlines()
    value['producer_inputs_sha256'] = result['inputs']
    qualification.write_text(json.dumps(value, indent=2) + '\n')
    print(json.dumps({'source_commit': value['source_commit'],
                      'arch_reference_imports': value['arch_reference_imports'],
                      'vm_qualified': False}))


if __name__ == '__main__':
    main()
