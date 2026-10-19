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

    def traced(self, frame, revision):
        value = self.sample(frame, round(frame * 1_000_000_000 / 60))
        value['streams'] = {'revisions': [{'stream_id': 7, 'revision': revision}], 'truncated': False}
        return value

    def test_repeating_one_revision_at_display_sixty_has_no_distinct_advancement(self):
        values = [self.traced(frame, 1) for frame in range(60)]
        result = measurement['stream_cadence'](values, {})['streams'][0]
        self.assertEqual(result['unique_prepared_revisions'], 1)
        self.assertEqual(result['distinct_advancements'], 0)
        self.assertEqual(result['repeated_revision_samples'], 59)
        self.assertEqual(result['presented_distinct_advancements_hz'], 0)
        self.assertAlmostEqual(measurement['presentation_intervals'](values)['fps'], 60, places=4)

    def test_twenty_four_source_frames_remain_twenty_four_unique_at_display_sixty(self):
        values = [self.traced(frame, frame * 24 // 60 + 1) for frame in range(60)]
        sources = {(7, revision): {'stream_id': 7, 'revision': revision, 'generation': 2, 'cycle': 0,
                                  'source_ns': round((revision - 1) * 1_000_000_000 / 24)}
                   for revision in range(1, 25)}
        result = measurement['stream_cadence'](values, sources)['streams'][0]
        self.assertEqual(result['unique_prepared_revisions'], 24)
        self.assertEqual(result['repeated_revision_samples'], 36)
        self.assertEqual(result['prepared_revision_gaps_between_presentations'], 0)
        self.assertEqual(result['joined_source_revisions'], 24)
        self.assertAlmostEqual(result['observed_prepared_source_pts_hz'], 24, places=4)

    def test_alias_ambiguity_and_incomplete_trace_cannot_invent_single_revision_cadence(self):
        value = self.traced(1, 1)
        value['streams']['revisions'].append({'stream_id': 7, 'revision': 2})
        value['streams']['truncated'] = True
        result = measurement['stream_cadence']([value, self.sample(2, 100)], {})
        self.assertEqual(result['truncated_trace_samples'], 1)
        self.assertEqual(result['missing_trace_samples'], 1)
        self.assertEqual(result['streams'][0]['ambiguous_multi_revision_samples'], 1)
        self.assertIsNone(result['streams'][0]['presented_distinct_advancements_hz'])

    def test_source_associations_are_bounded_and_immutable(self):
        samples = [{'stream_id': 7, 'revision': revision, 'generation': 1, 'source_ns': revision, 'cycle': 0}
                   for revision in range(3)]
        snapshot = {'client': {'live_wallpaper': {'producers': [{'source_frames': {'samples': samples}}]}}}
        storage = {}
        self.assertTrue(measurement['collect_sources'](storage, snapshot, limit=2))
        self.assertEqual(len(storage), 2)
        altered = dict(samples[0], source_ns=99)
        snapshot['client']['live_wallpaper']['producers'][0]['source_frames']['samples'] = [altered]
        with self.assertRaises(AssertionError):
            measurement['collect_sources'](storage, snapshot)

    def test_embedded_guest_sampler_is_valid_python(self):
        compile(measurement['SAMPLER'], '<guest-resource-sampler>', 'exec')


if __name__ == '__main__':
    unittest.main()
