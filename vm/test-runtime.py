#!/usr/bin/env python3
"""Verify native application lifetime and session behavior in the Lucent guest.

Creates one terminal through the launcher, restarts Lucent, and closes only that
terminal. Optional secure lock/unlock uses the existing guarded password helper.
No unrelated settings or applications are changed.
"""
import argparse
import json
from pathlib import Path
import runpy
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--login-file', type=Path)
    args = parser.parse_args()
    v = runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='harness')
    session, inspect, cli = v['session'], v['inspect'], v['cli']
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the VM first'
    v['eventually'](lambda: inspect()['client']['background_ready'], 'Initial desktop did not render', timeout=40)
    before = {c['address'] for c in json.loads(session('hyprctl', '-j', 'clients'))}
    terminal = None
    try:
        cli('launcher', 'open')
        v['click']('launcher-search')
        v['key'](29, 30)
        v['text']('foot')
        v['eventually'](lambda: inspect()['client']['query'] == 'foot', 'Search input did not update')
        v['key'](28)
        def created():
            return [c for c in json.loads(session('hyprctl', '-j', 'clients'))
                    if c['address'] not in before and c['class'].startswith('foot')]
        v['eventually'](lambda: bool(created()), 'Launcher did not start a terminal')
        terminal = created()[0]
        group = v['remote']('cat', f'/proc/{terminal["pid"]}/cgroup')
        assert 'lucent.service' not in group and '.service' in group, group
        session('systemctl', '--user', 'restart', 'lucent.service')
        v['eventually'](lambda: inspect()['client']['background_ready'], 'Desktop did not recover after restart', timeout=40)
        assert any(c['address'] == terminal['address'] and c['pid'] == terminal['pid']
                   for c in json.loads(session('hyprctl', '-j', 'clients'))), 'Restart killed the launched terminal'
        print('Real launcher input and application survival across Lucent restart passed', flush=True)
        lock_verified = False
        if args.login_file:
            session('python3', '/home/omarchy/.local/lib/lucent/lock.py')
            assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'true'
            assert v['remote']('pgrep', '-x', 'lucent-lock').strip()
            subprocess.run([sys.executable, str(Path(__file__).with_name('run.py')),
                            '--type-password', '--login-file', str(args.login_file), 'lucent'], check=True)
            v['eventually'](lambda: session('omarchy-shell', 'lock', 'isLocked').strip() == 'false',
                            'Native unlock failed', timeout=25)
            lock_verified = True
            print('Updated native secure lock/unlock passed', flush=True)
        report = {'launcher_input': True, 'independent_application_service': True,
                  'application_survives_shell_restart': True, 'restart_restores_background': True,
                  'native_lock_unlock': lock_verified,
                  'surface_fault_injection': False,
                  'notes': 'Native VM checks. Presentation recovery policy is unit-tested; driver fault injection is not covered here.'}
        (v['REPORT']/'lucent-runtime-foundations.json').write_text(json.dumps(report, indent=2)+'\n')
    finally:
        if terminal:
            session('hyprctl', 'dispatch', f'hl.dsp.window.close({{ window = "address:{terminal["address"]}" }})')
        cli('launcher', 'close')


if __name__ == '__main__':
    main()
