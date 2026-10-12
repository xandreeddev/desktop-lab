#!/usr/bin/env python3
"""Native audio panel and playback/capture routing with owned silent fixtures.

Requires the new desktop binary in an unlocked 1920x1080 scale-1 guest. Creates
two null outputs and two transient pacat units; restores prior default devices.
Capture reads only the owned output monitor, never a physical microphone.
"""
import importlib.util
import json
from pathlib import Path
import runpy
import uuid

spec = importlib.util.spec_from_file_location('audio_test', Path(__file__).with_name('test-audio.py'))
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)
v = a.v


def devices():
    return a.audio().get('device_details') or []


def streams():
    return a.audio().get('stream_details') or []


def panel():
    return next(s for s in v.inspect()['surfaces'] if s['id'] == 'audio')


def click(identifier):
    for _ in range(len(panel()['hits']) + 2):
        h = next(h for h in panel()['hits'] if h['id'] == identifier)
        c = h['clip']
        if (h['x'] >= c['x'] and h['y'] >= c['y'] and
                h['x']+h['width'] <= c['x']+c['width']+.01 and
                h['y']+h['height'] <= c['y']+c['height']+.01):
            v.click(identifier)
            return
        v.key(15)
    raise AssertionError('Control did not become reachable: '+identifier)


def main():
    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    monitors = json.loads(v.session('hyprctl', 'monitors', '-j'))
    assert len(monitors) == 1 and tuple(monitors[0][k] for k in ('width','height','x','y','scale')) == (1920,1080,0,0,1)
    v.eventually(lambda: a.audio()['status'] == 'ready', 'Audio not ready')
    previous_output, previous_input = a.pactl('get-default-sink'), a.pactl('get-default-source')
    tag = 'lucent_panel_' + uuid.uuid4().hex
    names = [tag+'_a', tag+'_b']
    modules, units, checks = [], [], []
    try:
        for name in names:
            module = a.pactl('load-module', 'module-null-sink', 'sink_name='+name)
            assert module.isdigit()
            modules.append(module)
        v.eventually(lambda: all(any(d['key'] == key for d in devices()) for name in names for key in (name, name+'.monitor')), 'Owned devices not observed')
        output = next(d for d in devices() if d['key'] == names[0])
        microphone = next(d for d in devices() if d['key'] == names[0]+'.monitor')
        v.cli('audio','open')
        v.eventually(lambda: a.audio()['open'] and any(s['id']=='audio' and s['frames']>0 for s in v.inspect()['surfaces']), 'Panel did not render')
        click('audio-tab-Outputs')
        click('audio-default-'+str(output['id']))
        v.eventually(lambda: a.pactl('get-default-sink') == names[0], 'Default output did not change')
        click('audio-volume-'+str(output['id']))
        v.eventually(lambda: abs(next(d['volume'] for d in devices() if d['id']==output['id'])-.5)<.01, 'Output slider did not apply')
        click('audio-mute-'+str(output['id']))
        v.eventually(lambda: next(d['muted'] for d in devices() if d['id']==output['id']), 'Output mute did not apply')
        checks += ['native_output_default_volume_mute']
        click('audio-tab-Inputs')
        click('audio-default-'+str(microphone['id']))
        v.eventually(lambda: a.pactl('get-default-source') == names[0]+'.monitor', 'Default input did not change')
        click('audio-volume-'+str(microphone['id']))
        v.eventually(lambda: abs(next(d['volume'] for d in devices() if d['id']==microphone['id'])-.5)<.01, 'Input slider did not apply')
        click('audio-mute-'+str(microphone['id']))
        v.eventually(lambda: next(d['muted'] for d in devices() if d['id']==microphone['id']), 'Input mute did not apply')
        checks += ['native_input_default_volume_mute']
        for mode, direction, device in [('playback','output',names[0]), ('record','input',names[0]+'.monitor')]:
            unit = tag+'-'+mode
            units.append(unit)
            args = ['systemd-run','--user','--collect','--unit',unit,'--property=StandardOutput=null']
            if mode == 'playback': args += ['--property=StandardInput=file:/dev/zero']
            args += ['pacat','--'+mode,'--raw','--rate=48000','--channels=2','--format=s16le',
                     '--device='+device,'--client-name='+tag,'--stream-name='+tag+'-'+mode]
            v.session(*args)
            v.eventually(lambda: any(s['name']==tag+'-'+mode and s['direction']==direction for s in streams()), 'Owned stream not observed')
        click('audio-tab-Apps')
        for mode, direction, target in [('playback','output',names[1]), ('record','input',names[1]+'.monitor')]:
            stream = next(s for s in streams() if s['name']==tag+'-'+mode and s['direction']==direction)
            destination = next(d for d in devices() if d['key']==target)
            if stream['volume_writable'] and stream['volume'] is not None:
                click('audio-volume-'+str(stream['id']))
                v.eventually(lambda: abs(next(s['volume'] for s in streams() if s['id']==stream['id'])-.5)<.01, 'Stream slider did not apply')
                checks.append('native_'+mode+'_stream_volume')
            click('audio-route-'+str(stream['id']))
            click('audio-route-device-'+str(destination['id']))
            v.eventually(lambda: next(s['device'] for s in streams() if s['id']==stream['id']) == destination['id'], 'Stream route not observed')
            # Independently confirm the backend index, not only our inspection JSON.
            kind, endpoints, field = ('sink-inputs','sinks','sink') if direction=='output' else ('source-outputs','sources','source')
            target_index = next(d['index'] for d in json.loads(a.pactl('--format=json','list',endpoints)) if d['name']==target)
            actual = next(s for s in json.loads(a.pactl('--format=json','list',kind)) if s['properties'].get('media.name')==tag+'-'+mode)
            assert actual[field] == target_index
            checks.append('native_'+mode+'_routing_and_backend_confirmation')
            click('audio-back')
        v.key(1)
        v.eventually(lambda: not a.audio()['open'] and not any(s['id']=='audio' for s in v.inspect()['surfaces']), 'Closed panel retained native buffers')
        v.click('bar-audio')
        v.eventually(lambda: a.audio()['open'], 'Top-bar entry did not reopen panel')
        v.cli('audio','close')
        v.eventually(lambda: not any(s['id']=='audio' for s in v.inspect()['surfaces']), 'Panel failed to unmount')
        checks += ['escape_close_unmount', 'bar_reopen', 'ipc_close_unmount']
        v.session('omarchy-shell', 'shell', 'toggle', 'omarchy.audio')
        v.eventually(lambda: a.audio()['open'], 'Compatibility toggle did not open panel')
        v.session('omarchy-shell', 'shell', 'toggle', 'omarchy.audio')
        v.eventually(lambda: not a.audio()['open'], 'Compatibility toggle did not close panel')
        v.session('omarchy-shell', 'shell', 'summon', 'omarchy.audio')
        v.eventually(lambda: a.audio()['open'], 'Compatibility summon did not open panel')
        v.session('omarchy-shell', 'shell', 'hide')
        v.eventually(lambda: not any(s['id']=='audio' for s in v.inspect()['surfaces']), 'Compatibility hide did not unmount panel')
        v.session('python3', '/home/omarchy/.local/lib/lucent/controls.py', 'audio')
        v.eventually(lambda: a.audio()['open'], 'Controls entrypoint did not open native panel')
        v.cli('audio', 'close')
        v.eventually(lambda: not any(s['id']=='audio' for s in v.inspect()['surfaces']), 'Controls entrypoint panel did not unmount')
        checks += ['omarchy_audio_toggle_open_close', 'omarchy_audio_summon_hide_unmount', 'controls_audio_opens_native_panel']
    finally:
        cleanup_errors = []
        def attempt(label, operation):
            try:
                operation()
            except Exception as error:
                cleanup_errors.append((label, type(error).__name__))
        attempt('close panel', lambda: v.cli('audio','close'))
        for unit in units:
            attempt('stop owned stream', lambda unit=unit: v.session('systemctl','--user','stop',unit))
        for module in reversed(modules):
            attempt('remove owned device', lambda module=module: a.pactl('unload-module',module))
        for kind, previous in [('sink',previous_output),('source',previous_input)]:
            attempt('observe original '+kind, lambda previous=previous: v.eventually(lambda: any(d['key']==previous for d in devices()), 'Original device not restored'))
            attempt('restore default '+kind, lambda kind=kind,previous=previous: a.pactl('set-default-'+kind,previous))
            def verify_default(kind=kind, previous=previous):
                assert a.pactl('get-default-'+kind) == previous
            attempt('verify default '+kind, verify_default)
        attempt('verify fixture removal', lambda: v.eventually(lambda: not any(d['key'].startswith(tag) for d in devices()) and not any(s['name'].startswith(tag) for s in streams()), 'Owned fixtures remained'))
        assert not cleanup_errors, cleanup_errors
    checks += ['owned_streams_and_devices_removed', 'original_default_devices_restored']
    report = {'checks':checks,'scope':'Owned silent virtual playback and capture streams on the native panel; no physical sound or microphone validation'}
    (v.REPORT/'lucent-audio-panel-native.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    measurement = runpy.run_path(str(Path(__file__).with_name('measure-framework.py')))
    with measurement['stay_awake'](v.remote):
        main()
