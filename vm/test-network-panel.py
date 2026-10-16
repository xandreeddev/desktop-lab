#!/usr/bin/env python3
"""Exercise the native network child through real pointer/keyboard input.

Uses only an owned virtual WPA2 AP inside the capped guest. Credentials enter
the verified masked field through virtual keyboard events, never CLI arguments.
Root owns the fixture; the desktop's ordinary user makes network requests.
The original profiles, routes, wallpaper and desktop process are preserved.
"""
import argparse
import ast
import importlib.util
import json
from pathlib import Path
from native_surfaces import active_surfaces, find_surface, role
import runpy
import shlex
import subprocess
import time
import uuid
import lab

spec = importlib.util.spec_from_file_location('network_input', Path(__file__).with_name('test-framework.py'))
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


def state():
    return v.inspect()['client']['network']


def panel():
    return find_surface(v.inspect(), 'network')


def click(identifier):
    # Tab uses the framework's scroll reveal; never click outside the clip.
    for _ in range(len(panel()['hits']) + 2):
        hit = next(h for h in panel()['hits'] if h['id'] == identifier)
        clip = hit['clip']
        if (hit['y'] >= clip['y'] and hit['y']+hit['height'] <= clip['y']+clip['height']+.01):
            v.click(identifier)
            return
        v.key(15)
    raise AssertionError('Network control did not become reachable')


def enter(value, field):
    v.eventually(lambda: panel()['focus'] == field, 'Expected input is not focused')
    assert state()['open'] and panel()['pointer_input'] == 'all'
    assert v.session('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    assert v.layer_geometry('network')['namespace'] == 'lucent-' + panel()['id']
    mapping = {}
    for node in ast.walk(ast.parse(Path(__file__).with_name('run.py').read_text())):
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name) and node.targets[0].id in ('normal','shifted','codes'):
            mapping[node.targets[0].id] = ast.literal_eval(node.value)
    for char in value:
        if char in mapping['normal']:
            keys = [mapping['codes'][mapping['normal'].index(char)]]
        elif char in mapping['shifted']:
            keys = [42, mapping['codes'][mapping['shifted'].index(char)]]
        else:
            raise ValueError('Unsupported fixture keyboard character')
        v.key(*keys)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--login-file', type=Path, required=True)
    args = parser.parse_args()
    password = next(line.split(':',1)[1].strip() for line in args.login_file.read_text().splitlines() if line.startswith('Password:'))

    def root(*command):
        return subprocess.check_output(lab.ssh_args('lucent') + ['sudo -S -p "" '+shlex.join(command)], input=password+'\n', text=True, stderr=subprocess.DEVNULL)

    assert v.remote('systemd-detect-virt','--vm').strip() in ('qemu','kvm')
    assert v.session('omarchy-shell','lock','isLocked').strip() == 'false'
    monitors=json.loads(v.session('hyprctl','monitors','-j'))
    assert len(monitors)==1 and tuple(monitors[0][k] for k in ('width','height','x','y','scale'))==(1920,1080,0,0,1)
    v.eventually(lambda: state()['status']=='ready','Network subscription unavailable')
    before=v.remote('nmcli','-t','-f','UUID,NAME,TYPE','connection','show').splitlines()
    routes=json.loads(v.remote('ip','-j','route','show','default'))
    desktop_pid=v.remote('pgrep','-x','lucent-desktop').strip()
    wallpaper=v.inspect()['client']['wallpaper']
    directory='/tmp/lucent-network-'+uuid.uuid4().hex
    v.remote('mkdir','-m','700',directory)
    subprocess.run(lab.ssh_args('lucent')+['cat > '+shlex.quote(directory+'/fixture.py')],input=Path(__file__).with_name('network_fixture.py').read_bytes(),check=True)
    started=False
    checks=[]
    try:
        assert not any(d['kind']=='Wifi' for d in state()['devices'])
        root('python3',directory+'/fixture.py','start',directory);started=True
        fixture=json.loads(root('python3',directory+'/fixture.py','info',directory))
        v.eventually(lambda: any(d['interface']==fixture['station'] and d['state']=='Disconnected' for d in state()['devices']),'Virtual Wi-Fi did not become ready',timeout=35)
        device=next(d for d in state()['devices'] if d['interface']==fixture['station'])
        v.click('bar-network')
        v.eventually(lambda: state()['open'] and panel()['frames']>0,'Bar did not open the native panel')
        for _ in range(5):
            click('network-scan')
            v.eventually(lambda: not state()['busy'],'Scan acknowledgement timed out')
            if state()['error'] is None: break
            time.sleep(3)
        else: raise RuntimeError('Scan never accepted; inspect the private failure report')
        ssid=list(fixture['ssid'].encode())
        v.eventually(lambda: any(a['device']==device['id'] and a['ssid']==ssid for a in state()['access_points']),'Fixture AP not discovered',timeout=35)
        ap=next(a for a in state()['access_points'] if a['device']==device['id'] and a['ssid']==ssid)
        click('network-ap-'+str(ap['id']))
        generation=state()['form']['generation'];field='network-password-'+str(generation)
        click(field);enter(fixture['password'],field)
        assert fixture['password'] not in json.dumps(v.inspect())
        v.screenshot('lucent-network-panel-password.png')
        v.key(15);v.eventually(lambda: panel()['focus']=='network-remember','Tab did not reach Remember')
        v.key(42,15);v.eventually(lambda: panel()['focus']==field,'Shift+Tab did not return to masked field')
        v.key(28)
        v.eventually(lambda: any(p['ssid']==ssid for p in state()['profiles']),'Join did not create a profile',timeout=35)
        profile=next(p for p in state()['profiles'] if p['ssid']==ssid)
        root('python3',directory+'/fixture.py','register',directory,profile['key'])

        def connected():
            return any(d['id']==device['id'] and d['state']=='Connected' for d in state()['devices'])

        v.eventually(connected,'WPA2 association did not complete',timeout=35)
        assert fixture['ssid'] in v.remote('iw','dev',fixture['station'],'link')
        assert '198.18.0.' in v.remote('nmcli','-g','IP4.ADDRESS','device','show',fixture['station'])
        checks += ['bar_opens_native_panel','scan_and_real_ap_discovery','masked_input_tab_backtab_enter_no_secret_in_inspect','regular_user_wpa2_association_and_dhcp']
        click('network-tab-Active')
        connection=next(c for c in state()['connections'] if device['id'] in c['devices'])
        click('network-disconnect-'+str(connection['id']))
        v.eventually(lambda: not connected() and not any(device['id'] in c['devices'] for c in state()['connections']),'Disconnect did not finish',timeout=35)
        click('network-tab-Saved');click('network-activate-'+str(profile['id']))
        v.eventually(connected,'Saved activation did not connect',timeout=35)
        assert v.remote('nmcli','-g','GENERAL.CON-UUID','device','show',fixture['station']).strip()==profile['key']
        click('network-forget-'+str(profile['id']))
        assert any(p['key']==profile['key'] for p in state()['profiles'])
        click('network-forget-confirm')
        v.eventually(lambda: not connected() and not any(p['key']==profile['key'] for p in state()['profiles']),'Confirmed deletion did not finish',timeout=35)
        checks += ['targeted_disconnect_saved_activation_confirmed_forget']
        root('python3',directory+'/fixture.py','hide',directory)
        click('network-tab-Wi-Fi');click('network-hidden')
        generation=state()['form']['generation'];name='network-name-'+str(generation)
        click(name);enter(fixture['ssid'],name)
        field='network-password-'+str(generation);click(field);enter(fixture['password'],field)
        click('network-remember');click(field);v.key(28)
        v.eventually(lambda: any(p['ssid']==ssid for p in state()['profiles']),'Hidden profile not observed',timeout=35)
        hidden=next(p for p in state()['profiles'] if p['ssid']==ssid)
        root('python3',directory+'/fixture.py','register',directory,hidden['key'])
        v.eventually(connected,'Hidden association did not complete',timeout=35)
        assert v.remote('nmcli','-g','802-11-wireless.hidden','connection','show','uuid',hidden['key']).strip()=='yes'
        assert fixture['ssid'] in v.remote('iw','dev',fixture['station'],'link')
        click('network-tab-Active')
        connection=next(c for c in state()['connections'] if device['id'] in c['devices'])
        click('network-disconnect-'+str(connection['id']))
        v.eventually(lambda: not any(p['key']==hidden['key'] for p in state()['profiles']),'Temporary profile persisted after disconnect',timeout=35)
        checks += ['native_hidden_join_and_temporary_profile_cleanup']
        click('network-wifi');v.eventually(lambda:not state()['wifi_enabled'],'Radio off not observed')
        click('network-wifi');v.eventually(lambda:state()['wifi_enabled'],'Radio on not observed')
        v.session('omarchy-shell','shell','hide');v.eventually(lambda:not state()['open'],'Compatibility hide failed')
        v.eventually(lambda:not any(role(s)=='network' for s in active_surfaces(v.inspect())),'Closed overlay did not unmount')
        v.session('omarchy-shell','shell','summon','omarchy.network');v.eventually(lambda:state()['open'],'Compatibility summon failed')
        v.key(1);v.eventually(lambda:not state()['open'],'Escape failed to close network panel')
        checks += ['wifi_radio_changes','compatibility_and_escape_release_surface']
        root('python3',directory+'/fixture.py','stop',directory);started=False
        v.eventually(lambda:not any(d['kind']=='Wifi' for d in state()['devices']),'Removed device remained in state')
        checks += ['virtual_device_removal_observed']
    except BaseException:
        (v.REPORT/'network-panel-native-failure.json').write_text(json.dumps(v.inspect(),indent=2))
        raise
    finally:
        v.cli('network','close')
        if started: root('python3',directory+'/fixture.py','stop',directory)
        root('rm','-r',directory)
    assert sorted(v.remote('nmcli','-t','-f','UUID,NAME,TYPE','connection','show').splitlines())==sorted(before)
    assert json.loads(v.remote('ip','-j','route','show','default'))==routes
    assert v.remote('pgrep','-x','lucent-desktop').strip()==desktop_pid
    assert v.inspect()['client']['wallpaper']==wallpaper
    checks += ['original_profiles_routes_wallpaper_desktop_and_fixture_cleanup']
    report={'checks':checks,'binary_sha256':v.remote('sha256sum','.local/bin/lucent-desktop').split()[0],
        'scope':'Regular-user native panel with real pointer/keyboard, NetworkManager and owned simulated WPA2/hidden radios. No physical Wi-Fi, SAE/OWE/enterprise, captive-portal or suspend proof.'}
    (v.REPORT/'lucent-network-panel-native.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    measurement=runpy.run_path(str(Path(__file__).with_name('measure-framework.py')))
    with measurement['stay_awake'](v.remote): main()
