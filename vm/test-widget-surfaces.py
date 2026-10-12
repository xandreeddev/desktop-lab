#!/usr/bin/env python3
"""Bounded widget placement, capture and transient native preview checks.

Requires the unlocked 1920x1080 Lucent VM. Restores saved settings, output and
idle marker; does not select a wallpaper. Captures and backups remain private.
"""
import importlib.util
import json
import math
from pathlib import Path
import time


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


vm = load('widgets_framework_test', 'test-framework.py')
measure = load('widgets_framework_measure', 'measure-framework.py')
wallpaper = load('widgets_wallpaper_test', 'test-wallpaper-resolution.py')


def client():
    return vm.inspect()['client']


def check_settled(scale):
    state = vm.inspect()
    assert not state['client']['widget_grid']
    assert 'widgets' not in {s['id'] for s in state['surfaces']}
    by_id = {s['id']: s for s in state['surfaces']}
    for group in state['client']['widget_surfaces']:
        source = by_id[group['id']]
        assert source['redraw_group'] == 'widgets'
        assert source['pointer_input'] == 'content'
        assert source['scale'] == scale
        geometry = vm.layer_geometry(group['id'])
        assert (geometry['x'], geometry['y'], geometry['w'], geometry['h']) == (
            group['x'], group['y'], group['width'], group['height']), geometry
        assert (source['width'], source['height']) == (group['width'], group['height'])
        assert source['paint_primitives'] > 0
        for widget in group['members']:
            vm.hit('widget-' + widget)
    return state


def begin_drag():
    hit = vm.hit('widget-calendar')
    x, y = hit['x'] + 100, hit['y'] + 20
    vm.move(x, y)
    time.sleep(.15)
    vm.button(True)
    for step in range(1, 13):
        vm.move(x + step * 30, y + step * 10)
        time.sleep(.04)
    vm.eventually(lambda: client()['widget_grid'], 'Drag did not enter preview')
    return hit, x + 360, y + 120


def main():
    monitors = json.loads(vm.session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1
    monitor = monitors[0]
    assert (monitor['width'], monitor['height'], monitor['scale']) == (1920, 1080, 1)
    assert vm.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    settings_path = '/home/omarchy/.local/state/lucent/desktop.json'
    backup = vm.remote('mktemp', '/tmp/lucent-widget-settings.XXXXXX').strip()
    vm.remote('cp', settings_path, backup)
    original = wallpaper.fingerprint()
    original_widgets = client()['widgets']
    pid = vm.remote('pgrep', '-x', 'lucent-desktop').strip()
    report = {'scope': 'Single-output VM; software Vulkan; integer scale 1 and 2'}
    held = False
    with measure.stay_awake(vm.remote):
        try:
            vm.cli('launcher', 'close')
            vm.eventually(lambda: not client()['dock_surface_expanded'], 'Dock did not close', timeout=20)
            vm.eventually(lambda: client()['background_ready'], 'Background not ready', timeout=40)
            before = check_settled(1)
            sources = before['client']['widget_surfaces']
            saved_positions = before['client']['positions']
            held = True
            hit, end_x, end_y = begin_drag()
            vm.eventually(lambda: all(s['paint_primitives'] == 0 for s in vm.inspect()['surfaces']
                                     if s['redraw_group'] == 'widgets' and s['id'] != 'widgets'),
                          'Drag source paint was not suppressed')
            during = vm.inspect()
            preview = next(s for s in during['surfaces'] if s['id'] == 'widgets')
            assert preview['pointer_input'] == 'none' and preview['paint_primitives'] > 0
            assert (preview['width'], preview['height']) == (1920, 1080)
            assert during['client']['positions'] == saved_positions
            assert during['client']['widget_surfaces'] == sources
            for source in sources:
                geometry = vm.layer_geometry(source['id'])
                assert (geometry['x'], geometry['y'], geometry['w'], geometry['h']) == (
                    source['x'], source['y'], source['width'], source['height'])
            vm.screenshot('lucent-widget-native-drag.png')
            vm.move(end_x + 100, end_y + 80)
            time.sleep(.2)
            vm.button(False)
            held = False
            vm.eventually(lambda: not client()['widget_grid'], 'Release did not remove preview')
            step = vm.token_value('component.widget_layout.grid_step')
            expected = {'x': math.floor((hit['x'] + 460) / step + .5) * step,
                        'y': math.floor((hit['y'] + 200) / step + .5) * step}
            assert client()['positions']['calendar'] == expected
            vm.eventually(lambda: client()['settings_save_status'] == 'saved', 'Placement did not save')
            check_settled(1)
            report['bounded_positions_and_frozen_capture'] = True
            report['pass_through_preview_and_snapped_release'] = True
            vm.session('systemctl', '--user', 'restart', 'lucent.service')
            vm.eventually(lambda: client()['positions'].get('calendar') == expected,
                          'Native placement did not survive restart', timeout=40)
            check_settled(1)
            report['position_restored_after_restart'] = True
            saved_positions = client()['positions']
            held = True
            begin_drag()
            wallpaper.output_mode(monitor, 1920, 1080, 2)
            wallpaper.wait_for_target(1920, 1080, 2)
            vm.eventually(lambda: not client()['widget_grid'], 'Resize did not cancel drag')
            vm.button(False)
            held = False
            time.sleep(.4)
            assert client()['positions'] == saved_positions
            check_settled(2)
            report['integer_scale_and_cancelled_resize'] = True
            wallpaper.output_mode(monitor, 1920, 1080, 1)
            wallpaper.wait_for_target(1920, 1080, 1)
            check_settled(1)
            report['no_competing_shell'] = not vm.remote('sh', '-c', 'pgrep -x quickshell || true').strip()
            assert report['no_competing_shell']
        finally:
            if held:
                vm.button(False)
            wallpaper.output_mode(monitor, monitor['width'], monitor['height'], monitor['scale'])
            wallpaper.wait_for_target(monitor['width'], monitor['height'], monitor['scale'])
            vm.session('systemctl', '--user', 'stop', 'lucent.service')
            vm.remote('cp', backup, settings_path)
            vm.session('systemctl', '--user', 'start', 'lucent.service')
            vm.eventually(lambda: client()['background_ready'] and client()['widgets'] == original_widgets,
                          'Original desktop did not restore', timeout=40)
            assert wallpaper.fingerprint() == original
            vm.remote('rm', '-f', backup)
    report['source_preferences_output_and_idle_restored'] = True
    report['restarts_explicit_only'] = vm.remote('pgrep', '-x', 'lucent-desktop').strip() != pid
    (vm.REPORT / 'lucent-widget-surfaces-integration.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
