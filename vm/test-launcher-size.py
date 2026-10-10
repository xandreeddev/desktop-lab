#!/usr/bin/env python3
"""Exercise saved launcher sizes in the running, unlocked Lucent guest."""
import importlib.util
import json
import subprocess
import time
from pathlib import Path

spec = importlib.util.spec_from_file_location('framework', Path(__file__).with_name('test-framework.py'))
fw = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fw)


def ready(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            data = fw.inspect()
            if predicate(data):
                return data
        except (ValueError, OSError, subprocess.CalledProcessError):
            pass
        time.sleep(.2)
    raise AssertionError('Lucent did not reach the expected state')


def main():
    # Save only launcher preferences; all other settings stay untouched.
    original = fw.inspect()['client']['launcher_size']
    report = []
    try:
        fw.cli('launcher', 'size', '800', '720')
        fw.cli('launcher', 'open')
        data = ready(lambda d: d['client']['launcher_panel_size'][0] == 800)
        assert data['client']['launcher_panel_size'][1] <= 720
        report.append({'custom': data['client']['launcher_panel_size']})
        # An IPC acknowledgement precedes the ordered background settings save.
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            saved = json.loads(fw.remote('cat', '/home/omarchy/.local/state/lucent/desktop.json'))
            if saved.get('launcher') == {'width': 800, 'max_height': 720}:
                break
            time.sleep(.2)
        else:
            raise AssertionError('Custom size was not persisted')
        fw.session('systemctl', '--user', 'restart', 'lucent.service')
        ready(lambda d: d['client']['applications'] > 0 and d['client']['launcher_size'] == {'width': 800, 'max_height': 720})
        fw.cli('launcher', 'open')
        ready(lambda d: d['client']['launcher_panel_size'][0] == 800)
        report.append({'survives_restart': True})
        fw.cli('themes', 'open')
        fw.click('launcher-size-small')
        ready(lambda d: d['client']['launcher_size'] == {'width': 448, 'max_height': 480})
        fw.click('launcher-size-large')
        ready(lambda d: d['client']['launcher_size'] == {'width': 800, 'max_height': 800})
        fw.click('launcher-size-default')
        ready(lambda d: d['client']['launcher_size'] == {'width': None, 'max_height': None})
        report.append({'all_presets_clickable': True})
        fw.cli('launcher', 'open')
        fw.screenshot('lucent-launcher-size.png')
        output = fw.REPORT / 'launcher-size.json'
        output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
    finally:
        fw.session('systemctl', '--user', 'stop', 'lucent.service')
        fw.remote('python3', '-c', "import json,sys; from pathlib import Path; p=Path.home()/'.local/state/lucent/desktop.json'; d=json.loads(p.read_text()); d['launcher']=json.loads(sys.argv[1]); t=p.with_suffix('.test-restore'); t.write_text(json.dumps(d,indent=2)+'\\n'); t.replace(p)", json.dumps(original))
        fw.session('systemctl', '--user', 'start', 'lucent.service')
        ready(lambda d: d['client']['applications'] > 0 and d['client']['launcher_size'] == original)
        fw.cli('launcher', 'open')


if __name__ == '__main__':
    main()
