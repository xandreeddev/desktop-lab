#!/usr/bin/env python3
"""Compatibility adapter: system commands/data → typed native picker requests.

Audio, media, network and Bluetooth open desktop children; other presentation uses lucent-menu.
Returned values are opaque row IDs,
validated against the current snapshot. They never become shell expressions.
"""
import calendar
from datetime import date
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('picker', HERE/'menu.py' if (HERE/'menu.py').exists() else HERE/'lucent-menu.py')
picker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(picker)


def output(*argv, timeout=8):
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout, check=False)
    if result.returncode:
        # Tool output can contain connection secrets; do not include it in logs.
        raise RuntimeError(f'{argv[0]} could not complete the request')
    return result.stdout.strip()


def choose(title, rows):
    """rows: (label, detail, value); values stay exclusively in the adapter."""
    payload = {'prompt': title, 'entries': [
        {'label': label, 'detail': detail, 'value': str(i), 'disabled': value is None}
        for i, (label, detail, value) in enumerate(rows)]}
    result = picker.pick(payload, capture=True)
    if result.returncode != 0:
        return None
    key = result.stdout.strip()
    if not key.isdecimal() or not 0 <= int(key) < len(rows):
        raise ValueError('The picker returned an unoffered action')
    return rows[int(key)][2]


def information(title, text):
    choose(title, [(line, '', None) for line in text.splitlines() if line] or [('No information available', '', None)])


def audio():
    output(str(Path.home()/'.local/bin/lucent-cli'), 'audio', 'open')


def media():
    output(str(Path.home()/'.local/bin/lucent-cli'), 'media', 'open')


def network():
    output(str(Path.home()/'.local/bin/lucent-cli'), 'network', 'open')


def bluetooth():
    output(str(Path.home()/'.local/bin/lucent-cli'), 'bluetooth', 'open')


def monitor():
    while True:
        monitors = json.loads(output('hyprctl','-j','monitors'))
        rows = [(m['name'],f"{m['width']} × {m['height']} · scale {m['scale']}",None) for m in monitors]
        rows.extend((f'Scale {scale}×','Focused display',('omarchy-hyprland-monitor-scaling',scale)) for scale in ['1','1.25','1.6','2','3','4'])
        rows.extend([('Brightness up','+5%',('omarchy-brightness-display','+5%')),
                     ('Brightness down','−5%',('omarchy-brightness-display','5%-'))])
        action = choose('Displays',rows)
        if action is None: return
        output(*action)


def power():
    output(str(Path.home()/'.local/bin/lucent-cli'), 'power', 'open')


def paste(text=None, path=None):
    if path:
        with Path(path).open('rb') as stream:
            subprocess.run(['wl-copy','--type','image/png'],stdin=stream,check=True,timeout=5)
    else:
        subprocess.run(['wl-copy'],input=text.encode(),check=True,timeout=5)
    time.sleep(.15)  # The picker has already destroyed its exclusive keyboard surface.
    subprocess.run(['wtype','-M','shift','-k','Insert','-m','shift'],check=True,timeout=5)


def clipboard():
    import clipboard_store
    history = clipboard_store.read()
    rows = [(item.get('text','Image').replace('\n',' ')[:120], 'Text' if item['type']=='text' else 'PNG image', item) for item in history]
    rows.append(('Clear clipboard history','Remove saved text and images','clear'))
    selected = choose('Clipboard',rows)
    if selected == 'clear': clipboard_store.clear()
    elif isinstance(selected,dict): paste(text=selected.get('text'),path=selected.get('path'))


def emojis():
    source = Path(os.environ.get('OMARCHY_PATH','/usr/share/omarchy'))/'shell/plugins/emojis/emojis.json'
    entries = json.loads(source.read_text())
    selected = choose('Emoji',[(item['k'],item['e'],item['e']) for item in entries])
    if selected: paste(text=selected)


def reminders():
    while True:
        data=json.loads(output('omarchy-reminder','list','--json'))
        rows=[(item['label'],item['remaining']+' · '+item['atTime'],None) for item in data['reminders']]
        rows+=[('Set a reminder','Choose a delay','create'),('Clear all reminders','Cancel scheduled reminders','clear')]
        choice=choose('Reminders',rows)
        if choice is None:return
        if choice=='clear':output('omarchy-reminder','clear')
        elif choice=='create':
            delay=choose('Remind me in',[(str(n)+' minutes','',str(n)) for n in (1,5,10,15,30,60)])
            if delay:
                result=picker.pick({'mode':'input','prompt':'Reminder message'},capture=True)
                if result.returncode==0:output('omarchy-reminder',delay,result.stdout.rstrip('\n'))


def main(panel):
    handlers = {'audio':audio,'media':media,'network':network,'bluetooth':bluetooth,'monitor':monitor,'power':power,
                'clipboard':clipboard,'emojis':emojis,'reminders':reminders,
                'clock':lambda: information('Calendar',calendar.month(date.today().year,date.today().month)),
                'weather':lambda: output('lucent-cli','widget','weather','show')}
    if panel not in handlers:
        raise ValueError('This panel has no Lucent adapter: '+panel)
    try: handlers[panel]()
    except (RuntimeError, OSError, subprocess.SubprocessError, ValueError) as error:
        information(panel.capitalize(),str(error))
        return 1
    return 0

if __name__=='__main__':
    raise SystemExit(main(sys.argv[1]))
