#!/usr/bin/env python3
"""Exercise native lazy previews past 64 files, bounded retention and retry.

Creates uniquely named images, browses without changing wallpaper/preferences,
and removes only those test files. Restarts the normal desktop on cleanup.
"""
import json
from pathlib import Path
import runpy
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
v = runpy.run_path(str(ROOT / 'vm/test-framework.py'), run_name='harness')
remote, session, cli = v['remote'], v['session'], v['cli']
eventually = v['eventually']


def client():
    return v['inspect']()['client']


def cache():
    return client()['prepared_assets']['local_previews']


def suite():
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    eventually(lambda: client()['background_ready'], 'Initial background not ready', timeout=35)
    initial = client()
    state = '/home/omarchy/.local/state/lucent/desktop.json'
    saved = remote('cat', state)
    tag = 'zzzz-lucent-preview-' + uuid.uuid4().hex
    checks = []
    samples = []
    try:
        remote('python3', '-c', '''from pathlib import Path
import struct,sys,zlib
root=Path.home()/'.config/lucent/wallpapers'; root.mkdir(parents=True,exist_ok=True)
def chunk(kind,data):
    return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data))
w=h=1024
for i in range(73):
    pixels=(b'\\0'+bytes([i*3%256,112,184])*w)*h
    png=b'\\x89PNG\\r\\n\\x1a\\n'+chunk(b'IHDR',struct.pack('!2I5B',w,h,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(pixels))+chunk(b'IEND',b'')
    with (root/(sys.argv[1]+f'-{i:03}.png')).open('xb') as f:
        f.write(b'invalid preview fixture' if i==72 else png)
''', tag)
        session('systemctl', '--user', 'restart', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['wallpaper_count'] >= 73,
                   'Generated preview library not discovered', timeout=35)
        assert cache()['entries'] <= 1 and not cache()['active']
        checks.append('closed_browser_does_not_eagerly_decode_library')
        cli('wallpapers', 'open')
        v['move'](960, 250)
        v['key'](102)  # Home selects first card without applying it.
        count = client()['wallpaper_count']
        for i in range(count):
            v['key'](106)
            if i % 12 == 11:
                eventually(lambda: not cache()['active'], 'Preview worker did not settle', timeout=25)
                snapshot = cache()
                assert snapshot['entries'] <= snapshot['entry_limit']
                assert snapshot['bytes'] <= max(snapshot['reuse_budget_bytes'], snapshot['pinned_bytes'])
                samples.append(snapshot)
        eventually(lambda: client()['wallpaper_index'] >= 64 and not cache()['active'],
                   'Did not reach and settle a card beyond the old preload limit', timeout=25)
        assert cache()['wanted_ready'] > 0
        checks.append('native_navigation_beyond_64_with_bounded_retention')
        assert cache()['failed'] == 1
        remote('python3', '-c', "from pathlib import Path; import sys; p=Path.home()/'.config/lucent/wallpapers'; (p/(sys.argv[1]+'-072.png')).write_bytes((p/(sys.argv[1]+'-071.png')).read_bytes())", tag)
        cli('wallpapers', 'retry')
        eventually(lambda: not cache()['active'] and cache()['failed'] == 0 and
                   cache()['wanted_ready'] == cache()['wanted'], 'Fixed preview did not reload', timeout=25)
        checks.append('decode_failure_and_explicit_retry')
        cli('launcher', 'close')
        eventually(lambda: cache()['wanted'] == 0 and not cache()['active'], 'Closed demand did not stop', timeout=8)
        assert cache()['bytes'] <= cache()['closed_budget_bytes']
        checks.append('closing_browser_releases_extra_cpu_previews')
        assert client()['wallpaper'] == initial['wallpaper']
        assert remote('cat', state) == saved
    finally:
        cli('launcher', 'close')
        remote('python3', '-c', "from pathlib import Path; import sys; p=Path.home()/'.config/lucent/wallpapers'; [(p/(sys.argv[1]+f'-{i:03}.png')).unlink(missing_ok=True) for i in range(73)]", tag)
        session('systemctl', '--user', 'restart', 'lucent.service')
        eventually(lambda: client()['background_ready'], 'Normal desktop not ready', timeout=35)
        assert client()['wallpaper'] == initial['wallpaper']
        assert remote('cat', state) == saved
    report = {'checks': checks, 'retention_samples': samples,
              'wallpaper_and_preferences_unchanged': True, 'owned_test_files_removed': True}
    (v['REPORT'] / 'lucent-preview-cache.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)


def main():
    measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
    with measure['stay_awake'](remote):
        suite()


if __name__ == '__main__':
    main()
