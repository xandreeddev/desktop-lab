"""Offline comparison of four private test-only decoder stage reports.

No guest/process access; refuse mismatched inputs, active threading or quality
before returning throughput/resource diagnostics. Never accepts native FPS/RSS.
"""
import argparse
import json
from fractions import Fraction
import math
import re
from pathlib import Path


def require(value, label):
    if not value:
        raise ValueError(label)


DURATIONS = ('setup_ns', 'decoder_api_wall_ns', 'demux_wall_ns', 'converter_setup_wall_ns',
             'conversion_and_owned_rgba_wall_ns', 'retirement_wall_ns', 'user_cpu_ns', 'system_cpu_ns')


def nonnegative(value):
    # Integer durations are finite, including JSON u128; never coerce bool/int
    # or a large integer through floating point merely to validate it.
    require(type(value) is int and value >= 0 or type(value) is float and math.isfinite(value) and value >= 0,
            'nonnegative_measurement_required')
    return value


def positive(value):
    require(type(value) is int and value > 0 or type(value) is float and math.isfinite(value) and value > 0,
            'positive_measurement_required')
    return value


def speedup(serial_ns, frame2_ns):
    try:
        result = positive(serial_ns)/positive(frame2_ns)
    except OverflowError:
        raise ValueError('finite_speedup_required') from None
    require(math.isfinite(result) and result > 0, 'finite_speedup_required')
    return result


def sha256(value, reason):
    require(type(value) is str and re.fullmatch('[0-9a-f]{64}', value), reason)


def dimensions(value):
    require(type(value) is list and len(value) == 2 and all(type(edge) is int and 1 <= edge <= 4096 for edge in value),
            'bounded_dimension_pair_required')
    return tuple(value)


def rational(value):
    require(type(value) is str and len(value) <= 64, 'positive_rational_required')
    if value.startswith('Rational(') and value.endswith(')'):
        value = value[9:-1]
    match = re.fullmatch('([1-9][0-9]{0,9})/([1-9][0-9]{0,9})', value)
    require(match is not None, 'positive_rational_required')
    numerator, denominator = map(int, match.groups())
    require(numerator <= 2**31-1 and denominator <= 2**31-1, 'positive_rational_required')
    return Fraction(numerator, denominator)


def prefix_metadata(value, stage, expected_threading):
    require(type(value) is dict, 'pass_metadata_required')
    for key in ('frames', 'requested_frames'):
        require(type(value.get(key)) is int and 1 <= value[key] <= 1800, 'positive_bounded_prefix_required')
    require(value['frames'] == value['requested_frames'], 'incomplete_prefix')
    active = value.get('active_threading')
    require(type(active) is dict and type(active.get('count')) is int
            and active == expected_threading, 'actual_threading_mismatch')
    positive(value.get('wall_ns'))
    for key in DURATIONS:
        nonnegative(value.get(key))
    converted = value.get('converted_dimensions')
    if stage == 'decode':
        require(converted is None, 'decode_has_converted_geometry')
    else:
        dimensions(converted)
    return (value['frames'], value['requested_frames'], dimensions(value.get('source_dimensions')),
            tuple(converted) if converted is not None else None,
            rational(value.get('source_time_base')), rational(value.get('source_average_frame_rate')))


def compare(reports):
    require(len(reports) == 4, 'four_cases_required')
    cases = {}
    for report in reports:
        for key in ('source_sha256', 'benchmark_source_sha256', 'adapter_source_sha256'):
            sha256(report.get(key), 'case_digest_shape')
        require(report.get('state') == 'completed' and report.get('timed_validation_pts_and_count_match') is True
                and report.get('source_identity_preserved') is True, 'completed_validated_case_required')
        key = report.get('stage'), report.get('threads')
        require(key in {(stage, threads) for stage in ('decode', 'convert') for threads in ('serial', 'frame2')}
                and key not in cases, 'four_distinct_cases_required')
        expected = {'kind': 'None', 'count': 1} if key[1] == 'serial' else {'kind': 'Frame', 'count': 2}
        dimensions(report.get('physical_target'))
        timed = prefix_metadata(report.get('timed'), key[0], expected)
        validated = prefix_metadata(report.get('validation_cost'), key[0], expected)
        require(timed == validated, 'timed_validation_prefix_metadata_changed')
        quality = report.get('quality')
        require(type(quality) is dict, 'quality_digest_shape')
        for name in ('pts_sha256', 'visible_yuv420p_sha256'):
            sha256(quality.get(name), 'quality_digest_shape')
        if key[0] == 'convert':
            sha256(quality.get('converted_rgba_sha256'), 'quality_digest_shape')
        else:
            require(quality.get('converted_rgba_sha256') is None, 'decode_has_converted_digest')
        for name in ('libavcodec_version', 'libavformat_version', 'libavutil_version', 'libswscale_version'):
            require(type(report.get(name)) is int and report[name] > 0, 'library_version_shape')
        cases[key] = report
    baseline = cases['decode', 'serial']
    identity = ('source_sha256', 'benchmark_source_sha256', 'adapter_source_sha256',
                'libavcodec_version', 'libavformat_version', 'libavutil_version', 'libswscale_version', 'physical_target')
    quality = ('pts_sha256', 'visible_yuv420p_sha256')
    for report in cases.values():
        require(all(report.get(key) is not None and report[key] == baseline[key] for key in identity), 'case_source_or_libraries_changed')
        require(all(report['quality'].get(key) is not None and report['quality'][key] == baseline['quality'][key]
                    for key in quality), 'decoded_pixels_or_pts_changed')
        require(report['timed']['frames'] == baseline['timed']['frames']
                and report['timed']['source_dimensions'] == baseline['timed']['source_dimensions'], 'prefix_or_dimensions_changed')
        require(rational(report['timed']['source_time_base']) == rational(baseline['timed']['source_time_base'])
                and rational(report['timed']['source_average_frame_rate']) == rational(baseline['timed']['source_average_frame_rate']), 'source_timing_changed')
    converted = [cases['convert', threads] for threads in ('serial', 'frame2')]
    require(converted[0]['quality'].get('converted_rgba_sha256') is not None
            and converted[0]['quality']['converted_rgba_sha256'] == converted[1]['quality'].get('converted_rgba_sha256'), 'converted_pixels_changed')
    require(converted[0]['timed']['converted_dimensions'] == converted[1]['timed']['converted_dimensions'], 'converted_dimensions_changed')
    result = {}
    for stage in ('decode', 'convert'):
        serial, frame2 = (cases[stage, threads]['timed'] for threads in ('serial', 'frame2'))
        result[stage] = {
            'wall_speedup_serial_over_frame2': speedup(serial['wall_ns'], frame2['wall_ns']),
            'serial': serial, 'frame2': frame2,
            'scope': 'Fresh-process unpaced prefix throughput with identical decoded PTS/pixels; '
                     'actual process CPU/RSS includes setup. Validation hashing runs afterward. '
                     'Asynchronous decoder work can overlap conversion, so API wall times are not exclusive CPU costs.'}
    return {'schema': 1, 'four_cases_match_input_libraries_prefix_and_decoded_quality': True,
            'converted_rgba_quality_matches': True, 'stages': result,
            'native_presentation_or_resource_acceptance': False,
            'scope': 'Stage diagnostic only. No frame-rate, density, effect or production thread-policy change, '
                     'native presentation, source-cadence or 300 MiB acceptance follows.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', nargs=4, type=Path)
    args = parser.parse_args()
    reports = []
    for path in args.reports:
        with path.open('rb') as stream:
            raw = stream.read(1024*1024+1)
        require(len(raw) <= 1024*1024, 'report_size_bound')
        reports.append(json.loads(raw))
    print(json.dumps(compare(reports), indent=2, sort_keys=True, allow_nan=False))


if __name__ == '__main__':
    main()
