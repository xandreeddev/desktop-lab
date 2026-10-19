"""Deterministic offline analysis of bounded Lucent presentation/source metadata.

No guest access or subprocesses. Output is an explicit allowlist of aggregates;
private source paths, settings, raw timestamps and process identities are omitted.
"""
import argparse
from collections import Counter
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import stat

LIMIT = 16_384


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def integer(value, reason):
    require(type(value) is int and value >= 0, reason)
    return value


def number(value, reason):
    require(type(value) in (int, float) and math.isfinite(value) and value >= 0, reason)
    return value


def distribution(values):
    """Nearest-rank quantiles, with integer nanoseconds retained until output."""
    if not values:
        return {'count': 0, 'min_ns': None, 'p50_ns': None, 'p95_ns': None,
                'p99_ns': None, 'max_ns': None, 'mean_ns': None}
    ordered = sorted(values)
    return {'count': len(ordered), 'min_ns': ordered[0],
            **{name: ordered[math.ceil(len(ordered)*percent/100)-1]
               for name, percent in (('p50_ns', 50), ('p95_ns', 95), ('p99_ns', 99))},
            'max_ns': ordered[-1], 'mean_ns': sum(ordered)/len(ordered)}


def histogram(values):
    return {str(key): count for key, count in sorted(Counter(values).items())}


def pts_gaps(rows, nominal_fps=None):
    """Never subtract media PTS across generations/cycles or repeated revisions."""
    gaps, revision_gaps = [], []
    crossings = repeats = unaligned = nominal_missing = 0
    for previous, current in zip(rows, rows[1:]):
        if (previous['stream_id'], previous['generation'], previous['cycle']) != (
                current['stream_id'], current['generation'], current['cycle']):
            crossings += 1
            continue
        delta_revision = current['revision']-previous['revision']
        require(delta_revision >= 0, 'source_revision_regressed')
        if delta_revision == 0:
            require(current == previous, 'same_revision_source_changed')
            repeats += 1
            continue
        delta = current['source_ns']-previous['source_ns']
        require(delta > 0, 'nonincreasing_pts_within_cycle')
        gaps.append(delta)
        revision_gaps.append(delta_revision)
        if nominal_fps is not None:
            slots = round(Fraction(delta)*nominal_fps/1_000_000_000)
            # A nominal-rate slot estimate, not a decoded-frame/drop count.
            # The 1us tolerance covers integer-ns rounding, not VFR motion.
            error = abs(Fraction(delta)-Fraction(slots*1_000_000_000, 1)/nominal_fps)
            if slots < 1 or error > 1000:
                unaligned += 1
            else:
                nominal_missing += slots-1
    return {'same_cycle_positive_pts_intervals': distribution(gaps),
            'pts_gap_ns_histogram': histogram(gaps),
            'prepared_revision_step_histogram': histogram(revision_gaps),
            'cycle_or_generation_crossings_excluded': crossings,
            'adjacent_repeated_revisions': repeats,
            'nominal_source_rate': str(nominal_fps) if nominal_fps is not None else None,
            'nominal_rate_unaligned_intervals': unaligned,
            'nominal_source_slots_not_referenced_between_same_cycle_samples_estimate':
                nominal_missing if nominal_fps is not None else None}


def source_catalog(records):
    require(type(records) is list and len(records) <= LIMIT, 'source_catalog_bound')
    result = {}
    for item in records:
        require(type(item) is dict, 'source_record_shape')
        row = {key: integer(item.get(key), 'source_record_shape')
               for key in ('stream_id', 'revision', 'generation', 'cycle', 'source_ns')}
        key = row['stream_id'], row['revision']
        require(key not in result or result[key] == row, 'conflicting_source_revision')
        result[key] = row
    return result


def presentation_samples(records):
    require(type(records) is list and len(records) <= LIMIT, 'presentation_history_bound')
    result = {}
    for row in records:
        require(type(row) is dict, 'presentation_record_shape')
        for key in ('frame', 'clock_id', 'presented_ns', 'refresh_ns', 'flags'):
            integer(row.get(key), 'presentation_record_shape')
        number(row.get('requested_at_ms'), 'presentation_request_time')
        require(row['frame'] not in result or result[row['frame']] == row,
                'conflicting_presentation_frame')
        result[row['frame']] = row
    return [result[key] for key in sorted(result)]


def summarize(samples, catalog, nominal_fps=None):
    require(len(samples) >= 2, 'insufficient_presentation_samples')
    require(len({row['clock_id'] for row in samples}) == 1, 'mixed_presentation_clocks')
    times = [row['presented_ns'] for row in samples]
    intervals = [b-a for a, b in zip(times, times[1:])]
    require(all(delta > 0 for delta in intervals), 'nonincreasing_presentation_timestamp')
    joined = []
    for sample in samples:
        streams = sample.get('streams')
        require(type(streams) is dict and streams.get('truncated') is False, 'incomplete_stream_trace')
        refs = streams.get('revisions')
        require(type(refs) is list and len(refs) == 1, 'ambiguous_background_stream_reference')
        ref = refs[0]
        key = (integer(ref.get('stream_id'), 'stream_reference_shape'),
               integer(ref.get('revision'), 'stream_reference_shape'))
        require(key in catalog, 'unjoined_stream_revision')
        joined.append(catalog[key])
    require(len({(row['stream_id'], row['generation']) for row in joined}) == 1,
            'background_stream_or_generation_changed')
    require(all(b['revision'] >= a['revision'] for a, b in zip(joined, joined[1:])),
            'source_revision_regressed')
    keys = [(row['stream_id'], row['revision']) for row in joined]
    unique_count = len(set(keys))
    repeat_count = sum(a == b for a, b in zip(keys, keys[1:]))
    span_ns = times[-1]-times[0]
    stream_id = joined[0]['stream_id']
    prepared = sorted((row for (stream, revision), row in catalog.items()
                       if stream == stream_id and joined[0]['revision'] <= revision <= joined[-1]['revision']),
                      key=lambda row: row['revision'])
    expected_prepared = joined[-1]['revision']-joined[0]['revision']+1
    require(all(row['generation'] == joined[0]['generation'] for row in prepared),
            'prepared_generation_changed')
    return {'feedback_samples': len(samples), 'presentation_clock_id': samples[0]['clock_id'],
            'presentation_span_ns': span_ns,
            'feedback_intervals_per_second': len(intervals)*1_000_000_000/span_ns,
            'presentation_interval': distribution(intervals),
            'refresh_ns_histogram': histogram(row['refresh_ns'] for row in samples),
            'flags_histogram': histogram(row['flags'] for row in samples),
            'joined_source_samples': len(joined), 'distinct_prepared_revisions_referenced': unique_count,
            'distinct_revision_advances': unique_count-1,
            'distinct_revision_advances_per_second': (unique_count-1)*1_000_000_000/span_ns,
            'adjacent_repeated_prepared_revisions': repeat_count,
            'referenced_source_pts': pts_gaps(joined, nominal_fps),
            'prepared_catalog_between_first_and_last_reference': {
                'records': len(prepared), 'revision_positions': expected_prepared,
                'unobserved_revision_positions': expected_prepared-len(prepared),
                'observed_prepared_revisions_not_referenced_in_this_feedback_set': len(prepared)-unique_count,
                'source_pts': pts_gaps(prepared, nominal_fps)},
            'scope': 'Successful compositor feedback with bound prepared-resource references; '
                     'not pixel visibility, decoder completeness, scanout or accepted performance.'}


def analyze(measurement):
    require(type(measurement) is dict and measurement.get('mode') == 'presentation'
            and measurement.get('state') == 'collected', 'collected_presentation_measurement_required')
    surfaces = measurement.get('observed_presentation_metadata')
    require(type(surfaces) is list and len(surfaces) <= 128, 'surface_history_bound')
    backgrounds = [row for row in surfaces if row.get('surface_id', '').startswith('background')]
    require(len(backgrounds) == 1, 'one_background_history_required')
    samples = presentation_samples(backgrounds[0]['samples'])
    catalog = source_catalog(measurement.get('observed_source_metadata'))
    since = number(measurement.get('metadata_interval_requested_at_ms'), 'interval_request_boundary')
    duration = number(measurement.get('resources', {}).get('seconds'), 'resource_duration')
    require(duration > 0, 'resource_duration')
    probe = measurement.get('private_before', {}).get('workload', {}).get('probe', {})
    video = [row for row in probe.get('streams', []) if row.get('codec_type', 'video') == 'video']
    require(len(video) == 1, 'one_probed_video_stream_required')
    rate = video[0].get('avg_frame_rate')
    require(type(rate) is str and len(rate) <= 64, 'source_rate_shape')
    nominal_fps = Fraction(rate)
    require(0 < nominal_fps <= 1000, 'source_rate_bound')
    selected = [row for row in samples if row['requested_at_ms'] >= since]
    nominal_window = [row for row in selected if row['requested_at_ms'] < since+duration*1000]
    monitor = measurement.get('private_before', {}).get('monitor', {})
    resources = measurement.get('resources', {})
    workload = measurement.get('private_before', {}).get('workload', {})
    return {'schema': 1, 'source_commit': measurement.get('source_commit'),
            'desktop_sha256': resources.get('binary_sha256'),
            'workload': {'media_sha256': workload.get('media', {}).get('sha256'),
                         'codec': video[0].get('codec_name'), 'width': video[0].get('width'),
                         'height': video[0].get('height'), 'nominal_source_rate': rate,
                         'fps_setting': workload.get('fps_setting'),
                         'output_width': monitor.get('width'), 'output_height': monitor.get('height'),
                         'output_scale': monitor.get('scale'), 'output_refresh_hz': monitor.get('refreshRate')},
            'retained_all_background_history': summarize(samples, catalog, nominal_fps),
            'collector_request_lower_bound': summarize(selected, catalog, nominal_fps),
            'nominal_resource_duration_request_window': summarize(nominal_window, catalog, nominal_fps),
            'window_definition': {
                'retained': 'All retained history, including pre-observation samples.',
                'collector': 'request_at >= first observed desktop uptime; ending at last captured feedback.',
                'nominal_resource_duration': 'first desktop-uptime request boundary <= request_at < '
                    'boundary + measured resource seconds; not synchronized resource/presentation clocks.',
                'history_before_collector_boundary': len(samples)-len(selected),
                'resource_seconds': duration,
                'rate_denominator': 'Own compositor last-first timestamp span; numerator is interval/advance count, not sample count.',
                'quantiles': 'Nearest rank ceil(N*p), integer nanoseconds; no interpolation.',
                'source_cycle': 'Non-increasing prepared PTS marks a cycle; all cross-cycle differences excluded.',
                'nominal_slot_estimate': 'Only same-cycle gaps within1us of an integer nominal-rate period; '
                    'not a count of decoded, visible, or definitively dropped frames.'},
            'instrumented_resources': {key: resources.get(key) for key in (
                'rss_mib_mean', 'rss_mib_max_interval', 'cpu_percent_one_core', 'swap_mib_max')},
            'eligibility': {'measurement_eligible': measurement.get('eligible') is True,
                'reasons': measurement.get('ineligible_reasons', []),
                'host_swap_in_pages': measurement.get('host_pressure', {}).get('swap_in_delta'),
                'host_io_full_stall_fraction': measurement.get('host_pressure', {}).get('psi_fraction', {}).get('io', {}).get('full'),
                'target_acceptance_established': False,
                'normal_uninstrumented_rss_target_met': None},
            'scope': 'Sanitized offline analysis of opt-in metadata. No submission-counter FPS, '
                     'visible-pixel, physical scanout, decoder-completeness or eligible performance claim.'}


def read_measurement(path):
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_size <= 32*1024*1024, 'bounded_regular_measurement')
        raw = stream.read(32*1024*1024+1)
        identity = lambda value: (value.st_dev, value.st_ino, value.st_mode, value.st_uid, value.st_gid,
                                   value.st_size, value.st_mtime_ns, value.st_ctime_ns)
        require(len(raw) <= 32*1024*1024 and identity(before) == identity(os.fstat(stream.fileno()))
                == identity(path.lstat()), 'measurement_changed')
    return raw


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('measurement', type=Path)
    args = parser.parse_args()
    raw = read_measurement(args.measurement)
    result = analyze(json.loads(raw))
    result['input_sha256'] = hashlib.sha256(raw).hexdigest()
    print(json.dumps(result, indent=2, sort_keys=True, allow_nan=False))


if __name__ == '__main__':
    main()
