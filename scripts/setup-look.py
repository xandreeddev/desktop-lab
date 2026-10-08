#!/usr/bin/env python3
"""Prepare the 1920x1080 reference layout in a disposable test guest."""
import json
from pathlib import Path
import shutil
import sys

from guest import HOME_DIR, ROOT, STATE, DATA, backup, output, run, write

profile = sys.argv[1]
if output('systemd-detect-virt') not in ('kvm', 'qemu'):
    raise SystemExit('The reference-resolution preset is only for test VMs.')
run(sys.executable, str(ROOT / 'scripts/fetch.py'), 'lucid')
src = ROOT / '.cache/upstream/lucid'
wall = next((src / 'wallpapers').rglob('Cherry Blossom Serenade*'))
DATA.mkdir(parents=True, exist_ok=True)
shutil.copy2(wall, DATA / 'wallpaper.jpg')
backup('.config/hypr/monitors.lua')
mon = HOME_DIR / '.config/hypr/monitors.lua'
text = mon.read_text()
line = 'hl.monitor({ output = "Virtual-1", mode = "1920x1080@60", position = "0x0", scale = 1 })'
if line not in text:
    write(mon, text + '\n-- Desktop Lab reference screenshot resolution.\n' + line + '\n')
if profile == 'lucid':
    for path in ('.config/hypr/scripts/wallpaper/set-wallpaper.sh', '.cache/current_wallpaper', '.cache/current_mode', '.cache/current_theme'):
        backup(path)
    target = HOME_DIR / '.config/hypr/scripts/wallpaper/set-wallpaper.sh'
    write(target, (ROOT / 'configs/lucid/set-wallpaper.sh').read_text(), executable=True)
    prefs = HOME_DIR / '.config/quickshell/lucidprefs/prefs.json'
    values = json.loads(prefs.read_text())
    values.update(json.loads((ROOT / 'configs/lucid/prefs.json').read_text()))
    values['wallpaperFolder'] = str(src / 'wallpapers/matugen')
    write(prefs, json.dumps(values, indent=2) + '\n')
else:
    cfg = HOME_DIR / '.config/noctalia/config.toml'
    text = cfg.read_text()
    if '[wallpaper.default]' not in text:
        write(cfg, text + '\n[wallpaper.default]\npath = ' + json.dumps(str(DATA / 'wallpaper.jpg')) + '\n')
print('Reference wallpaper and output layout prepared.')
