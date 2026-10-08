#!/usr/bin/env python3
"""Enable Venus for the stopped Lucent VM; preserve its previous XML for rollback."""
import argparse
import xml.etree.ElementTree as ET

import lab


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--restore', action='store_true')
    parser.add_argument('--experimental-venus', action='store_true',
                        help='Opt into the experimental device setup; see docs/lucent-vulkan.md')
    args = parser.parse_args()
    if not args.restore and not args.experimental_venus:
        parser.error('Use --restore, or --experimental-venus on a host with a reviewed renderer setup. '
                     'Venus did not work under this lab host’s default sandbox; software Vulkan is prepared.')
    name = lab.inventory()['vms']['lucent']['name']
    if lab.virsh('domstate', name, capture=True).stdout.strip() != 'shut off':
        raise SystemExit('Shut down the Lucent VM before changing its graphics device.')
    backup = lab.STATE / 'lucent/before-venus.xml'
    if args.restore:
        if not backup.is_file():
            raise SystemExit('No saved pre-Venus definition.')
        lab.virsh('define', str(backup))
        return
    xml = lab.virsh('dumpxml', name, '--inactive', capture=True).stdout
    tree = ET.fromstring(xml)
    video = tree.find('./devices/video')
    if video is None or video.find('model').get('type') != 'virtio':
        raise SystemExit('Expected the lab virtio video device.')
    alias = video.find('alias')
    if alias is None:
        alias = ET.SubElement(video, 'alias', name='ua-lucent-video')
    if not backup.exists():
        backup.write_text(xml)
        backup.chmod(0o600)
    backing = tree.find('memoryBacking')
    if backing is None:
        backing = ET.SubElement(tree, 'memoryBacking')
    for tag, attr, value in [('source', 'type', 'memfd'), ('access', 'mode', 'shared')]:
        element = backing.find(tag)
        if element is None:
            element = ET.SubElement(backing, tag)
        element.set(attr, value)
    ns = '{' + lab.QEMU + '}'
    override = tree.find(ns + 'override')
    if override is None:
        override = ET.SubElement(tree, ns + 'override')
    device = override.find(f"{ns}device[@alias='{alias.get('name')}']")
    if device is None:
        device = ET.SubElement(override, ns + 'device', alias=alias.get('name'))
    frontend = device.find(ns + 'frontend')
    if frontend is None:
        frontend = ET.SubElement(device, ns + 'frontend')
    for name, kind, value in [('hostmem', 'unsigned', '1073741824'),
                              ('blob', 'bool', 'true'), ('venus', 'bool', 'true')]:
        prop = frontend.find(f"{ns}property[@name='{name}']")
        if prop is None:
            prop = ET.SubElement(frontend, ns + 'property', name=name)
        prop.set('type', kind)
        prop.set('value', value)
    destination = lab.STATE / 'lucent/venus.xml'
    ET.indent(tree)
    destination.write_bytes(ET.tostring(tree))
    destination.chmod(0o600)
    lab.virsh('define', str(destination))
    print('Venus enabled with a 1 GiB host-visible aperture. Previous definition saved locally.')


if __name__ == '__main__':
    main()
