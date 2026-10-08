#!/usr/bin/env python3
"""Create isolated Omarchy clones in the user's libvirt session."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
STATE = ROOT / '.local-vms'
URI = 'qemu:///session'
QEMU = 'http://libvirt.org/schemas/domain/qemu/1.0'
ET.register_namespace('qemu', QEMU)
PROFILES = {'lucid': 2241, 'noctalia': 2242, 'lucent': 2243}


def run(*args, capture=False, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=capture, **kwargs)


def virsh(*args, capture=False):
    return run('virsh', '-c', URI, *args, capture=capture)


def inventory():
    return json.loads((STATE / 'inventory.json').read_text())


def ssh_args(profile):
    data = inventory()
    return ['ssh', '-i', data['ssh_key'], '-p', str(PROFILES[profile]),
            '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes',
            '-o', 'StrictHostKeyChecking=accept-new',
            '-o', f'UserKnownHostsFile={STATE / "known_hosts"}',
            '-o', 'ConnectTimeout=5', f'{data["user"]}@127.0.0.1']


def create(args):
    STATE.mkdir(mode=0o700, exist_ok=True)
    if (STATE / 'inventory.json').exists():
        raise SystemExit('Inventory exists; use status/start/console. Refusing to replace disks.')
    if virsh('domstate', args.base, capture=True).stdout.strip() != 'shut off':
        raise SystemExit('Base VM must be shut off before copying it.')
    source = ET.fromstring(virsh('dumpxml', args.base, capture=True).stdout)
    disks = source.findall('./devices/disk[@device="disk"]')
    if len(disks) != 1 or disks[0].find('driver').get('type') != 'qcow2':
        raise SystemExit('Expected exactly one qcow2 base disk.')
    source_disk = Path(disks[0].find('source').get('file'))
    key = Path(args.ssh_key).expanduser().resolve()
    if not key.is_file():
        raise SystemExit('SSH key does not exist.')
    base = STATE / 'base.qcow2'
    if base.exists():
        raise SystemExit('Base snapshot exists without inventory; inspect before retrying.')
    # Flatten any source backing chain so later base-VM changes cannot affect clones.
    run('qemu-img', 'convert', '-p', '-O', 'qcow2', str(source_disk), str(base))
    base.chmod(0o400)
    data = {'uri': URI, 'user': args.user, 'ssh_key': str(key), 'vms': {}}
    for profile, port in PROFILES.items():
        name = f'desktop-lab-{profile}'
        path = STATE / profile
        path.mkdir(mode=0o700)
        disk = path / 'disk.qcow2'
        run('qemu-img', 'create', '-f', 'qcow2', '-F', 'qcow2', '-b', str(base), str(disk))
        tree = ET.fromstring(ET.tostring(source))
        tree.find('name').text = name
        tree.find('uuid').text = str(uuid.uuid4())
        for tag in ('memory', 'currentMemory'):
            tree.find(tag).text = str(args.memory * 1024)
        tree.find('vcpu').text = str(args.cpus)
        tree.find('./devices/disk[@device="disk"]/source').set('file', str(disk))
        nvram = tree.find('./os/nvram')
        if nvram is not None:
            nvram_dest = path / 'nvram.fd'
            shutil.copyfile(nvram.text, nvram_dest)
            nvram.text = str(nvram_dest)
        for cd in tree.findall('./devices/disk[@device="cdrom"]'):
            src = cd.find('source')
            if src is not None:
                cd.remove(src)
        for interface in tree.findall('./devices/interface'):
            tree.find('devices').remove(interface)
        for elem in tree.findall(f'{{{QEMU}}}commandline'):
            tree.remove(elem)
        command = ET.SubElement(tree, f'{{{QEMU}}}commandline')
        for value in ('-netdev', f'user,id=net0,hostfwd=tcp:127.0.0.1:{port}-:22',
                      '-device', f'virtio-net-pci,netdev=net0,bus=pcie.0,addr=0x10,mac=52:54:00:58:41:{port % 256:02x}'):
            ET.SubElement(command, f'{{{QEMU}}}arg', value=value)
        for elem in tree.findall('./devices/channel/source'):
            # libvirt allocates fresh per-domain sockets.
            elem.attrib.pop('path', None)
        xml = path / 'domain.xml'
        ET.indent(tree)
        xml.write_bytes(ET.tostring(tree))
        virsh('define', str(xml))
        data['vms'][profile] = {'name': name, 'port': port}
    (STATE / 'inventory.json').write_text(json.dumps(data, indent=2) + '\n')
    print('Created three independent clones. Start one at a time on memory-limited hosts.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    setup = sub.add_parser('create')
    setup.add_argument('--base', default='omarchy-test-vm')
    setup.add_argument('--ssh-key', required=True)
    setup.add_argument('--user', default='omarchy')
    setup.add_argument('--memory', type=int, default=3072)
    setup.add_argument('--cpus', type=int, default=2)
    sub.add_parser('status')
    for action in ('start', 'shutdown', 'console', 'ssh'):
        p = sub.add_parser(action)
        p.add_argument('profile', choices=PROFILES)
        if action == 'ssh':
            p.add_argument('remote_command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command == 'create':
        create(args)
        return
    if args.command == 'status':
        for profile, vm in inventory()['vms'].items():
            print(profile, virsh('domstate', vm['name'], capture=True).stdout.strip(),
                  f'SSH 127.0.0.1:{vm["port"]}')
        return
    name = inventory()['vms'][args.profile]['name']
    if args.command == 'start':
        for profile, vm in inventory()['vms'].items():
            if profile != args.profile and virsh('domstate', vm['name'], capture=True).stdout.strip() == 'running':
                raise SystemExit(f'{profile} is running. Shut it down before starting another lab VM.')
        if virsh('domstate', name, capture=True).stdout.strip() == 'shut off':
            virsh('start', name)
        deadline = time.monotonic() + 150
        while time.monotonic() < deadline:
            p = subprocess.run(ssh_args(args.profile) + ['true'], capture_output=True)
            if p.returncode == 0:
                print(f'{name}: SSH ready')
                return
            time.sleep(3)
        raise SystemExit('VM started; SSH not ready. Inspect its console.')
    elif args.command == 'ssh':
        os.execvp('ssh', ssh_args(args.profile) + args.remote_command)
    elif args.command == 'console':
        run('virt-manager', '--connect', URI, '--show-domain-console', name)
    elif args.command == 'shutdown':
        virsh('shutdown', name, '--mode', 'agent')
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            if virsh('domstate', name, capture=True).stdout.strip() == 'shut off':
                return
            time.sleep(2)
        raise SystemExit('Shutdown is taking longer than expected; inspect the console.')


if __name__ == '__main__':
    main()
