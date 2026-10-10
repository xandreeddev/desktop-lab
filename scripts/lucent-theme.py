#!/usr/bin/env python3
"""Propagate Lucent's generated tokens. Omarchy is a headless config exporter only."""
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

HOME = Path.home()
STATE = HOME/'.local/state/lucent'
BACKUP = STATE/'integration-backup/appearance'
SOURCE = Path(__file__).resolve().parent/'themes'
if not SOURCE.exists():
    SOURCE = Path(__file__).resolve().parent.parent/'configs/lucent/themes'
BEGIN = '/* BEGIN LUCENT TOKENS */\n'
END = '/* END LUCENT TOKENS */\n'

def atomic(path, text):
    if path.is_file() and path.read_text()==text: return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode='w',dir=path.parent,delete=False) as stream:
        temporary=Path(stream.name)
        stream.write(text)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)

def run(*args, **kwargs):
    return subprocess.run(args,check=True,timeout=30,stdout=subprocess.DEVNULL,**kwargs)

def css_import(path, target):
    text=path.read_text() if path.exists() else ''
    if BEGIN in text:
        before,rest=text.split(BEGIN,1)
        if END not in rest: raise ValueError(f'Malformed Lucent CSS block: {path.name}')
        text=before+rest.split(END,1)[1]
    atomic(path,BEGIN+f'@import url("{target.as_uri()}");\n'+END+text)

def apply(mode):
    if mode not in ('dark','light'): raise ValueError('Expected dark or light')
    source=SOURCE/mode
    if not (source/'colors.toml').is_file(): raise ValueError('Install generated Lucent tokens first')
    STATE.mkdir(parents=True,exist_ok=True)
    with (STATE/'appearance.lock').open('w') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        current=HOME/'.local/state/omarchy/current/theme'
        if not BACKUP.exists():
            BACKUP.parent.mkdir(parents=True,exist_ok=True)
            staging=Path(tempfile.mkdtemp(prefix='.appearance-',dir=BACKUP.parent))
            try:
                if current.exists(): shutil.copytree(current,staging/'theme',symlinks=True)
                name=current.parent/'theme.name'
                atomic(staging/'theme.name',name.read_text() if name.exists() else '')
                for version in ('3.0','4.0'):
                    css=HOME/f'.config/gtk-{version}/gtk.css'
                    atomic(staging/f'gtk-{version}.json',json.dumps(css.read_text() if css.exists() else None))
                for key in ('color-scheme','gtk-theme'):
                    result=subprocess.run(['gsettings','get','org.gnome.desktop.interface',key],capture_output=True,text=True,check=True)
                    atomic(staging/key,result.stdout.strip())
                staging.replace(BACKUP)
            finally:
                if staging.exists(): shutil.rmtree(staging)
        # The only themes offered by Lucent come from the compiled token graph.
        target=HOME/f'.config/omarchy/themes/lucent-tokens-{mode}'
        target.mkdir(parents=True,exist_ok=True)
        atomic(target/'colors.toml',(source/'colors.toml').read_text())
        env=dict(os.environ,OMARCHY_THEME_HEADLESS='1',OMARCHY_THEME_SKIP_BACKGROUND='1')
        run('/usr/bin/omarchy-theme-set',f'lucent-tokens-{mode}',env=env)
        atomic(STATE/'theme/gtk.css',(source/'gtk.css').read_text())
        atomic(STATE/'theme/hyprlock.conf',(source/'hyprlock.conf').read_text())
        atomic(STATE/'theme/mode',mode+'\n')
        for version in ('3.0','4.0'):
            css_import(HOME/f'.config/gtk-{version}/gtk.css',STATE/'theme/gtk.css')
        run('gsettings','set','org.gnome.desktop.interface','color-scheme','prefer-'+mode)
        run('gsettings','set','org.gnome.desktop.interface','gtk-theme','Adwaita'+('-dark' if mode=='dark' else ''))
        # These apply exported values; none selects a palette or opens a picker.
        for helper in ('omarchy-theme-set-foot','omarchy-theme-set-tmux','omarchy-theme-set-browser','omarchy-restart-hyprctl'):
            if shutil.which(helper): run(helper)

def restore():
    if not BACKUP.exists(): return
    current=HOME/'.local/state/omarchy/current/theme'
    if (BACKUP/'theme').exists():
        if current.is_symlink(): current.unlink()
        elif current.exists(): shutil.rmtree(current)
        shutil.copytree(BACKUP/'theme',current,symlinks=True)
        atomic(current.parent/'theme.name',(BACKUP/'theme.name').read_text())
    for version in ('3.0','4.0'):
        original=json.loads((BACKUP/f'gtk-{version}.json').read_text())
        path=HOME/f'.config/gtk-{version}/gtk.css'
        text=path.read_text() if path.exists() else ''
        if BEGIN in text and END in text:
            before,rest=text.split(BEGIN,1)
            text=before+rest.split(END,1)[1]
            if original is None and not text: path.unlink(missing_ok=True)
            else: atomic(path,text)
    for key in ('color-scheme','gtk-theme'):
        run('gsettings','set','org.gnome.desktop.interface',key,(BACKUP/key).read_text())
    for helper in ('omarchy-theme-set-foot','omarchy-theme-set-tmux','omarchy-theme-set-browser','omarchy-restart-hyprctl'):
        if shutil.which(helper): run(helper)

def rollback():
    STATE.mkdir(parents=True,exist_ok=True)
    with (STATE/'appearance.lock').open('w') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        restore()

if __name__=='__main__':
    if sys.argv[1:]==['rollback']: rollback()
    elif len(sys.argv)==2: apply(sys.argv[1])
    else: raise SystemExit('Usage: lucent-theme.py dark|light|rollback')
