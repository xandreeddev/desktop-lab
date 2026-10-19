#!/usr/bin/env python3
"""Private real-Chromium peer for native clipboard interoperability acceptance.

CDP prepares/selects the fixture textarea and reads its DOM. It never accesses a
clipboard API or synthesizes a copy/paste event; those use real VM keyboard input.
An independently owned watchdog retains the browser lease across normal scope
migration; original-service expiry alone is not proof of scope drainage.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import time
import urllib.request

sys.dont_write_bytecode = True
from cdp_peer import Peer
from owned_application_units import (assert_unit_origin, capture_unit_origin,
                                     retire_original_unit, unit_snapshot)
import owned_chromium_lease as chromium

SOURCE_TEXT = 'browser one\n雪 e\u0301 👩\u200d💻\nlast line\n'


def require(value):
    if not value:
        raise ValueError('Owned GUI clipboard peer identity failed')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular(path, limit=262144):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and 0 <= before.st_size <= limit)
        identity = stable_file_identity(before)
        require(stable_file_identity(path.lstat()) == identity)
        data = stream.read(limit + 1)
        require(len(data) <= limit and stable_file_identity(os.fstat(stream.fileno())) == identity
                and stable_file_identity(path.lstat()) == identity)
        return data


def stable_file_identity(value):
    # Cold reads can change only atime; all identity/content metadata stays pinned.
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns]


def private(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)


def context(directory, run_id, initialized=True):
    directory = Path(directory)
    require(directory.is_absolute() and directory.resolve(strict=True) == directory
            and re.fullmatch(r'[0-9a-f]{24}', run_id))
    value = directory.lstat()
    require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid()
            and stat.S_IMODE(value.st_mode) == 0o700)
    if not initialized:
        return directory, None
    manifest = json.loads(regular(directory / 'manifest.json'))
    require(manifest['run_id'] == run_id)
    for name, expected in manifest['helpers'].items():
        require(name in ('fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'owned_chromium_lease.py')
                and digest(regular(directory / name)) == expected)
    require(digest(regular(directory / 'document.html')) == manifest['document_sha256'])
    return directory, manifest


def argv(directory, run_id):
    return ['/usr/bin/python3', str(directory / 'fixture.py'), 'serve', str(directory), run_id]


def unit_name(run_id):
    return 'lucent-gui-clipboard-' + run_id + '.service'


def watchdog_name(run_id):
    return 'lucent-gui-clipboard-watchdog-' + run_id + '.service'


def watchdog_argv(directory, run_id):
    return ['/usr/bin/python3', str(directory / 'fixture.py'), 'watchdog', str(directory), run_id]


def chromium_argv(directory, manifest):
    return ['/usr/bin/chromium', '--user-data-dir=' + str(directory / 'profile'),
            '--class=' + manifest['class'], '--ozone-platform=wayland',
            '--no-first-run', '--no-default-browser-check', '--disable-sync',
            '--disable-extensions', '--disable-background-networking', '--password-store=basic',
            '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
            '--new-window', manifest['uri']]


def initialize(directory, run_id):
    directory, _ = context(directory, run_id, False)
    require(unit_snapshot(unit_name(run_id))['load'] == 'not-found')
    require(unit_snapshot(watchdog_name(run_id))['load'] == 'not-found')
    require(Path('/usr/bin/chromium').is_file() and os.access('/usr/bin/chromium', os.X_OK))
    title = 'Lucent Clipboard ' + run_id
    document = ('<!doctype html><meta charset="utf-8"><title>' + title + '</title>'
                '<label for="clipboard">Owned clipboard acceptance</label>'
                '<textarea id="clipboard" autofocus style="display:block;width:80vw;'
                'height:65vh;font:24px monospace"></textarea>').encode()
    manifest = {'run_id': run_id, 'title': title, 'class': 'LucentClipboard' + run_id,
                'uri': (directory / 'document.html').as_uri(),
                'helpers': {name: digest(regular(directory / name))
                            for name in ('fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'owned_chromium_lease.py')},
                'document_sha256': digest(document)}
    manifest['startup'] = chromium.startup(chromium_argv(directory, manifest))
    private(directory / 'document.html', document)
    private(directory / 'manifest.json', json.dumps(manifest).encode())
    print(json.dumps({'class': manifest['class'], 'source_sha256': digest(SOURCE_TEXT.encode())}))


def absent(directory, run_id):
    context(directory, run_id)
    require(unit_snapshot(unit_name(run_id))['load'] == 'not-found')
    print(json.dumps({'owned_unit_name_absent': True}))


def owned_unit(directory, run_id, captured):
    require(captured == json.loads(regular(directory / 'admission.json')))
    directory, manifest = context(directory, run_id)
    receipt = json.loads(regular(directory / 'origin.json'))
    chromium.observe(receipt, manifest['startup'], captured['lease'])
    require(browser_endpoint(directory) == captured['endpoint'])
    return captured


def serve(directory, run_id):
    directory, manifest = context(directory, run_id)
    origin = capture_unit_origin(unit_name(run_id), argv(directory, run_id))
    require(not os.path.lexists(directory / 'profile'))
    private(directory / 'origin.json', json.dumps(origin).encode())
    require(unit_snapshot(watchdog_name(run_id))['load'] == 'not-found')
    subprocess.run(['systemd-run', '--user', '--quiet', '--collect', '--property=Type=exec',
                    '--property=RuntimeMaxSec=240s', '--property=TimeoutStopSec=10s',
                    '--unit='+watchdog_name(run_id), *watchdog_argv(directory, run_id)], check=True, timeout=10)
    deadline = time.monotonic()+10
    while not os.path.lexists(directory / 'watchdog-origin.json'):
        require(time.monotonic()<deadline)
        time.sleep(.1)
    watched = json.loads(regular(directory / 'watchdog-origin.json'))
    assert_unit_origin(unit_snapshot(watchdog_name(run_id)), watchdog_name(run_id),
                       watchdog_argv(directory, run_id), watched)
    command = chromium_argv(directory, manifest)
    os.execv(command[0], command)


def origin(directory, run_id):
    directory, _ = context(directory, run_id)
    receipt = json.loads(regular(directory / 'origin.json'))
    require(receipt == admit(directory, run_id, emit=False)['lease']['origin'])
    print(json.dumps(receipt))


def admit(directory, run_id, emit=True):
    directory, manifest = context(directory, run_id)
    if (directory / 'admission.json').exists():
        captured = json.loads(regular(directory / 'admission.json'))
        owned_unit(directory, run_id, captured)
    else:
        receipt = json.loads(regular(directory / 'origin.json'))
        # The port and exact owned page exist only after Chromium has exec'd;
        # never freeze an intermediate Python/shell executable as the browser.
        peer, control = page(directory, manifest, capture=True)
        peer.close()
        lease = chromium.observe(receipt, manifest['startup'])
        captured = {'lease':lease, **control}
        try:
            private(directory / 'admission.json', json.dumps(captured).encode())
        except FileExistsError:
            captured = json.loads(regular(directory / 'admission.json'))
        owned_unit(directory, run_id, captured)
    if emit:
        print(json.dumps(captured))
    return captured


def clients(directory, run_id, request):
    directory, manifest = context(directory, run_id)
    values = request['clients']
    require(isinstance(values, list) and len(values) <= 1)
    lease = owned_unit(directory, run_id, request['identity'])
    for value in values:
        require(value.get('class') == manifest['class'] and type(value.get('pid')) is int
                and value['pid'] == lease['lease']['process']['pid'] and value.get('workspace', {}).get('name') ==
                'lucent-clipboard-browser-' + run_id)
        process = Path('/proc') / str(value['pid'])
        require(process.stat().st_uid == os.getuid())
        groups = [line.split(':', 2)[2] for line in regular(process / 'cgroup').decode().splitlines()
                  if line.startswith('0::')]
        require(groups == [lease['lease']['process']['group']])
    print(json.dumps({'owned_clients': len(values)}))


def browser_endpoint(directory):
    path = directory / 'profile/DevToolsActivePort'
    before = stable_file_identity(path.lstat())
    data = regular(path, 4096)
    require(stable_file_identity(path.lstat()) == before)
    lines = data.decode('ascii').splitlines()
    require(len(lines) == 2 and lines[0].isdecimal())
    port = int(lines[0])
    require(1024 <= port <= 65535 and re.fullmatch(r'/devtools/browser/[A-Za-z0-9-]{1,128}', lines[1]))
    return {'port':port,'browser':lines[1],'file':before,'sha256':digest(data)}


def page(directory, manifest, expected=None, capture=False):
    endpoint = browser_endpoint(directory)
    require(expected is None or endpoint == expected['endpoint'])
    port = endpoint['port']
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *_args, **_kwargs):
            return None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open('http://127.0.0.1:' + str(port) + '/json/list', timeout=3) as response:
        require(response.status == 200 and response.geturl() == 'http://127.0.0.1:' + str(port) + '/json/list')
        data = response.read(262145)
        require(len(data) <= 262144)
    values = json.loads(data)
    require(isinstance(values, list) and len(values) <= 16)
    pages = [value for value in values if value.get('type') == 'page']
    require(len(pages) == 1 and pages[0].get('url') == manifest['uri']
            and pages[0].get('title') == manifest['title'])
    target = pages[0]
    control = {key:target[key] for key in ('id','url','title','webSocketDebuggerUrl')}
    require(expected is None or control == expected['control'])
    require(browser_endpoint(directory) == endpoint)
    peer = Peer.connect(target['webSocketDebuggerUrl'], port, target['id'])
    return (peer, {'endpoint':endpoint,'control':control}) if capture else peer


def dom(directory, run_id, operation, request):
    directory, manifest = context(directory, run_id)
    owned_unit(directory, run_id, request['identity'])
    require(operation in ('seed', 'clear', 'read'))
    # No navigator.clipboard, Input.dispatchKeyEvent or document.execCommand.
    prepare = ('e.value=' + json.dumps(SOURCE_TEXT) + ';e.focus();e.select();') if operation == 'seed' else (
        'e.value="replacement sentinel";e.focus();e.select();' if operation == 'clear' else '')
    expression = ('(()=>{if(location.href!==' + json.dumps(manifest['uri'])
                  + '||document.title!==' + json.dumps(manifest['title']) + ')throw 1;'
                  'const e=document.getElementById("clipboard");'
                  'if(!(e instanceof HTMLTextAreaElement))throw 1;' + prepare
                  + 'return {value:e.value,focused:document.hasFocus()&&document.activeElement===e,'
                  'start:e.selectionStart,end:e.selectionEnd};})()')
    peer = page(directory, manifest, expected=request['identity'])
    try:
        result = peer.call('Runtime.evaluate', {'expression': expression, 'returnByValue': True})
    finally:
        peer.close()
    value = result.get('result', {}).get('value')
    require(isinstance(value, dict) and isinstance(value.get('value'), str)
            and len(value['value'].encode()) <= 65536)
    print(json.dumps({'sha256': digest(value['value'].encode()), 'focused': value['focused'],
                      'selection_complete': value['start'] == 0 and value['end'] == len(value['value'].encode('utf-16-le')) // 2,
                      'bytes': len(value['value'].encode())}))


def retire_owned(directory, run_id, request):
    directory, _ = context(directory, run_id)
    if request.get('identity') is not None:
        captured = request['identity']
        require(captured == json.loads(regular(directory / 'admission.json')))
        chromium.retire(captured['lease'])
    else:
        receipt = request['origin']
        require(receipt == json.loads(regular(directory / 'origin.json')))
        if not os.path.lexists(directory / 'admission.json'):
            admit(directory, run_id, emit=False)
        captured = json.loads(regular(directory / 'admission.json'))
        require(receipt == captured['lease']['origin'])
        chromium.retire(captured['lease'])
    watched = json.loads(regular(directory / 'watchdog-origin.json'))
    retire_original_unit(watchdog_name(run_id), watchdog_argv(directory, run_id), watched)


def watchdog(directory, run_id):
    directory, _ = context(directory, run_id)
    signal.alarm(235)
    receipt = capture_unit_origin(watchdog_name(run_id), watchdog_argv(directory, run_id))
    private(directory / 'watchdog-origin.json', json.dumps(receipt).encode())
    started = time.monotonic()
    while True:
        try:
            captured = admit(directory, run_id, emit=False)
            break
        except Exception:
            require(time.monotonic()-started < 35)
            time.sleep(.15)
    time.sleep(max(0, 180-(time.monotonic()-started)))
    chromium.retire(captured['lease'])


def retire(directory, run_id, request):
    retire_owned(directory, run_id, request)
    print(json.dumps({'owned_browser_unit_retired': True}))


def cleanup(directory, run_id, request):
    directory, _ = context(directory, run_id)
    retire_owned(directory, run_id, request)
    require(set(path.name for path in directory.iterdir()) <= {
        'fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'manifest.json',
        'document.html', 'profile', 'origin.json', 'admission.json',
        'owned_chromium_lease.py', 'watchdog-origin.json'})
    shutil.rmtree(directory)
    print(json.dumps({'owned_private_profile_removed': True}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('initialize', 'absent', 'serve', 'origin', 'admit', 'clients', 'dom', 'retire', 'cleanup', 'watchdog'))
    parser.add_argument('directory')
    parser.add_argument('run_id')
    parser.add_argument('extra', nargs='?')
    args = parser.parse_args()
    if args.operation in ('clients', 'retire', 'cleanup'):
        globals()[args.operation](args.directory, args.run_id, json.load(sys.stdin))
    elif args.operation == 'dom':
        dom(args.directory, args.run_id, args.extra, json.load(sys.stdin))
    else:
        globals()[args.operation](args.directory, args.run_id)


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # No browser/profile contents or unsolicited protocol errors in output.
        print('Owned GUI clipboard peer operation failed', file=sys.stderr)
        raise SystemExit(1)
