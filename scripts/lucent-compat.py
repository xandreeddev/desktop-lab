#!/usr/bin/env python3
"""Explicit Omarchy command compatibility, with Lucent as the only shell UI.

This adapter owns names/argv translation, not rendering. Unknown stock plugin
operations fail visibly; they never launch a second shell behind the desktop.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

HOME=Path.home()
HERE=Path(__file__).resolve().parent
ENABLED=HOME/'.local/state/lucent/integration-backup/menu-enabled'


def cli(*args):
    return subprocess.run([str(HOME/'.local/bin/lucent-cli'),*args],check=True,capture_output=True,text=True).stdout


def launch(script,*args):
    subprocess.Popen(['python3',str(HERE/script),*args],stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)


def panel(name,toggle=False):
    if name.startswith('omarchy.'): name=name.removeprefix('omarchy.')
    if name in ('weather','clock'): return cli('widget','weather' if name=='weather' else 'calendar','show')
    if name in ('speedtest','disk-speedtest'):
        command='omarchy-network-speedtest' if name=='speedtest' else 'omarchy-disk-speedtest'
        subprocess.Popen(['xdg-terminal-exec','--app-id=org.lucent.utility','--title=System test','-e',command],start_new_session=True)
        return 'ok'
    if name in ('audio','network','bluetooth','monitor','power','clipboard','emojis','reminders'):
        import menu_routes
        current=menu_routes.picker_command('inspect')
        if current is not None:
            title=json.loads(current).get('client',{}).get('prompt','')
            menu_routes.picker_command('close')
            if toggle and title=={'monitor':'Displays','emojis':'Emoji'}.get(name,name.capitalize()): return 'ok'
            for _ in range(40):
                if menu_routes.picker_command('inspect') is None: break
                time.sleep(.05)
        cli('launcher','close')
        launch('controls.py',name)
        return 'ok'
    if name=='notifications': return cli('notifications','show')
    raise ValueError('No Lucent adapter for panel '+name)


def shell(args):
    if args[:1]==['-q']: args=args[1:]
    if len(args)<2: raise ValueError('Expected target and method')
    target,method,*args=args
    if target=='shell':
        if method=='ping': cli('inspect'); return 'ok'
        if method in ('toggle','summon') and args:
            if args[0]=='omarchy.menu':
                payload=json.loads(args[1]) if len(args)>1 else {}
                launch('menu.py','routes',method,str(payload.get('menu','root')))
                return 'ok'
            return panel(args[0],toggle=method=='toggle')
        if method=='togglePanelAt' and len(args)==2:
            names=['audio','network','bluetooth','power','monitor','clock']
            index=int(args[1])-1
            if 0<=index<len(names): return panel(names[index])
            raise ValueError('No Lucent panel at this position')
        if method=='hide':
            # Closing a compatibility panel only dismisses the native picker.
            import menu_routes
            menu_routes.picker_command('close')
            return 'ok'
    if target=='background' and method=='set' and len(args)==1:
        return cli('wallpaper','set',args[0])
    if target=='osd':
        if method=='show' and len(args)==1:
            value=json.loads(args[0])
            # Only text/progress is forwarded; no command-bearing payloads.
            return cli('osd','show',json.dumps({key:str(value.get(key,''))[:500] for key in ('message','value','progressText','max')}))
        if method=='close': return cli('osd','close')
    if target=='notifications':
        routes={'dismissOne':'dismiss-last','dismissAll':'dismiss-all','invokeLast':'invoke-last',
                'showHistory':'show','toggleDnd':'dnd','dismiss':'dismiss'}
        if method=='ping':
            if not json.loads(cli('inspect'))['client']['notifications_ready']: raise ValueError('Notifications not ready')
            return 'ok'
        if method in routes: return cli('notifications',routes[method],*args)
    if target=='media' and method in ('next','previous','playPause'):
        subprocess.run(['playerctl',{'playPause':'play-pause'}.get(method,method)],check=True)
        return 'ok'
    if target=='lock':
        if method=='isLocked':
            return str(subprocess.run(['omarchy-hyprland-session-locked'],stdout=subprocess.DEVNULL,check=False).returncode==0).lower()
        if method=='lock':
            subprocess.run(['python3',str(HERE/'lock.py')],check=True)
            return 'ok'
        if method=='status':
            locked=subprocess.run(['omarchy-hyprland-session-locked'],stdout=subprocess.DEVNULL,check=False).returncode==0
            return json.dumps({'locked':locked,'locking':False})
    if target=='omarchy.bar' and method=='syncHidden':
        hidden=(HOME/'.local/state/omarchy/toggles/bar-off').exists()
        marker=HOME/'.local/state/lucent/bar-hidden'
        if hidden: marker.touch()
        else: marker.unlink(missing_ok=True)
        return cli('bar','hide' if hidden else 'show')
    if (target,method) in {('nightlight','refresh'),('omarchy.clock','refresh'),('omarchy.indicators','refresh'),
                           ('omarchy.system-update','refresh'),('omarchy.system-update','clear')}:
        # Backends have already applied these operations. Lucent subscriptions
        # read time/system state independently; no stock-plugin reload is needed.
        return 'ok'
    raise ValueError(f'Unsupported stock shell operation: {target} {method}')


def main(name,args):
    if not ENABLED.exists(): os.execv('/usr/bin/'+name,[name,*args])
    if name=='omarchy-shell': return shell(args)
    if name in ('omarchy-launch-shell','omarchy-restart-shell'):
        subprocess.run(['systemctl','--user','restart' if name=='omarchy-restart-shell' else 'start','lucent.service'],check=True)
        return 'ok'
    if name=='omarchy-theme-set':
        mode=args[0].removeprefix('lucent-tokens-') if args else ''
        if mode not in ('dark','light'): raise ValueError('Lucent owns themes. Use lucent-cli themes open or lucent-cli theme dark|light.')
        return cli('theme',mode)
    if name in ('omarchy-theme-switcher','omarchy-theme-bg-switcher'):
        if '--preload' in args: return 'ok'
        return cli('themes' if name=='omarchy-theme-switcher' else 'wallpapers','open')
    if name=='omarchy-launch-screensaver':
        # Locking provides the same privacy boundary without another animated UI.
        subprocess.run(['python3',str(HERE/'lock.py')],check=True)
        return 'ok'
    raise ValueError('Unsupported compatibility command '+name)

if __name__=='__main__':
    try:
        result=main(sys.argv[1],sys.argv[2:])
        if result: print(result.strip())
    except (OSError,ValueError,subprocess.SubprocessError) as error:
        print('lucent: '+str(error),file=sys.stderr)
        raise SystemExit(1)
