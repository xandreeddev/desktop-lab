#!/usr/bin/env python3
"""Native dock buffer lifecycle, placement and input checks.

Requires the unlocked single-output Lucent guest at 1920x1080/1x. Restores
the output and idle marker, closes only its own terminal, and keeps captures
private. It does not select a wallpaper or change saved preferences.
"""
import importlib.util
import json
from pathlib import Path
import time


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


vm = load('dock_framework_test', 'test-framework.py')
measure = load('dock_framework_measure', 'measure-framework.py')
wallpaper = load('dock_wallpaper_test', 'test-wallpaper-resolution.py')


def dock(state):
    return next(s for s in state['surfaces'] if s['id'] == 'dock')


def settled(expanded, height):
    def ready():
        state = vm.inspect()
        return (state['client']['dock_surface_expanded'] == expanded
                and not state['client']['launcher_opening'] and dock(state)['height'] == height)
    vm.eventually(ready, f'Dock did not reach expanded={expanded}, height={height}', timeout=20)
    time.sleep(.3)
    return vm.inspect()


def main():
    monitors = json.loads(vm.session('hyprctl', '-j', 'monitors'))
    assert len(monitors) == 1
    monitor = monitors[0]
    assert (monitor['width'], monitor['height'], monitor['scale']) == (1920, 1080, 1)
    assert vm.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    original = wallpaper.fingerprint()
    pid = vm.remote('pgrep', '-x', 'lucent-desktop').strip()
    report = {'scope': 'Single-output VM; software Vulkan; integer scales 1 and 2', 'checks': []}
    terminal = None
    with measure.stay_awake(vm.remote):
        try:
            vm.cli('launcher', 'close')
            state = settled(False, 120)
            assert state['client']['desktop_viewport'] == [1920., 1080.]
            geometry = vm.layer_geometry('dock')
            assert (geometry['x'], geometry['y'], geometry['w'], geometry['h']) == (0, 960, 1920, 120), geometry
            panel = vm.hit('dock-panel')
            assert panel['y'] == 992 and panel['height'] == 64, panel
            initial_frames = dock(state)['frames']
            vm.screenshot('lucent-compact-dock.png')
            vm.click('dock-launcher')
            state = settled(True, 1080)
            assert state['client']['launcher']
            assert dock(state)['frames'] > initial_frames + 2
            vm.click('launcher-search')
            vm.key(29, 30)
            vm.text('foot')
            vm.eventually(lambda: vm.inspect()['client']['query'] == 'foot', 'Search input did not update')
            vm.key(15)
            vm.eventually(lambda: vm.inspect()['client']['mode'] == 'Commands', 'Tab did not change mode')
            vm.move(20, 400)
            vm.button(True)
            time.sleep(.06)
            vm.button(False)
            settled(False, 120)
            report['real_pointer_open_keyboard_search_tab_and_outside_close'] = True
            vm.cli('launcher', 'open')
            settled(True, 1080)
            vm.session('python3', '-c', '''from pathlib import Path
import subprocess,time
cli=str(Path.home()/'.local/bin/lucent-cli')
subprocess.run([cli,'launcher','close'],check=True,stdout=subprocess.DEVNULL)
time.sleep(.05)
subprocess.run([cli,'launcher','open'],check=True,stdout=subprocess.DEVNULL)''')
            state = settled(True, 1080)
            assert state['client']['launcher']
            report['reopen_during_close_keeps_full_buffer'] = True
            vm.click('launcher-search')
            vm.key(29, 30)
            vm.text('foot')
            before = {c['address'] for c in json.loads(vm.session('hyprctl', '-j', 'clients'))}
            vm.key(28)
            def launched():
                return any(c['address'] not in before and c['class'].startswith('foot')
                           for c in json.loads(vm.session('hyprctl', '-j', 'clients')))
            vm.eventually(launched, 'Launcher did not start a terminal')
            terminal = next(c for c in json.loads(vm.session('hyprctl', '-j', 'clients'))
                            if c['address'] not in before and c['class'].startswith('foot'))
            settled(False, 120)
            report['real_terminal_launch'] = True
            for scale, logical_height, compact_height in [(2, 540, 124), (1, 1080, 120)]:
                wallpaper.output_mode(monitor, 1920, 1080, scale)
                wallpaper.wait_for_target(1920, 1080, scale)
                state = settled(False, compact_height)
                assert state['client']['desktop_viewport'] == [1920 / scale, logical_height]
                assert dock(state)['buffer_height'] == compact_height * scale
                geometry = vm.layer_geometry('dock')
                assert geometry['y'] == logical_height - compact_height
                report['checks'].append({'scale': scale, 'logical_height': logical_height,
                    'compact_logical_height': compact_height, 'physical_buffer_height': compact_height * scale})
                vm.cli('launcher', 'open')
                settled(True, logical_height)
                vm.cli('launcher', 'close')
                settled(False, compact_height)
            vm.move(950, 400)
            # Leaving a hovered dock control legitimately submits its exit
            # state. Measure idle only after that interaction has settled.
            time.sleep(.5)
            frames = dock(vm.inspect())['frames']
            time.sleep(2)
            assert dock(vm.inspect())['frames'] == frames, 'Settled dock repainted while idle'
            report['closed_dock_idle_frames'] = 0
        finally:
            if terminal is not None:
                vm.session('hyprctl', 'dispatch', f'hl.dsp.window.close({{ window = "address:{terminal["address"]}" }})')
            wallpaper.output_mode(monitor, monitor['width'], monitor['height'], monitor['scale'])
            wallpaper.wait_for_target(monitor['width'], monitor['height'], monitor['scale'])
            vm.cli('launcher', 'close')
            settled(False, 120)
    assert wallpaper.fingerprint() == original, 'Test changed wallpaper or settings'
    assert vm.remote('pgrep', '-x', 'lucent-desktop').strip() == pid, 'Desktop restarted'
    report['source_preferences_pid_output_and_idle_marker_preserved'] = True
    (vm.REPORT / 'lucent-dock-surface-integration.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
