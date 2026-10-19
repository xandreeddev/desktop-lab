"""S3 prerequisite/proof policy only; no VM, power operation or system bus."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'vm'))
import native_suspend_policy as policy
spec = importlib.util.spec_from_file_location('suspend_inspector', ROOT / 'vm/inspect-native-suspend.py')
inspector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inspector)


def domain(enabled='yes'):
    return policy.domain_facts(('<domain><name>desktop-lab-lucent</name><os>'
        '<type arch="x86_64" machine="pc-q35-11.1">hvm</type></os><features><acpi/></features>'
        '<pm><suspend-to-mem enabled="' + enabled + '"/></pm></domain>').encode())


def guest():
    return {'power_states': ['freeze', 'mem'], 'memory_modes': ['s2idle', 'deep'],
            'selected_memory_mode': 'deep', 'logind_can_suspend': 'yes',
            'sleep_block_inhibitors': 0, 'sleep_delay_inhibitors': 1}


class Prerequisites(unittest.TestCase):
    def test_actual_candidate_shape_explicitly_disables_s3(self):
        result = policy.prerequisites(domain('no'), guest())
        self.assertFalse(result['candidate_only'])
        self.assertIn('domain_s3_advertisement_disabled', result['blockers'])
        self.assertFalse(result['execution_authorized'])

    def test_freeze_or_selected_s2idle_is_not_s3_and_is_never_changed(self):
        value = guest();value.update(power_states=['freeze'], memory_modes=['s2idle'], selected_memory_mode='s2idle')
        result = policy.prerequisites(domain(), value)
        self.assertIn('guest_mem_state_unavailable', result['blockers'])
        self.assertIn('guest_deep_s3_unavailable', result['blockers'])
        self.assertEqual(value['selected_memory_mode'], 's2idle')
        value = guest();value['selected_memory_mode'] = 's2idle'
        self.assertIn('guest_deep_s3_not_selected', policy.prerequisites(domain(), value)['blockers'])

    def test_authorization_or_blocking_inhibitor_refuses_but_delay_is_preserved(self):
        value = guest();value['logind_can_suspend'] = 'challenge'
        self.assertIn('regular_user_suspend_not_authorized_without_prompt', policy.prerequisites(domain(), value)['blockers'])
        value = guest();value['sleep_block_inhibitors'] = 1
        self.assertIn('sleep_block_inhibitor_present', policy.prerequisites(domain(), value)['blockers'])
        candidate = policy.prerequisites(domain(), guest())
        self.assertTrue(candidate['candidate_only'])
        self.assertFalse(candidate['execution_authorized'])
        self.assertIn('effective_systemd_sleep_policy_review', candidate['remaining'])

    def test_selected_mode_is_explicit_unique_and_never_guessed(self):
        self.assertEqual(policy.memory_modes('s2idle [deep]'), (['s2idle', 'deep'], 'deep'))
        self.assertEqual(policy.memory_modes('s2idle'), (['s2idle'], None))
        for value in ('[deep] [s2idle]', 'deep deep', 'hibernate', '[deep'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                policy.memory_modes(value)


class TransitionEvidence(unittest.TestCase):
    before = {'boot_id': 'same-boot', 'boottime_ns': 100_000_000_000, 'monotonic_ns': 90_000_000_000}
    after = {'boot_id': 'same-boot', 'boottime_ns': 107_000_000_000, 'monotonic_ns': 92_000_000_000}

    def test_actual_s3_needs_qmp_suspended_deep_kernel_and_guest_sleep_clock_gap(self):
        result = policy.actual_s3_interval(self.before, self.after, ['SUSPEND', 'WAKEUP'], 'suspended', True)
        self.assertEqual(result['sleep_clock_gap_ns'], 5_000_000_000)
        self.assertTrue(result['preservation_and_secure_lock_still_required'])

    def test_host_pause_or_reboot_or_no_guest_sleep_cannot_qualify(self):
        for events, status, deep, after in ((['STOP', 'RESUME'], 'paused', True, self.after),
                (['SUSPEND', 'WAKEUP'], 'suspended', False, self.after),
                (['SUSPEND', 'WAKEUP'], 'suspended', True, dict(self.after, boot_id='new-boot')),
                (['SUSPEND', 'WAKEUP'], 'suspended', True, dict(self.after, monotonic_ns=97_000_000_000)),
                (['SUSPEND', 'RESET', 'WAKEUP'], 'suspended', True, self.after)):
            with self.subTest(events=events, status=status, deep=deep, after=after), self.assertRaises(ValueError):
                policy.actual_s3_interval(self.before, after, events, status, deep)


class ReadonlyCollector(unittest.TestCase):
    def test_non_vm_is_refused_before_sysfs_or_bus_observation(self):
        with patch.object(inspector.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, 'none\n')), \
             patch.object(inspector, 'bounded') as read:
            with self.assertRaises(ValueError):
                inspector.guest()
        read.assert_not_called()

    def test_inhibitor_private_names_and_reasons_are_not_returned(self):
        manager = Mock()
        manager.CanSuspend.return_value = 'yes'
        manager.ListInhibitors.return_value = [('sleep', 'PRIVATE-NAME', 'PRIVATE-REASON', 'delay', 1000, 123),
                                               ('sleep', 'PRIVATE-NAME', 'PRIVATE-REASON', 'block', 1000, 456)]
        bus = Mock()
        dbus = types.SimpleNamespace(SystemBus=Mock(return_value=bus), Interface=Mock(return_value=manager))
        files = {'/sys/power/state': 'freeze mem', '/sys/power/mem_sleep': 's2idle [deep]',
                 '/proc/sys/kernel/random/boot_id': 'same-boot'}
        with patch.object(inspector.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'kvm\n')) as run, \
             patch.object(inspector, 'bounded', side_effect=lambda path: files[path]), patch.dict(sys.modules, {'dbus': dbus}):
            result = inspector.guest()
        self.assertEqual(run.call_count, 1)
        self.assertEqual(result['sleep_block_inhibitors'], 1)
        self.assertEqual(result['sleep_delay_inhibitors'], 1)
        self.assertNotIn('PRIVATE', json.dumps(result))
        manager.CanSuspend.assert_called_once_with(timeout=3)
        manager.ListInhibitors.assert_called_once_with(timeout=3)
        bus.close.assert_called_once()


if __name__ == '__main__':
    unittest.main()
