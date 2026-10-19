#!/usr/bin/env python3
"""Independent guest lease for test-native-multi-output.py, not a desktop service.

Owns only one freshly named headless output. Absolute lifetime cannot be extended
by requests. Never writes configuration/preferences or adopts a preexisting
output. On uncertainty it retains its private receipt for operator inspection.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import stat
import struct
import subprocess
import time

from multi_output_policy import (Blocked, CursorOwnership, case_layout, hit_center, host,
                                 layer_for, matches_case, monitor_signature, owned_name,
                                 output, require, require_owned_layers, require_unbound_input,
                                 surface, unchanged_layout, unique)


LEASE_SECONDS = 240
CLI = str(Path.home() / '.local/bin/lucent-cli')
REVIEWED_HYPRLAND_COMMIT = 'efb50993780079460b0cbed1363e2166a2de1d9f'
REMOVAL_SOURCE = ('https://github.com/hyprwm/Hyprland/blob/' + REVIEWED_HYPRLAND_COMMIT
                  + '/src/output/Monitor.cpp#L439')


def require_reviewed_hyprland(value):
    require(value.get('commit') == REVIEWED_HYPRLAND_COMMIT and value.get('dirty') is False,
            'installed_hyprland_commit_requires_matching_removal_source_review')


def command(*args):
    result = subprocess.run(args, capture_output=True, text=True, timeout=6, check=False)
    require(result.returncode == 0 and len(result.stdout) <= 8 * 1024 * 1024,
            'bounded_guest_command_failed')
    return result.stdout.strip()


def hypr(*args):
    return command('hyprctl', *args)


def inspect():
    return json.loads(command(CLI, 'inspect'))


def identity(info):
    return [info.st_dev, info.st_ino, info.st_mode, info.st_uid,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns]


def file_snapshot(path, hashed):
    """Record link and resolved-file identities, never follow a directory tree link."""
    path = Path(path)
    info = path.lstat()
    result = {'identity': identity(info)}
    if stat.S_ISLNK(info.st_mode):
        result['link'] = os.readlink(path)
        result['target'] = str(path.resolve(strict=True))
        info = Path(result['target']).stat()
        result['target_identity'] = identity(info)
    require(stat.S_ISREG(info.st_mode) and info.st_size <= 512 * 1024 * 1024,
            'unbounded_or_nonregular_preserved_file')
    if hashed:
        with path.open('rb') as stream:
            before = identity(os.fstat(stream.fileno()))
            result['sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
            require(before == identity(os.fstat(stream.fileno())), 'preserved_file_changed_while_reading')
    require(result['identity'] == identity(path.lstat()), 'preserved_link_changed_while_reading')
    return result


def preserved_files(hashed):
    home = Path.home()
    state = home / '.local/state/lucent'
    settings_path = state / 'desktop.json'
    require(settings_path.stat().st_size <= 4 * 1024 * 1024, 'bounded_settings_required')
    settings = json.loads(settings_path.read_text())
    paths = {settings_path, state / 'wallpaper',
             home / '.local/state/omarchy/indicators/stay-awake'}
    # Existing stay-awake authority is a precondition; this harness never changes
    # idle/lock settings and preserves the marker's inode as well as contents.
    require(next(p for p in paths if p.name == 'stay-awake').is_file(),
            'existing_stay_awake_marker_required')
    config = home / '.config/hypr'
    require(config.is_dir() and not config.is_symlink(), 'real_user_hypr_config_directory_required')
    directories = {}
    config_paths = set()
    for parent, dirs, files in os.walk(config, followlinks=False):
        require(not any((Path(parent) / name).is_symlink() for name in dirs),
                'symlinked_config_directory_requires_manual_review')
        directories[str(parent)] = identity(Path(parent).stat())
        config_paths.update(Path(parent) / name for name in files)
        paths.update(config_paths)
        require(len(paths) <= 512 and len(directories) <= 64, 'bounded_config_tree_required')
    require(all(path.stat().st_size <= 4 * 1024 * 1024 for path in config_paths)
            and sum(path.stat().st_size for path in config_paths) <= 16 * 1024 * 1024,
            'bounded_config_content_required')

    def assets(value):
        if isinstance(value, dict):
            for key, child in value.items():
                if key == 'path' and isinstance(child, str):
                    path = Path(child)
                    require(path.is_absolute(), 'absolute_live_asset_required')
                    paths.add(path)
                else:
                    assets(child)
        elif isinstance(value, list):
            for child in value:
                assets(child)

    assets(settings.get('live_wallpapers', {}))
    require(len(paths) <= 576, 'bounded_preserved_assets_required')
    require(sum(path.stat().st_size for path in paths) <= 1024 * 1024 * 1024,
            'bounded_total_preserved_payload_required')
    return {'directories': directories,
            'files': {str(p): file_snapshot(p, hashed) for p in sorted(paths)}}


def without_hashes(value):
    return {'directories': value['directories'], 'files': {
        path: {key: data for key, data in entry.items() if key != 'sha256'}
        for path, entry in value['files'].items()}}


def process_identity(pid):
    require(type(pid) is int and pid > 1, 'valid_process_required')
    data = Path(f'/proc/{pid}/stat').read_text()
    # comm can contain spaces and parentheses; starttime is field 22.
    return [pid, data[data.rfind(')') + 2:].split()[19],
            str(Path(f'/proc/{pid}/exe').resolve(strict=True)),
            Path(f'/proc/{pid}/cgroup').read_text()]


def supervisor():
    pids = command('pgrep', '-x', 'lucent-desktop').splitlines()
    require(len(pids) == 1 and pids[0].isdigit(), 'one_desktop_process_required')
    pid = int(pids[0])
    service = command('systemctl', '--user', 'show', 'lucent.service',
                      '--property=ActiveState', '--property=MainPID', '--property=ControlGroup',
                      '--property=ExecMainStartTimestampMonotonic')
    fields = dict(line.split('=', 1) for line in service.splitlines())
    require(fields.get('ActiveState') == 'active' and fields.get('ControlGroup')
            and ('0::' + fields['ControlGroup'] + '\n') == Path(f'/proc/{pid}/cgroup').read_text(),
            'desktop_owned_by_current_supervisor_required')
    return {'process': process_identity(pid), 'service': fields}


def compositor():
    signature = os.environ.get('HYPRLAND_INSTANCE_SIGNATURE')
    require(signature and '/' not in signature, 'captured_hyprland_instance_required')
    native = unique(json.loads(hypr('-j', 'instances')),
                    lambda value: value.get('instance') == signature, 'one_captured_compositor_required')
    sock = Path(os.environ['XDG_RUNTIME_DIR']) / 'hypr' / signature / '.socket.sock'
    info = sock.lstat()
    require(stat.S_ISSOCK(info.st_mode) and info.st_uid == os.getuid(), 'owned_compositor_socket_required')
    return {'signature': signature, 'process': process_identity(native['pid']),
            'socket': [info.st_dev, info.st_ino]}


def no_authorization_or_foreign_windows(desktop_pid):
    require(json.loads(hypr('-j', 'locked')) == {'locked': False}, 'unlocked_compositor_required')
    require(command('omarchy-shell', 'lock', 'isLocked') == 'false', 'unlocked_session_required')
    require(json.loads(hypr('-j', 'clients')) == [], 'no_windows_on_any_workspace_required')
    layers = hypr('-j', 'layers')
    require('lucent-authorization-' not in layers, 'no_authorization_surface_required')
    require_owned_layers(json.loads(layers), desktop_pid)
    client = inspect()['client']
    require(not client.get('network', {}).get('form')
            and not any(client.get('bluetooth', {}).get(key)
                        for key in ('form', 'requests', 'answering')),
            'no_native_credential_prompt_required')
    return client


def idle(client):
    require(not any(client.get(key) for key in ('launcher', 'query', 'launcher_opening',
            'dock_surface_expanded', 'notification_history_open', 'notification_count',
            'widgets_editing', 'widget_grid', 'wallpaper_busy', 'error', 'settings_error',
            'settings_save_error')), 'settled_idle_desktop_required')
    for name in ('audio', 'media', 'network', 'bluetooth', 'power', 'tray', 'weather',
                 'live_wallpaper', 'settings', 'displays', 'application_actions'):
        require(not any(client.get(name, {}).get(key) for key in ('open', 'visible', 'busy')),
                'all_panels_closed_and_idle_required')
    require(client.get('settings_writable') and client.get('settings_save_status') == 'saved'
            and client.get('background_ready') and client.get('named_monitor_hosts'),
            'saved_named_host_desktop_required')
    require(all(not entry['presentation']['transition_pending']
                for entry in client.get('background_transitions', [])), 'settled_wallpaper_required')


def private_json(path, value):
    """One exclusive private receipt; no settings or note text is copied."""
    data = (json.dumps(value, indent=2) + '\n').encode()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as stream:
        stream.write(data)


class Lease:
    def __init__(self, backend, name, baseline):
        self.backend, self.name, self.baseline = backend, owned_name(name), baseline
        self.expected = None
        self.global_id = None
        self.pending_case = None
        self.removing = False

    def checked(self):
        self.backend.preserve()
        monitors = self.backend.monitors()
        found = [m for m in monitors if m['name'] == self.name]
        if self.removing and not found:
            unchanged_layout(monitors, self.baseline, self.name, None)
            self.expected, self.global_id, self.removing = None, None, False
        elif self.pending_case is not None and len(found) == 1:
            after = monitor_signature(found[0])
            require(after['id'] == self.expected['id'], 'owned_output_replaced_during_configuration')
            if matches_case(found[0], self.pending_case):
                unchanged_layout(monitors, self.baseline, self.name, after)
                self.expected, self.pending_case = after, None
        current = unchanged_layout(monitors, self.baseline, self.name, self.expected)
        if current is not None:
            require(self.global_id is None or self.backend.global_id(self.name) == self.global_id,
                    'owned_wayland_lifetime_changed')
        return current

    def create(self):
        require(self.expected is None, 'output_already_owned')
        self.checked()
        self.backend.input_preflight()
        self.backend.create(self.name)
        # The create acknowledgement authorizes exactly the new monitor returned
        # by this compositor, including its initial default layout. If it cannot
        # be identified, do not guess a removal target on a later cleanup.
        current = self.backend.wait_monitor(self.name)
        unchanged_layout(self.backend.monitors(), self.baseline, self.name,
                         monitor_signature(current))
        self.expected = monitor_signature(current)
        self.global_id = self.backend.wait_global(self.name)
        return self.configure(1)

    def configure(self, scale):
        require(self.expected is not None, 'owned_output_required')
        self.checked()
        self.backend.input_preflight()
        case = case_layout(self.name, self.baseline, scale)
        before = self.expected
        self.pending_case = case
        self.backend.configure(case)
        # Only the exact requested transition may become the next cleanup
        # preimage. A third-party layout mutation remains an explicit blocker.
        current = self.backend.wait_case(self.name, case)
        after = monitor_signature(current)
        unchanged_layout(self.backend.monitors(), self.baseline, self.name, after)
        require(after['id'] == before['id'] and self.backend.global_id(self.name) == self.global_id,
                'owned_output_replaced_during_configuration')
        self.expected, self.pending_case = after, None
        return self.status()

    def remove(self):
        self.checked()
        if self.expected is not None:
            self.removing = True
            self.backend.remove(self.name)
            self.backend.wait_absent(self.name)
            self.expected, self.global_id, self.pending_case, self.removing = None, None, None, False
        self.checked()
        return self.status()

    def status(self):
        return {'owned': self.expected, 'global': self.global_id, 'baseline': self.baseline}


class Native:
    def __init__(self, sha):
        self.compositor = compositor()
        self.supervisor = supervisor()
        self.pid = self.supervisor['process'][0]
        with Path(f'/proc/{self.pid}/exe').open('rb') as executable:
            actual = hashlib.file_digest(executable, 'sha256').hexdigest()
        require(actual == sha, 'running_release_identity_mismatch')
        self.hyprland_version = json.loads(hypr('-j', 'version'))
        require_reviewed_hyprland(self.hyprland_version)
        idle(no_authorization_or_foreign_windows(self.pid))
        require_unbound_input(json.loads(hypr('-j', 'binds')))
        self.files = preserved_files(True)
        self.pointer = json.loads(hypr('-j', 'cursorpos'))
        self.cursor = CursorOwnership((self.pointer['x'], self.pointer['y']))
        self.baseline = [monitor_signature(m) for m in self.monitors()]
        require(len(self.baseline) == 1, 'one_original_output_required')
        native = self.monitors()[0]
        require(native['width'] <= 2560 and native['height'] <= 1440
                and native['scale'] == 1 and native['width'] % 2 == 0 and native['height'] % 2 == 0
                and abs(native['x']) <= 16384 and abs(native['y']) <= 16384,
                'bounded_original_physical_workload_required')
        require(native.get('activeWorkspace', {}).get('id', 0) > 0
                and not native.get('specialWorkspace', {}).get('name'),
                'ordinary_original_workspace_required')
        self.workspace = native['activeWorkspace']['id']
        self.workspace_ids = {workspace['id'] for workspace in json.loads(hypr('-j', 'workspaces'))}
        require(self.workspace_ids == {self.workspace}, 'one_original_workspace_required')
        self.owned_workspaces = set()
        require(native['x'] <= self.pointer['x'] < native['x'] + native['width'] / native['scale']
                and native['y'] <= self.pointer['y'] < native['y'] + native['height'] / native['scale'],
                'initial_cursor_on_original_output_required')
        require(hypr('eval', 'assert(type(hl.monitor)=="function" and '
                     'type(hl.dispatch)=="function" and type(hl.dsp.cursor.move)=="function")') == 'ok',
                'documented_lua_monitor_and_cursor_api_required')

    def preserve(self, full=False):
        require(compositor() == self.compositor, 'compositor_lifetime_changed')
        require(supervisor() == self.supervisor, 'desktop_supervisor_lifetime_changed')
        current = preserved_files(full)
        require(current == (self.files if full else without_hashes(self.files)),
                'configuration_settings_or_media_changed')
        no_authorization_or_foreign_windows(self.pid)
        original = unique(self.monitors(), lambda value: value['name'] == self.baseline[0]['name'],
                          'original_output_missing')
        require(original.get('activeWorkspace', {}).get('id') in
                ({self.workspace} | self.owned_workspaces)
                and not original.get('specialWorkspace', {}).get('name'),
                'foreign_workspace_focus_changed')

    def monitors(self):
        return json.loads(hypr('-j', 'monitors'))

    def global_id(self, name):
        return output(inspect(), name)['global']

    def wait(self, predicate):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            value = predicate()
            if value is not None:
                return value
            time.sleep(.2)
        raise Blocked('native_transition_did_not_settle')

    def wait_monitor(self, name):
        def current():
            found = [m for m in self.monitors() if m['name'] == name]
            require(len(found) <= 1, 'ambiguous_owned_output')
            return found[0] if found else None
        return self.wait(current)

    def wait_global(self, name):
        def current():
            found = [o for o in inspect()['outputs'] if o.get('name') == name]
            require(len(found) <= 1, 'ambiguous_wayland_global')
            return found[0]['global'] if found else None
        return self.wait(current)

    def wait_case(self, name, case):
        def current():
            monitor = self.wait_monitor(name)
            return monitor if matches_case(monitor, case) else None
        return self.wait(current)

    def wait_absent(self, name):
        self.wait(lambda: True if not any(m['name'] == name for m in self.monitors()) else None)

    def create(self, name):
        require(hypr('output', 'create', 'headless', name) == 'ok', 'headless_create_not_supported')
        current = self.wait_monitor(name)
        workspace = current.get('activeWorkspace', {}).get('id')
        require(type(workspace) is int and workspace > 0 and workspace not in self.workspace_ids,
                'headless_output_reused_preexisting_workspace')
        self.owned_workspaces.add(workspace)

    def configure(self, case):
        # Lua literals contain only validated generated ASCII values. This is
        # the current adapter's hl.monitor path, never a config-file rewrite.
        code = ('hl.monitor({output=' + json.dumps(case['name']) + ',disabled=false,mode="1280x960@60",'
                f'position="{case["x"]}x{case["y"]}",scale={case["scale"]},transform=0' + '})')
        require(hypr('eval', code) == 'ok', 'owned_monitor_configuration_rejected')

    def remove(self, name):
        current = json.loads(hypr('-j', 'cursorpos'))
        authorized = self.cursor.owns(current)
        # Pinned v0.56.2 onDisconnect moves to the sole backup output's
        # transformed physical center. Original 1x/even extents make it exact.
        backup = self.baseline[0]
        destination = (backup['x'] + backup['width'] // 2, backup['y'] + backup['height'] // 2)
        if authorized:
            self.cursor.begin(current, destination)
        require(hypr('output', 'remove', name) == 'ok', 'owned_output_remove_rejected')
        if authorized:
            try:
                self.cursor.confirm(json.loads(hypr('-j', 'cursorpos')))
            except Blocked:
                # Do not undo the safe output removal or adopt an unknown
                # backend position. Future input/restoration stays blocked.
                pass

    def input_preflight(self):
        require_unbound_input(json.loads(hypr('-j', 'binds')))
        self.cursor.check(json.loads(hypr('-j', 'cursorpos')))

    def warp_hit(self, name, identifier, generation, global_id):
        self.input_preflight()
        state = inspect()
        require(host(state, name)['generation'] == generation and output(state, name)['global'] == global_id,
                'guardian_hit_host_lifetime_changed')
        source = surface(state, name, 'bar')
        layers = json.loads(hypr('-j', 'layers'))
        require_owned_layers(layers, self.pid)
        native = layer_for(layers, name, source, self.pid)
        monitor = unique(self.monitors(), lambda value: value['name'] == name, 'guardian_hit_output_missing')
        destination = hit_center(monitor, source, native, identifier)
        self.cursor.begin(json.loads(hypr('-j', 'cursorpos')), destination)
        require(hypr('dispatch', f'hl.dsp.cursor.move({{x={destination[0]},y={destination[1]}}})') == 'ok',
                'guardian_hit_cursor_warp_rejected')
        self.cursor.confirm(json.loads(hypr('-j', 'cursorpos')))
        return {'point': destination, 'surface': source['id'], 'generation': generation, 'global': global_id}

    def restore_focus(self):
        self.preserve()
        self.input_preflight()
        require(hypr('dispatch', f'hl.dsp.focus({{workspace="{self.workspace}"}})') == 'ok',
                'original_workspace_focus_restore_failed')
        x, y = self.pointer['x'], self.pointer['y']
        self.cursor.begin(json.loads(hypr('-j', 'cursorpos')), (int(x), int(y)))
        require(hypr('dispatch', f'hl.dsp.cursor.move({{x={int(x)},y={int(y)}}})') == 'ok',
                'original_cursor_restore_failed')
        self.cursor.confirm(json.loads(hypr('-j', 'cursorpos')))
        current = self.monitors()
        require(len(current) == 1 and current[0]['activeWorkspace']['id'] == self.workspace,
                'original_workspace_not_restored')
        require(json.loads(hypr('-j', 'cursorpos')) == self.pointer, 'original_cursor_not_restored')


def serve(directory, name, sha):
    backend = Native(sha)
    lease = Lease(backend, name, backend.baseline)
    private_json(directory / 'checkpoint.json', {'files': backend.files,
                 'supervisor': backend.supervisor, 'compositor': backend.compositor,
                 'baseline': backend.baseline, 'workspace': backend.workspace, 'pointer': backend.pointer,
                 'hyprland_version': backend.hyprland_version, 'removal_source': REMOVAL_SOURCE})
    lock_path = Path(os.environ['XDG_RUNTIME_DIR']) / 'lucent-native-multi-output.lock'
    descriptor = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'r+') as lock:
        require(stat.S_ISREG(os.fstat(lock.fileno()).st_mode)
                and os.fstat(lock.fileno()).st_uid == os.getuid(), 'owned_guard_lock_required')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        deadline = time.monotonic() + LEASE_SECONDS
        finished = False
        failure = None
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            server.bind(str(directory / 'control.sock'))
        except BaseException:
            server.close()
            raise
        os.chmod(directory / 'control.sock', 0o600)
        server.listen(1)
        server.settimeout(.5)
        private_json(directory / 'ready.json', {'ready': True, 'desktop_pid': backend.pid,
                     'baseline': backend.baseline, 'lease_seconds': LEASE_SECONDS,
                     'hyprland_version': backend.hyprland_version, 'removal_source': REMOVAL_SOURCE})
        try:
            while not finished and time.monotonic() < deadline:
                try:
                    client, _ = server.accept()
                except TimeoutError:
                    continue
                with client:
                    request_deadline = min(deadline, time.monotonic() + 2)
                    _, uid, _ = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                    require(uid == os.getuid(), 'owned_request_peer_required')
                    data = bytearray()
                    while b'\n' not in data and len(data) <= 1024:
                        remaining = request_deadline - time.monotonic()
                        require(remaining > 0, 'absolute_request_read_deadline_expired')
                        client.settimeout(remaining)
                        block = client.recv(1024)
                        require(bool(block), 'truncated_guard_request')
                        data.extend(block)
                    require(len(data) <= 1024, 'oversized_guard_request')
                    request = json.loads(data)
                    try:
                        require(type(request) is dict and time.monotonic() < deadline,
                                'absolute_guard_deadline_expired')
                        operation = request.get('operation')
                        require(set(request) <= {'operation', 'scale', 'monitor', 'identifier', 'generation', 'global'},
                                'invalid_guard_request_fields')
                        if operation == 'create':
                            value = lease.create()
                        elif operation == 'configure':
                            value = lease.configure(request.get('scale'))
                        elif operation == 'remove':
                            value = lease.remove()
                        elif operation == 'status':
                            lease.checked()
                            backend.input_preflight()
                            value = lease.status()
                        elif operation == 'warp':
                            lease.checked()
                            require(request.get('monitor') in (lease.name, lease.baseline[0]['name'])
                                    and request.get('identifier') in ('bar-audio', 'bar-network')
                                    and type(request.get('generation')) is int and request['generation'] > 0
                                    and type(request.get('global')) is int and request['global'] > 0,
                                    'validated_native_bar_hit_request_required')
                            value = backend.warp_hit(request['monitor'], request['identifier'],
                                                     request['generation'], request['global'])
                        elif operation == 'finish':
                            lease.checked()
                            finished, value = True, {'finishing': True}
                        else:
                            raise Blocked('unsupported_guard_operation')
                        client.settimeout(2)
                        client.sendall((json.dumps({'ok': True, 'value': value}) + '\n').encode())
                    except Exception as error:
                        label = str(error) if isinstance(error, Blocked) else type(error).__name__
                        client.sendall((json.dumps({'ok': False, 'blocked': label}) + '\n').encode())
                        failure, finished = label, True
        except BaseException as error:
            failure = str(error) if isinstance(error, Blocked) else type(error).__name__
        finally:
            server.close()
            result = {'requested_finish': finished and failure is None,
                      'expired': time.monotonic() >= deadline, 'failure': failure,
                      'removed_only_owned_output': False, 'preserved': False, 'cursor_restored': False}
            try:
                lease.remove()
                result['removed_only_owned_output'] = True
                backend.preserve(full=True)
                result['preserved'] = True
                backend.restore_focus()
                result['cursor_restored'] = True
            except Exception as error:
                result['cleanup_blocked'] = str(error) if isinstance(error, Blocked) else type(error).__name__
            result['last_owned_identity'] = lease.status()
            private_json(directory / 'finished.json', result)


def request(directory, operation, scale, monitor=None, identifier=None, generation=None, global_id=None):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        deadline = time.monotonic() + 35
        client.settimeout(35)
        client.connect(str(directory / 'control.sock'))
        value = {'operation': operation}
        if scale is not None:
            value['scale'] = scale
        if operation == 'warp':
            value.update(monitor=monitor, identifier=identifier, generation=generation, **{'global': global_id})
        client.sendall((json.dumps(value) + '\n').encode())
        result = bytearray()
        while b'\n' not in result and len(result) <= 65536:
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'absolute_guard_response_deadline_expired')
            client.settimeout(remaining)
            block = client.recv(65536)
            require(bool(block), 'truncated_guard_response')
            result.extend(block)
        require(len(result) <= 65536, 'oversized_guard_response')
        print(result.decode().strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('serve', 'request'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('argument')
    parser.add_argument('--sha256')
    parser.add_argument('--scale', type=float)
    parser.add_argument('--monitor')
    parser.add_argument('--identifier')
    parser.add_argument('--generation', type=int)
    parser.add_argument('--global-id', type=int)
    args = parser.parse_args()
    info = args.directory.lstat()
    require(args.directory.is_absolute() and args.directory.resolve() == args.directory
            and args.directory.name.startswith('lucent-native-outputs-')
            and stat.S_ISDIR(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o700
            and info.st_uid == os.getuid(), 'private_guard_directory_required')
    if args.mode == 'serve':
        require(args.sha256 is not None and len(args.sha256) == 64, 'release_hash_required')
        signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
        try:
            serve(args.directory, args.argument, args.sha256)
        except Exception as error:
            private_json(args.directory / 'startup-blocked.json', {
                'blocked': str(error) if isinstance(error, Blocked) else type(error).__name__})
            raise SystemExit(1) from None
    else:
        request(args.directory, args.argument, args.scale, args.monitor, args.identifier,
                args.generation, args.global_id)


if __name__ == '__main__':
    main()
