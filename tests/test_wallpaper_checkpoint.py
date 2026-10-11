"""A test rollback must survive the same snapshot pruning as ordinary use."""
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('checkpoint', Path(__file__).resolve().parents[1] / 'vm/wallpaper_checkpoint.py')
checkpoint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checkpoint)


class WallpaperCheckpoint(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.state = self.root / 'state'
        self.directory = self.state / 'wallpaper-assets/asset-fixture'
        self.directory.mkdir(parents=True)
        self.image = self.directory / 'image.png'
        self.image.write_bytes(b'immutable selected image')
        self.image.chmod(0o400)
        self.asset = {'path': str(self.image), 'source': {'path': str(self.root / 'original.png'), 'name': 'Original'}}
        (self.directory / 'source.json').write_text(json.dumps(self.asset))

    def create(self):
        return checkpoint.create_checkpoint(self.state, self.asset, self.root)

    def test_pruned_snapshot_restores_pixels_metadata_and_read_only_mode(self):
        backup = self.create()
        self.assertEqual(Path(backup).stat().st_mode & 0o777, 0o700)
        shutil.rmtree(self.directory)
        checkpoint.restore_checkpoint(self.state, backup)
        self.assertEqual(self.image.read_bytes(), b'immutable selected image')
        self.assertEqual(json.loads((self.directory / 'source.json').read_text()), self.asset)
        self.assertEqual(self.image.stat().st_mode & 0o777, 0o400)
        checkpoint.discard_checkpoint(self.state, backup)
        self.assertFalse(Path(backup).exists())
        self.assertTrue(self.image.exists())

    def test_present_snapshot_is_not_rewritten(self):
        backup = self.create()
        timestamp = self.image.stat().st_mtime_ns
        checkpoint.restore_checkpoint(self.state, backup)
        self.assertEqual(self.image.stat().st_mtime_ns, timestamp)

    def test_independent_change_preserves_both_target_and_checkpoint(self):
        backup = self.create()
        self.image.chmod(0o600)
        self.image.write_bytes(b'changed independently')
        with self.assertRaisesRegex(ValueError, 'independently changed'):
            checkpoint.restore_checkpoint(self.state, backup)
        self.assertEqual(self.image.read_bytes(), b'changed independently')
        self.assertTrue(Path(backup).exists())

    def test_corrupt_checkpoint_cannot_restore(self):
        backup = self.create()
        shutil.rmtree(self.directory)
        stored = Path(backup) / 'image'
        stored.chmod(0o600)
        stored.write_bytes(b'changed checkpoint')
        with self.assertRaisesRegex(ValueError, 'content changed'):
            checkpoint.restore_checkpoint(self.state, backup)
        self.assertFalse(self.directory.exists())

    def test_legacy_originals_are_outside_pruning_and_need_no_checkpoint(self):
        original = self.root / 'user-original.png'
        original.write_bytes(b'original')
        self.asset['path'] = str(original)
        self.assertIsNone(self.create())
        checkpoint.restore_checkpoint(self.state, None)
        checkpoint.discard_checkpoint(self.state, None)
        self.assertEqual(original.read_bytes(), b'original')

    def test_symlink_replacement_cannot_redirect_restore(self):
        backup = self.create()
        shutil.rmtree(self.directory)
        elsewhere = self.root / 'elsewhere'
        elsewhere.mkdir()
        self.directory.symlink_to(elsewhere, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'Invalid owned'):
            checkpoint.restore_checkpoint(self.state, backup)
        self.assertEqual(list(elsewhere.iterdir()), [])
