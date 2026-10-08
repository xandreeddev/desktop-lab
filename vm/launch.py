#!/usr/bin/env python3
"""Boot a prepared lab guest and open its graphical console."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parent
if len(sys.argv) != 2 or sys.argv[1] not in ('lucid', 'noctalia', 'lucent'):
    raise SystemExit('Usage: launch.py lucid|noctalia|lucent')
subprocess.run([sys.executable, str(root/'lab.py'), 'start', sys.argv[1]], check=True)
subprocess.run([sys.executable, str(root/'lab.py'), 'console', sys.argv[1]], check=True)
