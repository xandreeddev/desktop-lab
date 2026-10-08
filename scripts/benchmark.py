#!/usr/bin/env python3
"""Measure process RSS and CPU time over a real idle interval; never infer GPU memory."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--seconds', type=int, default=120)
p.add_argument('--pid', type=int)
p.add_argument('--output', required=True)
args = p.parse_args()
if args.seconds < 1:
    p.error('seconds must be positive')
pid = args.pid or int(subprocess.check_output(['systemctl', '--user', 'show', 'desktop-lab-shell.service', '-p', 'MainPID', '--value']))
proc = Path('/proc') / str(pid)

def sample():
    # comm may contain spaces and parentheses; numeric fields follow the final ')'.
    stat = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
    rss = next(int(line.split()[1]) for line in (proc / 'status').read_text().splitlines() if line.startswith('VmRSS:'))
    return int(stat[11]) + int(stat[12]), int(stat[19]), rss

initial, identity, _ = sample()
start = time.monotonic()
rss_samples = []
while time.monotonic() - start < args.seconds:
    time.sleep(min(1, args.seconds - (time.monotonic() - start)))
    ticks, same, rss = sample()
    if same != identity:
        raise SystemExit('Process restarted during the measurement.')
    rss_samples.append(rss)
duration = time.monotonic() - start
data = {
    'duration_seconds': round(duration, 3),
    'process': (proc / 'comm').read_text().strip(),
    'cpu_percent_one_core': round((ticks-initial)/os.sysconf('SC_CLK_TCK')/duration*100, 3),
    'rss_mib_mean': round(sum(rss_samples)/len(rss_samples)/1024, 2),
    'rss_mib_max': round(max(rss_samples)/1024, 2),
    'scope': 'main process only; child processes and GPU allocations excluded',
    'gpu_memory': 'not measured',
    'virtualization': subprocess.check_output(['systemd-detect-virt'], text=True).strip(),
}
Path(args.output).parent.mkdir(parents=True, exist_ok=True)
Path(args.output).write_text(json.dumps(data, indent=2)+'\n')
print(json.dumps(data, indent=2))
