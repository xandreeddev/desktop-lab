"""Owned local temporary files only; no SSH, browser, VM or retention sweep."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import tempfile
import types
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('fixture_scratch', ROOT / 'vm/owned_fixture_scratch.py')
scratch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scratch)


class FixtureScratchTests(unittest.TestCase):
    def setUp(self):
        self.guest = {'__name__': 'source_only_fixture_allocator'}
        exec(compile(scratch.PROGRAM, 'fixture-scratch-guest', 'exec'), self.guest)
        self.temporary = self.enterContext(tempfile.TemporaryDirectory())
        self.home = Path(self.temporary)
        self.enterContext(patch.object(self.guest['Path'], 'home', return_value=self.home))

    def disk(self):
        return patch.dict(self.guest, disk=Mock(return_value=(0xef53, 2 * scratch.MIN_FREE_BYTES)))

    def test_new_private_disk_path_is_random_and_does_not_reuse_or_remove_old_fixture(self):
        with self.disk():
            first = self.guest['allocate']('lucent-application-intent')
            old = Path(first['path']); marker = old / 'retained'; marker.write_bytes(b'keep')
            second = self.guest['allocate']('lucent-application-intent')
        new = Path(second['path'])
        self.assertNotEqual(old, new)
        self.assertEqual(new.parent, self.home / '.cache/lucent-acceptance')
        self.assertRegex(new.name, r'^lucent-application-intent\.[0-9a-f]{12}$')
        self.assertEqual(stat.S_IMODE(new.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(new.parent.stat().st_mode), 0o700)
        self.assertEqual(list(new.iterdir()), [])
        self.assertEqual(marker.read_bytes(), b'keep')

    def test_wrong_prefix_is_refused_before_local_creation_or_transport(self):
        for prefix in ('other', 'lucent-gui-clipboard/../other', '', None):
            with self.assertRaisesRegex(ValueError, 'known_fixture_prefix_required'):
                self.guest['allocate'](prefix)
            remote = Mock()
            with self.assertRaisesRegex(ValueError, 'known_fixture_prefix_required'):
                scratch.allocate(remote, prefix)
            remote.assert_not_called()
        self.assertEqual(list(self.home.iterdir()), [])

    def test_symlinked_home_or_cache_parent_is_refused(self):
        other = self.home / 'other'; other.mkdir()
        linked = self.home / 'linked'; linked.symlink_to(other, target_is_directory=True)
        with patch.object(self.guest['Path'], 'home', return_value=linked), self.disk():
            with self.assertRaisesRegex(ValueError, 'canonical_fixture_home_required'):
                self.guest['allocate']('lucent-gui-clipboard')
        (self.home / '.cache').symlink_to(other, target_is_directory=True)
        with self.disk(), self.assertRaises(OSError):
            self.guest['allocate']('lucent-gui-clipboard')
        self.assertEqual(list(other.iterdir()), [])

    def test_symlinked_private_parent_is_refused_without_following_it(self):
        cache = self.home / '.cache'; cache.mkdir()
        other = self.home / 'other'; other.mkdir()
        (cache / 'lucent-acceptance').symlink_to(other, target_is_directory=True)
        with self.disk(), self.assertRaises(OSError):
            self.guest['allocate']('lucent-gui-clipboard')
        self.assertEqual(list(other.iterdir()), [])

    def test_foreign_home_owner_and_unsafe_existing_permissions_are_refused(self):
        uid = os.getuid()
        with patch.object(self.guest['os'], 'getuid', return_value=uid + 1), self.disk():
            with self.assertRaisesRegex(ValueError, 'owned_safe_fixture_parent_required'):
                self.guest['allocate']('lucent-gui-clipboard')
        self.assertEqual(list(self.home.iterdir()), [])
        cache = self.home / '.cache'; cache.mkdir(mode=0o700); cache.chmod(0o777)
        with self.disk(), self.assertRaisesRegex(ValueError, 'owned_safe_fixture_parent_required'):
            self.guest['allocate']('lucent-gui-clipboard')
        cache.chmod(0o700)
        private = cache / 'lucent-acceptance'; private.mkdir(mode=0o755); private.chmod(0o755)
        with self.disk(), self.assertRaisesRegex(ValueError, 'owned_safe_fixture_parent_required'):
            self.guest['allocate']('lucent-gui-clipboard')
        self.assertEqual(list(private.iterdir()), [])

    def test_tmpfs_ramfs_and_insufficient_space_refuse_before_new_fixture(self):
        for kind in self.guest['RAM_FILESYSTEMS']:
            with patch.dict(self.guest, filesystem_type=Mock(return_value=kind)):
                with self.assertRaisesRegex(ValueError, 'disk_backed_fixture_storage_required'):
                    self.guest['allocate']('lucent-gui-clipboard')
        space = types.SimpleNamespace(f_bavail=255, f_frsize=4096)
        with patch.dict(self.guest, filesystem_type=Mock(return_value=0xef53)), \
                patch.object(self.guest['os'], 'fstatvfs', return_value=space):
            with self.assertRaisesRegex(ValueError, 'fixture_storage_reserve_required'):
                self.guest['allocate']('lucent-gui-clipboard')
        self.assertEqual(list((self.home / '.cache').iterdir()), [])

    def test_collision_retries_are_bounded_and_never_reuse_existing_child(self):
        with self.disk(), patch.object(self.guest['secrets'], 'token_hex', return_value='a' * 12):
            old = Path(self.guest['allocate']('lucent-gui-clipboard')['path'])
            marker = old / 'retained'; marker.write_bytes(b'keep')
            with self.assertRaisesRegex(ValueError, 'fixture_directory_collision_bound'):
                self.guest['allocate']('lucent-gui-clipboard')
        self.assertEqual(marker.read_bytes(), b'keep')

    def test_fixed_remote_program_stdin_and_reply_are_bound(self):
        reply = {'path': '/home/omarchy/.cache/lucent-acceptance/lucent-gui-clipboard.' + 'a' * 12,
                 'filesystem_type': 0xef53, 'available_bytes': scratch.MIN_FREE_BYTES}
        remote = Mock(return_value=json.dumps(reply).encode())
        self.assertEqual(scratch.allocate(remote, 'lucent-gui-clipboard'), reply['path'])
        args, kwargs = remote.call_args
        self.assertEqual(args, ('/usr/bin/python3', '-I', '-c', scratch.PROGRAM))
        self.assertEqual(json.loads(kwargs['data']), {'prefix': 'lucent-gui-clipboard'})
        self.assertEqual(kwargs['timeout'], 15)
        self.assertEqual(scratch.PROGRAM_SHA256, hashlib.sha256(args[3].encode()).hexdigest())
        for change in ({'path': '/tmp/lucent-gui-clipboard.' + 'a' * 12},
                       {'filesystem_type': 0x01021994}, {'available_bytes': 0}):
            remote.return_value = json.dumps(dict(reply, **change))
            with self.assertRaisesRegex(ValueError, 'private_disk_fixture_directory_required'):
                scratch.allocate(remote, 'lucent-gui-clipboard')


if __name__ == '__main__':
    unittest.main()
