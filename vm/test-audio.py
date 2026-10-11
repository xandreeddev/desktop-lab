#!/usr/bin/env python3
"""Observe and control a temporary virtual audio output through the real desktop.

Requires an unlocked 1920x1080 scale-1 Lucent guest. Uses a uniquely named null
sink; restores the previous default output, its channel volumes and mute state.
No hardware sound or credentials are used. Reports contain no device names.
"""
import importlib.util
import json
from pathlib import Path
import runpy
import time
import uuid

spec = importlib.util.spec_from_file_location('framework_test', Path(__file__).with_name('test-framework.py'))
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def pactl(*args):
    return v.session('pactl', *args).strip()


def sinks():
    return json.loads(pactl('--format=json', 'list', 'sinks'))


def audio():
    return v.inspect()['client']['audio']


def main():
    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'Unlock the guest first'
    monitors = json.loads(v.session('hyprctl', 'monitors', '-j'))
    assert len(monitors) == 1 and monitors[0]['scale'] == 1
    assert (monitors[0]['width'], monitors[0]['height'], monitors[0]['x'], monitors[0]['y']) == (1920, 1080, 0, 0)
    v.eventually(lambda: audio()['status'] == 'ready', 'Native audio subscription did not become ready')
    previous = pactl('get-default-sink')
    original = next(s for s in sinks() if s['name'] == previous)
    name = 'lucent_test_' + uuid.uuid4().hex
    module = None
    checks = []
    measurement = runpy.run_path(str(Path(__file__).with_name('measure-framework.py')))
    with measurement['stay_awake'](v.remote):
        try:
            module = pactl('load-module', 'module-null-sink', 'sink_name=' + name)
            assert module.isdigit()
            v.eventually(lambda: any(s['name'] == name for s in sinks()), 'Temporary output was not created')
            pactl('set-sink-volume', name, '37%')
            pactl('set-sink-mute', name, '0')
            pactl('set-default-sink', name)
            v.eventually(lambda: abs((audio()['output_volume'] or 0) - .37) < .001 and audio()['output_muted'] is False,
                         'Default-output/volume event did not reach the desktop')
            checks.append('native_default_policy_and_external_volume_event')
            v.cli('launcher', 'close')
            v.click('bar-mute')
            v.eventually(lambda: audio()['output_muted'] is True, 'Bar mute did not reach the adapter')
            assert next(s for s in sinks() if s['name'] == name)['mute'] is True
            v.click('bar-mute')
            v.eventually(lambda: audio()['output_muted'] is False, 'Bar unmute did not reach the adapter')
            checks.append('real_pointer_mute_and_unmute')
            v.session('systemctl', '--user', 'restart', 'lucent.service')
            v.eventually(lambda: audio()['status'] == 'ready' and abs((audio()['output_volume'] or 0) - .37) < .001,
                         'Audio subscription did not recover with the desktop', timeout=20)
            checks.append('desktop_restart_restores_audio_observation')
        finally:
            if module is not None:
                # Keep the original output selected if it survived creation of
                # the fixture. PipeWire's auto_null may instead be recreated.
                if any(s['name'] == previous for s in sinks()):
                    pactl('set-default-sink', previous)
                pactl('unload-module', module)
                v.eventually(lambda: any(s['name'] == previous for s in sinks()), 'Original output was not restored')
                pactl('set-default-sink', previous)
                pactl('set-sink-volume', previous, *(str(c['value']) for c in original['volume'].values()))
                pactl('set-sink-mute', previous, '1' if original['mute'] else '0')
                assert not any(s['name'] == name for s in sinks())
                restored = next(s for s in sinks() if s['name'] == previous)
                assert restored['volume'] == original['volume'] and restored['mute'] == original['mute']
    v.eventually(lambda: audio()['status'] == 'ready', 'Audio unavailable after cleanup')
    checks.append('owned_fixture_removed_and_original_audio_restored')
    report = {'checks': checks, 'audio_status': audio()['status'], 'scope': 'virtual default output and existing bar control; no stream-routing or physical audio validation'}
    (v.REPORT / 'lucent-audio-native.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
