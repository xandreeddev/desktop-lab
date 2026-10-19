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
                      'source_average_frame_rate': '60/1', 'converted_dimensions': [1920, 1080] if stage == 'convert' else None,
                      **{key: 0 for key in m.DURATIONS}}
            result.append({'state': 'completed', 'threads': threads, 'stage': stage,
                'timed_validation_pts_and_count_match': True, 'source_identity_preserved': True,
                'source_sha256': 'a'*64, 'benchmark_source_sha256': 'b'*64, 'adapter_source_sha256': 'c'*64, 'physical_target': [1920, 1080],
                'libavcodec_version': 1, 'libavformat_version': 2, 'libavutil_version': 3, 'libswscale_version': 4,
                'timed': timing, 'validation_cost': copy.deepcopy(timing),
                'quality': {'pts_sha256': 'd'*64, 'visible_yuv420p_sha256': 'e'*64, 'converted_rgba_sha256': 'f'*64 if stage == 'convert' else None}})
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
            reports = fixture();reports[1]['quality'][key] = '0'*64
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'decoded_pixels_or_pts_changed'):
                m.compare(reports)
        reports = fixture();reports[3]['quality']['converted_rgba_sha256'] = '0'*64
        with self.assertRaisesRegex(ValueError, 'converted_pixels_changed'):m.compare(reports)

    def test_truncated_prefix_duplicate_case_and_nonfinite_timing_refuse(self):
        reports = fixture();reports[1]['timed']['frames'] = 359
        with self.assertRaisesRegex(ValueError, 'incomplete_prefix'):m.compare(reports)
        reports = fixture();reports[1] = copy.deepcopy(reports[0])
        with self.assertRaisesRegex(ValueError, 'four_distinct_cases_required'):m.compare(reports)
        reports = fixture();reports[1]['timed']['wall_ns'] = float('nan')
        with self.assertRaisesRegex(ValueError, 'positive_measurement_required'):m.compare(reports)


    def test_matching_empty_short_uppercase_or_nonhex_quality_never_passes(self):
        for key in ('pts_sha256', 'visible_yuv420p_sha256', 'converted_rgba_sha256'):
            for bad in ('', 'a'*63, 'A'*64, 'g'*64, None, True):
                reports = fixture()
                for report in reports:
                    if key != 'converted_rgba_sha256' or report['stage'] == 'convert':
                        report['quality'][key] = bad
                with self.subTest(key=key, bad=bad), self.assertRaisesRegex(ValueError, 'quality_digest_shape'):
                    m.compare(reports)

    def test_matching_zero_bool_negative_float_or_unbounded_prefix_never_passes(self):
        for bad in (0, False, True, -1, 1.0, 1801, None):
            reports = fixture()
            for report in reports:
                for phase in ('timed', 'validation_cost'):
                    report[phase]['frames'] = report[phase]['requested_frames'] = bad
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, 'positive_bounded_prefix_required'):
                m.compare(reports)

    def test_validation_requested_prefix_cannot_be_stale_while_frames_match(self):
        reports = fixture();reports[0]['validation_cost']['requested_frames'] = 359
        with self.assertRaisesRegex(ValueError, 'incomplete_prefix'):
            m.compare(reports)

    def test_every_timed_validation_geometry_or_rational_disagreement_refuses(self):
        for key, different in (('source_dimensions', [3840, 2158]), ('converted_dimensions', [1918, 1080]),
                               ('source_time_base', '1/1000'), ('source_average_frame_rate', '30/1')):
            reports = fixture();reports[2]['validation_cost'][key] = different
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'timed_validation_prefix_metadata_changed'):
                m.compare(reports)

    def test_invalid_geometry_cannot_pass_by_matching_all_reports(self):
        for bad in ([0, 1080], [True, 1080], [-1, 1080], [1920, 4097], [1920.0, 1080], [1920], None):
            for key in ('source_dimensions', 'converted_dimensions'):
                reports = fixture()
                for report in reports:
                    if key == 'source_dimensions' or report['stage'] == 'convert':
                        for phase in ('timed', 'validation_cost'):
                            report[phase][key] = bad
                with self.subTest(key=key, bad=bad), self.assertRaisesRegex(ValueError, 'bounded_dimension_pair_required'):
                    m.compare(reports)

    def test_source_smaller_than_target_and_odd_cover_output_remain_valid(self):
        for source, converted in (([1280, 720], [1280, 720]), ([3840, 2161], [1920, 1081])):
            reports = fixture()
            for report in reports:
                for phase in ('timed', 'validation_cost'):
                    report[phase]['source_dimensions'] = source
                    if report['stage'] == 'convert': report[phase]['converted_dimensions'] = converted
            self.assertTrue(m.compare(reports)['converted_rgba_quality_matches'])

    def test_decode_report_cannot_claim_converted_geometry_or_pixels(self):
        for key in ('geometry', 'pixels'):
            reports = fixture()
            if key == 'geometry': reports[0]['timed']['converted_dimensions'] = [1920, 1080]
            else: reports[0]['quality']['converted_rgba_sha256'] = 'f'*64
            with self.subTest(key=key), self.assertRaises(ValueError):
                m.compare(reports)

    def test_rationals_are_positive_well_formed_and_compared_semantically(self):
        reports = fixture()
        reports[1]['timed']['source_average_frame_rate'] = 'Rational(120/2)'
        reports[1]['validation_cost']['source_average_frame_rate'] = '60/1'
        self.assertTrue(m.compare(reports)['converted_rgba_quality_matches'])
        for bad in ('', '1/0', 'NaN', 'Rational(0/1)', '60', '1/-1', True):
            reports = fixture()
            for report in reports:
                for phase in ('timed', 'validation_cost'): report[phase]['source_time_base'] = bad
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, 'positive_rational_required'):
                m.compare(reports)

    def test_all_duration_fields_refuse_missing_negative_boolean_or_nonfinite_values(self):
        for phase in ('timed', 'validation_cost'):
            for key in m.DURATIONS:
                for bad in (None, -1, True, float('nan'), float('inf'), '1'):
                    reports = fixture();reports[0][phase][key] = bad
                    with self.subTest(phase=phase, key=key, bad=bad), self.assertRaisesRegex(ValueError, 'nonnegative_measurement_required'):
                        m.compare(reports)
            for bad in (0, -1, True, float('nan'), float('inf')):
                reports = fixture();reports[0][phase]['wall_ns'] = bad
                with self.subTest(phase=phase, wall=bad), self.assertRaisesRegex(ValueError, 'positive_measurement_required'):
                    m.compare(reports)

    def test_u128_integer_durations_are_preserved_without_validation_float_rounding(self):
        reports = fixture()
        for report in reports:
            for phase in ('timed', 'validation_cost'):
                report[phase]['wall_ns'] = 2**100*(2 if report['threads'] == 'serial' else 1)
                for key in m.DURATIONS: report[phase][key] = 2**80
        result = m.compare(reports)
        self.assertEqual(result['stages']['decode']['wall_speedup_serial_over_frame2'], 2)
        self.assertEqual(result['stages']['decode']['serial']['decoder_api_wall_ns'], 2**80)

    def test_finite_input_walls_cannot_publish_infinite_speedup(self):
        reports = fixture();reports[0]['timed']['wall_ns'] = 1e308;reports[1]['timed']['wall_ns'] = 1e-308
        with self.assertRaisesRegex(ValueError, 'finite_speedup_required'):
            m.compare(reports)

    def test_boolean_library_version_or_active_thread_count_is_not_an_integer_measurement(self):
        reports = fixture()
        for report in reports: report['libavcodec_version'] = True
        with self.assertRaisesRegex(ValueError, 'library_version_shape'):m.compare(reports)
        reports = fixture();reports[0]['timed']['active_threading']['count'] = True
        with self.assertRaisesRegex(ValueError, 'actual_threading_mismatch'):m.compare(reports)


if __name__ == '__main__':
    unittest.main()
