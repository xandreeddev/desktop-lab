#!/usr/bin/env python3
"""Run native visual fixtures on the prepared VM's software Vulkan driver.

Only copies a test binary and fixtures to a temporary guest directory. Does not
interact with or restart the desktop. --update explicitly accepts new baselines.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import lab


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--update', action='store_true')
    args = parser.parse_args()
    env = os.environ.copy()
    cargo = shutil.which('cargo')
    if not cargo:
        cargo = str(lab.ROOT / '.cache/cargo/bin/cargo')
        env.update(CARGO_HOME=str(lab.ROOT / '.cache/cargo'), RUSTUP_HOME=str(lab.ROOT / '.cache/rustup'))
    build = subprocess.check_output([cargo, 'test', '--manifest-path', str(lab.ROOT/'lucent/Cargo.toml'), '--locked', '-p', 'lucent-desktop', '--no-run', '--message-format=json'], env=env, text=True)
    binary = next(Path(item['executable']) for line in build.splitlines() if (item := json.loads(line)).get('reason') == 'compiler-artifact' and item.get('executable') and item.get('profile', {}).get('test'))
    ssh = lab.ssh_args('lucent')
    def remote(*command):
        return subprocess.check_output(ssh+[shlex.join(command)], text=True).strip()
    directory = remote('mktemp', '-d', '/tmp/lucent-visual.XXXXXXXX')
    try:
        with binary.open('rb') as source:
            subprocess.run(ssh+['cat > '+shlex.quote(directory+'/tests')], stdin=source, check=True)
        remote('chmod', '700', directory+'/tests')
        baseline = Path('lucent/tests/visual/baselines')
        with tempfile.TemporaryFile() as archive:
            with tarfile.open(fileobj=archive, mode='w') as tar:
                if (lab.ROOT/baseline).exists(): tar.add(lab.ROOT/baseline, arcname=str(baseline))
            archive.seek(0)
            subprocess.run(ssh+[shlex.join(['tar','-xf','-','-C',directory])],stdin=archive,check=True)
        command = ['env', 'LUCENT_WORKSPACE='+directory, 'VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json']
        if args.update: command += ['LUCENT_UPDATE_GOLDENS=1']
        run = subprocess.run(ssh+[shlex.join(command+[directory+'/tests', 'visual_regressions', '--ignored', '--nocapture'])])
        with tempfile.TemporaryFile() as archive:
            paths = ['reports/local/visual-tests'] + ([str(baseline)] if args.update else [])
            subprocess.run(ssh+[shlex.join(['tar','-cf','-','-C',directory,*paths])], stdout=archive, check=True)
            archive.seek(0)
            with tarfile.open(fileobj=archive) as tar: tar.extractall(lab.ROOT, filter='data')
        print('Visual report: reports/local/visual-tests/index.html')
        if run.returncode: raise SystemExit(run.returncode)
    finally:
        remote('rm', '-rf', '--', directory)


if __name__ == '__main__': main()
