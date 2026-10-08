#!/usr/bin/env python3
"""Provision lab guests over loopback SSH. Credentials never enter the repository."""
import argparse
import getpass
from pathlib import Path
import shlex
import subprocess
import sys
import tarfile
import tempfile

import lab


def password(args):
    if args.login_file:
        for line in Path(args.login_file).expanduser().read_text().splitlines():
            if line.startswith('Password:'):
                return line.split(':', 1)[1].strip()
        raise SystemExit('No Password: field in login file.')
    return getpass.getpass('Guest password: ')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('profile', choices=lab.PROFILES)
    p.add_argument('--login-file')
    p.add_argument('--root', action='store_true')
    p.add_argument('--sync', action='store_true')
    p.add_argument('--type-password', action='store_true')
    p.add_argument('--session', action='store_true')
    p.add_argument('command', nargs=argparse.REMAINDER)
    args = p.parse_args()
    if args.type_password:
        vm = lab.inventory()['vms'][args.profile]['name']
        normal = '`1234567890-=qwertyuiop[]\\asdfghjkl;\'zxcvbnm,./ '
        shifted = '~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>? '
        codes = [41,2,3,4,5,6,7,8,9,10,11,12,13,16,17,18,19,20,21,22,23,24,25,26,27,43,30,31,32,33,34,35,36,37,38,39,40,44,45,46,47,48,49,50,51,52,53,57]
        secret = password(args)
        for char in secret:
            if char in normal:
                keys = [str(codes[normal.index(char)])]
            elif char in shifted:
                keys = ['42', str(codes[shifted.index(char)])]
            else:
                raise SystemExit('Password contains a character unsupported by the US test keyboard.')
            lab.virsh('send-key', vm, '--codeset', 'linux', '--holdtime', '30', *keys, capture=True)
        lab.virsh('send-key', vm, '--codeset', 'linux', '28', capture=True)
        print('Guest password submitted to the focused password field.')
        return
    if args.sync:
        with tempfile.NamedTemporaryFile(suffix='.tar.gz') as temp:
            with tarfile.open(temp.name, 'w:gz') as tf:
                for name in ('scripts', 'configs', 'manifests', 'patches', 'lucent'):
                    path = lab.ROOT / name
                    if path.exists():
                        tf.add(path, arcname=name, filter=lambda item: None if '/target/' in item.name or '/__pycache__/' in item.name else item)
            with open(temp.name, 'rb') as stream:
                subprocess.run(lab.ssh_args(args.profile) + ['mkdir -p ~/desktop-lab && tar -xz -C ~/desktop-lab'], stdin=stream, check=True)
        print('Synced repository configuration and scripts to ~/desktop-lab.')
        return
    command = args.command
    if command and command[0] == '--':
        command = command[1:]
    if not command:
        p.error('Provide a command after --')
    cmd = shlex.join(command)
    if args.session:
        cmd = 'exec python3 ~/desktop-lab/scripts/in-session.py ' + cmd
    if args.root:
        cmd = 'sudo -S -p "" ' + cmd
    result = subprocess.run(lab.ssh_args(args.profile) + [cmd], text=True,
                            input=password(args) + '\n' if args.root else None)
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
