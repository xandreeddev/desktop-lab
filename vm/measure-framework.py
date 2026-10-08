#!/usr/bin/env python3
"""Measure the actual warm launcher animation and 120-second idle interval."""
import json
from pathlib import Path
import runpy
import statistics
import time

h=runpy.run_path(str(Path(__file__).with_name('test-framework.py')),run_name='harness')
cli,inspect,session,remote=h['cli'],h['inspect'],h['session'],h['remote']
report_dir=h['REPORT']
assert session('omarchy-shell','lock','isLocked').strip()=='false'
cli('launcher','close');h['move'](960,400);time.sleep(1)
transitions=[]
for action in ['open','close','open','close']:
    start=inspect()['uptime_ms']
    cli('launcher',action);time.sleep(.8)
    frames=next(s for s in inspect()['surfaces'] if s['id']=='dock')['frame_times_ms']
    frames=[t for t in frames if t>=start]
    intervals=[b-a for a,b in zip(frames,frames[1:])]
    transitions.append({'action':action,'frames':len(frames),
        'median_frame_interval_ms':round(statistics.median(intervals),2),
        'max_frame_interval_ms':round(max(intervals),2),
        'observed_frame_span_ms':round(frames[-1]-frames[0],2)})
pid=int(remote('pgrep','-x','lucent-desktop').strip())
frames_before={s['id']:s['frames'] for s in inspect()['surfaces']}
group=session('systemctl','--user','show','lucent.service','-p','ControlGroup','--value').strip()
def usage():
    lines=remote('cat','/sys/fs/cgroup'+group+'/cpu.stat').splitlines()
    return int(next(s.split()[1] for s in lines if s.startswith('usage_usec ')))
start=time.monotonic();cpu_before=usage()
print('Animation sampled; measuring 120 seconds of idle time.',flush=True)
idle=json.loads(session('python3','desktop-lab/scripts/benchmark.py','--pid',str(pid),'--seconds','120','--output','/tmp/lucent-framework-idle.json'))
duration=time.monotonic()-start
idle['service_cpu_percent_one_core']=round((usage()-cpu_before)/1_000_000/duration*100,3)
frames_after={s['id']:s['frames'] for s in inspect()['surfaces']}
extra={id:frames_after[id]-frames_before[id] for id in frames_before}
assert extra['dock']==0,extra
assert extra['bar']<=4 and extra['widgets']<=4,extra
report={'display':'1920x1080 at 60 Hz, scale 1','vcpus':2,'guest_memory_gib':3,
        'adapter':inspect()['adapter'],'animations':transitions,'idle':idle,'idle_extra_frames':extra,
        'notes':'Software Vulkan. Frame intervals measured between render submissions, not scanout. Main RSS includes mapped driver memory; independent GPU allocation accounting unavailable. Service CPU includes helper processes but excludes retained Omarchy services/compositor.'}
(report_dir/'lucent-framework-performance.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
