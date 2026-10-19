"""No FFmpeg execution: strict comparison boundaries for future measured cases."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('video_stage_compare', Path(__file__).resolve().parents[1]/'vm/compare_video_stage_reports.py')
m = importlib.util.module_from_spec(spec);spec.loader.exec_module(m)


def fixture():
    result = []
    for stage in ('decode', 'convert'):
        for threads in ('serial', 'frame2'):
            timing = {'active_threading': {'kind': 'None', 'count': 1} if threads == 'serial' else {'kind': 'Frame', 'count': 2},
                      'frames': 360, 'requested_frames': 360, 'wall_ns': 2_000 if threads == 'serial' else 1_000,
                      'source_dimensions': [3840, 2160], 'source_time_base': '1/15360',
                      'source_average_frame_rate': '60/1', 'converted_dimensions': [1920, 1080] if stage == 'convert' else None}
            result.append({'state': 'completed', 'threads': threads, 'stage': stage,
                'timed_validation_pts_and_count_match': True, 'source_identity_preserved': True,
                'source_sha256': 'a'*64, 'benchmark_source_sha256': 'b'*64, 'adapter_source_sha256': 'c'*64, 'physical_target': [1920, 1080],
                'libavcodec_version': 1, 'libavformat_version': 2, 'libavutil_version': 3, 'libswscale_version': 4,
                'timed': timing, 'validation_cost': copy.deepcopy(timing),
                'quality': {'pts_sha256': 'p', 'visible_yuv420p_sha256': 'y', 'converted_rgba_sha256': 'r' if stage == 'convert' else None}})
    return result


class VideoStageComparison(unittest.TestCase):
    def test_identical_quality_and_explicit_thread_modes_allow_stage_diagnostics_only(self):
        result = m.compare(fixture())
        self.assertEqual(result['stages']['decode']['wall_speedup_serial_over_frame2'], 2)
        self.assertTrue(result['converted_rgba_quality_matches'])
        self.assertFalse(result['native_presentation_or_resource_acceptance'])

    def test_wrong_actual_frame_threading_cannot_masquerade_as_a_parallel_test(self):
        reports = fixture();reports[1]['timed']['active_threading'] = {'kind': 'None', 'count': 2}
        with self.assertRaisesRegex(ValueError, 'actual_threading_mismatch'):m.compare(reports)

    def test_changed_input_codec_library_or_source_refuses_comparison(self):
        for key in ('source_sha256', 'benchmark_source_sha256', 'adapter_source_sha256', 'libavcodec_version'):
            reports = fixture();reports[1][key] = 'changed'
            with self.subTest(key=key), self.assertRaises(ValueError):
                m.compare(reports)

    def test_same_frame_count_but_changed_pts_or_visible_pixels_refuses(self):
        for key in ('pts_sha256', 'visible_yuv420p_sha256'):
            reports = fixture();reports[1]['quality'][key] = 'changed'
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'decoded_pixels_or_pts_changed'):
                m.compare(reports)
        reports = fixture();reports[3]['quality']['converted_rgba_sha256'] = 'changed'
        with self.assertRaisesRegex(ValueError, 'converted_pixels_changed'):m.compare(reports)

    def test_truncated_prefix_duplicate_case_and_nonfinite_timing_refuse(self):
        reports = fixture();reports[1]['timed']['frames'] = 359
        with self.assertRaisesRegex(ValueError, 'incomplete_prefix'):m.compare(reports)
        reports = fixture();reports[1] = copy.deepcopy(reports[0])
        with self.assertRaisesRegex(ValueError, 'four_distinct_cases_required'):m.compare(reports)
        reports = fixture();reports[1]['timed']['wall_ns'] = float('nan')
        with self.assertRaisesRegex(ValueError, 'positive_measurement_required'):m.compare(reports)


if __name__ == '__main__':
    unittest.main()
