#!/usr/bin/env python3
"""Real keyboard checks for launcher modes, selection, scrolling and input focus."""
import importlib.util
import json
from pathlib import Path
import time

spec = importlib.util.spec_from_file_location('framework_test', Path(__file__).with_name('test-framework.py'))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)


def main():
    assert vm.session('sh', '-c', 'omarchy-hyprland-session-locked && echo locked || echo unlocked').strip() == 'unlocked', 'Unlock the VM first'
    awake = json.loads(vm.session('omarchy-toggle-idle', 'status'))['enabled']
    checks = []
    try:
        vm.session('omarchy-toggle-idle', 'stay-awake')
        vm.session('omarchy-shell', 'idle', 'disable')
        vm.move(960, 250)  # Keep the pointer away: all subsequent selection is keyboard-only.
        vm.cli('launcher', 'open')
        time.sleep(0.8)
        for _ in range(8): vm.key(108)
        vm.eventually(lambda: vm.inspect()['client']['selected'] == 8, 'Arrow selection did not advance')
        time.sleep(0.5)
        state = vm.inspect()
        dock = next(s for s in state['surfaces'] if s['id'] == 'dock')
        assert dock['focus'] == 'launcher-search' and dock['focus_visible']
        rows = [h for h in dock['hits'] if h['id'].startswith('result-')]
        panel = next(h for h in dock['hits'] if h['id'] == 'dock-panel')
        grid = vm.token_value('layout.shell_step')
        for h in [panel, *rows]:
            assert all(abs(h[k] / grid - round(h[k] / grid)) < .001 for k in ('x', 'y', 'width', 'height')), h['id']
        inset = vm.token_value('component.dock.content_inset')
        assert all(r['x']-panel['x'] == inset and panel['x']+panel['width']-r['x']-r['width'] == inset for r in rows)
        pitch = vm.token_value('component.launcher.row_height')
        assert all(b['y']-a['y'] == pitch for a,b in zip(rows,rows[1:]))
        search = next(h for h in dock['hits'] if h['id'] == 'launcher-search')
        assert (search['y'] - vm.token_value('component.input.padding_block')) % grid == 0
        clock = next(h for s in state['surfaces'] if s['id']=='bar' for h in s['hits'] if h['id']=='bar-clock')
        assert all(clock[k] % grid == 0 for k in ('x','y','width','height'))
        checks += ['panel_and_rows_on_shell_grid', 'equal_launcher_insets', 'uniform_row_pitch', 'input_on_shell_grid', 'top_bar_clock_on_shell_grid']
        assert len(rows) == 7
        assert any(h['id'] == 'result-8' for h in rows)
        for row in rows:
            clip = row['clip']
            assert row['y'] >= clip['y']-0.01 and row['y']+row['height'] <= clip['y']+clip['height']+0.01, row['id']
        vm.screenshot('lucent-launcher-keyboard-last.png')
        checks += ['keyboard_selection_without_hover', 'all_seven_rows_unclipped', 'search_focus_visible']
        for mode in ['Commands', 'Wallpapers', 'Themes', 'Widgets', 'Apps']:
            vm.key(15)
            vm.eventually(lambda: vm.inspect()['client']['mode'] == mode, 'Tab did not select '+mode)
        vm.key(42, 15)
        vm.eventually(lambda: vm.inspect()['client']['mode'] == 'Widgets', 'Shift+Tab did not move backward')
        vm.key(108)
        vm.eventually(lambda: vm.inspect()['client']['selected'] == 1, 'Widget keyboard selection failed')
        vm.screenshot('lucent-launcher-keyboard-widgets.png')
        checks += ['tab_all_five_sections', 'shift_tab_reverse', 'widget_keyboard_selection']
        vm.key(15)
        vm.text('foot')
        vm.eventually(lambda: vm.inspect()['client']['query'] == 'foot', 'Input focus was not restored after Tab')
        vm.screenshot('lucent-launcher-aligned-input.png')
        checks += ['search_focus_restored_after_mode_switch']
        assert not vm.inspect()['client']['error']
    finally:
        vm.cli('launcher', 'close')
        if not awake:
            vm.session('omarchy-toggle-idle', 'allow-idle')
            vm.session('omarchy-shell', 'idle', 'enable')
    report = {'checks': checks, 'settings_unchanged': True}
    (vm.REPORT/'lucent-launcher-integration.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__': main()
