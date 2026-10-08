#!/usr/bin/env python3
"""Fetch exact upstream archives recorded in the lock manifest."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def fetch(name):
    lock = json.loads((ROOT / 'manifests/upstream-lock.json').read_text())[name]
    dest = ROOT / '.cache/upstream' / name
    marker = dest / '.desktop-lab-revision'
    if marker.exists() and marker.read_text().strip() == lock['revision']:
        return
    if dest.exists():
        raise SystemExit(f'{dest} already exists without a matching revision marker; inspect it.')
    url = lock['repository'].replace('https://github.com/', 'https://codeload.github.com/') + '/tar.gz/' + lock['revision']
    archive = urllib.request.urlopen(url, timeout=120).read()
    if hashlib.sha256(archive).hexdigest() != lock['archive_sha256']:
        raise SystemExit('Archive checksum mismatch.')
    dest.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tf:
        prefix = tf.getmembers()[0].name + '/'
        for member in tf.getmembers():
            if member.name.startswith(prefix):
                member.name = member.name[len(prefix):]
                if member.name:
                    tf.extract(member, dest, filter='data')
    marker.write_text(lock['revision'] + '\n')


if __name__ == '__main__':
    fetch(sys.argv[1])
