#!/usr/bin/env python3
"""Private real-Chromium peer for native clipboard interoperability acceptance.

CDP prepares/selects the fixture textarea and reads its DOM. It never accesses a
clipboard API or synthesizes a copy/paste event; those use real VM keyboard input.
The transient browser unit has its own 210-second lifetime even if the host exits.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import urllib.request

sys.dont_write_bytecode = True
from cdp_peer import Peer
from owned_application_units import (assert_unit_identity, assert_unit_origin,
                                     capture_unit_origin, process_snapshot,
                                     retire_original_unit, retire_unit, unit_snapshot)

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
        require(name in ('fixture.py', 'owned_application_units.py', 'cdp_peer.py')
                and digest(regular(directory / name)) == expected)
    require(digest(regular(directory / 'document.html')) == manifest['document_sha256'])
    return directory, manifest


def argv(directory, run_id):
    return ['/usr/bin/python3', str(directory / 'fixture.py'), 'serve', str(directory), run_id]


def unit_name(run_id):
    return 'lucent-gui-clipboard-' + run_id + '.service'


def initialize(directory, run_id):
    directory, _ = context(directory, run_id, False)
    require(unit_snapshot(unit_name(run_id))['load'] == 'not-found')
    require(Path('/usr/bin/chromium').is_file() and os.access('/usr/bin/chromium', os.X_OK))
    title = 'Lucent Clipboard ' + run_id
    document = ('<!doctype html><meta charset="utf-8"><title>' + title + '</title>'
                '<label for="clipboard">Owned clipboard acceptance</label>'
                '<textarea id="clipboard" autofocus style="display:block;width:80vw;'
                'height:65vh;font:24px monospace"></textarea>').encode()
    manifest = {'run_id': run_id, 'title': title, 'class': 'LucentClipboard' + run_id,
                'uri': (directory / 'document.html').as_uri(),
                'helpers': {name: digest(regular(directory / name))
                            for name in ('fixture.py', 'owned_application_units.py', 'cdp_peer.py')},
                'document_sha256': digest(document)}
    private(directory / 'document.html', document)
    private(directory / 'manifest.json', json.dumps(manifest).encode())
    print(json.dumps({'class': manifest['class'], 'source_sha256': digest(SOURCE_TEXT.encode())}))


def absent(directory, run_id):
    context(directory, run_id)
    require(unit_snapshot(unit_name(run_id))['load'] == 'not-found')
    print(json.dumps({'owned_unit_name_absent': True}))


def owned_unit(directory, run_id, captured):
    require(captured == json.loads(regular(directory / 'admission.json')))
    value = unit_snapshot(unit_name(run_id))
    assert_unit_identity(value, unit_name(run_id), argv(directory, run_id))
    require(value['active'] == 'active' and value['main_pid'] > 1 and value['group'])
    require({key: item for key, item in value.items() if key != 'active'} ==
            {key: item for key, item in captured['unit'].items() if key != 'active'})
    process = process_snapshot(value['main_pid'], max_executable_bytes=512*1024*1024)
    require({key: item for key, item in process.items() if key != 'sha256'} ==
            {key: item for key, item in captured['process'].items() if key != 'sha256'})
    return value


def serve(directory, run_id):
    directory, manifest = context(directory, run_id)
    origin = capture_unit_origin(unit_name(run_id), argv(directory, run_id))
    require(not os.path.lexists(directory / 'profile'))
    private(directory / 'origin.json', json.dumps(origin).encode())
    command = ['/usr/bin/chromium', '--user-data-dir=' + str(directory / 'profile'),
               '--class=' + manifest['class'], '--ozone-platform=wayland',
               '--no-first-run', '--no-default-browser-check', '--disable-sync',
               '--disable-extensions', '--disable-background-networking', '--password-store=basic',
               '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
               '--new-window', manifest['uri']]
    os.execv(command[0], command)


def origin(directory, run_id):
    directory, _ = context(directory, run_id)
    receipt = json.loads(regular(directory / 'origin.json'))
    value = unit_snapshot(unit_name(run_id))
    assert_unit_origin(value, unit_name(run_id), argv(directory, run_id), receipt,
                       max_executable_bytes=512*1024*1024)
    print(json.dumps(receipt))


def admit(directory, run_id):
    directory, manifest = context(directory, run_id)
    if (directory / 'admission.json').exists():
        captured = json.loads(regular(directory / 'admission.json'))
        owned_unit(directory, run_id, captured)
    else:
        receipt = json.loads(regular(directory / 'origin.json'))
        # The port and exact owned page exist only after Chromium has exec'd;
        # never freeze an intermediate Python/shell executable as the browser.
        peer = page(directory, manifest)
        peer.close()
        value = unit_snapshot(unit_name(run_id))
        current = assert_unit_origin(value, unit_name(run_id), argv(directory, run_id), receipt,
                                     max_executable_bytes=512*1024*1024, hashed=True)
        require(value['active'] == 'active' and current is not None
                and isinstance(current.get('sha256'), str)
                and re.fullmatch(r'[0-9a-f]{64}', current['sha256']))
        captured = {'unit': value, 'process': current}
        require(unit_snapshot(unit_name(run_id)) == value)
        private(directory / 'admission.json', json.dumps(captured).encode())
        owned_unit(directory, run_id, captured)
    print(json.dumps(captured))


def clients(directory, run_id, request):
    directory, manifest = context(directory, run_id)
    values = request['clients']
    require(isinstance(values, list) and len(values) <= 1)
    unit = owned_unit(directory, run_id, request['identity'])
    for value in values:
        require(value.get('class') == manifest['class'] and type(value.get('pid')) is int
                and value['pid'] == unit['main_pid'] and value.get('workspace', {}).get('name') ==
                'lucent-clipboard-browser-' + run_id)
        process = Path('/proc') / str(value['pid'])
        require(process.stat().st_uid == os.getuid())
        groups = [line.split(':', 2)[2] for line in regular(process / 'cgroup').decode().splitlines()
                  if line.startswith('0::')]
        require(groups == [unit['group']])
    print(json.dumps({'owned_clients': len(values)}))


def page(directory, manifest):
    lines = regular(directory / 'profile/DevToolsActivePort', 4096).decode('ascii').splitlines()
    require(len(lines) == 2 and lines[0].isdecimal())
    port = int(lines[0])
    require(1024 <= port <= 65535 and re.fullmatch(r'/devtools/browser/[A-Za-z0-9-]{1,128}', lines[1]))
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
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
    return Peer.connect(target['webSocketDebuggerUrl'], port, target['id'])


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
    peer = page(directory, manifest)
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
        retire_unit(unit_name(run_id), argv(directory, run_id), captured=captured,
                    max_executable_bytes=512*1024*1024)
    else:
        receipt = request['origin']
        require(receipt == json.loads(regular(directory / 'origin.json')))
        retire_original_unit(unit_name(run_id), argv(directory, run_id), receipt,
                             max_executable_bytes=512*1024*1024)


def retire(directory, run_id, request):
    retire_owned(directory, run_id, request)
    print(json.dumps({'owned_browser_unit_retired': True}))


def cleanup(directory, run_id, request):
    directory, _ = context(directory, run_id)
    retire_owned(directory, run_id, request)
    require(set(path.name for path in directory.iterdir()) <= {
        'fixture.py', 'owned_application_units.py', 'cdp_peer.py', 'manifest.json',
        'document.html', 'profile', 'origin.json', 'admission.json'})
    shutil.rmtree(directory)
    print(json.dumps({'owned_private_profile_removed': True}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('initialize', 'absent', 'serve', 'origin', 'admit', 'clients', 'dom', 'retire', 'cleanup'))
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
