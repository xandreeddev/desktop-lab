"""The login wallpaper is a narrow copy, never access to the user's home."""
import importlib.util
from pathlib import Path
import stat
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]

class SessionWallpaper(unittest.TestCase):
    def test_copy_tracks_selected_link_preserves_permissions_and_keeps_last_good_file(self):
        spec=importlib.util.spec_from_file_location('wallpaper',ROOT/'scripts/lucent-wallpaper.py')
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);home=root/'private';home.mkdir(mode=0o700)
            destination=root/'published';destination.mkdir()
            first=home/'first';first.write_bytes(b'first image')
            second=home/'second';second.write_bytes(b'next image')
            selected=home/'background';selected.symlink_to(first)
            module.publish(selected,destination)
            result=destination/'wallpaper'
            self.assertFalse(result.is_symlink())
            self.assertEqual(stat.S_IMODE(result.stat().st_mode),0o640)
            self.assertEqual(stat.S_IMODE(home.stat().st_mode),0o700)
            previous=result.stat().st_mtime_ns
            module.publish(selected,destination)
            self.assertEqual(result.stat().st_mtime_ns,previous)
            selected.unlink();selected.symlink_to(second)
            module.publish(selected,destination)
            self.assertEqual(result.read_bytes(),b'next image')
            second.unlink()
            with self.assertRaises(FileNotFoundError):module.publish(selected,destination)
            self.assertEqual(result.read_bytes(),b'next image')
