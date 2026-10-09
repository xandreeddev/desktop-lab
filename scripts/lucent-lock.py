#!/usr/bin/env python3
"""Request the native secure locker, preserving stock fallback and sleep deadlines."""
import os
from pathlib import Path
import subprocess
import sys
import time


def call(*args,timeout=1):
    try:
        return subprocess.run(args,timeout=timeout,stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL,check=False).returncode==0
    except (OSError,subprocess.TimeoutExpired):
        return False


def main():
    started=time.monotonic()
    sleeping=sys.argv[1:2]==['sleep']
    fallback='/usr/bin/omarchy-system-sleep-lock' if sleeping else '/usr/bin/omarchy-system-lock'
    args=sys.argv[2:] if sleeping else []
    budget=3.0
    if sleeping:
        try:
            if args:budget=min(12.,max(.1,int(args[0])/1000))
            else:
                result=subprocess.check_output(['busctl','get-property','org.freedesktop.login1','/org/freedesktop/login1','org.freedesktop.login1.Manager','InhibitDelayMaxUSec'],text=True,timeout=.5)
                window=int(result.split()[-1])/1_000_000
                budget=min(12.,max(.1,window-max(1.,window/5)))
        except (ValueError,OSError,subprocess.SubprocessError):pass
    deadline=started+budget
    def remaining(limit):
        return min(limit,max(.01,deadline-time.monotonic()))
    def stock():
        stock_args=[str(max(1,int((deadline-time.monotonic())*1000)))] if sleeping else args
        os.execv(fallback,[fallback,*stock_args])
    if not call('systemctl','--user','is-active','--quiet','lucent.service',timeout=remaining(.5)):
        stock()
    if call('omarchy-hyprland-session-locked',timeout=remaining(.3)):return
    # A native startup failure must leave time for the proven stock locker.
    native_deadline=deadline-min(1.5,budget*.4) if sleeping else deadline
    call('systemctl','--user','start','lucent-lock.service',timeout=min(1.,max(.01,native_deadline-time.monotonic())))
    while time.monotonic()<native_deadline:
        if call('omarchy-hyprland-session-locked',timeout=min(.3,max(.01,native_deadline-time.monotonic()))):
            # Screen-lock is already acknowledged by the compositor.
            if not sleeping:
                call('hyprctl','switchxkblayout','all','0')
                call('pkill','-x','ttfx');call('pkill','-f','[o]rg.omarchy.screensaver')
                if call('pgrep','-x','1password'):
                    subprocess.Popen(['timeout','--kill-after=1s','3s','1password','--lock'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            return
        time.sleep(min(.05,max(0.,native_deadline-time.monotonic())))
    # A failed renderer never unlocks the compositor. The stock locker can take
    # over an orphaned secure lock and remains installed as a recovery path.
    call('systemctl','--user','stop','lucent-lock.service',timeout=remaining(.2))
    stock()

if __name__=='__main__':main()
