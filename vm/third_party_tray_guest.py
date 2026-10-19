#!/usr/bin/env python3
"""Read-only probes and exact owned-unit lifecycle for genuine nm-applet tests.

No SNI producer is implemented here. The installed, identified nm-applet exports
the item/menu. This helper never installs packages, registers a secret agent,
asks for secrets, changes NetworkManager or writes shared desktop preferences.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time

from third_party_tray_policy import (Blocked, Unsupported, cgroup_path, digest,
                                     information_entry, owned_process, require, unique, unit_name,
                                     unbound_input)

APP = 'org.freedesktop.network-manager-applet'
NM = 'org.freedesktop.NetworkManager'
WATCHER = 'org.kde.StatusNotifierWatcher'
PROPERTIES = 'org.freedesktop.DBus.Properties'
ITEM = 'org.kde.StatusNotifierItem'
MENU = 'com.canonical.dbusmenu'
CLI = str(Path.home() / '.local/bin/lucent-cli')


def run(*args, optional=False):
    value = subprocess.run(args, capture_output=True, text=True, timeout=5)
    require(value.returncode in ((0, 1) if optional else (0,)), 'bounded_guest_command_failed')
    require(len(value.stdout) <= 8 * 1024 * 1024, 'bounded_guest_command_output_required')
    return value.stdout.strip()


def private(path, value):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        json.dump(value, stream, sort_keys=True)


def identity(info):
    return [info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_size, info.st_mtime_ns]


def file_record(path):
    path = Path(path)
    try:
        info = path.lstat()
    except FileNotFoundError:
        return None
    result = {'identity': identity(info)}
    if path.is_symlink():
        result.update(link=os.readlink(path), resolved=str(path.resolve(strict=True)))
        target = path.stat()
        result['target_identity'] = identity(target)
    else:
        target = info
    require(stat.S_ISREG(target.st_mode) and target.st_size <= 512 * 1024 * 1024,
            'bounded_regular_preserved_file_required')
    with path.open('rb') as source:
        before = identity(os.fstat(source.fileno()))
        require(before == identity(target), 'preserved_target_changed_before_hash')
        result['sha256'] = hashlib.file_digest(source, 'sha256').hexdigest()
        require(before == identity(os.fstat(source.fileno())), 'preserved_file_changed_during_hash')
    require(result['identity'] == identity(path.lstat()), 'preserved_path_changed_during_hash')
    require(identity(path.stat()) == before, 'preserved_target_changed_during_hash')
    return result


def files(home=None, environment=None):
    home = Path.home() if home is None else Path(home)
    environment = os.environ if environment is None else environment
    require(environment.get('XDG_STATE_HOME', str(home / '.local/state')) == str(home / '.local/state')
            and environment.get('XDG_CONFIG_HOME', str(home / '.config')) == str(home / '.config'),
            'prepared_default_state_and_config_roots_required')
    state = home / '.local/state/lucent'
    settings = state / 'desktop.json'
    require(settings.is_file() and not settings.is_symlink() and settings.stat().st_size <= 4 * 1024 * 1024,
            'existing_bounded_settings_required')
    saved = json.loads(settings.read_text())
    paths = {settings, state / 'wallpaper', state / 'notifications.json', home / '.config/dconf/user',
             home / '.local/state/omarchy/indicators/stay-awake'}
    require((home / '.local/state/omarchy/indicators/stay-awake').is_file(),
            'existing_stay_awake_marker_required')
    directories_saved = {}
    for root in (home / '.config/hypr', home / '.config/nm-applet'):
        directories_saved[str(root)] = identity(root.lstat()) if root.exists() else None
        if root.exists():
            require(not root.is_symlink() and root.is_dir(), 'real_preserved_config_directory_required')
            for parent, directories, names in os.walk(root, followlinks=False):
                directories_saved[str(parent)] = identity(Path(parent).lstat())
                require(not any((Path(parent) / name).is_symlink() for name in directories),
                        'symlinked_config_directory_requires_review')
                paths.update(Path(parent) / name for name in names)
                require(len(paths) <= 512, 'bounded_preserved_config_tree_required')
    def assets(value):
        if isinstance(value, dict):
            for key, child in value.items():
                if key == 'path' and isinstance(child, str):
                    path = Path(child)
                    require(path.is_absolute(), 'absolute_live_media_reference_required')
                    paths.add(path)
                else:
                    assets(child)
        elif isinstance(value, list):
            for child in value:
                assets(child)
    assets(saved.get('live_wallpapers', {}))
    require(len(paths) <= 576 and sum(path.stat().st_size for path in paths if path.exists())
            <= 1024 * 1024 * 1024, 'bounded_preserved_payload_required')
    return {'files': {str(path): file_record(path) for path in sorted(paths)},
            'directories': directories_saved}


def process(pid, private_token=False):
    require(type(pid) is int and pid > 1 and Path(f'/proc/{pid}').stat().st_uid == os.getuid(),
            'owned_regular_user_process_required')
    stat_text = Path(f'/proc/{pid}/stat').read_text()
    with Path(f'/proc/{pid}/exe').open('rb') as executable:
        hashed = hashlib.file_digest(executable, 'sha256').hexdigest()
    value = {'pid': pid, 'start': stat_text.rsplit(')', 1)[1].split()[19],
             'cgroup': Path(f'/proc/{pid}/cgroup').read_text().removeprefix('0::').strip(),
             'executable_sha256': hashed}
    if private_token:
        value['argv'] = Path(f'/proc/{pid}/cmdline').read_bytes().decode().rstrip('\0').split('\0')
        # Read only our own marker value; no environment values are emitted.
        environment = Path(f'/proc/{pid}/environ').read_bytes().split(b'\0')
        marker = [entry.partition(b'=')[2].decode() for entry in environment
                  if entry.startswith(b'LUCENT_TRAY_TEST_TOKEN=')]
        value['token'] = marker[0] if len(marker) == 1 else None
    return value


def unit_properties(unit):
    unit_name(unit)
    value = run('systemctl', '--user', 'show', unit, '--property=ActiveState', '--property=SubState',
                '--property=MainPID', '--property=ControlGroup', '--property=InvocationID',
                '--property=LoadState')
    return dict(line.split('=', 1) for line in value.splitlines())


def desktop(allowed_pid=None):
    require(json.loads(run('hyprctl', '-j', 'locked')) == {'locked': False}
            and run('omarchy-shell', 'lock', 'isLocked') == 'false', 'unlocked_session_required')
    pids = run('pgrep', '-x', 'lucent-desktop').splitlines()
    require(len(pids) == 1 and pids[0].isdigit(), 'one_desktop_process_required')
    pid = int(pids[0])
    client = json.loads(run(CLI, 'inspect'))['client']
    require(not client.get('network', {}).get('form')
            and not any(client.get('bluetooth', {}).get(key) for key in ('form', 'requests', 'answering')),
            'no_credential_requests_required')
    layers = json.loads(run('hyprctl', '-j', 'layers'))
    def walk(value):
        if isinstance(value, dict):
            if 'namespace' in value:
                require(value.get('pid') == pid and str(value['namespace']).startswith('lucent-')
                        and not str(value['namespace']).startswith('lucent-authorization-'),
                        'no_foreign_or_authorization_layers_required')
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    walk(layers)
    windows = json.loads(run('hyprctl', '-j', 'clients'))
    require(len(windows) <= (1 if allowed_pid else 0), 'no_foreign_windows_on_any_workspace_required')
    for window in windows:
        require(window.get('pid') == allowed_pid and window.get('class') in (APP, 'nm-applet')
                and window.get('title') == 'Connection Information' and window.get('xwayland') is False,
                'only_exact_owned_native_information_dialog_allowed')
    monitors = json.loads(run('hyprctl', '-j', 'monitors'))
    require(len(monitors) == 1 and tuple(monitors[0].get(key) for key in
            ('width', 'height', 'x', 'y', 'scale', 'transform')) == (1920, 1080, 0, 0, 1, 0),
            'single_prepared_native_output_required')
    native = monitors[0]
    signature = {key: native[key] for key in ('id', 'name', 'width', 'height', 'x', 'y',
                                             'scale', 'transform', 'refreshRate', 'dpmsStatus')}
    require(signature['dpmsStatus'] is True, 'awake_output_required')
    supervisor = run('systemctl', '--user', 'show', 'lucent.service', '--property=ActiveState',
                     '--property=MainPID', '--property=ControlGroup', '--property=ExecMainStartTimestampMonotonic')
    fields = dict(line.split('=', 1) for line in supervisor.splitlines())
    current = process(pid)
    require(fields.get('ActiveState') == 'active' and fields.get('MainPID') == str(pid)
            and fields.get('ControlGroup') == current['cgroup'], 'desktop_owned_by_current_supervisor_required')
    instance = os.environ.get('HYPRLAND_INSTANCE_SIGNATURE')
    require(instance and '/' not in instance, 'captured_native_compositor_required')
    compositor = unique(json.loads(run('hyprctl', '-j', 'instances')),
                        lambda value: value.get('instance') == instance, 'one_current_compositor_required')
    socket = Path(os.environ['XDG_RUNTIME_DIR']) / 'hypr' / instance / '.socket.sock'
    info = socket.lstat()
    require(stat.S_ISSOCK(info.st_mode) and info.st_uid == os.getuid(), 'owned_native_compositor_socket_required')
    installed = file_record(Path.home() / '.local/bin/lucent-desktop')
    require(installed and installed['sha256'] == current['executable_sha256'], 'installed_running_release_mismatch')
    return {'process': current, 'supervisor': supervisor, 'monitor': signature, 'installed': installed,
            'compositor': {'process': process(compositor['pid']), 'instance': instance,
                           'socket': identity(info)[:2]}}, client, windows


class Bus:
    def __init__(self):
        try:
            import dbus
        except ImportError:
            raise Unsupported('guest_python_dbus_unavailable') from None
        self.dbus = dbus
        self.session, self.system = dbus.SessionBus(), dbus.SystemBus()
        self.deadline = time.monotonic() + 20

    def call(self, bus, owner, path, interface, method, *args):
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, 'absolute_read_only_bus_probe_deadline_expired')
        try:
            target = bus.get_object(owner, path, introspect=False)
            return target.get_dbus_method(method, interface)(*args, timeout=min(3, remaining))
        except self.dbus.DBusException:
            raise Blocked('native_read_only_bus_observation_unavailable') from None

    def owner(self, bus, name, absent=False):
        owned = self.call(bus, 'org.freedesktop.DBus', '/org/freedesktop/DBus',
                          'org.freedesktop.DBus', 'NameHasOwner', name)
        if not owned:
            require(absent, 'required_native_bus_owner_absent')
            return None
        return str(self.call(bus, 'org.freedesktop.DBus', '/org/freedesktop/DBus',
                             'org.freedesktop.DBus', 'GetNameOwner', name))

    def pid(self, owner):
        return int(self.call(self.session, 'org.freedesktop.DBus', '/org/freedesktop/DBus',
                             'org.freedesktop.DBus', 'GetConnectionUnixProcessID', owner))

    def properties(self, bus, owner, path, interface):
        return self.call(bus, owner, path, PROPERTIES, 'GetAll', interface)

    def plain(self, value):
        if isinstance(value, self.dbus.Boolean):
            return bool(value)
        if isinstance(value, dict):
            return {str(key): self.plain(child) for key, child in value.items()}
        if isinstance(value, (list, tuple, bytes, bytearray)):
            return [self.plain(child) for child in value]
        if isinstance(value, str):
            return str(value)
        if isinstance(value, int):
            return int(value)
        if isinstance(value, float):
            return float(value)
        raise Blocked('unsupported_read_only_bus_value')

    def profiles(self):
        owner = self.owner(self.system, NM)
        root = self.properties(self.system, owner, '/org/freedesktop/NetworkManager', NM)
        require(len(root['Devices']) <= 64 and len(root['ActiveConnections']) <= 64,
                'bounded_network_device_snapshot_required')
        devices = []
        for path in root['Devices']:
            props = self.properties(self.system, owner, str(path), NM + '.Device')
            require(int(props['DeviceType']) in (1, 32), 'wired_only_guest_required_to_avoid_implicit_wifi_scans')
            devices.append([str(path), int(props['DeviceType']), str(props['Interface']), int(props['State'])])
        active = []
        for path in root['ActiveConnections']:
            props = self.properties(self.system, owner, str(path), NM + '.Connection.Active')
            require(str(props['Type']) == '802-3-ethernet' and int(props['State']) == 2,
                    'stable_active_wired_connection_required')
            active.append([str(path), str(props['Uuid']), int(props['State'])])
        require(len(active) == 1, 'one_active_wired_connection_required')
        settings = '/org/freedesktop/NetworkManager/Settings'
        paths = self.call(self.system, owner, settings, NM + '.Settings', 'ListConnections')
        require(len(paths) <= 64, 'bounded_nonsecret_profile_set_required')
        profiles = []
        for path in paths:
            # NetworkManager's documented GetSettings explicitly excludes secrets.
            values = self.call(self.system, owner, str(path), NM + '.Settings.Connection', 'GetSettings')
            props = self.properties(self.system, owner, str(path), NM + '.Settings.Connection')
            profiles.append([str(path), digest(self.plain(values)), bool(props['Unsaved']), int(props['Flags'])])
        require(self.owner(self.system, NM) == owner, 'network_manager_owner_changed_during_probe')
        return {'owner': owner, 'devices': sorted(devices), 'active': sorted(active),
                'profiles': sorted(profiles),
                'radio': [bool(root[key]) for key in ('NetworkingEnabled', 'WirelessEnabled', 'WwanEnabled')]}

    def items(self, desktop_pid):
        owner = self.owner(self.session, WATCHER)
        require(self.pid(owner) == desktop_pid, 'desktop_must_own_current_sni_watcher')
        props = self.properties(self.session, owner, '/StatusNotifierWatcher', WATCHER)
        return owner, [str(value) for value in props['RegisteredStatusNotifierItems']]


def read(directory, name):
    path = directory / name
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 4 * 1024 * 1024
            and path.stat().st_uid == os.getuid() and stat.S_IMODE(path.stat().st_mode) == 0o600,
            'bounded_private_receipt_required')
    return json.loads(path.read_text())


def idle(client, allow_tray=False):
    require(not any(client.get(key) for key in ('launcher', 'query', 'widgets_editing', 'widget_grid',
                                               'notification_history_open', 'notification_count', 'error')),
            'idle_native_desktop_required')
    for panel in ('audio', 'network', 'bluetooth', 'power', 'weather', 'media', 'tray',
                  'live_wallpaper', 'settings', 'displays', 'application_actions'):
        if panel == 'tray' and allow_tray:
            continue
        require(not any(client.get(panel, {}).get(key) for key in ('open', 'visible', 'busy')),
                'closed_idle_panels_required')
    require(client.get('settings_save_status') == 'saved' and client.get('background_ready')
            and not client.get('wallpaper_busy'), 'saved_ready_background_required')


def prepare(directory, sha):
    binary = Path('/usr/bin/nm-applet')
    if not binary.is_file():
        raise Unsupported('nm_applet_not_installed_no_automatic_install')
    require(binary.stat().st_uid == 0 and not binary.stat().st_mode & 0o022,
            'root_owned_installed_applet_required')
    package = run('pacman', '-Q', 'network-manager-applet')
    require(package.startswith('network-manager-applet '), 'official_arch_applet_package_required')
    baseline, client, _ = desktop()
    unbound_input(json.loads(run('hyprctl', '-j', 'binds')))
    require(baseline['process']['executable_sha256'] == sha, 'desktop_release_identity_mismatch')
    idle(client)
    require(client['tray']['ready'] and client['tray']['items'] == 0, 'idle_empty_native_tray_required')
    require(not run('pgrep', '-x', 'nm-applet', optional=True), 'existing_real_applet_must_be_left_alone')
    bus = Bus()
    require(bus.owner(bus.session, APP, absent=True) is None, 'existing_real_applet_bus_owner_must_be_left_alone')
    watcher, items = bus.items(baseline['process']['pid'])
    require(not items, 'existing_sni_items_must_be_left_alone')
    value = {'unit': unit_name(directory.name + '.service'), 'token': directory.name.rsplit('-', 1)[-1],
             'desktop': baseline, 'watcher': watcher, 'files': files(), 'network': bus.profiles(),
             'applet': file_record(binary), 'package': package,
             'cursor': json.loads(run('hyprctl', '-j', 'cursorpos'))}
    private(directory / 'checkpoint.json', value)
    for name in ('stdout.log', 'stderr.log'):
        descriptor = os.open(directory / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        os.close(descriptor)
    return {'prepared': True, 'applet_sha256': value['applet']['sha256'], 'package': package}


def launch(directory):
    saved = read(directory, 'checkpoint.json')
    require(files() == saved['files'] and file_record('/usr/bin/nm-applet') == saved['applet'],
            'preserved_state_or_installed_applet_changed_before_exec')
    properties = unit_properties(saved['unit'])
    pid = os.getpid()
    require(properties['MainPID'] == str(pid) and properties['InvocationID'], 'exact_owning_user_unit_required')
    current = process(pid)
    cgroup_path(current['cgroup'], os.getuid(), saved['unit'])
    group = Path('/sys/fs/cgroup') / current['cgroup'].lstrip('/')
    require(group.resolve(strict=True) == group and str(pid) in (group / 'cgroup.procs').read_text().split(),
            'main_pid_in_exact_owned_cgroup_required')
    intent = {key: current[key] for key in ('pid', 'start', 'cgroup')}
    intent.update(unit=saved['unit'], invocation=properties['InvocationID'], token=saved['token'],
                  applet_sha256=saved['applet']['sha256'], group_identity=identity(group.stat())[:2])
    private(directory / 'exec-intent.json', intent)
    environment = os.environ.copy()
    for key, name in (('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data')):
        path = directory / name
        path.mkdir(mode=0o700)
        environment[key] = str(path)
    environment.update(GSETTINGS_BACKEND='memory', GDK_BACKEND='wayland', LC_ALL='C.UTF-8',
                       LANG='C.UTF-8', LUCENT_TRAY_TEST_TOKEN=saved['token'])
    os.execve('/usr/bin/nm-applet', ['/usr/bin/nm-applet', '--indicator', '--no-agent'], environment)


def owner(directory):
    saved, intent = read(directory, 'checkpoint.json'), read(directory, 'exec-intent.json')
    properties = unit_properties(saved['unit'])
    require(properties.get('InvocationID') == intent['invocation']
            and properties.get('MainPID') == str(intent['pid'])
            and properties.get('ControlGroup') == intent['cgroup'], 'owned_unit_invocation_changed')
    current = process(intent['pid'], private_token=True)
    owned_process(intent, current)
    cgroup_path(current['cgroup'], os.getuid(), saved['unit'])
    return saved, intent, current


def verify(directory, allow_dialog=False):
    saved = read(directory, 'checkpoint.json')
    app_pid = owner(directory)[2]['pid'] if allow_dialog else None
    current, _, windows = desktop(app_pid)
    require(current == saved['desktop'], 'desktop_supervisor_or_output_changed')
    require(files() == saved['files'], 'configuration_settings_wallpaper_or_media_changed')
    bus = Bus()
    require(bus.profiles() == saved['network'], 'nonsecret_profile_radio_or_connection_state_changed')
    require(bus.owner(bus.session, WATCHER) == saved['watcher'], 'watcher_owner_changed')
    return {'preserved': True, 'owned_dialogs': len(windows)}


def input_guard(directory):
    value = verify(directory)
    owner(directory)
    unbound_input(json.loads(run('hyprctl', '-j', 'binds')))
    # Recheck native ownership after the potentially longer profile probes.
    baseline, client, _ = desktop()
    require(baseline == read(directory, 'checkpoint.json')['desktop'], 'fresh_native_identity_changed')
    idle(client, allow_tray=True)
    return value


def restore_admission(directory):
    result = retired(directory)
    unbound_input(json.loads(run('hyprctl', '-j', 'binds')))
    baseline, client, _ = desktop()
    require(baseline == read(directory, 'checkpoint.json')['desktop'], 'fresh_native_identity_changed')
    idle(client)
    return result


def describe(directory, with_menu=False):
    saved, intent, current = owner(directory)
    verify(directory)
    bus = Bus()
    require(bus.pid(bus.owner(bus.session, APP)) == current['pid'], 'genuine_application_bus_owner_changed')
    watcher, items = bus.items(saved['desktop']['process']['pid'])
    require(watcher == saved['watcher'] and len(items) == 1, 'one_genuine_owned_sni_item_required')
    service, separator, rest = items[0].partition('/')
    require(separator and service, 'native_sni_endpoint_required')
    target = bus.owner(bus.session, service)
    require(bus.pid(target) == current['pid'], 'sni_item_not_exported_by_owned_genuine_applet')
    path = '/' + rest
    props = bus.properties(bus.session, target, path, ITEM)
    require(str(props.get('Id', '')) == 'nm-applet' and str(props.get('Status', '')) in ('Active', 'NeedsAttention'),
            'expected_genuine_visible_applet_item_required')
    menu_path = str(props.get('Menu', '/'))
    require(menu_path.startswith('/') and menu_path != '/', 'genuine_dbus_menu_not_available')
    value = {'ready': True, 'owner': target, 'pid': current['pid'], 'item_path': path,
             'menu_path': menu_path, 'icon_provided': bool(props.get('IconName') or props.get('IconPixmap'))}
    require(value['icon_provided'], 'genuine_applet_icon_metadata_missing')
    if (directory / 'owner.json').exists():
        require(read(directory, 'owner.json') == value, 'genuine_sni_owner_or_descriptor_retired')
    else:
        private(directory / 'owner.json', value)
    if with_menu:
        revision, raw = bus.call(bus.session, target, menu_path, MENU, 'GetLayout',
                                 bus.dbus.Int32(0), bus.dbus.Int32(8),
                                 bus.dbus.Array(['label', 'enabled', 'visible', 'type', 'toggle-type',
                                                 'toggle-state', 'children-display'], signature='s'))
        count = 0
        def node(raw, depth=0):
            nonlocal count
            count += 1
            require(count <= 256 and depth <= 8 and len(raw) == 3, 'bounded_genuine_menu_required')
            return {'id': int(raw[0]), 'properties': bus.plain(raw[1]),
                    'children': [node(child, depth + 1) for child in raw[2]]}
        tree = node(raw)
        value.update(revision=int(revision), layout_sha256=digest(tree),
                     information_id=information_entry(tree))
    return value


def stop(directory):
    saved = read(directory, 'checkpoint.json')
    intent = read(directory, 'exec-intent.json')
    require(intent['unit'] == saved['unit'], 'original_owned_unit_required')
    cgroup_path(intent['cgroup'], os.getuid(), saved['unit'])
    properties = unit_properties(saved['unit'])
    if properties.get('MainPID', '0') != '0':
        owner(directory)  # PID/start/executable/argv/token/cgroup/invocation before stop
        run('systemctl', '--user', 'stop', saved['unit'])
    else:
        require(properties.get('InvocationID', '') in ('', intent['invocation'])
                and properties.get('ActiveState') in ('inactive', 'failed'), 'owned_unit_not_retired')
    group = Path('/sys/fs/cgroup') / intent['cgroup'].lstrip('/')
    if group.exists():
        require(group.resolve(strict=True) == group and identity(group.stat())[:2] == intent['group_identity'],
                'owned_cgroup_lifetime_changed')
        count = 0
        for parent, directories, _ in os.walk(group, followlinks=False):
            count += 1
            require(count <= 128, 'bounded_owned_cgroup_tree_required')
            require(not any((Path(parent) / name).is_symlink() for name in directories), 'canonical_owned_cgroup_required')
            require(not (Path(parent) / 'cgroup.procs').read_text().strip()
                    and not (Path(parent) / 'cgroup.threads').read_text().strip(), 'owned_cgroup_not_quiescent')
            events = dict(line.split() for line in (Path(parent) / 'cgroup.events').read_text().splitlines())
            require(events.get('populated') == '0', 'owned_cgroup_descendants_not_quiescent')
    final = unit_properties(saved['unit'])
    require(final.get('MainPID') == '0' and final.get('ActiveState') in ('inactive', 'failed')
            and final.get('InvocationID', '') in ('', intent['invocation']), 'owned_unit_not_finally_quiescent')
    return {'exact_owned_unit_quiescent': True}


def retired(directory):
    result = verify(directory)
    saved = read(directory, 'checkpoint.json')
    bus = Bus()
    watcher, items = bus.items(saved['desktop']['process']['pid'])
    require(watcher == saved['watcher'] and not items
            and bus.owner(bus.session, APP, absent=True) is None, 'real_applet_item_or_bus_owner_not_retired')
    client = json.loads(run(CLI, 'inspect'))['client']
    require(client['tray']['ready'] and client['tray']['items'] == 0
            and not any(client['tray'].get(key) for key in ('open', 'visible', 'busy')),
            'native_applet_ui_not_retired')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('prepare', 'launch', 'describe', 'menu', 'verify', 'dialog', 'stop',
                                             'input', 'retired', 'restore-admission'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('--desktop-sha256')
    args = parser.parse_args()
    info = args.directory.lstat()
    require(args.directory.is_absolute() and args.directory.resolve() == args.directory
            and stat.S_ISDIR(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o700
            and info.st_uid == os.getuid(), 'private_owned_test_directory_required')
    unit_name(args.directory.name + '.service')
    try:
        if args.operation == 'prepare':
            result = prepare(args.directory, args.desktop_sha256)
        elif args.operation == 'launch':
            launch(args.directory)
            return
        elif args.operation in ('describe', 'menu'):
            result = describe(args.directory, args.operation == 'menu')
        elif args.operation in ('verify', 'dialog'):
            result = verify(args.directory, args.operation == 'dialog')
        elif args.operation == 'input':
            result = input_guard(args.directory)
        elif args.operation == 'retired':
            result = retired(args.directory)
        elif args.operation == 'restore-admission':
            result = restore_admission(args.directory)
        else:
            result = stop(args.directory)
        print(json.dumps({'status': 'ok', 'value': result}))
    except (Blocked, Unsupported) as error:
        print(json.dumps({'status': 'unsupported' if isinstance(error, Unsupported) else 'blocked',
                          'reason': str(error)}))
        if args.operation == 'launch':
            raise SystemExit(1)
    except Exception:
        print(json.dumps({'status': 'blocked', 'reason': 'bounded_guest_probe_failed'}))


if __name__ == '__main__':
    main()
