#!/usr/bin/env python3
"""Verify native permission cancellation and repeated surface cleanup.

Requires the existing unlocked Lucent guest. Requests CheckAuthorization for
the graphical session and cancels with Escape. It performs no privileged action,
supplies no credentials and leaves policy, login and desktop settings unchanged.
Only compositor metadata is inspected; authentication clients expose no IPC.
"""
import json
from pathlib import Path
import runpy
import shlex
import subprocess
import time

import lab

h = runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='harness')

AUDIT = r'''
import hashlib,json,os,pathlib,subprocess
uid=str(os.getuid())
subject=subprocess.check_output(['pgrep','-x','-u',uid,'Hyprland'],text=True).strip()
agent=subprocess.check_output(['pgrep','-x','-u',uid,'lucent-polkit'],text=True).strip()
assert subject.isdigit() and agent.isdigit(), 'Expected one graphical session and agent'
def environment(pid):
 return dict(item.split('=',1) for item in pathlib.Path('/proc/'+pid+'/environ').read_bytes().decode().split('\0') if '=' in item)
session=environment(subject).get('XDG_SESSION_ID')
assert session and session==environment(agent).get('XDG_SESSION_ID'), 'Agent and subject sessions differ'
start=pathlib.Path('/proc/'+subject+'/stat').read_text().rsplit(')',1)[1].split()[19]
fields=dict(line.split('=',1) for line in subprocess.check_output(['systemctl','--user','show','lucent-polkit.service','--property=ActiveState,SubState,MainPID,NRestarts'],text=True).splitlines())
assert fields['ActiveState']=='active' and fields['SubState']=='running' and fields['MainPID']==agent
layers=json.loads(subprocess.check_output(['hyprctl','-j','layers'],text=True))
def namespaces(value):
 if isinstance(value,dict):
  if 'namespace' in value:yield value['namespace']
  for child in value.values():yield from namespaces(child)
 elif isinstance(value,list):
  for child in value:yield from namespaces(child)
print(json.dumps({'subject':subject+','+start+','+uid,'agent':agent,'sha256':hashlib.sha256(pathlib.Path('/proc/'+agent+'/exe').read_bytes()).hexdigest(),'restarts':int(fields['NRestarts']),'surfaces':list(namespaces(layers)).count('lucent-authorization-authorization'),'socket_absent':not (pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-authorization.sock').exists()}))
'''


def audit():
    return json.loads(h['session']('python3', '-c', AUDIT))


def wait_for(predicate, request, message, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        state = audit()
        if predicate(state):
            return state
        if request is not None and request.poll() is not None:
            raise AssertionError(message + ': request exited before showing a dialog')
        time.sleep(.2)
    raise AssertionError(message)


def main():
    assert lab.virsh('domstate', h['NAME'], capture=True).stdout.strip() == 'running'
    assert h['session']('omarchy-shell', 'lock', 'isLocked').strip() == 'false'
    initial = audit()
    assert initial['surfaces'] == 0 and initial['socket_absent'], 'Close existing dialogs first'
    outcomes = []
    for _ in range(2):
        command = ['python3', 'desktop-lab/scripts/in-session.py', 'timeout', '30s',
                   'pkcheck', '--action-id', 'org.freedesktop.login1.set-user-linger',
                   '--process', initial['subject'], '--allow-user-interaction']
        request = subprocess.Popen(lab.ssh_args('lucent') + [shlex.join(command)],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            opened = wait_for(lambda s: s['surfaces'] == 1, request,
                              'Native permission dialog did not open')
            assert opened['agent'] == initial['agent'] and opened['sha256'] == initial['sha256']
            assert opened['socket_absent'], 'Permission dialog exposed a command socket'
            h['key'](1)
            result = request.wait(timeout=15)
            assert result == 1, 'Cancellation did not deny the policy check'
            closed = wait_for(lambda s: s['surfaces'] == 0, None,
                              'Cancelled dialog retained a native surface')
            assert closed['agent'] == initial['agent'] and closed['sha256'] == initial['sha256']
            assert closed['restarts'] == initial['restarts'] and closed['socket_absent']
            outcomes.append(result)
        finally:
            if request.poll() is None:
                current = audit()
                if current['agent'] == initial['agent'] and current['surfaces'] == 1:
                    h['key'](1)
                try:
                    request.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    request.terminate()
                    request.wait(timeout=5)
    report = {
        'native_check_authorization_cancel_cycles': len(outcomes),
        'policy_check_exit_codes': outcomes,
        'graphical_subject_matches_agent_session': True,
        'command_socket_absent_during_and_after_prompt': True,
        'native_surfaces_removed_between_cycles': True,
        'same_agent_process_and_binary': True,
        'agent_sha256': initial['sha256'],
        'automatic_restarts_unchanged': True,
        'credentials_supplied': False,
        'privileged_operations_performed': False,
    }
    (h['REPORT'] / 'lucent-authorization-cancel.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
