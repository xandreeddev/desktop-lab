#!/usr/bin/env python3
"""Real input regression: native submenus, drag grid and compositor corners.

Requires an unlocked Lucent guest with a Chrome/Chromium window and calendar.
Leaves applications open, restores widget positions and the original workspace.
"""
import importlib.util
import json
import math
from pathlib import Path
import time

spec = importlib.util.spec_from_file_location('menu_test', Path(__file__).with_name('test-menu.py'))
menus = importlib.util.module_from_spec(spec)
spec.loader.exec_module(menus)
vm = menus.vm


def prompt(label):
    vm.eventually(lambda: menus.state()['client']['prompt'] == label, 'Expected native menu: '+label, timeout=20)
    vm.eventually(lambda: menus.state()['surfaces'][0]['frames'] > 0, 'Menu did not paint')


def click_menu(identifier):
    time.sleep(.5)
    h = next(h for s in menus.state()['surfaces'] for h in s['hits'] if h['id'] == identifier)
    vm.move(h['x']+h['width']/2, h['y']+h['height']/2)
    vm.button(True); vm.button(False)


def workspace(identifier):
    vm.session('hyprctl', 'dispatch', f'hl.dsp.focus({{ workspace = "{identifier}" }})')
    vm.eventually(lambda: json.loads(vm.session('hyprctl', '-j', 'activeworkspace'))['id'] == identifier, 'Workspace did not switch')
    time.sleep(.4)


def main():
    assert vm.session('sh', '-c', 'omarchy-hyprland-session-locked && echo locked || echo unlocked').strip() == 'unlocked'
    initial = vm.inspect()['client']
    original_workspace = json.loads(vm.session('hyprctl', '-j', 'activeworkspace'))['id']
    clients = json.loads(vm.session('hyprctl', '-j', 'clients'))
    empty = next(i for i in range(1, 100) if i not in {c['workspace']['id'] for c in clients})
    checks, processes = [], []
    try:
        radius = vm.token_value('component.window.radius')
        browsers = [c for c in clients if c['class'] == 'chromium' or c['class'].startswith('chrome')]
        assert browsers, 'Open a Chromium or Chrome app window for this check'
        for c in browsers:
            assert float(vm.session('hyprctl', 'getprop', 'address:'+c['address'], 'rounding')) == radius
        assert not vm.session('hyprctl', 'configerrors').strip()
        checks += ['chrome_and_chromium_use_shared_radius', 'hyprland_config_valid']

        vm.cli('launcher', 'close')
        vm.move(40, 400)
        menus.open_keys()
        menus.query('system menu')
        vm.eventually(lambda: menus.state()['client']['results'] == 1, 'System binding not found')
        vm.key(28); prompt('System')
        assert any(h['id'] == 'menu-back' for s in menus.state()['surfaces'] for h in s['hits'])
        vm.screenshot('lucent-native-system-submenu.png')
        vm.key(1); prompt('Omarchy')
        click_menu('menu-close'); vm.eventually(menus.is_closed, 'Close did not cancel')
        checks += ['super_k_dispatches_native_submenu', 'submenu_back_button', 'escape_returns_to_parent', 'close_cancels_tree']

        for route, title in [('theme', 'Theme'), ('style.font', 'Font')]:
            p = menus.caller('omarchy-menu', 'summon', route); processes.append(p)
            prompt(title)
            assert menus.state()['client']['results'] > 0
            click_menu('menu-back'); prompt('Style')
            menus.menu('close'); assert p.wait(timeout=10) == 1
            checks.append(route+'_native_provider_and_back')

        workspace(empty)
        h = vm.hit('widget-calendar')
        x, y = h['x']+100, h['y']+20
        assert not vm.inspect()['client']['widget_grid']
        vm.move(x, y); time.sleep(.2); vm.button(True)
        for i in range(1, 9):
            vm.move(x+i*17, y+i*7); time.sleep(.04)
        vm.eventually(lambda: vm.inspect()['client']['widget_grid'], 'Grid did not appear during drag')
        vm.screenshot('lucent-widget-drag-grid.png')
        vm.button(False)
        vm.eventually(lambda: not vm.inspect()['client']['widget_grid'], 'Grid remained after release')
        step = vm.token_value('component.widget_layout.grid_step')
        position = vm.inspect()['client']['positions']['calendar']
        expected = {'x': math.floor((h['x']+136)/step+.5)*step, 'y': math.floor((h['y']+56)/step+.5)*step}
        assert position == expected, (position, expected)
        checks += ['grid_hidden_at_rest', 'grid_visible_while_dragging', 'grid_hidden_after_release', 'release_matches_spacing_grid']
    finally:
        vm.button(False)
        if not menus.is_closed(): menus.menu('close')
        for p in processes:
            if p.poll() is None: p.terminate(); p.wait(timeout=5)
        # Restore only the positions touched by this test, retaining other settings.
        vm.session('python3', '-c', '''from pathlib import Path
import json,sys
p=Path.home()/'.local/state/lucent/desktop.json'
data=json.loads(p.read_text()); data['positions']=json.loads(sys.argv[1]); p.write_text(json.dumps(data))
''', json.dumps(initial['positions']))
        vm.session('systemctl', '--user', 'restart', 'lucent.service')
        vm.eventually(lambda: vm.inspect()['client']['positions'] == initial['positions'], 'Original positions not restored')
        workspace(original_workspace)
    report = {'checks': checks, 'renderer': 'Vulkan', 'window_radius': radius}
    (vm.REPORT/'lucent-desktop-polish.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__': main()
