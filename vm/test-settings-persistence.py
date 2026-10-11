#!/usr/bin/env python3
"""Exercise a real settings-write failure, visible retry and latest-note recovery.

Creates an owned directory at the adapter's temporary-file path to reject writes.
Never overwrites an existing temporary path. Restores settings/workspace and removes
only the owned injection in finally. Requires an unlocked prepared Lucent VM.
"""
import json
from pathlib import Path
import runpy
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
v = runpy.run_path(str(ROOT / 'vm/test-framework.py'), run_name='harness')
remote, session, cli = v['remote'], v['session'], v['cli']
eventually = v['eventually']


def client():
    return v['inspect']()['client']


def suite():
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    eventually(lambda: client()['background_ready'] and client()['settings_save_status'] == 'saved',
               'Initial settings not ready', timeout=35)
    initial = client()
    workspace = json.loads(session('hyprctl', '-j', 'activeworkspace'))['id']
    assert workspace > 0
    occupied = {w['id'] for w in json.loads(session('hyprctl', '-j', 'workspaces'))}
    target = next(i for i in range(70, 10000) if i not in occupied)
    state = '/home/omarchy/.local/state/lucent/desktop.json'
    temporary = '/home/omarchy/.local/state/lucent/desktop.tmp'
    backup = remote('mktemp', '/tmp/lucent-persistence-test.XXXXXX').strip()
    remote('cp', state, backup)
    injected = False
    checks = []
    try:
        session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{target}" }})')
        cli('widget', 'notes', 'show')
        cli('widgets', 'open')
        v['click']('reset-layout')
        cli('launcher', 'close')
        eventually(lambda: client()['settings_save_status'] == 'saved', 'Preparation did not save')
        before = remote('cat', state)
        remote('mkdir', temporary)  # Fails if any preexisting file/directory exists.
        injected = True
        v['click']('notes-input')
        v['text'](' persistent')
        eventually(lambda: client()['settings_save_status'] == 'failed', 'Write failure not visible')
        assert client()['settings_save_error']
        assert client()['error'] == client()['settings_save_error']
        assert remote('cat', state) == before, 'Failed write changed the original settings'
        assert v['hit']('notes-save-retry')
        checks.append('real_write_failure_preserves_disk_and_exposes_retry')

        v['text'](' latest')
        eventually(lambda: client()['notes'].endswith(' persistent latest') and
                   client()['settings_save_status'] == 'failed', 'Newer failed intent not retained')
        latest = client()['notes']
        assert remote('cat', state) == before
        remote('rmdir', temporary)
        injected = False
        v['click']('notes-save-retry')
        eventually(lambda: client()['settings_save_status'] == 'saved' and
                   json.loads(remote('cat', state))['notes'] == latest, 'Retry did not save latest edit')
        assert not client()['settings_save_error']
        assert not client()['error']
        checks.append('native_retry_saves_latest_edit_and_clears_matching_failure')

        cli('settings', 'retry')  # A retry after acknowledgement is a no-op.
        session('systemctl', '--user', 'restart', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['settings_save_status'] == 'saved',
                   'Restart did not load acknowledged settings', timeout=35)
        assert client()['notes'] == latest
        checks.append('acknowledged_notes_survive_restart')
    finally:
        v['button'](False)
        session('systemctl', '--user', 'stop', 'lucent.service')
        if injected:
            remote('rmdir', temporary)
        remote('cp', backup, state)
        session('systemctl', '--user', 'start', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['settings_save_status'] == 'saved',
                   'Original desktop not restored', timeout=35)
        for field in ('positions', 'widgets', 'notes', 'wallpaper', 'palette'):
            assert client()[field] == initial[field], 'Failed to restore ' + field
        session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{workspace}" }})')
        remote('rm', '-f', backup)
    report = {'checks': checks, 'preferences_and_workspace_restored': True,
              'owned_failure_injection_removed': True, 'native_queue_overload_injection': False}
    (v['REPORT'] / 'lucent-settings-persistence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)


def main():
    measure = runpy.run_path(str(ROOT / 'vm/measure-framework.py'))
    with measure['stay_awake'](remote):
        suite()


if __name__ == '__main__':
    main()
