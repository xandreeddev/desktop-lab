#!/usr/bin/env python3
"""Independent resource-only output lease; default/import do nothing.

The guest owns one runtime profile on Virtual-1. It never writes user config,
injects input, changes idle/security policy, starts tracing or restarts Lucent.
Only a freshly staged, source-bound private bundle may start its finite server.
"""
import argparse
import copy
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import stat
import struct
import sys
import time

sys.dont_write_bytecode = True


def require(value, label):
    if not value:
        raise ValueError(label)


def identity(value):
    return [value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns]


def read(path, maximum=4 * 1024 * 1024):
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and stat.S_IMODE(before.st_mode) == 0o600 and before.st_size <= maximum,
                'private_bounded_guest_source_required')
        raw = stream.read(maximum + 1)
        require(len(raw) <= maximum and identity(before) == identity(os.fstat(stream.fileno()))
                == identity(path.lstat()), 'guest_source_changed')
        return raw


def private(path, value):
    raw = (json.dumps(value, sort_keys=True) + '\n').encode()
    require(len(raw) <= 4 * 1024 * 1024, 'private_receipt_bound')
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(raw); stream.flush(); os.fsync(stream.fileno())
    parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    sys.modules[name] = value
    return value


def context(directory, digest):
    directory = Path(directory)
    require(directory.is_absolute() and directory.resolve(strict=True) == directory,
            'canonical_guest_lease_directory_required')
    info = directory.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            and stat.S_IMODE(info.st_mode) == 0o700, 'private_guest_lease_directory_required')
    raw = read(directory / 'bundle.json')
    require(hashlib.sha256(raw).hexdigest() == digest, 'exact_frozen_guest_bundle_required')
    bundle = json.loads(raw)
    require(directory.name == 'lucent-output-4k.' + bundle['tag'], 'captured_output_directory_required')
    for name, expected in bundle['sources'].items():
        require(Path(name).name == name and hashlib.sha256(read(directory / name)).hexdigest() == expected,
                'frozen_guest_lease_source_changed')
    return directory, bundle


class Backend:
    def __init__(self, directory, digest):
        self.directory, self.bundle = context(directory, digest)
        self.digest = digest
        sys.path.insert(0, str(self.directory))
        self.p = module('runtime_output_policy', self.directory / 'runtime_output_policy.py')
        self.p.tag(self.bundle['tag']); self.p.resource_mode(self.bundle['mode'])
        self.base = module('multi_output_guard', self.directory / 'multi_output_guard.py')
        self.worker = module('runtime_output_proof', self.directory / 'runtime_output_proof.py')
        self.units = module('units', self.directory / 'units.py')
        self.owner_units = module('owned_application_units', self.directory / 'owned_application_units.py')
        self.preservation = module('preservation', self.directory / 'preservation.py')
        module('readiness', self.directory / 'readiness.py')
        self.seq = 0
        self.reference = None
        binding = self.bundle['binding']
        require(binding['source_commit'] == self.p.SOURCE and binding['release_manifest_sha256'] == self.p.RELEASE_SHA
                and binding['desktop_sha256'] == self.p.DESKTOP_SHA and binding['mode'] == 'resources'
                and binding['expected_monitor'] == self.p.BASELINE, 'fixed_resource_source_profile_required')
        self.manifest = {key: binding[key] for key in
                         ('desktop_sha256', 'supervisor_sha256', 'settings_sha256', 'critical_user_files')}
        self.manifest.update(expected_monitor=self.p.BASELINE, measurement_mode='resources')
        self.base.require_reviewed_hyprland(json.loads(self.base.hypr('-j', 'version')))
        self.reference = self.worker.snapshot(self.directory, self.manifest)
        self.awake_path = Path.home() / '.local/state/omarchy/indicators/stay-awake'
        require(self.awake_path.is_file() and not self.awake_path.is_symlink(), 'existing_stay_awake_authority_required')
        self.awake_stamp = self.worker.file_stamp(self.awake_path, 4096)
        runtime = binding['runtime_identity']; observed = self.reference['process']
        require(0 <= time.time() - runtime['captured_unix'] <= 300
                and self.worker.boot_identity() == runtime['boot_id']
                and all(observed[key] == runtime[key] for key in ('pid', 'start', 'sha256')),
                'fresh_exact_original_runtime_required')
        require(self.reference['adapter'] == binding['adapter'], 'fixed_vulkan_adapter_required')
        self.initial_compositor = self.base.compositor()
        self.initial_supervisor = self.base.supervisor()
        self.reference_metadata = self.base.preserved_files(False)
        require(self.initial_supervisor['process'][0] == observed['pid']
                and self.initial_supervisor['process'][1] == observed['start'], 'same_supervised_desktop_required')
        preferences = json.loads(self.worker.read(Path.home() / '.local/state/lucent/desktop.json', 4 * 1024 * 1024))
        live = preferences['live_wallpapers']
        selected = live.get('monitors', {}).get('Virtual-1', live.get('default'))
        require(selected and selected.get('enabled', True) and not selected.get('paused', False)
                and not live.get('paused', False) and selected.get('fps') == 'source',
                'original_source_rate_live_workload_required')
        self.asset = Path(selected['asset']['path'])
        require(self.asset.is_absolute() and self.asset.resolve(strict=True) == self.asset,
                'canonical_original_live_asset_required')
        self.asset_stamp = self.worker.file_stamp(self.asset, 512 * 1024 * 1024)
        require(self.asset_stamp['sha256'] == self.p.MEDIA_SHA, 'fixed_original_4k60_media_required')
        streams = json.loads(self.worker.checked(['/usr/bin/ffprobe', '-v', 'error', '-protocol_whitelist', 'file,pipe',
            '-select_streams', 'v:0', '-show_entries', 'stream=codec_name,width,height,r_frame_rate,avg_frame_rate',
            '-of', 'json', str(self.asset)], 30, limit=65536))['streams']
        require(streams == [{'codec_name': 'h264', 'width': 3840, 'height': 2160,
                             'r_frame_rate': '60/1', 'avg_frame_rate': '60/1'}], 'fixed_source_stream_required')

    def verify_sources(self):
        context(self.directory, self.digest)

    def now(self):
        return time.monotonic()

    def record(self, phase, value):
        self.verify_sources()
        private(self.directory / f'journal-{self.seq:04d}.json',
                {'bundle_sha256': self.digest, 'phase': phase, 'value': value})
        self.seq += 1

    def source_proof(self):
        old = self.reference
        require(self.units.unit_snapshot('lucent.service') == old['service'], 'original_supervisor_unit_changed')
        require(self.units.process_snapshot(old['process']['pid'], True) == old['process']
                and self.units.process_snapshot(old['supervisor']['pid'], True) == old['supervisor'],
                'original_supervised_process_changed')
        preservation = self.preservation.fingerprint(
            lambda *argv: self.worker.checked(list(argv), 45, limit=4 * 1024 * 1024).decode())
        preservation.pop('service', None)
        critical = {}
        for key in old['critical']:
            path = Path(key) if key.startswith('/') else Path.home() / (
                key if '/' in key else '.local/bin/' + key)
            critical[key] = self.worker.file_stamp(path, owners={0} if key.startswith('/') else None)
        result = {'mount_authority': self.worker.mount_authority(), 'preservation': preservation,
                  'critical': critical}
        require(all(result[key] == old[key] for key in result), 'source_security_config_or_media_changed')
        require(self.worker.file_stamp(self.asset, 512 * 1024 * 1024) == self.asset_stamp,
                'original_live_media_identity_changed')
        require(self.worker.file_stamp(self.awake_path, 4096) == self.awake_stamp, 'original_idle_policy_marker_changed')
        return result

    def observe(self, *, full, idle):
        self.verify_sources()
        proof = self.source_proof() if full else None
        state = self.base.inspect()
        self.p.normal_off(state)
        if idle:
            self.base.idle(self.base.no_authorization_or_foreign_windows(self.reference['process']['pid']))
        monitors = json.loads(self.base.hypr('-j', 'monitors'))
        require(len(monitors) == 1, 'one_same_native_output_required')
        monitor = monitors[0]
        host = self.p.unique(state['client']['monitor_hosts'], lambda row: row['id'] == 'Virtual-1',
                             'one_native_output_host_required')
        output = self.p.unique(state['outputs'], lambda row: row['name'] == 'Virtual-1',
                               'one_native_output_global_required')
        compositor = self.base.compositor(); supervisor = self.base.supervisor()
        require(compositor == self.initial_compositor and supervisor == self.initial_supervisor,
                'same_original_compositor_and_supervisor_required')
        require(self.worker.wallpaper_digest(state['client']) == self.reference['wallpaper_sha256']
                and state['adapter'] == self.reference['adapter'], 'original_wallpaper_or_adapter_changed')
        # Read output/global again after the slower source/authority checks.
        latest = json.loads(self.base.hypr('-j', 'monitors'))
        latest_state = self.base.inspect()
        require(latest == monitors
                and self.p.unique(latest_state['outputs'], lambda row: row['name'] == 'Virtual-1',
                                  'one_current_output_global_required')['global'] == output['global'],
                'native_output_changed_during_observation')
        latest_host = self.p.unique(latest_state['client']['monitor_hosts'], lambda row: row['id'] == 'Virtual-1',
                                    'one_latest_output_host_required')
        require(all(latest_host[key] == host[key] for key in ('id', 'instance', 'generation')),
                'native_host_changed_during_observation')
        self.p.normal_off(latest_state)
        value = {'compositor': compositor, 'service': self.reference['service'],
                'supervisor': self.reference['supervisor'], 'process': self.reference['process'],
                'global': output['global'], 'host': {key: host[key] for key in ('id', 'instance', 'generation')},
                'monitor': monitor, 'source_proof': proof, 'off': True,
                'cursor': json.loads(self.base.hypr('-j', 'cursorpos')),
                'workspace': {'active': monitor['activeWorkspace'], 'special': monitor['specialWorkspace']}}
        if not hasattr(self, 'first_observation'):
            self.first_observation = copy.deepcopy(value)
        return value

    def configure(self, monitor, *, expected):
        self.verify_sources()
        require(self.base.preserved_files(False) == self.reference_metadata,
                'last_configuration_or_media_metadata_changed')
        for key, value in self.reference['critical'].items():
            path = Path(key) if key.startswith('/') else Path.home() / (
                key if '/' in key else '.local/bin/' + key)
            require(list(self.worker.stable_identity(path.lstat())) == value['identity'],
                    'last_source_or_security_metadata_changed')
        require(self.base.compositor() == self.initial_compositor
                and self.base.supervisor() == self.initial_supervisor, 'last_original_lifetime_changed')
        state = self.base.inspect(); self.p.normal_off(state)
        original = self.first_observation
        host = self.p.unique(state['client']['monitor_hosts'], lambda row: row['id'] == 'Virtual-1', 'last_original_host_required')
        require(all(host[key] == original['host'][key] for key in ('id', 'instance', 'generation'))
                and self.p.unique(state['outputs'], lambda row: row['name'] == 'Virtual-1',
                                  'last_original_global_required')['global'] == original['global'],
                'last_original_host_or_global_changed')
        latest = json.loads(self.base.hypr('-j', 'monitors'))
        require(len(latest) == 1, 'last_single_output_required')
        self.p.same_output(latest[0], original['monitor'], expected)
        require(self.base.hypr('eval', self.p.mode_code(monitor)) == 'ok', 'runtime_output_mode_command_failed')

    def settle(self, expected, deadline):
        deadline = min(deadline, time.monotonic() + 45)
        while time.monotonic() < deadline:
            require(self.base.compositor() == self.initial_compositor
                    and self.base.supervisor() == self.initial_supervisor, 'original_lifetime_changed_while_settling')
            current = json.loads(self.base.hypr('-j', 'monitors'))
            require(len(current) == 1, 'single_original_output_changed_while_settling')
            self.p.same_output(current[0], expected,
                               expected if self.p.profile(current[0]) == self.p.profile(expected) else
                               {**expected, **(self.p.BASELINE if expected['scale'] == 1.5 else self.p.PROFILE)})
            if self.p.monitor_signature(current[0]) == expected:
                return
            time.sleep(.1)
        raise ValueError('bounded_output_profile_settle_timeout')

    def geometry(self):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            try:
                return self.p.ready_geometry(self.base.inspect())
            except self.p.Blocked:
                time.sleep(.1)
        raise ValueError('bounded_actual_4k_geometry_not_ready')


def packet(peer, deadline):
    require(struct.unpack('3i', peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[1] == os.getuid(),
            'same_user_output_lease_request_required')
    raw = bytearray()
    while b'\n' not in raw:
        require(time.monotonic() < deadline and len(raw) < 4096, 'bounded_output_lease_request_required')
        peer.settimeout(min(.5, max(.001, deadline - time.monotonic())))
        data = peer.recv(4096 - len(raw))
        require(data, 'complete_output_lease_request_required')
        raw.extend(data)
    require(raw.endswith(b'\n') and raw.count(b'\n') == 1, 'one_output_lease_request_required')
    return json.loads(raw)


def serve(directory, digest):
    require(signal.getitimer(signal.ITIMER_REAL) == (0.0, 0.0), 'fresh_guard_process_without_timer_required')
    backend = Backend(directory, digest)
    p = backend.p
    lockpath = Path(os.environ['XDG_RUNTIME_DIR']) / 'lucent-native-output-profile.lock'
    lockfd = os.open(lockpath, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(lockfd, 'r+') as lock:
        info = os.fstat(lock.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid(), 'owned_output_profile_lock_required')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        lease = p.OutputLease(backend, time.monotonic())
        # Install one handler before any native mutation. It raises only while
        # a protected operation is running, against that operation's absolute
        # deadline. Pending lease expiry is harmless during cleanup setup and
        # cannot inherit the old deadline after the cleanup budget is selected.
        timer = {'busy': False, 'phase': 'active', 'stop': False,
                 'deadline': lease.deadline - p.CLEANUP_RESERVE}
        timer_errors = []
        alarm_installed = False
        def expired(*_):
            if timer['busy'] and time.monotonic() >= timer['deadline']:
                raise TimeoutError('absolute_resource_output_operation_expired')
        def terminated(*_):
            timer['stop'] = True
            if timer['busy'] and timer['phase'] == 'active':
                raise TimeoutError('resource_output_guard_termination')
        def timer_error(phase, error):
            kind = type(error).__name__
            timer_errors.append({'phase': phase, 'error_class': kind if kind in
                ('TimeoutError', 'OSError', 'RuntimeError', 'ValueError', 'AssertionError', 'KeyboardInterrupt') else 'OtherError'})
        finished = False
        primary = None
        setup_phase = 'initial_handler_alrm'
        try:
            signal.signal(signal.SIGALRM, expired); alarm_installed = True
            setup_phase = 'initial_handler_term'
            signal.signal(signal.SIGTERM, terminated)
            setup_phase = 'initial_arm'
            signal.setitimer(signal.ITIMER_REAL, max(.001, timer['deadline'] - time.monotonic()))
            setup_phase = None
            with socket.socket(socket.AF_UNIX) as server:
                server.bind(str(backend.directory / 'control.sock'))
                os.chmod(backend.directory / 'control.sock', 0o600)
                server.listen(1); server.settimeout(.25)
                unit = 'lucent-output-4k-' + backend.bundle['tag'] + '.service'
                argv = ['/usr/bin/python3', '-I', str(backend.directory / 'runtime_output_guard.py'),
                        'serve', str(backend.directory), digest]
                origin = backend.owner_units.capture_unit_origin(unit, argv)
                require(origin['process']['pid'] == os.getpid(), 'current_independent_output_guard_required')
                private(backend.directory / 'ready.json', {'bundle_sha256': digest, 'guardian': origin,
                    'pid': os.getpid(), 'deadline': lease.deadline, 'resource_only': True,
                    'original': lease.original, 'target': lease.target})
                while not finished and not timer['stop'] and time.monotonic() < timer['deadline']:
                    try:
                        peer, _ = server.accept()
                    except socket.timeout:
                        continue
                    with peer:
                        request = packet(peer, min(lease.deadline - p.CLEANUP_RESERVE, time.monotonic() + 2))
                        require(request == {'operation': request.get('operation'), 'bundle_sha256': digest},
                                'exact_owned_output_request_required')
                        operation = request['operation']
                        try:
                            timer['busy'] = True
                            if operation == 'status':
                                result = lease.summary()
                            elif operation == 'facts':
                                result = lease.facts(time.monotonic())
                                current = result['authority']
                                result.update(source_commit=p.SOURCE, release_manifest_sha256=p.RELEASE_SHA,
                                    media_sha256=p.MEDIA_SHA, adapter=backend.bundle['binding']['adapter'],
                                    runtime_identity={'boot_id': current['source_proof']['mount_authority']['boot_id'],
                                        **{key: current['process'][key] for key in ('pid', 'start', 'sha256')},
                                        'captured_unix': time.time()},
                                    runtime_profiling={'frames': False, 'presentation': False, 'source_pts': False})
                            elif operation == 'apply':
                                result = lease.apply(time.monotonic())
                            elif operation == 'finish':
                                finished = True; result = {'finishing': True}
                            else:
                                raise ValueError('unknown_or_presentation_output_operation_refused')
                        finally:
                            timer['busy'] = False
                        try:
                            peer.sendall((json.dumps({'success': True, 'result': result}) + '\n').encode())
                        except (BrokenPipeError, ConnectionResetError):
                            # State was journalled before the lost reply. End
                            # this lease and reconcile once; never repeat apply.
                            primary = 'output_reply_lost'; finished = True
        except BaseException as error:
            primary = type(error).__name__
            if setup_phase is not None:
                timer_error(setup_phase, error)
        finally:
            timer['busy'] = False
            timer['phase'] = 'cleanup'
            timer['deadline'] = min(lease.deadline + p.CLEANUP_RESERVE, time.monotonic() + p.CLEANUP_RESERVE)
            result = {'cleanup_success': False, 'cleanup_attempted': False}
            try:
                try:
                    if alarm_installed:
                        signal.setitimer(signal.ITIMER_REAL, max(.001, timer['deadline'] - time.monotonic()))
                    else:
                        timer_error('cleanup_guard_absent', RuntimeError())
                except BaseException as error:
                    timer_error('cleanup_arm', error)
            finally:
                # Setup failure never skips the one owned restoration attempt.
                # A failed native restore is never retried by this finalizer.
                try:
                    result['cleanup_attempted'] = True
                    timer['busy'] = True
                    result['restoration'] = lease.cleanup()
                    result['cleanup_success'] = True
                except BaseException as error:
                    result['failure_class'] = type(error).__name__
                finally:
                    # The private standalone guardian exits immediately after
                    # this receipt. Retain its inactive handlers until exit:
                    # even a pending alarm or failed cancellation cannot call
                    # an old handler between cancellation and receipt writing.
                    timer['busy'] = False
                    try:
                        signal.setitimer(signal.ITIMER_REAL, 0)
                    except BaseException as error:
                        timer_error('final_cancel', error)
                    finally:
                        private(backend.directory / 'finished.json', {**result,
                            'cleanup_success': result['cleanup_success'] and not timer_errors,
                            'timer_errors': timer_errors, 'handlers_retained_inactive_until_process_exit': True,
                            'primary_failure_class': primary,
                            'expired': time.monotonic() >= lease.deadline - p.CLEANUP_RESERVE,
                            'bundle_sha256': digest, 'measured': False, 'native_or_performance_accepted': False})


def request(directory, digest, operation):
    directory, bundle = context(directory, digest)
    require(operation in ('status', 'facts', 'apply', 'finish'), 'resource_only_output_operation_required')
    if (directory / 'finished.json').exists():
        require(operation != 'apply', 'finished_output_lease_is_never_restarted')
        result = json.loads(read(directory / 'finished.json'))
        require(result['bundle_sha256'] == digest, 'exact_finished_output_receipt_required')
        return {'finished': result}
    if not (directory / 'ready.json').exists():
        return {'ready': False}
    ready = json.loads(read(directory / 'ready.json'))
    require(ready['bundle_sha256'] == digest, 'exact_ready_output_receipt_required')
    units = module('owned_application_units', directory / 'owned_application_units.py')
    unit = 'lucent-output-4k-' + bundle['tag'] + '.service'
    argv = ['/usr/bin/python3', '-I', str(directory / 'runtime_output_guard.py'), 'serve', str(directory), digest]
    units.assert_unit_origin(units.unit_snapshot(unit), unit, argv, ready['guardian'])
    if operation == 'status':
        # Status is entirely file/unit observation, including while the server
        # is in its final cleanup and no longer accepts socket requests.
        journals = sorted(directory.glob('journal-[0-9][0-9][0-9][0-9].json'))
        require(1 <= len(journals) <= 32, 'bounded_existing_output_journal_required')
        latest = json.loads(read(journals[-1]))
        require(latest['bundle_sha256'] == digest, 'exact_existing_output_journal_required')
        return {'ready': True, 'status': {'phase': latest['phase'], 'deadline': ready['deadline'],
                                         'live_guardian': True, 'read_only': True}}
    path = directory / 'control.sock'; before = path.lstat()
    require(stat.S_ISSOCK(before.st_mode) and before.st_uid == os.getuid()
            and stat.S_IMODE(before.st_mode) == 0o600, 'owned_output_guard_socket_required')
    with socket.socket(socket.AF_UNIX) as peer:
        peer.settimeout(240)
        peer.connect(str(path))
        require(struct.unpack('3i', peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[:2]
                == (ready['pid'], os.getuid()), 'original_output_guard_peer_required')
        units.assert_unit_origin(units.unit_snapshot(unit), unit, argv, ready['guardian'])
        peer.sendall((json.dumps({'operation': operation, 'bundle_sha256': digest}) + '\n').encode())
        raw = bytearray(); deadline = time.monotonic() + 240
        while b'\n' not in raw:
            require(len(raw) <= 4 * 1024 * 1024 and time.monotonic() < deadline, 'output_guard_reply_bound')
            value = peer.recv(65536)
            require(value, 'output_guard_reply_lost')
            raw.extend(value)
        require(identity(path.lstat()) == identity(before), 'output_guard_socket_replaced')
        return {'ready': True, 'reply': json.loads(raw)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', nargs='?', choices=('serve', 'status', 'facts', 'apply', 'finish'))
    parser.add_argument('directory', nargs='?', type=Path)
    parser.add_argument('bundle_sha256', nargs='?')
    args = parser.parse_args()
    if args.operation is None:
        print(json.dumps({'guest_actions': False, 'resource_only': True,
                          'presentation_supported': False, 'native_or_performance_accepted': False}))
        return
    if args.operation == 'serve':
        serve(args.directory, args.bundle_sha256)
    else:
        print(json.dumps(request(args.directory, args.bundle_sha256, args.operation)))


if __name__ == '__main__':
    main()
