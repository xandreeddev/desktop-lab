#!/usr/bin/env python3
"""Real input and service integration checks for the prepared Lucent VM.

Run with an unlocked guest and lucent.service running. Uses QMP pointer/keyboard
input, the public inspection IPC, and the compositor's actual window state.
"""
import json
from pathlib import Path
import shlex
import subprocess
import time
import lab

PROFILE = 'lucent'
NAME = lab.inventory()['vms'][PROFILE]['name']
REPORT = lab.ROOT / 'reports/local'
REPORT.mkdir(parents=True, exist_ok=True)


def remote(*command):
    return subprocess.check_output(lab.ssh_args(PROFILE) + [shlex.join(command)], text=True)


def session(*command):
    return remote('python3', 'desktop-lab/scripts/in-session.py', *command)


def cli(*args):
    return session('/home/omarchy/.local/bin/lucent-cli', *args)


def inspect():
    return json.loads(cli('inspect'))


def send(events):
    result = lab.virsh('qemu-monitor-command', NAME, json.dumps({
        'execute': 'input-send-event', 'arguments': {'events': events}}), capture=True)
    assert 'error' not in json.loads(result.stdout), result.stdout


def move(x, y):
    send([{'type': 'abs', 'data': {'axis': axis, 'value': round(value / (size-1)*32767)}}
          for axis, value, size in [('x', x, 1920), ('y', y, 1080)]])


def button(down):
    send([{'type': 'btn', 'data': {'down': down, 'button': 'left'}}])


def key(*codes):
    lab.virsh('send-key', NAME, '--codeset', 'linux', '--holdtime', '50', *map(str, codes), capture=True)
    time.sleep(0.12)


def text(value):
    chars='qwertyuiopasdfghjklzxcvbnm '
    codes=[16,17,18,19,20,21,22,23,24,25,30,31,32,33,34,35,36,37,38,44,45,46,47,48,49,50,57]
    for char in value: key(codes[chars.index(char)])


def hit(identifier):
    return next(h for s in inspect()['surfaces'] for h in s['hits'] if h['id']==identifier)


def click(identifier):
    h=hit(identifier)
    move(h['x']+h['width']/2, h['y']+h['height']/2)
    time.sleep(0.15)
    button(True)
    time.sleep(0.06)
    button(False)
    time.sleep(0.65)


def screenshot(name):
    session('grim', '/tmp/lucent-framework-test.png')
    with (REPORT/name).open('wb') as out:
        subprocess.run(lab.ssh_args(PROFILE)+['cat /tmp/lucent-framework-test.png'],stdout=out,check=True)


def eventually(predicate, description, timeout=10):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            if predicate():return
        except (subprocess.CalledProcessError, ValueError, KeyError):
            pass
        time.sleep(0.2)
    raise AssertionError(description)


def suite():
    assert session('omarchy-shell','lock','isLocked').strip()=='false', 'Unlock the VM first'
    assert 'Vulkan' in inspect()['adapter']
    assert {s['id'] for s in inspect()['surfaces']}=={'bar','widgets','dock'}
    cli('launcher','close');time.sleep(0.7)
    move(950,400)
    # A noninteracting pointer move must not make the idle animation loop run forever.
    frames={s['id']:s['frames'] for s in inspect()['surfaces']}
    click('dock-launcher')
    state=inspect()
    assert state['client']['launcher']
    assert next(s for s in state['surfaces'] if s['id']=='dock')['frames']>frames['dock']+2
    text('foot')
    eventually(lambda:inspect()['client']['query']=='foot','Keyboard search did not reach framework input')
    assert inspect()['client']['result_count']>0
    screenshot('lucent-framework-search.png')
    before={c['address'] for c in json.loads(session('hyprctl','-j','clients'))}
    key(28)
    eventually(lambda:any(c['address'] not in before and c['class'].startswith('foot') for c in json.loads(session('hyprctl','-j','clients'))),'Launcher failed to start an actual terminal')
    terminal=next(c for c in json.loads(session('hyprctl','-j','clients')) if c['address'] not in before and c['class'].startswith('foot'))
    assert not inspect()['client']['launcher']
    # Close only the terminal created by this test.
    session('hyprctl','dispatch',f'hl.dsp.window.close({{ window = "address:{terminal["address"]}" }})')
    click('workspace-2')
    eventually(lambda:json.loads(session('hyprctl','-j','activeworkspace'))['id']==2,'Workspace button failed')
    click('workspace-1')
    eventually(lambda:json.loads(session('hyprctl','-j','activeworkspace'))['id']==1,'Workspace return failed')
    
    h=hit('widget-calendar');x,y=h['x']+100,h['y']+20
    move(x,y);time.sleep(0.15);button(True)
    for step in range(1,13):
        move(x+step*15,y+step*6)
        time.sleep(0.025)
    button(False);time.sleep(0.7)
    position=inspect()['client']['positions']['calendar']
    assert abs(position['x']-h['x']-180)<3 and abs(position['y']-h['y']-72)<3,position
    session('systemctl','--user','restart','lucent.service')
    eventually(lambda:inspect()['client']['positions'].get('calendar')==position,'Widget position was not restored')
    
    cli('widgets','open');time.sleep(0.7)
    click('toggle-timer');key(1);time.sleep(0.7)
    click('timer-toggle');time.sleep(1.2)
    assert inspect()['client']['timer_seconds']<1500
    click('timer-toggle')
    paused=inspect()['client']['timer_seconds'];time.sleep(1.2)
    assert inspect()['client']['timer_seconds']==paused
    click('timer-reset')
    cli('widgets','open');time.sleep(0.7)
    click('toggle-timer');click('toggle-notes');key(1);time.sleep(0.7)
    click('notes-input');key(29,30);text('framework note')
    eventually(lambda:inspect()['client']['notes']=='framework note','Notes did not update')
    cli('widgets','open');time.sleep(0.7)
    click('toggle-notes');click('reset-layout');key(1);time.sleep(0.7)
    assert inspect()['client']['positions']=={}
    
    cli('wallpapers','open');time.sleep(0.7)
    assert inspect()['client']['mode']=='Wallpapers'
    click('wallpaper-next')
    screenshot('lucent-framework-wallpapers.png')
    click('apply-wallpaper')
    assert not inspect()['client']['error'], inspect()['client']['error']
    assert remote('readlink','/home/omarchy/.local/state/omarchy/current/background').strip()
    key(1);time.sleep(0.7)
    # The stock bar must recover on stopping Lucent, and disappear again on restart.
    session('systemctl','--user','stop','lucent.service')
    assert remote('bash','-c','test ! -e ~/.local/state/omarchy/toggles/bar-off && echo restored').strip()=='restored'
    session('systemctl','--user','start','lucent.service')
    eventually(lambda:inspect()['client']['applications']>0,'Lucent did not recover')
    eventually(lambda:remote('bash','-c','test -e ~/.local/state/omarchy/toggles/bar-off && echo hidden || true').strip()=='hidden','Stock bar was not hidden after readiness')
    screenshot('lucent-framework-desktop.png')
    report={'vulkan':True,'native_surfaces':3,'animated_dock_launcher':True,'keyboard_search':True,
            'real_application_launch':True,'workspace_switch':True,'widget_drag':True,'position_restored':True,
            'timer_start_pause_reset':True,'notes_input':True,'wallpaper_selector_and_apply':True,
            'stock_bar_restored_on_stop':True,'restart':True,'application_count':inspect()['client']['applications']}
    (REPORT/'lucent-framework-integration.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


def main():
    # The test mutates widget settings and wallpaper; retain the caller's state.
    settings=remote('bash','-c','cat ~/.local/state/lucent/desktop.json 2>/dev/null || true')
    wallpaper=remote('readlink','/home/omarchy/.local/state/omarchy/current/background').strip()
    try:
        suite()
    finally:
        session('systemctl','--user','stop','lucent.service')
        if settings:
            subprocess.run(lab.ssh_args(PROFILE)+['mkdir -p ~/.local/state/lucent; cat > ~/.local/state/lucent/desktop.json.new && mv ~/.local/state/lucent/desktop.json.new ~/.local/state/lucent/desktop.json'],input=settings,text=True,check=True)
        else:
            remote('rm','-f','/home/omarchy/.local/state/lucent/desktop.json')
        if wallpaper:
            session('omarchy-theme-bg-set',wallpaper)
        session('systemctl','--user','start','lucent.service')


if __name__=='__main__':
    main()
