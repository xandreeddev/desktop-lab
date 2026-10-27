"""Pure parser/admission tests; these never compile or decode native fixtures."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('diagnostic', Path(__file__).with_name('native_admission_diagnostic.py'))
D = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(D)


def row(phase='decoded-first-frame', guard=None):
    data = dict(schema=1, phase=phase, rejecting_guard=guard,
                dimensions=[64, 48], raw_sar=[0, 1], mapped_sar=[0, 1],
                raw_flags=0, mapped_flags=0, interlaced=False, corrupt=False, decode_error_flags=0,
                crop=[0, 0, 0, 0], strides=[64, 32, 32], buffer_zero_present=True,
                side_data_count=0, side_data_kind_ids=[], side_data_inventory_valid=True, side_data_null_entries=0,
                blocked_side_data={key: False for key in D.BLOCKED}, checks={key: False for key in D.CHECKS})
    for key in ('pixel_format', 'color_space', 'primaries', 'range', 'transfer', 'chroma'):
        data[key] = dict(raw=0, mapped='Unspecified')
    return data


def fixture(code=101, events=None, name=None):
    name = name or D.PROFILE['fixtures'][0]
    events = events if events is not None else [row(), row('admission-rejected', 'format-interlace-sar')]
    text = 'test '+name+' ... '+''.join(D.PREFIX+json.dumps(event)+'\n' for event in events)
    return text + ('FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 77 filtered out;\n'
                   if code else 'ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 77 filtered out;\n')


class NativeAdmissionDiagnosticTests(unittest.TestCase):
    def test_default_is_inert_and_identifies_only_three_fixtures(self):
        with patch.object(sys, 'argv', ['diagnostic']), patch.object(D, 'run', side_effect=AssertionError('native action')), contextlib.redirect_stdout(io.StringIO()) as stream:
            self.assertEqual(D.main(), 0)
        self.assertEqual(json.loads(stream.getvalue()), dict(prepared_only=True, actions=False, source_commit=D.PROFILE['source_commit'], fixtures=3))

    def test_existing_failure_remains_exit101_and_failed(self):
        result = D.fixture_result(D.PROFILE['fixtures'][0], 101, fixture())
        self.assertFalse(result['test_passed'])
        self.assertEqual(result['exit'], 101)
        self.assertEqual(result['metadata'][1]['rejecting_guard'], 'format-interlace-sar')

    def test_actual_success_is_recorded_without_inventing_native_acceptance(self):
        result = D.fixture_result(D.PROFILE['fixtures'][0], 0, fixture(0, [row(), row('admission-accepted')]))
        self.assertTrue(result['test_passed'])
        self.assertNotIn('native_accepted', result)

    def test_metadata_requires_exact_fixed_fields_and_bounded_kind_only_inventory(self):
        for mutate in (lambda value: value.update(path='/private/input'),
                       lambda value: value.pop('raw_sar'),
                       lambda value: value.update(side_data_kind_ids=list(range(65))),
                       lambda value: value['checks'].update(pointer=True),
                       lambda value: value['pixel_format'].update(raw='0')):
            value = row(); mutate(value)
            with self.assertRaises(RuntimeError):
                D.metadata(D.PREFIX+json.dumps(value))
        self.assertEqual(D.metadata(D.PREFIX+json.dumps(row()))[0]['raw_sar'], [0, 1])

    def test_header_without_decoded_first_frame_or_admission_is_rejected(self):
        for events in ([], [row()], [row('admission-rejected', 'color')]):
            with self.assertRaises(RuntimeError):
                D.fixture_result(D.PROFILE['fixtures'][0], 101, fixture(events=events))

    def test_wrong_case_exit_summary_and_guard_are_rejected(self):
        cases = [(101, fixture(name='unrelated::test')), (0, fixture()), (127, fixture()),
                 (101, fixture(events=[row(), row('admission-rejected', 'assumed-reason')]))]
        for code, text in cases:
            with self.assertRaises(RuntimeError):
                D.fixture_result(D.PROFILE['fixtures'][0], code, text)

    def test_profile_names_are_exactly_the_original_failed_cases(self):
        self.assertEqual(D.PROFILE['fixtures'], [
            'video::tests::video_native_planes_real_port_drains_tagged_h264_eof_and_preserves_retained_pixels',
            'video::tests::video_native_planes_rewinds_one_session_without_rgba_scratch_and_honors_callback_cancel',
            'video::tests::video_native_planes_source_or_upscaled_cover_keeps_owned_source_planes'])
        self.assertEqual(D.PROFILE['failed_original_run'], 40459098835)
        self.assertEqual(D.PROFILE['source_commit'], '9d3667b658ffac682bd291106613c076528810aa')

    def test_run_mode_without_explicit_paths_refuses_before_any_action(self):
        with patch.object(sys, 'argv', ['diagnostic', '--run']), patch.object(D, 'run', side_effect=AssertionError('native action')):
            with self.assertRaisesRegex(RuntimeError, 'explicit_hosted_paths_required'):
                D.main()


if __name__ == '__main__':
    unittest.main()
