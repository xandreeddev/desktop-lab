#!/usr/bin/env python3
"""Propagate Lucent's generated tokens. Omarchy is a headless config exporter only."""
import fcntl
import json
import os
import re
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
    catalog=json.loads((SOURCE.parent/'palettes.json').read_text())
    apply_palette(next(palette for palette in catalog if palette['id']==mode))

def exports(palette):
    colors=palette['colors']
    roles=('surface','surface_container','on_surface','primary','on_primary','error','success','warning','info','focus')
    if any(not re.fullmatch(r'#[0-9a-fA-F]{6}',str(colors.get(key,''))) for key in roles):raise ValueError('Invalid palette colors')
    def luminance(value):
        rgb=[int(value[i:i+2],16)/255 for i in (1,3,5)]
        return sum(weight*(v/12.92 if v<=.04045 else ((v+.055)/1.055)**2.4) for weight,v in zip((.2126,.7152,.0722),rgb))
    for fg,bg in [('on_surface','surface'),('on_surface','surface_container'),('on_primary','primary'),('error','surface')]:
        a,b=luminance(colors[fg]),luminance(colors[bg])
        if (max(a,b)+.05)/(min(a,b)+.05)<4.5:raise ValueError('Palette text contrast must be at least 4.5:1')
    mode='light' if luminance(colors['surface'])>.5 else 'dark'
    mapping={'accent':'primary','selection':'surface_container','muted':'surface_container',
             'background':'surface','dark_background':'surface_container','darker_background':'surface_container','lighter_background':'surface_container',
             'foreground':'on_surface','dark_foreground':'on_surface','light_foreground':'on_surface','bright_foreground':'on_surface',
             'red':'error','yellow':'warning','orange':'warning','green':'success','cyan':'primary','blue':'info','magenta':'primary','brown':'warning'}
    values={name:colors[role] for name,role in mapping.items()}
    for name in ('red','yellow','green','cyan','blue','magenta'):values['bright_'+name]=values[name]
    toml=f'mode = "{mode}"\n'+''.join(f'{key} = "{value}"\n' for key,value in values.items())
    gtk={}
    for base,role in [('window','surface'),('view','surface_container'),('headerbar','surface'),('card','surface_container')]:
        gtk[base+'_bg_color']=colors[role];gtk[base+'_fg_color']=colors['on_surface']
    for key,role in {'accent_bg_color':'primary','accent_fg_color':'on_primary','accent_color':'primary','error_color':'error','warning_color':'warning','success_color':'success',
                     'theme_bg_color':'surface','theme_fg_color':'on_surface','theme_base_color':'surface_container','theme_text_color':'on_surface','theme_selected_bg_color':'primary','theme_selected_fg_color':'on_primary'}.items():gtk[key]=colors[role]
    css=''.join(f'@define-color {key} {value};\n' for key,value in gtk.items())
    lock=(SOURCE/mode/'hyprlock.conf').read_text()
    # Replace colors by field semantics, preserving generated radius and dimensions.
    for field,role in [('color','surface'),('inner_color','surface'),('outer_color','primary'),('font_color','on_surface'),('fail_color','error')]:
        lock=re.sub(r'(?m)^(  '+field+r' = )rgb\([0-9a-f]+\)',lambda match:match[1]+'rgb('+colors[role][1:]+')',lock)
    return mode,toml,css,lock

def apply_palette(palette):
    mode,toml,generated_css,lock_config=exports(palette)
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
        target=HOME/'.config/omarchy/themes/lucent-tokens-active'
        target.mkdir(parents=True,exist_ok=True)
        atomic(target/'colors.toml',toml)
        env=dict(os.environ,OMARCHY_THEME_HEADLESS='1',OMARCHY_THEME_SKIP_BACKGROUND='1')
        run('/usr/bin/omarchy-theme-set','lucent-tokens-active',env=env)
        atomic(STATE/'theme/gtk.css',generated_css)
        atomic(STATE/'theme/hyprlock.conf',lock_config)
        atomic(STATE/'theme/palette.json',json.dumps(palette,indent=2)+'\n')
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
    elif len(sys.argv)==3 and sys.argv[1]=='palette': apply_palette(json.loads(sys.argv[2]))
    elif len(sys.argv)==2: apply(sys.argv[1])
    else: raise SystemExit('Usage: lucent-theme.py dark|light|rollback')
