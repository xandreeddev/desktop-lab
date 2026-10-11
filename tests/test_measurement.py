"""The measurement harness must not leave idle disabled after a failed run."""
from pathlib import Path
import runpy
import unittest

stay_awake = runpy.run_path(str(Path(__file__).resolve().parents[1]/'vm/measure-framework.py'))['stay_awake']


class IdleRestoration(unittest.TestCase):
    def test_previous_preference_is_preserved_on_success_and_failure(self):
        for previous in (False, True):
            for fail in (False, True):
                with self.subTest(previous=previous, fail=fail):
                    enabled = previous

                    def remote(*command):
                        nonlocal enabled
                        script = command[-1]
                        if 'print(int(p.exists()))' in script:
                            return str(int(enabled))
                        if 'p.touch()' in script:
                            enabled = True
                        if 'p.unlink(' in script:
                            enabled = False
                        return ''

                    try:
                        with stay_awake(remote):
                            self.assertTrue(enabled)
                            if fail:
                                raise RuntimeError('measurement failed')
                    except RuntimeError:
                        self.assertTrue(fail)
                    self.assertEqual(enabled, previous)
