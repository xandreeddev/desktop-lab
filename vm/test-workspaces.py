#!/usr/bin/env python3
"""Verify actual compositor state and client-owned workspace shortcuts in the VM.

Uses the prepared wide single-monitor guest. Restores its original workspace;
does not change saved preferences or close applications.
"""
from native_surfaces import active_surfaces, find_surface, role
import json
from pathlib import Path
import runpy


def main():
    vm = runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='harness')
    session, inspect = vm['session'], vm['inspect']
    assert session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the VM first'
    original = json.loads(session('hyprctl', '-j', 'activeworkspace'))['id']
    assert original > 0, 'This test requires a standard workspace'

    def state_matches():
        def native_identity(value):
            address = value.get('address')
            if address:
                if address == 'special': return ('special', '')
                if address.startswith('special:'): return ('special', address[8:])
                if address.startswith('name:'): return ('named', address[5:])
                if address.isdecimal(): return ('numbered', int(address))
                return ('named', address)
            identifier = value.get('id')
            if isinstance(identifier, int) and identifier > 0: return ('numbered', identifier)
            name = value.get('name', '')
            if name == 'special': return ('special', '')
            if name.startswith('special:'): return ('special', name[8:])
            if name: return ('named', name)
            return None
        reported = {('special' if w['special'] else 'named', w['name'])
                    if w['id'] is None else ('numbered', w['id'])
                    for w in inspect()['client']['workspaces']}
        actual = {native_identity(w) for w in json.loads(session('hyprctl', '-j', 'workspaces'))}
        actual.discard(None)
        return reported == actual

    try:
        vm['eventually'](state_matches, 'Adapter invented or omitted workspaces')
        ids = {h['id'] for h in find_surface(inspect(), 'bar')['hits']}
        assert {f'workspace-{i}' for i in range(1, 7)} <= ids, 'Default shortcuts missing'
        target = 2 if original != 2 else 1
        vm['click'](f'workspace-{target}')
        vm['eventually'](lambda: json.loads(session('hyprctl', '-j', 'activeworkspace'))['id'] == target,
                         'Shortcut did not switch the real workspace')
        vm['eventually'](state_matches, 'Adapter did not reflect the workspace change')
        report = {'actual_compositor_workspaces': True, 'client_shortcuts': True,
                  'native_workspace_switch': True, 'scope': 'Prepared single-monitor wide VM'}
        (vm['REPORT'] / 'lucent-workspace-policy.json').write_text(json.dumps(report, indent=2)+'\n')
        print(json.dumps(report, indent=2))
    finally:
        session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{original}" }})')


if __name__ == '__main__':
    main()
