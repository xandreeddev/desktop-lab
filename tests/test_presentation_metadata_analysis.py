"""Offline integer-clock/source-reference regressions; no compositor or process."""
import copy
from fractions import Fraction
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('presentation_analysis', ROOT/'vm/analyze_presentation_metadata.py')
a = importlib.util.module_from_spec(spec);spec.loader.exec_module(a)


def source(revision, pts, cycle=0, generation=1, stream=7):
    return dict(stream_id=stream, revision=revision, generation=generation, cycle=cycle, source_ns=pts)


def sample(frame, at, revision, *, requested=None):
    return dict(frame=frame, clock_id=1, presented_ns=at, refresh_ns=16_666_667, flags=7,
                requested_at_ms=at/1_000_000 if requested is None else requested,
                streams={'truncated': False, 'revisions': [{'stream_id': 7, 'revision': revision}]})


class SourcePresentationAnalysis(unittest.TestCase):
    def test_sixty_hz_repeated_resource_is_zero_source_advancement(self):
        samples = [sample(i, i*16_666_667, 1) for i in range(61)]
        result = a.summarize(samples, a.source_catalog([source(1, 0)]))
        self.assertAlmostEqual(result['feedback_intervals_per_second'], 60, places=5)
        self.assertEqual(result['distinct_prepared_revisions_referenced'], 1)
        self.assertEqual(result['distinct_revision_advances'], 0)
        self.assertEqual(result['distinct_revision_advances_per_second'], 0)
        self.assertEqual(result['adjacent_repeated_prepared_revisions'], 60)

    def test_twentyfour_source_advances_with_sixty_display_intervals(self):
        catalog = a.source_catalog([source(i+1, round(Fraction(i*1_000_000_000, 24))) for i in range(25)])
        samples = [sample(i, round(Fraction(i*1_000_000_000, 60)), i*24//60+1) for i in range(61)]
        result = a.summarize(samples, catalog, Fraction(24))
        self.assertEqual(result['feedback_intervals_per_second'], 60)
        self.assertEqual(result['distinct_revision_advances_per_second'], 24)
        self.assertEqual(result['adjacent_repeated_prepared_revisions'], 36)
        # A half-open one-second sample list holds24 unique values, while a
        # closed interval has25 endpoints and24 advancements: never N/T FPS.
        self.assertEqual(a.summarize(samples[:-1], catalog)['distinct_prepared_revisions_referenced'], 24)

    def test_cycle_boundary_excludes_pts_reset_without_negative_or_invented_gap(self):
        rows = [source(1, 9_000_000_000), source(2, 0, cycle=1), source(3, 16_666_667, cycle=1)]
        result = a.pts_gaps(rows, Fraction(60))
        self.assertEqual(result['cycle_or_generation_crossings_excluded'], 1)
        self.assertEqual(result['same_cycle_positive_pts_intervals']['count'], 1)
        self.assertEqual(result['same_cycle_positive_pts_intervals']['max_ns'], 16_666_667)
        self.assertEqual(result['nominal_source_slots_not_referenced_between_same_cycle_samples_estimate'], 0)

    def test_skipped_prepared_revision_and_missing_nominal_pts_are_separate(self):
        catalog = a.source_catalog([source(1, 0), source(2, 33_333_333), source(3, 50_000_000)])
        result = a.summarize([sample(1, 0, 1), sample(2, 50_000_000, 3)], catalog, Fraction(60))
        prepared = result['prepared_catalog_between_first_and_last_reference']
        self.assertEqual(prepared['records'], 3)
        self.assertEqual(prepared['unobserved_revision_positions'], 0)
        self.assertEqual(prepared['observed_prepared_revisions_not_referenced_in_this_feedback_set'], 1)
        self.assertEqual(prepared['source_pts']['nominal_source_slots_not_referenced_between_same_cycle_samples_estimate'], 1)
        self.assertEqual(result['referenced_source_pts']['nominal_source_slots_not_referenced_between_same_cycle_samples_estimate'], 2)

    def test_unknown_ambiguous_or_truncated_join_refuses_instead_of_counting_a_frame(self):
        original = [sample(1, 0, 1), sample(2, 16_666_667, 1)]
        catalog = a.source_catalog([source(1, 0)])
        for kind in ('unknown', 'ambiguous', 'truncated', 'missing'):
            rows = copy.deepcopy(original)
            if kind == 'unknown': rows[0]['streams']['revisions'][0]['revision'] = 99
            if kind == 'ambiguous': rows[0]['streams']['revisions'].append({'stream_id': 7, 'revision': 1})
            if kind == 'truncated': rows[0]['streams']['truncated'] = True
            if kind == 'missing': rows[0]['streams'] = None
            with self.subTest(kind=kind), self.assertRaises(ValueError): a.summarize(rows, catalog)

    def test_duplicate_samples_do_not_inflate_counts_and_conflicts_refuse(self):
        row = sample(1, 0, 1)
        self.assertEqual(len(a.presentation_samples([row, copy.deepcopy(row)])), 1)
        changed = dict(row, presented_ns=1)
        with self.assertRaisesRegex(ValueError, 'conflicting_presentation_frame'):
            a.presentation_samples([row, changed])
        with self.assertRaisesRegex(ValueError, 'conflicting_source_revision'):
            a.source_catalog([source(1, 0), source(1, 1)])

    def test_clock_switch_regression_and_same_cycle_nonincreasing_pts_refuse(self):
        catalog = a.source_catalog([source(1, 0), source(2, 16_666_667)])
        for rows in ([sample(1, 0, 1), dict(sample(2, 16_666_667, 2), clock_id=2)],
                     [sample(1, 10, 1), sample(2, 10, 2)],
                     [sample(1, 0, 2), sample(2, 16_666_667, 1)]):
            with self.assertRaises(ValueError): a.summarize(rows, catalog)
        with self.assertRaisesRegex(ValueError, 'nonincreasing_pts_within_cycle'):
            a.pts_gaps([source(1, 10), source(2, 10)])

    def test_nearest_rank_quantiles_and_integer_nanosecond_precision(self):
        values = list(range(1, 101))
        result = a.distribution(values)
        self.assertEqual((result['p50_ns'], result['p95_ns'], result['p99_ns']), (50, 95, 99))
        origin = 9_000_000_000_000_000_000
        result = a.summarize([sample(1, origin, 1), sample(2, origin+16_666_667, 1)], a.source_catalog([source(1, 0)]))
        self.assertEqual(result['presentation_span_ns'], 16_666_667)

    def test_non_nominal_variable_gap_is_not_mislabeled_as_source_slot_drop(self):
        result = a.pts_gaps([source(1, 0), source(2, 21_000_000)], Fraction(60))
        self.assertEqual(result['nominal_rate_unaligned_intervals'], 1)
        self.assertEqual(result['nominal_source_slots_not_referenced_between_same_cycle_samples_estimate'], 0)

    def measurement(self):
        return {'mode': 'presentation', 'state': 'collected', 'source_commit': 'a'*40,
                'resources': {'seconds': .05, 'binary_sha256': 'b'*64, 'rss_mib_mean': 219},
                'metadata_interval_requested_at_ms': 20, 'eligible': False,
                'ineligible_reasons': ['host_swap_activity'],
                'observed_source_metadata': [source(1, 0), source(2, 10_000_000), source(3, 20_000_000)],
                'observed_presentation_metadata': [{'surface_id': 'background [PRIVATE OUTPUT]',
                    'samples': [sample(1, 0, 1, requested=0), sample(2, 20_000_000, 2, requested=20),
                                sample(3, 40_000_000, 3, requested=40), sample(4, 80_000_000, 3, requested=80)]}],
                'private_before': {'workload': {'probe': {'streams': [{'codec_type': 'video', 'avg_frame_rate': '60/1'}]}}}}

    def test_request_boundary_trims_old_history_without_dividing_total_samples_by_resource_duration(self):
        result = a.analyze(self.measurement())
        self.assertEqual(result['retained_all_background_history']['feedback_samples'], 4)
        self.assertEqual(result['collector_request_lower_bound']['feedback_samples'], 3)
        self.assertEqual(result['nominal_resource_duration_request_window']['feedback_samples'], 2)
        self.assertEqual(result['collector_request_lower_bound']['presentation_span_ns'], 60_000_000)
        self.assertNotEqual(result['collector_request_lower_bound']['feedback_intervals_per_second'], 3/.05)
        self.assertFalse(result['eligibility']['target_acceptance_established'])
        self.assertIsNone(result['eligibility']['normal_uninstrumented_rss_target_met'])

    def test_input_order_is_deterministic_and_private_fields_never_enter_output(self):
        value = self.measurement();value['private_after'] = {'credentials': 'PRIVATE_SECRET_SENTINEL'}
        value['private_before']['settings'] = {'path': '/private/PRIVATE_SECRET_SENTINEL'}
        result = a.analyze(value)
        value['observed_source_metadata'].reverse()
        value['observed_presentation_metadata'][0]['samples'].reverse()
        self.assertEqual(result, a.analyze(value))
        serialized = json.dumps(result)
        self.assertNotIn('PRIVATE_SECRET_SENTINEL', serialized)
        self.assertNotIn('PRIVATE OUTPUT', serialized)

    def test_bounded_catalog_rejects_excess_before_processing(self):
        with self.assertRaisesRegex(ValueError, 'source_catalog_bound'):
            a.source_catalog([None]*(a.LIMIT+1))


if __name__ == '__main__':
    unittest.main()
