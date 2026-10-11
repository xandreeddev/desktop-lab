#!/usr/bin/env python3
"""Check native graceful IPC shutdown/restart and complete profiled responses.

Restarts only lucent.service. Temporary profiling/idle overrides are restored;
no preferences, wallpaper, login configuration or existing app windows are changed.
"""
import json
from pathlib import Path
import runpy
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
h = runpy.run_path(str(ROOT / 'vm/test-framework.py'), run_name='harness')
m = runpy.run_path(str(ROOT / 'vm/measure-framework.py'), run_name='measurement')


def ready():
    h['eventually'](lambda: h['inspect']()['client']['background_ready'],
                    'Desktop did not become ready', timeout=45)


def main():
    assert h['session']('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the VM first'
    report = {}
    with m['stay_awake'](h['remote']):
        try:
            ready()
            for _ in range(2):
                assert h['cli']('quit').strip() == 'ok', 'Graceful quit acknowledgement was lost'
                h['eventually'](
                    lambda: h['session']('systemctl', '--user', 'show', 'lucent.service', '--property=ActiveState', '--value').strip() == 'inactive',
                    'Desktop service did not exit cleanly', timeout=15)
                assert h['session']('python3', '-c', "import os; from pathlib import Path; print(int((Path(os.environ['XDG_RUNTIME_DIR'])/'lucent.sock').exists()))").strip() == '0', 'Owned socket survived quit'
                h['session']('systemctl', '--user', 'start', 'lucent.service')
                ready()
            report['graceful_quit_ack_socket_cleanup_and_restart_cycles'] = 2
            with m['frame_profiling'](h['remote'], h['session'], True):
                h['session']('systemctl', '--user', 'restart', 'lucent.service')
                ready()
                for _ in range(8):
                    h['cli']('launcher', 'open'); time.sleep(.7)
                    h['cli']('launcher', 'close'); time.sleep(.7)
                    snapshot = h['inspect']()
                    dock = next(s for s in snapshot['surfaces'] if s['id'] == 'dock')
                    if len(dock['frame_profiles']) >= 60:
                        break
                snapshot = h['inspect']()
                assert snapshot['frame_profiling']
                dock = next(s for s in snapshot['surfaces'] if s['id'] == 'dock')
                assert len(dock['frame_profiles']) >= 60, 'Profiling fixture did not collect enough frames'
                report['complete_profiled_json_bytes'] = len(json.dumps(snapshot).encode())
                report['dock_frame_profiles'] = len(dock['frame_profiles'])
            ready()
            assert not h['inspect']()['frame_profiling'], 'Temporary profiling configuration survived'
            report['profiling_and_idle_override_restored'] = True
        finally:
            h['session']('systemctl', '--user', 'start', 'lucent.service')
            ready()
            h['cli']('launcher', 'close')
    report['native_wayland_error_injection'] = False
    (h['REPORT'] / 'lucent-ipc-lifetime.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
