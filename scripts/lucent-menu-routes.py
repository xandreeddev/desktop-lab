#!/usr/bin/env python3
"""Adapt installed Omarchy JSONC menus to Lucent. UI and action policy stay separate."""
import fcntl
import os
from pathlib import Path
import socket
import subprocess
import time

from menu_model import Model, jsonc


def condition(command, default):
    if not command:
        return default
    # These expressions are trusted Omarchy/user configuration, as in stock.
    # Display labels, search text, and returned IDs never become shell code.
    try:
        return subprocess.run(['bash', '-c', command], stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              timeout=2, check=False).returncode == 0
    except subprocess.TimeoutExpired as error:
        raise ValueError('Menu availability check timed out') from error


def output(*args):
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return ''


def load():
    root = Path(os.environ.get('OMARCHY_PATH', '/usr/share/omarchy'))
    defaults = jsonc((root/'default/omarchy/omarchy-menu.jsonc').read_text())
    user = Path.home()/'.config/omarchy/extensions/omarchy-menu.jsonc'
    return Model(native_defaults(defaults), jsonc(user.read_text()) if user.is_file() else {})


def picker_command(command):
    try:
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(2)
            client.connect(os.environ['XDG_RUNTIME_DIR']+'/lucent-menu.sock')
            client.sendall((command+'\n').encode())
            data = b''
            while chunk := client.recv(65536):
                data += chunk
            return data.decode()
    except OSError:
        return None


def native_defaults(defaults):
    """Replace known UI routes before merging user extensions.

    These are stable Omarchy route IDs, not command-string heuristics. An
    extension's action still takes precedence. Other stock actions are retained.
    """
    overrides = {
        'root': {'label': 'Lucent', 'parent': '', 'aliases': ['go', 'menu']},
        'style.background': {'action': ['lucent-cli', 'wallpapers', 'open']},
        'style.theme': {'action': ['lucent-cli', 'themes', 'open'], 'provider': None},
        'style.screensaver': {'label': 'Lock appearance', 'action': ['lucent-cli', 'themes', 'open']},
        'install.style.theme': {'when': 'false'},
        'setup.plugin': {'when': 'false'},
    }
    result = {key: dict(value) for key, value in defaults.items()}
    for key, fields in overrides.items():
        if key in result or key == 'root':
            result[key] = result.get(key, {}) | fields
    return result


def dispatch(action):
    # Only trusted configuration may supply a shell expression. Native actions
    # and dynamic provider values remain argv arrays from discovery to exec.
    argv = ['bash', '-c', action] if isinstance(action, str) else action
    subprocess.Popen(argv, stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     start_new_session=True)


def provider_rows(provider):
    if provider == 'fonts':
        values, current, command = output('omarchy-font-list'), output('omarchy-font-current'), ['omarchy-font-set']
    elif provider == 'power-profiles':
        values, current, command = output('omarchy-powerprofiles-list'), output('powerprofilesctl', 'get'), ['omarchy-powerprofiles-set', 'autodetect']
    else:
        raise ValueError('Unsupported Omarchy menu provider: ' + provider)
    rows, actions = [], {}
    for i, value in enumerate(values.splitlines()):
        key = '@provider:' + str(i)
        rows.append({'label': value, 'detail': 'Current' if value == current else '', 'value': key})
        actions[key] = [*command, value]
    return rows, actions


def run(args, pick):
    verb = args[0] if args else 'toggle'
    route = args[1] if len(args) > 1 else 'root'
    if verb in ('help', '--help', '-h'):
        print('omarchy-menu [toggle|summon|close|refresh|ping] [route] — native Lucent menus')
        return 0
    if verb == 'close':
        picker_command('close')
        return 0
    if verb in ('refresh', 'ping'):
        load()  # Sources are re-read on every navigation, not cached in a daemon.
        return 0
    if verb not in ('toggle', 'summon'):
        raise ValueError('Unknown menu verb: ' + verb)
    if picker_command('inspect') is not None:
        picker_command('close')
        if verb == 'toggle':
            return 0
        # A normal menu releases its process lock on exit, before opening another.
        for _ in range(20):
            if picker_command('inspect') is None:
                break
            time.sleep(.05)
    lock = open(Path(os.environ['XDG_RUNTIME_DIR'])/'lucent-menu-routes.lock', 'w')
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        lock.close()
        return 0
    with lock:
        while True:
            model = load()
            route = model.resolve(route)
            item = model.items[route]
            if not condition(item.get('when'), True) or condition(item.get('disabled'), False):
                raise ValueError('This menu action is currently unavailable')
            action, provider = item.get('action', ''), item.get('provider', '')
            if provider == 'apps' and not action:
                action = ['lucent-cli', 'launcher', 'open']
            if action:
                dispatch(action)
                return 0
            rows = model.rows(route, condition)
            actions = {}
            if provider:
                extra, actions = provider_rows(provider)
                rows.extend(extra)
            if not rows:
                rows = [{'label': 'No available actions', 'value': '', 'disabled': True}]
            result = pick({'mode': 'select', 'prompt': item.get('title') or item['label'],
                           'entries': rows, 'back': route != 'root'}, capture=True)
            if result.returncode == 3:
                route = item['parent'] or 'root'
                continue
            if result.returncode != 0:
                return result.returncode
            selected = result.stdout.rstrip('\n')
            # Validate against the exact offered rows, including disabled state.
            if not any(r['value'] == selected and not r.get('disabled', False) for r in rows):
                raise ValueError('Invalid menu selection')
            if selected in actions:
                dispatch(actions[selected])
                return 0
            route = selected
