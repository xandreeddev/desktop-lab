#!/usr/bin/env python3
"""Verify opt-in profiling and failure cleanup in the unlocked Lucent VM.

Restarts only lucent.service; restores its runtime configuration and idle setting.
"""
from pathlib import Path
import runpy,sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'vm'))
h=runpy.run_path(str(ROOT/'vm/test-framework.py'),run_name='harness')
m=runpy.run_path(str(ROOT/'vm/measure-framework.py'),run_name='measurement')
h['eventually'](lambda:h['inspect']()['client']['background_ready'],'Deployed desktop not ready',timeout=60)
assert not h['inspect']()['frame_profiling'],'Profiling should be off by default'
with m['stay_awake'](h['remote']):
    try:
        with m['frame_profiling'](h['remote'],h['session'],True):
            h['session']('systemctl','--user','restart','lucent.service')
            h['eventually'](lambda:h['inspect']()['client']['background_ready'],'Profile mode not ready',timeout=60)
            assert h['inspect']()['frame_profiling']
            raise RuntimeError('intentional restoration test')
    except RuntimeError as error:
        assert str(error)=='intentional restoration test'
    h['eventually'](lambda:h['inspect']()['client']['background_ready'],'Runtime did not recover after intentional failure',timeout=60)
    assert not h['inspect']()['frame_profiling'],'Temporary profiling override survived failure'
print('Native profiling enabled and original service configuration restored after intentional failure',flush=True)
