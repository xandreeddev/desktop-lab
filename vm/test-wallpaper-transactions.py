#!/usr/bin/env python3
"""Native wallpaper failure/restart checks using a generated image, with restoration.

Uses the public IPC and the real storage/decoding adapters. Does not depend on
provider availability or publish desktop captures. The existing VM must be unlocked.
"""
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
spec = importlib.util.spec_from_file_location('v', ROOT / 'vm/test-framework.py')
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def main():
    def client():
        return v.inspect()['client']

    def selected():
        return client()['wallpaper']

    def digest(path):
        return v.remote('sha256sum', path).split()[0]

    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    original = selected()
    assert original and client()['background_ready']
    state = '/home/omarchy/.local/state/lucent/desktop.json'
    backup = '/tmp/lucent-transaction-original-settings.json'
    v.remote('cp', state, backup)
    source = '/tmp/lucent-wallpaper-transaction.png'
    report = {}
    try:
        # Generate PNG without guest image libraries, networking or private assets.
        v.remote('python3', '-c', '''import struct, zlib, sys
from pathlib import Path
def chunk(kind, data):
    return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
w, h = 128, 64
pixels = b''.join(b'\\0' + bytes([32, 112, 184]) * w for _ in range(h))
Path(sys.argv[1]).write_bytes(b'\\x89PNG\\r\\n\\x1a\\n' + chunk(b'IHDR', struct.pack('!2I5B', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))
''', source)
        expected = digest(source)
        v.cli('wallpaper', 'set', source)
        v.eventually(lambda: selected()['source']['path'] == source and not client()['wallpaper_busy'], 'Generated wallpaper did not commit', timeout=25)
        committed = selected()
        assert committed['path'] != source
        assert digest(committed['path']) == expected
        v.eventually(lambda: digest('/etc/greetd/lucent-wallpaper') == expected, 'Login copy did not publish committed wallpaper', timeout=20)
        report['immutable_snapshot_and_login_copy'] = True
        v.remote('python3', '-c', 'from pathlib import Path; import sys; Path(sys.argv[1]).write_text("invalid image payload")', source)
        v.cli('wallpaper', 'set', source)
        v.eventually(lambda: not client()['wallpaper_busy'] and bool(client()['error']), 'Invalid image did not report a decode failure', timeout=25)
        assert selected() == committed and client()['background_ready']
        assert digest(committed['path']) == expected
        report['decode_failure_preserves_selected_asset'] = True
        v.session('systemctl', '--user', 'restart', 'lucent.service')
        v.eventually(lambda: client()['background_ready'] and selected() == committed, 'Restart failed after original-file corruption', timeout=35)
        report['restart_uses_immutable_snapshot'] = True
        v.cli('palette', 'wallpaper')
        v.eventually(lambda: client()['palette']['id'].startswith('wallpaper-'), 'Palette extraction did not use committed snapshot', timeout=25)
        v.eventually(lambda: json.loads(v.remote('cat', '/home/omarchy/.local/state/lucent/theme/palette.json')) == client()['palette'], 'Palette exports did not converge', timeout=35)
        report['palette_reads_snapshot_and_exports'] = True
        print(json.dumps(report, indent=2), flush=True)
    finally:
        # A legacy original is copied on first reselection; retained snapshots can
        # be selected directly. Compare provenance instead of requiring a legacy path.
        v.cli('wallpaper', 'set', original['path'])
        v.eventually(lambda: not client()['wallpaper_busy'] and selected()['source']['path'] == original['source']['path'], 'Original wallpaper did not restore', timeout=30)
        v.session('systemctl', '--user', 'stop', 'lucent.service')
        v.remote('cp', backup, state)
        v.session('systemctl', '--user', 'start', 'lucent.service')
        v.eventually(lambda: client()['background_ready'], 'Restored wallpaper did not render', timeout=35)
        v.eventually(lambda: json.loads(v.remote('cat', '/home/omarchy/.local/state/lucent/theme/palette.json')) == client()['palette'], 'Original palette export did not restore', timeout=35)
        v.remote('rm', '-f', source, backup)
        print('Original wallpaper and palette restored', flush=True)
    (ROOT / 'reports/local/lucent-wallpaper-transactions.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
