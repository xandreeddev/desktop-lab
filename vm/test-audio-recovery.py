#!/usr/bin/env python3
"""Native audio dragging, failed commands, hotplug and service reconnection.

Uses owned null outputs. Briefly suspends the guest's pipewire-pulse process
to exercise a real command deadline, then stops/restarts its service and socket.
Restores those services and original defaults/channel volumes/mute on failure.
No physical playback or microphone capture is performed.
"""
import importlib.util
import json
from pathlib import Path
import runpy
import time
import uuid

spec = importlib.util.spec_from_file_location('audio_panel_test', Path(__file__).with_name('test-audio-panel.py'))
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
a, v = p.a, p.v
UNITS = ('pipewire-pulse.socket', 'pipewire-pulse.service')


def device(name):
    return next(d for d in p.devices() if d['key'] == name)


def observed(name):
    return any(d['key'] == name for d in p.devices())


def settled(name, value):
    return observed(name) and abs(device(name)['volume'] - value) < .01 and not a.audio()['busy']


def slider(name):
    identifier = 'audio-volume-' + str(device(name)['id'])
    p.click(identifier)
    return v.hit(identifier)


def begin(h):
    v.move(h['x'] + h['width']/2, h['y'] + h['height']/2)
    time.sleep(.15)
    v.button(True)


def move(h, fraction):
    # Endpoints are tested outside the thumb inset; the midpoint is exact.
    v.move(h['x'] + h['width']*fraction, h['y'] + h['height']/2)


def process():
    pid = int(v.session('systemctl', '--user', 'show', 'pipewire-pulse.service', '--property=MainPID', '--value').strip())
    assert pid > 0
    assert v.remote('cat', f'/proc/{pid}/comm').strip() == 'pipewire-pulse'
    return pid


def owned_modules(names):
    # This guest's pactl JSON module records omit their numeric indices. The
    # short listing supplies them; exact kind/argument matching avoids removing
    # a recycled index or a module owned by another application.
    arguments = {'sink_name=' + name for name in names}
    for line in a.pactl('list', 'short', 'modules').splitlines():
        fields = line.split('\t')
        if len(fields) >= 3 and fields[1] == 'module-null-sink' and fields[2] in arguments:
            assert fields[0].isdigit()
            yield fields[0], fields[2]


def main():
    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    monitors = json.loads(v.session('hyprctl', 'monitors', '-j'))
    assert len(monitors) == 1 and tuple(monitors[0][k] for k in ('width', 'height', 'x', 'y', 'scale')) == (1920, 1080, 0, 0, 1)
    for unit in UNITS:
        assert v.session('systemctl', '--user', 'is-active', unit).strip() == 'active'
    v.eventually(lambda: a.audio()['status'] == 'ready', 'Audio not ready')
    initial = v.inspect()
    assert initial['client']['audio']['streams'] == 0, 'Close existing audio applications before the recovery test'
    previous_output, previous_input = a.pactl('get-default-sink'), a.pactl('get-default-source')
    original = next(s for s in a.sinks() if s['name'] == previous_output)
    original_input = next(s for s in json.loads(a.pactl('--format=json', 'list', 'sources')) if s['name'] == previous_input)
    desktop_pid = v.remote('pgrep', '-x', 'lucent-desktop').strip()
    tag = 'lucent_recovery_' + uuid.uuid4().hex
    names = [tag + '_a', tag + '_b']
    suspended = None
    restart_needed = False
    checks, maximum_pending = [], 0
    try:
        for name in names:
            assert a.pactl('load-module', 'module-null-sink', 'sink_name=' + name).isdigit()
        v.eventually(lambda: all(observed(name) for name in names), 'Owned outputs missing')
        v.cli('audio', 'open')
        v.eventually(lambda: a.audio()['open'] and any(s['id'] == 'audio' and s['frames'] > 0 for s in v.inspect()['surfaces']), 'Panel missing')
        p.click('audio-tab-Outputs')
        h = slider(names[0])
        begin(h)
        for i in range(120):
            move(h, (i % 21)/20)
            if i % 12 == 0:
                state = a.audio()
                maximum_pending = max(maximum_pending, state['pending_commands'])
                assert state['pending_commands'] <= 1, 'A single volume gesture accumulated commands'
        move(h, 2.)
        v.button(False)
        v.eventually(lambda: settled(names[0], 1.), 'Latest drag endpoint did not apply')
        assert all(c['value'] == 65536 for c in next(s for s in a.sinks() if s['name'] == names[0])['volume'].values())
        checks.append('sustained_120_move_drag_latest_endpoint_and_bounded_pending')

        # Freeze only the verified guest audio process. The UI and compositor
        # remain running; command failure comes from the real adapter deadline.
        h = slider(names[0])
        suspended = process()
        failed_id = device(names[0])['id']
        v.session('kill', '-STOP', str(suspended))
        begin(h)
        move(h, -.5)
        v.button(False)
        v.eventually(lambda: bool(a.audio()['control_error']) and not a.audio()['busy'], 'Audio command did not fail visibly', timeout=20)
        assert v.hit('audio-retry' if a.audio()['control_retry_available'] else 'audio-dismiss-error')
        v.screenshot('lucent-audio-command-failure.png')
        v.session('kill', '-CONT', str(suspended))
        suspended = None
        v.eventually(lambda: a.audio()['status'] == 'ready' and observed(names[0]), 'Timeout connection did not recover', timeout=25)
        if device(names[0])['id'] == failed_id:
            assert a.audio()['control_retry_available']
            p.click('audio-retry')
            v.eventually(lambda: settled(names[0], 0.) and a.audio()['control_error'] is None, 'Retry did not apply the failed endpoint', timeout=20)
            checks.append('real_command_deadline_visible_retry_and_recovery')
        else:
            # Connection recovery invalidates object identities. The client
            # must not reinterpret the old command as a new device operation.
            assert not a.audio()['control_retry_available']
            assert not any(hit['id'] == 'audio-retry' for hit in p.panel()['hits'])
            v.screenshot('lucent-audio-stale-failure.png')
            p.click('audio-dismiss-error')
            assert a.audio()['control_error'] is None
            h = slider(names[0])
            begin(h)
            move(h, -.5)
            v.button(False)
            v.eventually(lambda: settled(names[0], 0.), 'Fresh control failed after timeout reconnect')
            checks.append('real_command_deadline_stale_identity_dismiss_and_fresh_control_recovery')

        # Removing a control while held must cancel capture rather than sending
        # its old identity or moving the remaining control underneath it.
        h = slider(names[0])
        remaining_volume = device(names[1])['volume']
        begin(h)
        v.eventually(lambda: not a.audio()['busy'], 'Press did not settle')
        module, _ = next(owned_modules([names[0]]))
        a.pactl('unload-module', module)
        v.eventually(lambda: not observed(names[0]) and not any(hit['id'] == h['id'] for hit in p.panel()['hits']), 'Removed control remained', timeout=15)
        move(h, -.5)
        move(h, 2.)
        v.button(False)
        time.sleep(.4)
        assert not a.audio()['busy'] and a.audio()['pending_commands'] == 0
        assert device(names[1])['volume'] == remaining_volume, 'Removed slider capture changed another device'
        checks.append('device_removal_cancels_held_slider_without_changing_other_device')

        # Stop socket activation as well as the server, so connection attempts
        # cannot silently restart it before the unavailable state is exercised.
        h = slider(names[1])
        begin(h)
        restart_needed = True
        v.session('systemctl', '--user', 'stop', *UNITS)
        v.eventually(lambda: a.audio()['status'] == 'unavailable' and not a.audio()['busy'], 'Disconnect was not observed', timeout=20)
        state = a.audio()
        assert state['device_details'] is None and state['stream_details'] is None and state['pending_commands'] == 0
        error = state['control_error']
        move(h, -.5)
        move(h, 2.)
        v.button(False)
        time.sleep(.4)
        assert not a.audio()['busy'] and a.audio()['control_error'] == error, 'Disconnected capture produced stale commands'
        v.screenshot('lucent-audio-service-unavailable.png')
        v.session('systemctl', '--user', 'start', *UNITS)
        restart_needed = False
        v.eventually(lambda: a.audio()['status'] == 'ready', 'Adapter did not reconnect', timeout=25)
        assert v.remote('pgrep', '-x', 'lucent-desktop').strip() == desktop_pid, 'Desktop restarted during audio recovery'
        assert a.audio()['open'] and initial['client']['wallpaper'] == v.inspect()['client']['wallpaper']
        checks.append('service_disconnect_clears_stale_controls_and_reconnects_without_desktop_restart')

        # Pulse-owned temporary nodes may disappear across server restart.
        # Recreate only missing owned fixtures and prove new controls work.
        if not observed(names[1]):
            assert a.pactl('load-module', 'module-null-sink', 'sink_name=' + names[1]).isdigit()
        v.eventually(lambda: observed(names[1]), 'Recovered owned output missing')
        h = slider(names[1])
        begin(h)
        move(h, 2.)
        v.button(False)
        v.eventually(lambda: settled(names[1], 1.), 'Fresh control failed after reconnect')
        p.click('audio-default-' + str(device(names[1])['id']))
        v.eventually(lambda: a.pactl('get-default-sink') == names[1], 'Default selection failed after reconnect')
        v.screenshot('lucent-audio-recovered-selection.png')
        v.move(900, 800)
        time.sleep(.6)
        frames = p.panel()['frames']
        time.sleep(1)
        assert p.panel()['frames'] == frames, 'Settled panel repaints while idle'
        checks += ['new_control_and_default_selection_after_reconnect', 'no_panel_idle_redraw_over_one_second']
    finally:
        cleanup_errors = []
        def attempt(label, operation):
            try:
                operation()
            except Exception as error:
                cleanup_errors.append((label, type(error).__name__))
        attempt('release pointer', lambda: v.button(False))
        if suspended is not None:
            attempt('resume guest audio process', lambda: v.session('kill', '-CONT', str(suspended)))
        if restart_needed:
            attempt('restart guest audio', lambda: v.session('systemctl', '--user', 'start', *UNITS))
        attempt('verify audio service recovery', lambda: v.eventually(lambda: a.audio()['status'] == 'ready', 'Audio not restored', timeout=25))
        attempt('close panel', lambda: v.cli('audio', 'close'))
        # Server restart can recycle numeric module IDs. Match both module kind
        # and our complete owned argument before unloading; never reuse old IDs.
        def remove_modules():
            for index, _ in owned_modules(names):
                a.pactl('unload-module', index)
        attempt('remove owned null outputs', remove_modules)
        for kind, previous in [('sink', previous_output), ('source', previous_input)]:
            attempt('observe original ' + kind, lambda previous=previous: v.eventually(lambda: observed(previous), 'Original device missing', timeout=20))
            attempt('restore default ' + kind, lambda kind=kind, previous=previous: a.pactl('set-default-' + kind, previous))
        attempt('restore original channels', lambda: a.pactl('set-sink-volume', previous_output, *(str(c['value']) for c in original['volume'].values())))
        attempt('restore original mute', lambda: a.pactl('set-sink-mute', previous_output, '1' if original['mute'] else '0'))
        attempt('restore original input channels', lambda: a.pactl('set-source-volume', previous_input, *(str(c['value']) for c in original_input['volume'].values())))
        attempt('restore original input mute', lambda: a.pactl('set-source-mute', previous_input, '1' if original_input['mute'] else '0'))
        def verified():
            assert a.pactl('get-default-sink') == previous_output and a.pactl('get-default-source') == previous_input
            restored = next(s for s in a.sinks() if s['name'] == previous_output)
            assert restored['volume'] == original['volume'] and restored['mute'] == original['mute']
            restored_input = next(s for s in json.loads(a.pactl('--format=json', 'list', 'sources')) if s['name'] == previous_input)
            assert restored_input['volume'] == original_input['volume'] and restored_input['mute'] == original_input['mute']
            assert not any(d['key'].startswith(tag) for d in p.devices())
            assert not any(s['id'] == 'audio' for s in v.inspect()['surfaces'])
            for unit in UNITS:
                assert v.session('systemctl', '--user', 'is-active', unit).strip() == 'active'
        attempt('verify original audio and fixture cleanup', verified)
        assert not cleanup_errors, cleanup_errors
    report = {'checks': checks + ['original_audio_services_defaults_channels_mute_and_panel_restored'],
              'maximum_sampled_pending_commands': maximum_pending,
              'scope': 'Native pointer controls and real guest audio failure/recovery using silent owned virtual outputs; no physical hardware validation'}
    (v.REPORT / 'lucent-audio-recovery-native.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    measurement = runpy.run_path(str(Path(__file__).with_name('measure-framework.py')))
    with measurement['stay_awake'](v.remote):
        main()
