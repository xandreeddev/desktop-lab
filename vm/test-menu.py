#!/usr/bin/env python3
"""Native picker integration: real keys, Omarchy action dispatch and user bindings."""
import importlib.util
import json
from pathlib import Path
import shlex
import subprocess
import time

spec = importlib.util.spec_from_file_location('framework_test', Path(__file__).with_name('test-framework.py'))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)


def menu(command='inspect'):
    return vm.session('python3', '-c', '''import os,socket,sys
try:
 with socket.socket(socket.AF_UNIX) as s:
  s.settimeout(2)
  s.connect(os.environ['XDG_RUNTIME_DIR']+'/lucent-menu.sock')
  s.sendall((sys.argv[1]+'\\n').encode())
  data=b''
  while part:=s.recv(65536): data+=part
  print(data.decode())
except OSError: raise SystemExit(1)
''', command)


def state():
    return json.loads(menu())


def is_closed():
    try:
        menu()
        return False
    except subprocess.CalledProcessError:
        return True


def open_keys():
    vm.key(125, 37)  # actual Super+K; never bypass the compositor binding
    vm.eventually(lambda: state()['client']['prompt'] == 'Keybindings', 'Super+K did not open Lucent')
    vm.eventually(lambda: state()['surfaces'][0]['frames'] > 0, 'Menu did not draw')


def query(value):
    vm.key(29, 30)
    vm.text(value)


def caller(*args):
    return subprocess.Popen(vm.lab.ssh_args('lucent') + [shlex.join([
        'python3', 'desktop-lab/scripts/in-session.py', *args])],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)


def main():
    assert vm.session('sh', '-c', 'omarchy-hyprland-session-locked && echo locked || echo unlocked').strip() == 'unlocked'
    checks = []
    vm.cli('launcher', 'close')
    vm.move(40, 400)
    before = {c['address'] for c in json.loads(vm.session('hyprctl', '-j', 'clients'))}
    marker = '/tmp/lucent-menu-integration-action'
    block = '\n-- BEGIN LUCENT MENU TEST\no.bind("SUPER + CTRL + ALT + F12", "Lucent menu test action", "touch ' + marker + '")\n-- END LUCENT MENU TEST\n'
    processes = []
    try:
        open_keys()
        live_count = len(vm.session('omarchy-menu-keybindings', '--print').splitlines())
        assert state()['client']['entries'] == live_count
        vm.key(15)
        assert state()['client']['selected'] == 1
        vm.key(42, 15)
        assert state()['client']['selected'] == 0
        for _ in range(9): vm.key(108)
        vm.eventually(lambda: state()['client']['selected'] == 9, 'Arrow navigation failed')
        s = state()['surfaces'][0]
        assert s['focus'] == 'menu-search' and s['focus_visible']
        assert any(h['id'] == 'menu-row-9' for h in s['hits'])
        checks += ['super_k_native_surface', 'installed_binding_count', 'tab_and_shift_tab', 'arrow_scroll_with_input_focus']
        vm.screenshot('lucent-keybindings-scrolled.png')
        query('terminal')
        vm.eventually(lambda: 0 < state()['client']['results'] < live_count, 'Search failed')
        vm.screenshot('lucent-keybindings-search.png')
        vm.key(28)
        vm.eventually(is_closed, 'Menu did not release its keyboard surface')
        vm.eventually(lambda: any(c['address'] not in before for c in json.loads(vm.session('hyprctl', '-j', 'clients'))), 'Omarchy failed to execute the selected Terminal action')
        checks += ['search', 'stock_terminal_action', 'focus_released_before_dispatch']
        for c in json.loads(vm.session('hyprctl', '-j', 'clients')):
            if c['address'] not in before:
                vm.session('hyprctl', 'dispatch', f'hl.dsp.window.close({{ window = "address:{c["address"]}" }})')
        open_keys()
        query('no such action exists')
        vm.eventually(lambda: state()['client']['results'] == 0, 'Empty search failed')
        vm.key(28)
        assert not is_closed(), 'Enter executed a missing action'
        vm.key(1)
        vm.eventually(is_closed, 'Escape failed to close')
        checks += ['no_result_enter_is_safe', 'escape_cancels']

        # Exercise the real user-Lua discovery path, restoring only our block.
        vm.session('python3', '-c', 'from pathlib import Path; import sys; p=Path.home()/".config/hypr/bindings.lua"; p.write_text(p.read_text()+sys.argv[1])', block)
        vm.session('hyprctl', 'reload')
        open_keys()
        query('lucent menu test action')
        vm.eventually(lambda: state()['client']['results'] == 1, 'Custom Lua binding missing')
        vm.key(28)
        vm.eventually(is_closed, 'Custom action menu did not close')
        vm.eventually(lambda: vm.remote('test', '-f', marker) == '', 'Custom Lua action failed')
        checks += ['user_binding_discovery', 'user_binding_dispatch']

        p = caller('omarchy-menu-select', 'Fixture', 'glyph\tSame\tfirst', 'glyph\tSame\tsecond')
        processes.append(p)
        vm.eventually(lambda: state()['client']['prompt'] == 'Fixture', 'Generic select failed')
        vm.key(108); vm.key(28)
        out, err = p.communicate(timeout=10)
        assert p.returncode == 0 and out == 'Same\tsecond\n', (out, err)
        checks += ['generic_select_preserves_subtext_value']

        p = caller('omarchy-menu-input', 'Fixture input')
        processes.append(p)
        vm.eventually(lambda: state()['client']['prompt'] == 'Fixture input', 'Input failed')
        vm.text('test reminder'); vm.key(28)
        out, err = p.communicate(timeout=10)
        assert p.returncode == 0 and out == 'test reminder\n', (out, err)
        checks += ['generic_input_return_value']

        p = caller('omarchy-menu-select', 'Cancel fixture', 'one')
        processes.append(p)
        vm.eventually(lambda: state()['client']['prompt'] == 'Cancel fixture', 'Cancel menu failed')
        vm.move(40, 400); vm.button(True); vm.button(False)
        out, err = p.communicate(timeout=10)
        assert p.returncode == 1 and not out, (out, err)
        checks += ['outside_click_cancel_status']
    finally:
        if not is_closed(): menu('close')
        for p in processes:
            if p.poll() is None: p.terminate(); p.wait(timeout=5)
        vm.session('python3', '-c', 'from pathlib import Path; import sys; p=Path.home()/".config/hypr/bindings.lua"; p.write_text(p.read_text().replace(sys.argv[1],"")); Path(sys.argv[2]).unlink(missing_ok=True)', block, marker)
        vm.session('hyprctl', 'reload')
    report = {'checks': checks, 'bindings_source': 'installed Omarchy', 'renderer': 'Lucent Vulkan'}
    (vm.REPORT/'lucent-menu-integration.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
