#!/usr/bin/env python3
"""Check wallpaper preparation after native size and integer-scale changes.

Requires the prepared unlocked, single-output Lucent guest at 1920x1080/1x.
Restores its output and idle marker even after failure. Does not select another
wallpaper, change settings or modify the immutable source. Captures stay local.
"""
import importlib.util
from native_surfaces import active_surfaces, find_surface, role
import json
from pathlib import Path
import time

spec = importlib.util.spec_from_file_location('framework_test', Path(__file__).with_name('test-framework.py'))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)
measure_spec = importlib.util.spec_from_file_location('framework_measure', Path(__file__).with_name('measure-framework.py'))
measure = importlib.util.module_from_spec(measure_spec)
measure_spec.loader.exec_module(measure)


def output_mode(monitor, width, height, scale):
    code = 'hl.monitor({output=%s, mode=%s, position=%s, scale=%s})' % (
        json.dumps(monitor['name']), json.dumps(f'{width}x{height}@{monitor["refreshRate"]}'),
        json.dumps(f'{monitor["x"]}x{monitor["y"]}'), scale)
    response = vm.session('hyprctl', 'eval', code)
    if 'error' in response.lower():
        raise RuntimeError(response)


def fingerprint():
    return json.loads(vm.remote('python3', '-c', '''from pathlib import Path
import hashlib,json
base=Path.home()/'.local/state/lucent'
wallpaper=base/'wallpaper'
settings=base/'desktop.json'
print(json.dumps({'source':str(wallpaper.resolve()),
    'pixels':hashlib.sha256(wallpaper.read_bytes()).hexdigest(),
    'settings':hashlib.sha256(settings.read_bytes()).hexdigest() if settings.exists() else None}))'''))


def wait_for_target(width, height, scale):
    def ready():
        state = vm.inspect()
        fitted = state['client']['wallpaper_presentation']
        background = next((s for s in active_surfaces(state) if role(s) == 'background'), None)
        return (background is not None and background['scale'] == scale
                and [background['buffer_width'], background['buffer_height']] == [width, height]
                and fitted['target_physical_pixels'] == [width, height]
                and fitted['prepared_for_physical_pixels'] == [width, height]
                and not fitted['busy'] and fitted['error'] is None)
    vm.eventually(ready, f'Background did not prepare {width}x{height}/{scale}x', timeout=30)
    return vm.inspect()


def main():
    monitors = json.loads(vm.session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1, 'This check requires one output'
    monitor = monitors[0]
    assert (monitor['width'], monitor['height'], monitor['scale']) == (1920, 1080, 1)
    assert vm.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    assert any(mode.startswith('1024x768@') for mode in monitor['availableModes'])
    original = fingerprint()
    pid = vm.remote('pgrep', '-x', 'lucent-desktop').strip()
    report = {'checks': [], 'scope': 'Prepared single-output VM; software Vulkan; integer scales only'}
    with measure.stay_awake(vm.remote):
        try:
            initial = wait_for_target(1920, 1080, 1)
            original_aspect = initial['client']['wallpaper_presentation']['aspect_pixels']
            for width, height, scale in [(1920, 1080, 2), (1024, 768, 1), (1920, 1080, 1)]:
                output_mode(monitor, width, height, scale)
                state = wait_for_target(width, height, scale)
                presentation = state['client']['wallpaper_presentation']
                assert presentation['aspect_pixels'] == original_aspect
                assert fingerprint() == original, 'Resize modified selected source or saved settings'
                assert vm.remote('pgrep', '-x', 'lucent-desktop').strip() == pid, 'Desktop restarted'
                report['checks'].append({'physical_pixels': [width, height], 'scale': scale,
                    'logical_pixels': [width / scale, height / scale],
                    'retained_pixels': presentation['retained_pixels'],
                    'retained_bytes': state['client']['prepared_assets']['background']['bytes']})
            # Rapid output changes must finish on the latest physical target.
            for width, height in [(1024, 768), (1920, 1080), (1024, 768), (1920, 1080)]:
                output_mode(monitor, width, height, 1)
            wait_for_target(1920, 1080, 1)
            assert fingerprint() == original
            vm.cli('launcher', 'open')
            vm.eventually(lambda: vm.inspect()['client']['launcher'], 'Launcher did not open')
            time.sleep(.7)
            vm.cli('launcher', 'close')
            vm.eventually(lambda: not vm.inspect()['client']['launcher'], 'Launcher did not close')
            report['latest_target_after_burst'] = True
            report['launcher_open_close'] = True
        finally:
            output_mode(monitor, monitor['width'], monitor['height'], monitor['scale'])
            wait_for_target(monitor['width'], monitor['height'], monitor['scale'])
    assert fingerprint() == original
    report['source_and_preferences_unchanged'] = True
    report['original_output_and_idle_marker_restored'] = True
    report['desktop_process_preserved'] = vm.remote('pgrep', '-x', 'lucent-desktop').strip() == pid
    (vm.REPORT / 'lucent-wallpaper-resolution-integration.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
