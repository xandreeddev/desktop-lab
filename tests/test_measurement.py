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


class FrameProfiling(unittest.TestCase):
    def setUp(self):
        self.harness = runpy.run_path(str(Path(__file__).resolve().parents[1]/'vm/measure-framework.py'))

    def test_runtime_override_is_removed_on_success_and_failure_and_disabled_is_inert(self):
        import ast
        for enabled in (False, True):
            for fail in (False, True):
                with self.subTest(enabled=enabled, fail=fail):
                    remotes, sessions = [], []
                    def remote(*args):
                        remotes.append(args)
                        return ''
                    def session(*args):
                        sessions.append(args)
                        return ''
                    try:
                        with self.harness['frame_profiling'](remote, session, enabled):
                            if fail:
                                raise RuntimeError('measurement failed')
                    except RuntimeError:
                        self.assertTrue(fail)
                    if enabled:
                        self.assertEqual(len(remotes), 2)
                        self.assertEqual(remotes[0][-1], remotes[1][-1])
                        self.assertIn('p.unlink(missing_ok=True)', remotes[1][-2])
                        code = ast.parse(remotes[0][-2])
                        body = code.body[-1].value.args[0].value
                        self.assertEqual(body, '[Service]\nEnvironment=LUCENT_PROFILE_FRAMES=1\n')
                        self.assertEqual(sessions, [('systemctl', '--user', 'daemon-reload'),
                                                    ('systemctl', '--user', 'daemon-reload'),
                                                    ('systemctl', '--user', 'restart', 'lucent.service')])
                    else:
                        self.assertEqual(remotes, [])
                        self.assertEqual(sessions, [])

    def test_summary_filters_old_frames_and_keeps_nested_scope_separate(self):
        def frame(stamp, duration, misses):
            return {'submitted_ms': stamp, 'total_ms': duration, 'stages_ms': {'prepare': duration-1},
                    'prepare_breakdown_ms': {'text_raster': duration-2}, 'resources': {'text_misses': misses}}
        summary = self.harness['summarize_profiles']({'frame_profiles': [frame(0, 90, 80), frame(10, 6, 3), frame(20, 4, 0)]}, 10)
        self.assertEqual(summary['frames'], 2)
        self.assertEqual(summary['total_ms'], {'median': 5, 'max': 6})
        self.assertEqual(summary['stages_ms']['prepare'], {'median': 4, 'max': 5})
        self.assertEqual(summary['prepare_breakdown_ms']['text_raster']['max'], 4)
        self.assertEqual(summary['resources_total']['text_misses'], 3)
        self.assertEqual(summary['slowest']['submitted_ms'], 10)
        self.assertIsNone(self.harness['summarize_profiles']({}, 10))

    def test_resident_snapshot_contains_only_numeric_memory_fields(self):
        snapshot = self.harness['resident_breakdown'](lambda *_: 'address-range private-path\nRss: 123 kB\nPss_Anon: 50 kB\nPrivate_Dirty: 25 kB\nUnknown: 99 kB\n', 42)
        self.assertEqual(snapshot, {'Rss': 123, 'Pss_Anon': 50, 'Private_Dirty': 25})
