#!/usr/bin/env python3
"""Compatibility entry point; the card prototype is now the framework desktop."""
from pathlib import Path
import runpy
runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='__main__')
