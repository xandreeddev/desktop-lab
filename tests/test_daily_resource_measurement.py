"""Measurement policy must distinguish compositor timestamps from submissions."""
import runpy
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
# The measurement module imports the VM inventory helper but performs no guest
# operation until main(). Keep VM paths available without invoking the harness.
import sys
sys.path.insert(0, str(ROOT/'vm'))
measurement = runpy.run_path(str(ROOT/'vm/measure-daily-resources.py'), run_name='measurement_policy')


class PresentationMeasurement(unittest.TestCase):
    def sample(self, frame, nanos, clock=1, flags=1):
        return {'frame': frame, 'presented_ns': nanos, 'clock_id': clock, 'flags': flags}

    def test_absent_feedback_never_becomes_fps_from_a_submission_counter(self):
        result = measurement['presentation_intervals']([])
        self.assertIsNone(result['fps'])
        self.assertEqual(result['intervals'], 0)

    def test_cadence_uses_protocol_clock_and_preserves_hardware_limits(self):
        values = [self.sample(i, i*16_666_667) for i in range(10)]
        result = measurement['presentation_intervals'](values)
        self.assertAlmostEqual(result['fps'], 60, places=4)
        self.assertTrue(result['all_vsync_flag'])
        self.assertFalse(result['all_hw_clock_flag'])
        self.assertFalse(result['all_hw_completion_flag'])
        self.assertEqual(result['intervals'], 9)

    def test_clock_changes_and_repeated_timestamps_do_not_invent_intervals(self):
        values = [self.sample(1, 500, 1), self.sample(2, 500, 1),
                  self.sample(3, 100, 2), self.sample(4, 20_000_100, 2)]
        result = measurement['presentation_intervals'](values)
        self.assertEqual(result['clock_changes'], 1)
        self.assertEqual(result['nonincreasing'], 1)
        self.assertEqual(result['intervals'], 1)
        self.assertAlmostEqual(result['fps'], 50)

    def test_host_collection_cap_is_explicit_and_never_confused_with_discarded_commits(self):
        storage = {}
        values = [dict(self.sample(i, i*16_666_667), requested_at_ms=i*16.) for i in range(8)]
        snapshot = {'surfaces': [{'id': 'background', 'presentation': {'samples': values}}]}
        self.assertTrue(measurement['collect_presentations'](storage, snapshot, limit=3))
        self.assertEqual(len(storage['background']), 3)
        self.assertEqual(measurement['presentation_intervals'](list(storage['background'].values()))['intervals'], 2)

    def test_embedded_guest_sampler_is_valid_python(self):
        compile(measurement['SAMPLER'], '<guest-resource-sampler>', 'exec')


if __name__ == '__main__':
    unittest.main()
