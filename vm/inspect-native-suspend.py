#!/usr/bin/env python3
"""Read-only guest S3 prerequisites or offline assessment; NEVER suspends a VM.

The explicit --guest mode refuses non-QEMU guests before any other observation.
No authentication, kernel sleep policy, domain XML or service state is changed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time

from native_suspend_policy import domain_facts, memory_modes, prerequisites, require


def bounded(path, limit=4096):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        value = os.fstat(stream.fileno())
        require(stat.S_ISREG(value.st_mode) and value.st_uid == 0)
        raw = stream.read(limit + 1)
        require(len(raw) <= limit)
        return raw.decode('ascii').strip()


def guest():
    result = subprocess.run(['systemd-detect-virt', '--vm'], capture_output=True, text=True, timeout=3)
    require(result.returncode == 0 and result.stdout.strip() in ('qemu', 'kvm'))
    states = bounded('/sys/power/state').split()
    require(len(set(states)) == len(states) and all(state in ('freeze', 'standby', 'mem', 'disk') for state in states))
    try:
        modes, selected = memory_modes(bounded('/sys/power/mem_sleep'))
    except FileNotFoundError:
        modes, selected = [], None
    import dbus
    bus = dbus.SystemBus(private=True)
    try:
        manager = dbus.Interface(bus.get_object('org.freedesktop.login1', '/org/freedesktop/login1'),
                                 'org.freedesktop.login1.Manager')
        available = str(manager.CanSuspend(timeout=3))
        require(available in ('yes', 'no', 'na', 'challenge'))
        inhibitors = manager.ListInhibitors(timeout=3)
        require(len(inhibitors) <= 256)
        # Who/Why may contain private data. Do not print or hash those strings.
        sleep_modes = [str(item[3]) for item in inhibitors if 'sleep' in str(item[0]).split(':')]
        require(all(mode in ('delay', 'block', 'block-weak') for mode in sleep_modes))
    finally:
        bus.close()
    return {'scope': 'readonly_guest_prerequisites_only', 'virtualization': result.stdout.strip(),
            'power_states': states, 'memory_modes': modes, 'selected_memory_mode': selected,
            'logind_can_suspend': available,
            'sleep_block_inhibitors': sum(mode in ('block', 'block-weak') for mode in sleep_modes),
            'sleep_delay_inhibitors': sleep_modes.count('delay'),
            'boot_id': bounded('/proc/sys/kernel/random/boot_id'),
            'monotonic_ns': time.clock_gettime_ns(time.CLOCK_MONOTONIC),
            'boottime_ns': time.clock_gettime_ns(time.CLOCK_BOOTTIME)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--guest', action='store_true')
    mode.add_argument('--assess', action='store_true')
    parser.add_argument('--domain-xml', type=Path)
    parser.add_argument('--guest-json', type=Path)
    args = parser.parse_args()
    if args.guest:
        require(args.domain_xml is None and args.guest_json is None)
        value = guest()
    else:
        require(args.domain_xml is not None and args.guest_json is not None)
        # Captures are explicitly supplied offline artifacts, not current HEAD
        # or inferred live-domain state. Keep the source hashes in the result.
        with args.domain_xml.open('rb') as stream:
            xml = stream.read(1024 * 1024 + 1)
        with args.guest_json.open('rb') as stream:
            raw = stream.read(65537)
        require(len(raw) <= 65536)
        facts = domain_facts(xml)
        value = {'domain': facts, 'prerequisites': prerequisites(facts, json.loads(raw)),
                 'domain_sha256': hashlib.sha256(xml).hexdigest(),
                 'guest_capture_sha256': hashlib.sha256(raw).hexdigest()}
    print(json.dumps(value, sort_keys=True))


if __name__ == '__main__':
    main()
