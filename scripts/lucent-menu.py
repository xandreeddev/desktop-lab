#!/usr/bin/env python3
"""Omarchy's select/input interface, rendered by the native Lucent picker."""
import json
import os
from pathlib import Path
import subprocess
import sys


def entry(option, prompt):
    """Preserve the exact return value expected by Omarchy's action resolver."""
    fields = option.split('\t', 2)
    if len(fields) > 1:
        label = fields[1]
        detail = fields[2] if len(fields) == 3 else ''
        value = '\t'.join(fields[1:])
    else:
        label, detail, value = option, '', option
    if prompt == 'Keybindings' and ' → ' in label:
        chord, action = label.split(' → ', 1)
        label, detail = action.strip(), chord.strip()
    return {'label': label, 'detail': detail, 'value': value}


def request(mode, args, stdin):
    if mode not in ('select', 'input'):
        raise ValueError('Expected select or input')
    if mode == 'select' and not args:
        raise ValueError('A menu prompt is required')
    prompt = args[0] if args else 'Input'
    args = args[1:]
    options, flags = [], []
    if mode == 'select':
        if '--' in args:
            index = args.index('--')
            options, flags = args[:index], args[index + 1:]
        else:
            options = args
        if not options and not stdin.isatty():
            data = stdin.read(2 * 1024 * 1024 + 1)
            if len(data.encode()) > 2 * 1024 * 1024:
                raise ValueError('Menu exceeds 2 MiB')
            options = data.splitlines()
        if not options:
            raise ValueError('No menu options supplied')
    else:
        flags = args
    result = {'mode': mode, 'prompt': prompt, 'entries': [entry(o, prompt) for o in options]}
    while flags:
        flag, *flags = flags
        if flag in ('--width', '--height', '--maxheight'):
            if not flags:
                raise ValueError(flag + ' requires a value')
            value, *flags = flags
            number = int(value)
            if number <= 0:
                raise ValueError(flag + ' requires a positive value')
            result['width' if flag == '--width' else 'max_height'] = number
    return result


def main():
    mode, *args = sys.argv[1:]
    # Wrappers remain on PATH in some long-lived applications after rollback.
    # Route those callers to stock too; never leave a half-active integration.
    enabled = Path.home() / '.local/state/lucent/integration-backup/menu-enabled'
    if not enabled.exists():
        os.execv('/usr/bin/omarchy-menu-' + mode, ['omarchy-menu-' + mode, *args])
    payload = request(mode, args, sys.stdin)
    state = Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state')))
    if not state.is_absolute():
        state = Path.home() / '.local/state'
    settings = state / 'lucent/desktop.json'
    try:
        payload['light'] = bool(json.loads(settings.read_text()).get('light', False))
    except (OSError, ValueError):
        pass
    binary = Path.home() / '.local/bin/lucent-menu'
    # The child writes only a selected value to stdout, after releasing focus.
    result = subprocess.run([str(binary)], input=json.dumps(payload), text=True, check=False)
    return result.returncode


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError) as error:
        print('lucent-menu: ' + str(error), file=sys.stderr)
        raise SystemExit(2)
