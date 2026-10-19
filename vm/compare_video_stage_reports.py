"""Offline comparison of four private test-only decoder stage reports.

No guest/process access; refuse mismatched inputs, active threading or quality
before returning throughput/resource diagnostics. Never accepts native FPS/RSS.
"""
import argparse
import json
import math
import re
from pathlib import Path


def require(value, label):
    if not value:
        raise ValueError(label)


def positive(value):
    require(type(value) in (int, float) and math.isfinite(value) and value > 0, 'positive_measurement_required')
    return value


def compare(reports):
    require(len(reports) == 4, 'four_cases_required')
    cases = {}
    for report in reports:
        for key in ('source_sha256', 'benchmark_source_sha256', 'adapter_source_sha256'):
            require(type(report.get(key)) is str and re.fullmatch('[0-9a-f]{64}', report[key]), 'case_digest_shape')
        require(report.get('state') == 'completed' and report.get('timed_validation_pts_and_count_match') is True
                and report.get('source_identity_preserved') is True, 'completed_validated_case_required')
        key = report.get('stage'), report.get('threads')
        require(key in {(stage, threads) for stage in ('decode', 'convert') for threads in ('serial', 'frame2')}
                and key not in cases, 'four_distinct_cases_required')
        expected = {'kind': 'None', 'count': 1} if key[1] == 'serial' else {'kind': 'Frame', 'count': 2}
        require(report['timed']['active_threading'] == expected
                and report['validation_cost']['active_threading'] == expected, 'actual_threading_mismatch')
        require(report['timed']['frames'] == report['timed']['requested_frames']
                == report['validation_cost']['frames'], 'incomplete_prefix')
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
        require(report['timed']['source_time_base'] == baseline['timed']['source_time_base']
                and report['timed']['source_average_frame_rate'] == baseline['timed']['source_average_frame_rate'], 'source_timing_changed')
    converted = [cases['convert', threads] for threads in ('serial', 'frame2')]
    require(converted[0]['quality'].get('converted_rgba_sha256') is not None
            and converted[0]['quality']['converted_rgba_sha256'] == converted[1]['quality'].get('converted_rgba_sha256'), 'converted_pixels_changed')
    require(converted[0]['timed']['converted_dimensions'] == converted[1]['timed']['converted_dimensions'], 'converted_dimensions_changed')
    result = {}
    for stage in ('decode', 'convert'):
        serial, frame2 = (cases[stage, threads]['timed'] for threads in ('serial', 'frame2'))
        result[stage] = {
            'wall_speedup_serial_over_frame2': positive(serial['wall_ns'])/positive(frame2['wall_ns']),
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
