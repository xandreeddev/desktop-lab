"""Real owned-file transactions with a fake supervised service; no native IO."""
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from lucent_dev import deploy, model


def executable(label):
    value = bytearray(64)
    value[:7] = b'\x7fELF\x02\x01\x01'
    value[16:18] = (3).to_bytes(2, 'little')
    value[18:20] = {'x86_64': 62, 'aarch64': 183}[platform.machine()].to_bytes(2, 'little')
    return bytes(value) + label.encode()


def put(path, value, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(value)
    path.chmod(mode)


class Supervised:
    """Only this temp home's files and this in-memory service may be changed."""
    def __init__(self, home):
        self.home = home
        self.service_state = {'ActiveState': 'active', 'SubState': 'running', 'MainPID': '10', 'ControlPID': '0',
                              'ControlGroup': '/owned/lucent.service', 'InvocationID': 'a' * 32,
                              'NRestarts': '0', 'ExecMainStartTimestampMonotonic': '1'}
        self.events = []
        self.readiness_hook = None
        self.admissions = 0
        self.deferred_at = None
        self.stop_reply_lost = False
        self.start_reply_lost = False
        self.fast_bad_candidate = False
        self.original_apps = ['ordinary-editor-with-unsaved-content']

    def service(self):
        return dict(self.service_state)

    def admission(self):
        self.admissions += 1
        if self.admissions == self.deferred_at:
            raise deploy.Deferred('screen locked')
        return {'service': self.service(), 'peer': {'pid': 11, 'sha256': self.original_sha}}

    def protected(self):
        return {'settings': deploy._file(self.home / '.local/state/lucent/desktop.json')}

    def preserved(self, before):
        current = self.protected()
        for name, value in before.items():
            deploy.require(current[name]['sha256'] == value['sha256'], 'Protected settings changed')

    def stop(self, expected, *, drain_group=None):
        deploy.require(self.service() == expected, 'foreign invocation')
        deploy.require((drain_group or expected['ControlGroup']) == '/owned/lucent.service', 'wrong drain group')
        self.events.append('stop')
        self.service_state.update(ActiveState='inactive', SubState='dead', MainPID='0')
        if self.stop_reply_lost:
            self.stop_reply_lost = False
            raise deploy.DeployError('lost stop reply')

    def quiescent(self, group):
        deploy.require(group == '/owned/lucent.service'
                       and self.service_state['ActiveState'] == 'inactive', 'not stopped')

    def start(self):
        self.events.append('start')
        self.service_state.update(ActiveState='active', SubState='running', MainPID='20', InvocationID='b' * 32,
                                  ControlGroup='/owned/lucent.service',
                                  ExecMainStartTimestampMonotonic=str(time.monotonic_ns() // 1000))
        if self.fast_bad_candidate:
            self.fast_bad_candidate = False
            self.service_state.update(ActiveState='activating', SubState='auto-restart', MainPID='0')
        if self.start_reply_lost:
            self.start_reply_lost = False
            raise deploy.DeployError('lost start reply')
        return self.service()

    def drained(self, group):
        return group == '/owned/lucent.service'

    def ready(self, sha, started, timeout):
        self.events.append('ready')
        if self.readiness_hook:
            hook, self.readiness_hook = self.readiness_hook, None
            hook()
        deploy.require(self.service_state['ActiveState'] == 'active', 'candidate is pending automatic restart')
        deploy.require(deploy._file(self.home / '.local/bin/lucent-desktop')['sha256'] == sha,
                       'wrong restored cohort')
        return {'service': self.service(), 'peer': {'pid': 21}}


class ReloadTransaction(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / 'home'
        self.home.mkdir()
        self.bundle = self.root / 'bundle'
        self.bundle.mkdir(mode=0o700)
        put(self.home / '.local/state/lucent/desktop.json', b'{"notes":"private user data"}\n', 0o600)
        self.manifest = {'format': 1, 'source_commit': 'a' * 40, 'source_dirty': True,
                         'profile': 'release', 'binaries': {}, 'support_files': {}}
        self.targets = []
        for name in model.BINARY_NAMES:
            target = self.home / '.local/bin' / name
            source = self.bundle / name
            put(target, executable('old-' + name), 0o755)
            put(source, executable('new-' + name), 0o755)
            self.targets.append(target)
            self.manifest['binaries'][name] = model.file_digest(source)
        for name in model.SUPPORT_FILES:
            target = self.home / '.local/lib/lucent' / name.removeprefix('configs/lucent/')
            source = self.bundle / 'support' / name
            put(target, ('old-' + name).encode())
            put(source, ('new-' + name).encode())
            self.targets.append(target)
            self.manifest['support_files'][name] = model.file_digest(source)
        put(self.bundle / 'manifest.json', json.dumps(self.manifest).encode(), 0o600)
        self.original = {path: path.read_bytes() for path in self.targets}
        self.session = Supervised(self.home)
        self.session.original_sha = hashlib.sha256(self.original[self.targets[0]]).hexdigest()

    def apply(self):
        with patch.object(deploy, '_Session', return_value=self.session):
            return deploy.apply_bundle(self.bundle)

    def journal(self):
        paths = list((self.home / '.local/state/lucent/dev-reload').glob('*/journal.json'))
        self.assertEqual(len(paths), 1)
        return json.loads(paths[0].read_text())

    def unchanged(self):
        self.assertEqual({path: path.read_bytes() for path in self.targets}, self.original)

    def test_atomic_publication_keeps_existing_reader_and_ordinary_apps(self):
        old_inode = self.targets[0].stat().st_ino
        with self.targets[0].open('rb') as old_reader:
            result = self.apply()
            self.assertEqual(old_reader.read(), self.original[self.targets[0]])
        self.assertNotEqual(self.targets[0].stat().st_ino, old_inode)
        self.assertEqual(self.targets[0].read_bytes(), executable('new-lucent-desktop'))
        self.assertTrue(result['success'])
        self.assertEqual(self.session.events, ['stop', 'start', 'ready'])
        self.assertEqual(self.session.original_apps, ['ordinary-editor-with-unsaved-content'])
        self.assertEqual(self.journal()['phase'], 'complete')

    def test_failed_readiness_rolls_back_real_files_before_old_restart(self):
        self.session.readiness_hook = lambda: (_ for _ in ()).throw(deploy.DeployError('not rendered'))
        with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
            self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready', 'stop', 'start', 'ready'])
        self.assertTrue(self.journal()['rollback_complete'])

    def test_stop_reply_loss_recovers_a_proven_stopped_original_service(self):
        self.session.stop_reply_lost = True
        with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
            self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready'])

    def test_lost_start_reply_reconciles_attempt_time_and_exact_installed_cohort(self):
        self.session.start_reply_lost = True
        with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
            self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, ['stop', 'start', 'stop', 'start', 'ready'])
        self.assertEqual(len(self.journal()['candidate_invocations']), 1)

    def test_watchdog_successor_with_increased_restart_counter_can_roll_back(self):
        def restarted():
            self.session.service_state.update(InvocationID='c' * 32, NRestarts='1', MainPID='30')
            raise deploy.DeployError('watchdog restart')
        self.session.readiness_hook = restarted
        with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
            self.apply()
        self.unchanged()
        self.assertEqual([p['restarts'] for p in self.journal()['candidate_invocations']], [0, 1])

    def test_same_cohort_manual_successor_without_restart_progress_is_not_stopped(self):
        def restarted():
            self.session.service_state.update(InvocationID='c' * 32, MainPID='30')
            raise deploy.DeployError('independent restart')
        self.session.readiness_hook = restarted
        with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
            self.apply()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready'])
        self.assertEqual(self.session.service()['InvocationID'], 'c' * 32)

    def test_owned_fast_failure_auto_restart_pending_can_be_cancelled_and_rolled_back(self):
        self.session.fast_bad_candidate = True
        with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
            self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready', 'stop', 'start', 'ready'])

    def test_empty_pending_group_uses_original_captured_group_for_rollback(self):
        def pending():
            self.session.service_state.update(ActiveState='activating', SubState='auto-restart',
                                              MainPID='0', ControlGroup='')
            raise deploy.DeployError('candidate watchdog ended')
        self.session.readiness_hook = pending
        with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
            self.apply()
        self.unchanged()
        self.assertEqual(self.journal()['error_message'], 'candidate watchdog ended')
        self.assertEqual(self.journal()['failure_phase'], 'starting')
        self.assertEqual(self.journal()['rollback_step'], 'complete')

    def test_unrelated_activating_substate_is_not_authority_for_rollback_stop(self):
        def unrelated():
            self.session.service_state.update(ActiveState='activating', SubState='start-pre', MainPID='0')
            raise deploy.DeployError('new activation')
        self.session.readiness_hook = unrelated
        with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
            self.apply()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready'])

    def test_deferred_admission_after_staging_never_stops_the_service(self):
        self.session.deferred_at = 2
        with self.assertRaises(deploy.Deferred):
            self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, [])
        self.assertTrue(self.journal()['rollback_complete'])
        self.assertFalse(list((self.home / '.local/bin').glob('.lucent-dev-*')))

    def test_foreign_target_edit_blocks_rollback_and_old_start(self):
        def changed():
            foreign = self.targets[0].with_suffix('.independent')
            put(foreign, executable('independent change'), 0o755)
            os.replace(foreign, self.targets[0])
            raise deploy.DeployError('readiness failed')
        self.session.readiness_hook = changed
        with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
            self.apply()
        self.assertEqual(self.targets[0].read_bytes(), executable('independent change'))
        self.assertEqual(self.session.events, ['stop', 'start', 'ready'])
        self.assertFalse(self.journal()['rollback_complete'])
        self.assertIn('Independent file edit', self.journal()['rollback_error_message'])
        self.assertEqual(self.journal()['rollback_step'], 'inspect-service')

    def test_real_target_rename_in_check_exchange_gap_is_preserved(self):
        exchange = deploy._rename_exchange
        injected = False
        foreign_inode = None
        def racing(left, right):
            nonlocal injected, foreign_inode
            if not injected:
                injected = True
                foreign = right.with_suffix('.independent')
                put(foreign, executable('concurrent installer'), 0o755)
                foreign_inode = foreign.stat().st_ino
                os.replace(foreign, right)
            exchange(left, right)
        with patch.object(deploy, '_rename_exchange', side_effect=racing):
            with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
                self.apply()
        self.assertEqual(self.targets[0].read_bytes(), executable('concurrent installer'))
        self.assertEqual(self.targets[0].stat().st_ino, foreign_inode)
        self.assertEqual(self.session.events, ['stop'])

    def test_real_restore_exchange_gap_preserves_independent_target(self):
        exchange = deploy._rename_exchange
        inject = False
        def fail_ready():
            nonlocal inject
            inject = True
            raise deploy.DeployError('not rendered')
        def racing(left, right):
            nonlocal inject
            if inject:
                inject = False
                foreign = right.with_suffix('.independent')
                put(foreign, b'independent support change')
                os.replace(foreign, right)
            exchange(left, right)
        self.session.readiness_hook = fail_ready
        with patch.object(deploy, '_rename_exchange', side_effect=racing):
            with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
                self.apply()
        self.assertEqual(self.targets[-1].read_bytes(), b'independent support change')
        self.assertEqual(self.session.events, ['stop', 'start', 'ready', 'stop'])

    def test_changed_backup_is_never_published(self):
        def changed():
            item = self.journal()['files'][-1]
            Path(item['backup']).write_bytes(b'independent backup change')
            raise deploy.DeployError('readiness failed')
        self.session.readiness_hook = changed
        with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
            self.apply()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready', 'stop'])
        self.assertEqual(self.session.service()['ActiveState'], 'inactive')

    def test_protected_preferences_are_never_restored_over_a_change(self):
        def changed():
            put(self.home / '.local/state/lucent/desktop.json', b'independent user settings', 0o600)
            raise deploy.DeployError('readiness failed')
        self.session.readiness_hook = changed
        with self.assertRaisesRegex(deploy.DeployError, 'recovery-required'):
            self.apply()
        self.assertEqual((self.home / '.local/state/lucent/desktop.json').read_bytes(), b'independent user settings')

    def test_catchable_signal_enters_guarded_rollback_and_restores_handler(self):
        previous = signal.getsignal(signal.SIGTERM)
        self.session.readiness_hook = lambda: signal.raise_signal(signal.SIGTERM)
        with self.assertRaisesRegex(deploy.Interrupted, 'rolled-back') as caught:
            self.apply()
        self.unchanged()
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous)
        self.assertEqual(caught.exception.signum, signal.SIGTERM)
        self.assertEqual(caught.exception.recovery_phase, 'rolled-back')
        self.assertTrue(Path(caught.exception.journal).is_file())

    def test_control_c_is_rethrown_after_rollback_for_the_cli_to_exit(self):
        self.session.readiness_hook = lambda: signal.raise_signal(signal.SIGINT)
        with self.assertRaises(deploy.Interrupted) as caught:
            self.apply()
        self.unchanged()
        self.assertEqual(caught.exception.signum, signal.SIGINT)
        self.assertEqual(caught.exception.recovery_phase, 'rolled-back')

    def test_journal_io_failure_does_not_prevent_in_memory_file_rollback(self):
        real = deploy._journal
        def journal(path, value):
            if value['phase'] != 'staging' and value['phase'] != 'stopping':
                raise OSError('owned filesystem full')
            return real(path, value)
        with patch.object(deploy, '_journal', side_effect=journal):
            with self.assertRaisesRegex(deploy.DeployError, 'rolled-back'):
                self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, ['stop', 'start', 'ready'])

    def test_non_elf_payload_rejected_before_any_stop(self):
        source = self.bundle / 'lucent-menu'
        put(source, b'not an executable', 0o755)
        self.manifest['binaries']['lucent-menu'] = model.file_digest(source)
        put(self.bundle / 'manifest.json', json.dumps(self.manifest).encode(), 0o600)
        with self.assertRaisesRegex(deploy.DeployError, 'ELF'):
            self.apply()
        self.unchanged()
        self.assertEqual(self.session.events, [])

    def test_symlink_installed_target_refused_before_stop(self):
        value = self.targets[-1].read_bytes()
        referent = self.root / 'unrelated'
        put(referent, value)
        self.targets[-1].unlink()
        self.targets[-1].symlink_to(referent)
        with self.assertRaises(deploy.DeployError):
            self.apply()
        self.assertEqual(referent.read_bytes(), value)
        self.assertEqual(self.session.events, [])

    def test_readiness_timeout_cannot_bypass_startup_survival(self):
        with self.assertRaisesRegex(deploy.DeployError, 'Readiness timeout'):
            with patch.object(deploy, '_Session', return_value=self.session):
                deploy.apply_bundle(self.bundle, ready_timeout=1)
        self.assertEqual(self.session.events, [])


class Readiness(unittest.TestCase):
    def setUp(self):
        self.session = object.__new__(deploy._Session)
        self.service = {'ActiveState': 'active', 'MainPID': '10', 'InvocationID': 'a' * 32,
                        'NRestarts': '0', 'ControlGroup': '/owned/lucent.service'}
        self.peer = {'pid': 11, 'birth': '100', 'sha256': 'b' * 64,
                     'group': '/owned/lucent.service', 'executable': [1, 2]}
        self.state = {'client': {'theme_error': None, 'widget_surfaces': []},
                      'surfaces': [{'id': role, 'frames': 1} for role in ('background', 'bar', 'dock')]}
        self.clock = 0.

    def sleep(self, seconds):
        self.clock += seconds

    def inspect(self, hashed=True):
        return self.state, {key: value for key, value in self.peer.items() if hashed or key != 'sha256'}

    def test_shared_rendered_contract_and_startup_deadline_both_required(self):
        with patch.object(self.session, 'service', return_value=self.service), \
             patch.object(self.session, 'inspect', side_effect=self.inspect), \
             patch.object(deploy.time, 'monotonic', side_effect=lambda: self.clock), \
             patch.object(deploy.time, 'sleep', side_effect=self.sleep):
            value = self.session.ready('b' * 64, 0., 45)
        self.assertGreaterEqual(self.clock, 35)
        self.assertLess(self.clock, 36)
        self.assertEqual(value['peer'], self.peer)

    def test_same_binary_new_native_peer_does_not_satisfy_stability(self):
        count = 0
        def inspect(hashed=True):
            nonlocal count
            count += 1
            if count > 1:
                self.peer['pid'] = 12
            return self.inspect(hashed)
        with patch.object(self.session, 'service', return_value=self.service), \
             patch.object(self.session, 'inspect', side_effect=inspect), \
             patch.object(deploy.time, 'monotonic', side_effect=lambda: self.clock), \
             patch.object(deploy.time, 'sleep', side_effect=self.sleep):
            with self.assertRaisesRegex(deploy.DeployError, 'process changed'):
                self.session.ready('b' * 64, 0., 45)

    def test_theme_or_unrendered_surface_cannot_be_called_ready(self):
        self.state['surfaces'].pop()
        with patch.object(self.session, 'service', return_value=self.service), \
             patch.object(self.session, 'inspect', side_effect=self.inspect), \
             patch.object(deploy.time, 'monotonic', side_effect=lambda: self.clock), \
             patch.object(deploy.time, 'sleep', side_effect=self.sleep):
            with self.assertRaisesRegex(deploy.DeployError, 'startup deadline'):
                self.session.ready('b' * 64, 0., 45)


class CapturedCgroup(unittest.TestCase):
    def test_real_stop_uses_captured_group_when_pending_unit_group_is_empty(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            put(root / 'cgroup.events', b'populated 1\n')
            put(root / 'owned/lucent.service/cgroup.events', b'populated 0\n')
            session = object.__new__(deploy._Session)
            state = {'ActiveState': 'activating', 'SubState': 'auto-restart', 'MainPID': '0',
                     'ControlPID': '0', 'ControlGroup': '', 'InvocationID': 'b' * 32}
            original = dict(state)
            def command(*argv, **_):
                self.assertEqual(argv, ('systemctl', '--user', 'stop', 'lucent.service'))
                state.update(ActiveState='inactive', SubState='dead')
                return ''
            with patch.object(deploy, 'CGROUP_ROOT', root), \
                 patch.object(session, 'service', side_effect=lambda: dict(state)), \
                 patch.object(session, 'command', side_effect=command) as called:
                session.stop(original, drain_group='/owned/lucent.service')
            self.assertEqual(called.call_count, 1)

    def test_empty_or_root_group_is_rejected_before_any_stop_command(self):
        session = object.__new__(deploy._Session)
        with patch.object(session, 'command') as called:
            for group in ('', '/'):
                with self.assertRaisesRegex(deploy.DeployError, 'nonempty captured'):
                    session.stop({'ControlGroup': group})
            called.assert_not_called()
        with patch.object(session, 'service', return_value={'ActiveState': 'inactive', 'MainPID': '0', 'ControlPID': '0'}):
            for group in ('', '/'):
                with self.assertRaisesRegex(deploy.DeployError, 'Invalid captured'):
                    session.quiescent(group)



if __name__ == '__main__':
    unittest.main()
