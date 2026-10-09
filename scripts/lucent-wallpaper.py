#!/usr/bin/env python3
"""Publish only the selected wallpaper for the separate login user to read."""
import os
from pathlib import Path
import tempfile

LIMIT=32*1024*1024

def publish(source, directory):
    # Root installs the per-account destination; normal desktop users update it.
    if not directory.is_dir(): return
    with source.open('rb') as stream:
        data=stream.read(LIMIT+1)
    if len(data)>LIMIT: raise ValueError('Wallpaper exceeds the 32 MiB source limit')
    target=directory/'wallpaper'
    if target.is_file():
        with target.open('rb') as stream:
            if stream.read(LIMIT+1)==data: return
    temporary=None
    try:
        with tempfile.NamedTemporaryFile(dir=directory,prefix='.wallpaper-',delete=False) as stream:
            temporary=Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
            # The setgid destination assigns the greeter group without opening HOME.
            os.fchmod(stream.fileno(),0o640)
        temporary.replace(target)
    finally:
        if temporary is not None: temporary.unlink(missing_ok=True)

if __name__=='__main__':
    source=Path.home()/'.local/state/omarchy/current/background'
    try:
        publish(source,Path('/var/lib/lucent/wallpapers')/str(os.getuid()))
    except FileNotFoundError:
        pass # A theme switch may briefly remove the link; its next change retries.
