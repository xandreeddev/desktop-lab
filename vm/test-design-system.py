#!/usr/bin/env python3
"""Exercise theme switching and integer HiDPI in the prepared Lucent VM.

Requires an unlocked, single-output guest. Restores the original output, theme,
widget settings and idle policy even when a check fails. Captures stay local.
"""
import importlib.util
import json
import struct
import subprocess
import time
from pathlib import Path

spec = importlib.util.spec_from_file_location('framework_test', Path(__file__).with_name('test-framework.py'))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)


def output_mode(monitor, mode, scale):
    # JSON quoting is compatible with these Lua string arguments.
    code = 'hl.monitor({output=%s, mode=%s, position=%s, scale=%s})' % (
        json.dumps(monitor['name']), json.dumps(mode),
        json.dumps(f"{monitor['x']}x{monitor['y']}"), scale)
    response = vm.session('hyprctl', 'eval', code)
    if 'error' in response.lower():
        raise RuntimeError(response)


def main():
    monitors = json.loads(vm.session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1, 'This lab check expects one virtual display'
    monitor = monitors[0]
    assert monitor['width'] / monitor['scale'] == 1920 and monitor['height'] / monitor['scale'] == 1080, 'Start at 1920×1080 logical pixels'
    assert vm.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the VM first'
    assert any(mode.startswith('3840x2160@60') for mode in monitor['availableModes']), '4K/60 mode unavailable'
    settings = vm.remote('bash', '-c', 'cat ~/.local/state/lucent/desktop.json 2>/dev/null || true')
    awake = json.loads(vm.session('omarchy-toggle-idle', 'status'))['enabled']
    report = {'renderer': 'Vulkan', 'checks': []}
    try:
        vm.session('omarchy-toggle-idle', 'stay-awake')
        vm.session('omarchy-shell', 'idle', 'disable')
        for scale in (1, 2):
            output_mode(monitor, f'{1920*scale}x{1080*scale}@60', scale)
            vm.eventually(lambda: all(s.get('scale') == scale for s in vm.inspect()['surfaces']), 'Output scale did not reach all native surfaces', timeout=20)
            vm.cli('launcher', 'open')
            time.sleep(1)
            vm.click('mode-themes')
            for mode in ('dark', 'light'):
                vm.click('theme-'+mode)
                time.sleep(1)
                vm.screenshot(f'lucent-design-{mode}-{scale}x.png')
                image = vm.REPORT / f'lucent-design-{mode}-{scale}x.png'
                dimensions = struct.unpack('>II', image.read_bytes()[16:24])
                assert dimensions == (1920*scale,1080*scale), dimensions
                surfaces = vm.inspect()['surfaces']
                assert {s['id'] for s in surfaces} == {'bar','widgets','dock'}
                assert all(s['buffer_width'] == s['width']*scale and s['buffer_height'] == s['height']*scale for s in surfaces)
                assert next(s for s in surfaces if s['id']=='dock')['width'] == 1920
                assert not vm.inspect()['client']['error'], 'Desktop service reported an error'
                report['checks'].append({'theme':mode, 'scale':scale, 'capture_pixels':dimensions, 'logical_width':1920, 'surfaces':3})
            vm.key(1)
    finally:
        output_mode(monitor, f"{monitor['width']}x{monitor['height']}@{monitor['refreshRate']}", monitor['scale'])
        vm.session('systemctl','--user','stop','lucent.service')
        if settings:
            subprocess.run(vm.lab.ssh_args(vm.PROFILE)+['cat > ~/.local/state/lucent/desktop.json.new && mv ~/.local/state/lucent/desktop.json.new ~/.local/state/lucent/desktop.json'], input=settings, text=True, check=True)
        else:
            vm.remote('rm','-f','.local/state/lucent/desktop.json')
        vm.session('systemctl','--user','start','lucent.service')
        if not awake:
            vm.session('omarchy-toggle-idle','allow-idle')
            vm.session('omarchy-shell','idle','enable')
    restored = json.loads(vm.session('hyprctl','-j','monitors'))[0]
    assert (restored['width'],restored['height'],restored['scale']) == (monitor['width'],monitor['height'],monitor['scale'])
    report['original_output_restored'] = True
    (vm.REPORT/'lucent-design-integration.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__ == '__main__': main()
