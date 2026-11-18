"""Measurement policy must distinguish compositor timestamps from submissions."""
import copy
import io
import json
import runpy
import subprocess
import sys
import tempfile
from contextlib import ExitStack, contextmanager, redirect_stdout
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
# Loading the policy module does not import VM helpers or invoke the sampler.
measurement = runpy.run_path(str(ROOT/'vm/measure-daily-resources.py'), run_name='measurement_policy')


class PresentationMeasurement(unittest.TestCase):
    def sample(self, frame, nanos, clock=1, flags=1):
        return {'frame': frame, 'presented_ns': nanos, 'clock_id': clock, 'flags': flags}

    def surface(self, role='background', instance=None, generation=7, samples=None):
        return {'id': role if instance is None else f'{role} [{instance}]',
                'role': role, 'instance': instance, 'native_generation': generation,
                'frames': 20, 'presentation': {'samples': samples or []}}

    def feedback(self, frame=1, generation=7, **changes):
        return dict(self.sample(frame, frame * 16_666_667),
                    requested_at_ms=100+frame, native_generation=generation, **changes)

    def collect(self, storage, snapshot, **kwargs):
        return measurement['collect_presentations'](
            storage, snapshot, measurement['surface_inventory'](snapshot), **kwargs)

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
        values = [self.feedback(frame=i) for i in range(8)]
        snapshot = {'surfaces': [self.surface(samples=values)]}
        self.assertTrue(self.collect(storage, snapshot, limit=3)['truncated'])
        key = measurement['surface_key'](snapshot['surfaces'][0])
        self.assertEqual(len(storage[key]), 3)
        self.assertEqual(measurement['presentation_intervals'](list(storage[key].values()))['intervals'], 2)

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

    def idle(self):
        return {'client': {'background_ready': True, 'wallpaper_busy': False,
                           'launcher': False, 'widgets_editing': False,
                           'live_wallpaper': {'running': True}}}

    def test_old_uninstrumented_inspector_can_supply_a_read_only_baseline(self):
        snapshot = self.idle()
        measurement['assert_idle_snapshot'](snapshot, 'animated-background')
        self.assertFalse(self.collect({}, dict(snapshot, surfaces=[]))['truncated'])
        self.assertFalse(measurement['collect_sources']({}, {'client': {'live_wallpaper': {'producers': []}}}))

    def test_new_display_and_application_activity_rejects_an_idle_interval(self):
        for name in ['displays', 'application_actions']:
            for state in ['open', 'visible', 'busy', 'refreshing']:
                with self.subTest(panel=name, state=state):
                    snapshot = self.idle()
                    snapshot['client'][name] = {state: True}
                    with self.assertRaises(AssertionError):
                        measurement['assert_idle_snapshot'](snapshot, 'animated-background')

    def test_transient_preparation_or_history_cannot_be_reported_as_settled_idle(self):
        for mutate in [
            lambda client: client.update(notification_history_open=True),
            lambda client: client.update(wallpaper_browser={'busy': True}),
            lambda client: client.update(settings_save_status='saving'),
            lambda client: client.update(settings_save_status='retrying'),
            lambda client: client['live_wallpaper'].update(library_loading=True),
            lambda client: client['live_wallpaper'].update(awaiting_first_frame=True),
            lambda client: client['live_wallpaper'].update(running=False),
        ]:
            snapshot = self.idle()
            mutate(snapshot['client'])
            with self.assertRaises(AssertionError):
                measurement['assert_idle_snapshot'](snapshot, 'animated-background')

    def test_typed_addresses_remain_distinct_when_display_labels_collide(self):
        surfaces = [self.surface('background', 'HDMI [one]', samples=[self.feedback()]),
                    self.surface('background [HDMI [one]]', samples=[self.feedback()]),
                    {'id': 'background [HDMI [one]]', 'frames': 20}]
        self.assertEqual(surfaces[0]['id'], surfaces[1]['id'])
        snapshot = {'surfaces': surfaces}
        inventory = measurement['surface_inventory'](snapshot)
        self.assertEqual(len(inventory), 3)
        self.assertEqual({tuple(json.loads(key)) for key in inventory},
                         {('instance', 'role'), ('legacy_id',)})
        storage = {}
        self.collect(storage, snapshot)
        self.assertEqual(len(storage), 2)
        counts = measurement['submission_counts'](snapshot, inventory)
        self.assertEqual(set(counts), set(inventory))
        self.assertEqual(list(counts.values()), [20, 20, 20])
        renamed = copy.deepcopy(snapshot)
        renamed['surfaces'][0]['id'] = 'a changed diagnostic label'
        measurement['assert_surface_inventory'](renamed, inventory)

    def test_inventory_rejects_duplicate_incomplete_and_oversized_addresses(self):
        one = self.surface()
        cases = [
            [one, copy.deepcopy(one)],
            [{'id': 'same'}, {'id': 'same'}],
            [dict(one, instance='')],
            [dict(one, role='')],
            [dict(one, instance=3)],
            [{key: value for key, value in one.items() if key != 'instance'}],
            [{key: value for key, value in one.items() if key != 'role'}],
            [self.surface(instance=str(index)) for index in range(65)],
        ]
        for surfaces in cases:
            with self.subTest(surfaces=surfaces), self.assertRaises(AssertionError):
                measurement['surface_inventory']({'surfaces': surfaces})

    def test_only_positive_integer_generations_can_be_verified(self):
        for generation in [0, -1, True, False, 7., '7', 2**64]:
            with self.subTest(generation=generation), self.assertRaises(AssertionError):
                measurement['surface_inventory']({'surfaces': [self.surface(generation=generation)]})
        for generation in [1, 2**64-1]:
            snapshot = {'surfaces': [self.surface(generation=generation)]}
            identity = next(iter(measurement['surface_inventory'](snapshot).values()))
            self.assertTrue(identity['presentation_identity_verified'])

    def test_all_inventory_continuity_is_checked_before_any_history_mutation(self):
        before = {'surfaces': [self.surface(), self.surface('bar', generation=9)]}
        inventory = measurement['surface_inventory'](before)
        for change in [
            lambda surfaces: surfaces.pop(),
            lambda surfaces: surfaces.append(self.surface('dock')),
            lambda surfaces: surfaces[1].update(native_generation=10),
            lambda surfaces: surfaces[1].update(native_generation=None),
            lambda surfaces: surfaces[1].update(instance='replacement'),
            lambda surfaces: surfaces[1].pop('role'),
        ]:
            snapshot = copy.deepcopy(before)
            snapshot['surfaces'][0]['presentation']['samples'] = [self.feedback()]
            change(snapshot['surfaces'])
            storage = {}
            with self.assertRaises(AssertionError):
                measurement['collect_presentations'](storage, snapshot, inventory)
            self.assertEqual(storage, {}, 'A later invalid surface must not partially admit the first')

    def test_old_retained_native_history_is_filtered_before_cadence(self):
        values = [self.feedback(1, generation=6), self.feedback(2), self.feedback(3)]
        snapshot = {'surfaces': [self.surface(samples=values)]}
        storage = {}
        result = self.collect(storage, snapshot)
        self.assertEqual(result['old_generation_history_observations_skipped'], 1)
        self.assertEqual(result['unverified_history_observations_skipped'], 0)
        retained = next(iter(storage.values()))
        self.assertEqual(set(retained), {2, 3})
        self.assertEqual(measurement['presentation_intervals'](list(retained.values()))['intervals'], 1)

    def test_future_missing_and_invalid_sample_generations_reject_the_entire_batch(self):
        for generation in [None, 0, -1, True, 7., '7', 8, 'absent']:
            invalid = self.feedback(2)
            if generation == 'absent':
                invalid.pop('native_generation')
            else:
                invalid['native_generation'] = generation
            snapshot = {'surfaces': [self.surface(samples=[self.feedback(), invalid])]}
            storage = {}
            with self.subTest(generation=generation), self.assertRaises(AssertionError):
                self.collect(storage, snapshot)
            self.assertEqual(storage, {})

    def test_exact_duplicates_are_idempotent_and_conflicting_feedback_never_overwrites(self):
        sample = self.feedback(streams={'revisions': [{'stream_id': 7, 'revision': 1}], 'truncated': False})
        snapshot = {'surfaces': [self.surface(samples=[sample, copy.deepcopy(sample)])]}
        storage = {}
        self.collect(storage, snapshot)
        original = copy.deepcopy(storage)
        self.collect(storage, snapshot)
        self.assertEqual(storage, original)
        for key, value in [('presented_ns', 99), ('clock_id', 2), ('flags', 0),
                           ('requested_at_ms', 999),
                           ('streams', {'revisions': [{'stream_id': 7, 'revision': 2}]})]:
            changed = dict(sample, **{key: value})
            conflict = {'surfaces': [self.surface(samples=[self.feedback(2), changed])]}
            with self.subTest(field=key), self.assertRaises(AssertionError):
                self.collect(storage, conflict)
            self.assertEqual(storage, original)
        # A duplicate conflict must not hide behind the host cap or since filter.
        conflict = {'surfaces': [self.surface(samples=[sample, dict(sample, flags=0)])]}
        for kwargs in [{'limit': 0}, {'since': 1000}]:
            with self.subTest(kwargs=kwargs), self.assertRaises(AssertionError):
                self.collect({}, conflict, **kwargs)

    def test_null_missing_and_legacy_identities_never_supply_cadence_from_old_history(self):
        modern = self.surface(generation=None, samples=[self.feedback()])
        missing = self.surface('bar', samples=[self.feedback()])
        missing.pop('native_generation')
        legacy = {'id': 'background', 'frames': 20,
                  'native_generation': 7, 'presentation': {'samples': [self.feedback()]}}
        snapshot = {'surfaces': [modern, missing, legacy]}
        inventory = measurement['surface_inventory'](snapshot)
        self.assertFalse(any(row['presentation_identity_verified'] for row in inventory.values()))
        storage = {}
        result = self.collect(storage, snapshot)
        self.assertEqual(result['unverified_history_observations_skipped'], 3)
        self.assertEqual(storage, {})
        self.assertEqual(len(measurement['submission_counts'](snapshot, inventory)), 3)

    def test_snapshot_history_bound_rejects_before_retaining_any_samples(self):
        snapshot = {'surfaces': [self.surface(samples=[self.feedback()] * 16_385)]}
        storage = {}
        with self.assertRaises(AssertionError):
            self.collect(storage, snapshot)
        self.assertEqual(storage, {})

    def test_live_preview_background_refit_and_native_retirement_reject_idle(self):
        mutations = [lambda snapshot: snapshot.update(retiring_native_surfaces=1),
                     lambda snapshot: snapshot['client'].update(wallpaper_presentation={'busy': True})]
        for flag in ['active', 'retiring', 'loading']:
            mutations.append(lambda snapshot, flag=flag: snapshot['client']['live_wallpaper'].update(preview={flag: True}))
        for flag in ['transition_pending', 'awaiting_first_frame']:
            mutations.append(lambda snapshot, flag=flag: snapshot['client'].update(
                background_transitions=[{'instance': 'first', 'presentation': {}},
                                        {'instance': 'second', 'presentation': {flag: True}}]))
        for mutate in mutations:
            snapshot = self.idle()
            mutate(snapshot)
            with self.subTest(snapshot=snapshot), self.assertRaises(AssertionError):
                measurement['assert_idle_snapshot'](snapshot, 'animated-background')
        settled = self.idle()
        settled['retiring_native_surfaces'] = 0
        settled['client'].update(wallpaper_presentation={'busy': False},
                                 background_transitions=[{'presentation': {'transition_pending': False, 'awaiting_first_frame': False}}])
        settled['client']['live_wallpaper']['preview'] = {'active': False, 'retiring': False, 'loading': False}
        measurement['assert_idle_snapshot'](settled, 'animated-background')

    def test_embedded_guest_sampler_is_valid_python(self):
        compile(measurement['SAMPLER'], '<guest-resource-sampler>', 'exec')


class OwnedSampler:
    """An in-memory child-process fixture; no command is ever executed."""
    def __init__(self, pending_polls=1, stubborn=False, returncode=None):
        self.pending_polls = pending_polls
        self.stubborn = stubborn
        self.returncode = returncode
        self.terminated = False
        self.killed = False
        self.communications = []

    def poll(self):
        if self.returncode is None and self.pending_polls is not None:
            if self.pending_polls == 0:
                self.returncode = 0
            else:
                self.pending_polls -= 1
        return self.returncode

    def communicate(self, timeout=None):
        self.communications.append(timeout)
        if self.terminated and self.stubborn and not self.killed:
            raise subprocess.TimeoutExpired('owned-sampler-fixture', timeout)
        return json.dumps({'rss_mib_max_interval': 90}), 'owned failure'

    def terminate(self):
        self.terminated = True
        if not self.stubborn:
            self.returncode = -15

    def kill(self):
        self.killed = True
        self.returncode = -9


class ResourceMain(unittest.TestCase):
    def snapshot(self, uptime=100):
        policy = PresentationMeasurement()
        snapshot = policy.idle()
        snapshot.update(uptime_ms=uptime, adapter='owned-fixture',
                        surfaces=[policy.surface()], retiring_native_surfaces=0)
        snapshot['client']['live_wallpaper'].update(retained_frame_bytes=0, producers=[{
            'revision': 2, 'running': True, 'target': [1920, 1080],
            'frame_size': [1920, 1080], 'source_aspect': [16, 9],
        }])
        return snapshot

    @contextmanager
    def harness(self, snapshots, process=None):
        process = process or OwnedSampler()
        inspectors = Mock(side_effect=snapshots)
        remote = Mock(return_value='42\n')
        monitors = [{'width': 1920, 'height': 1080, 'scale': 1, 'transform': 0, 'refreshRate': 60}]

        def session(*command):
            if command == ('omarchy-shell', 'lock', 'isLocked'):
                return 'false'
            self.assertEqual(command, ('hyprctl', '-j', 'monitors'))
            return json.dumps(monitors)

        lab = Mock()
        lab.ssh_args.return_value = ['owned-ssh-fixture']
        helpers = Mock(return_value=(lab, {'inspect': inspectors, 'remote': remote, 'session': session}))
        writer = Mock()
        with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
            root = Path(directory)
            release = root/'release.json'
            release.write_text(json.dumps({'source_commit': 'a'*40,
                                           'binaries': {'lucent-desktop': {'sha256': 'b'*64}}}))
            stack.enter_context(patch.dict(measurement['main'].__globals__, {
                'ROOT': root, 'load_helpers': helpers, 'private': writer,
            }))
            stack.enter_context(patch.object(sys, 'argv', [
                'measure-daily-resources.py', '--release-manifest', str(release),
                '--label', 'owned-fixture', '--phase', 'animated-background', '--seconds', '10',
            ]))
            # Patch every external-operation boundary, including the initial Git
            # check and sleep. Even the embedded SAMPLER string is never run.
            git = stack.enter_context(patch.object(subprocess, 'run', return_value=Mock(returncode=0)))
            popen = stack.enter_context(patch.object(subprocess, 'Popen', return_value=process))
            sleep = stack.enter_context(patch.object(measurement['time'], 'sleep'))
            stack.enter_context(redirect_stdout(io.StringIO()))
            yield {'run': measurement['main'], 'popen': popen, 'process': process,
                   'writer': writer, 'inspect': inspectors, 'remote': remote,
                   'git': git, 'sleep': sleep}

    def test_invalid_initial_activity_inventory_or_histories_never_start_a_sampler(self):
        policy = PresentationMeasurement()
        mutations = [
            lambda snapshot: snapshot.update(retiring_native_surfaces=1),
            lambda snapshot: snapshot['surfaces'].append(copy.deepcopy(snapshot['surfaces'][0])),
            lambda snapshot: snapshot['surfaces'][0].update(native_generation=0),
            lambda snapshot: snapshot['surfaces'][0]['presentation'].update(samples=[policy.feedback(generation=8)]),
            lambda snapshot: snapshot['client']['live_wallpaper']['producers'][0].update(
                source_frames={'samples': [{'stream_id': 1, 'revision': 1, 'source_ns': 2},
                                           {'stream_id': 1, 'revision': 1, 'source_ns': 3}]}),
        ]
        for mutate in mutations:
            before = self.snapshot()
            mutate(before)
            with self.subTest(snapshot=before), self.harness([before]) as harness:
                with self.assertRaises(AssertionError):
                    harness['run']()
                harness['popen'].assert_not_called()
                harness['writer'].assert_not_called()

    def test_intermediate_change_aborts_even_when_the_final_inventory_would_match(self):
        mutations = [
            lambda snapshot: snapshot['surfaces'].clear(),
            lambda snapshot: snapshot['surfaces'][0].update(native_generation=8),
            lambda snapshot: snapshot['client']['live_wallpaper'].update(preview={'retiring': True}),
            lambda snapshot: snapshot['client']['live_wallpaper']['producers'][0].update(revision=3),
        ]
        for mutate in mutations:
            before, during, after = self.snapshot(), self.snapshot(200), self.snapshot(300)
            mutate(during)
            process = OwnedSampler(pending_polls=None)
            with self.subTest(snapshot=during), self.harness([before, during, after], process) as harness:
                with self.assertRaises(AssertionError):
                    harness['run']()
                self.assertTrue(process.terminated)
                self.assertFalse(process.killed)
                self.assertEqual(process.communications, [5])
                self.assertEqual(harness['inspect'].call_count, 2)
                harness['popen'].assert_called_once()
                harness['writer'].assert_not_called()

    def test_interruption_and_stubborn_owned_process_are_terminated_and_reaped(self):
        process = OwnedSampler(pending_polls=None, stubborn=True)
        with self.harness([self.snapshot(), KeyboardInterrupt()], process) as harness:
            with self.assertRaises(KeyboardInterrupt):
                harness['run']()
            self.assertTrue(process.terminated)
            self.assertTrue(process.killed)
            self.assertEqual(process.communications, [5, None])
            harness['writer'].assert_not_called()

    def test_invalid_final_snapshot_or_submission_counter_never_writes_a_report(self):
        mutations = [
            lambda snapshot: snapshot['surfaces'].clear(),
            lambda snapshot: snapshot['surfaces'][0].update(native_generation=8),
            lambda snapshot: snapshot['surfaces'][0].update(frames=19),
            lambda snapshot: snapshot['surfaces'][0].update(frames=-1),
            lambda snapshot: snapshot['client'].update(wallpaper_presentation={'busy': True}),
            lambda snapshot: snapshot['client'].update(background_transitions=[
                {'presentation': {'transition_pending': True}}]),
        ]
        for mutate in mutations:
            after = self.snapshot(300)
            mutate(after)
            process = OwnedSampler(pending_polls=0)
            with self.subTest(snapshot=after), self.harness([self.snapshot(), after], process) as harness:
                with self.assertRaises(AssertionError):
                    harness['run']()
                harness['writer'].assert_not_called()
                self.assertFalse(process.terminated, 'An exited owned sampler must not be signalled')

    def test_failed_sampler_cannot_supply_a_resource_report(self):
        process = OwnedSampler(returncode=1)
        with self.harness([self.snapshot()], process) as harness:
            with self.assertRaisesRegex(AssertionError, 'Read-only guest sampler failed'):
                harness['run']()
            harness['writer'].assert_not_called()
            self.assertFalse(process.terminated)

    def test_schema_two_reports_verified_cadence_and_submission_keys_without_aliases(self):
        policy = PresentationMeasurement()
        before, during, after = self.snapshot(), self.snapshot(200), self.snapshot(300)
        for snapshot in [before, during, after]:
            snapshot['surfaces'].append(policy.surface('background', 'output'))
            snapshot['surfaces'].append(policy.surface('background [output]'))
        during['surfaces'][0]['presentation']['samples'] = [policy.feedback(1, generation=6), policy.feedback(2)]
        after['surfaces'][0]['presentation']['samples'] = [policy.feedback(2), policy.feedback(3)]
        after['surfaces'][0]['frames'] += 3
        with self.harness([before, during, after]) as harness:
            harness['run']()
            harness['writer'].assert_called_once()
            report = harness['writer'].call_args.args[1]
            self.assertEqual(report['schema'], 2)
            self.assertEqual(len(report['surface_inventory']), 3)
            self.assertEqual(set(report['successful_submission_deltas']), set(report['surface_inventory']))
            self.assertEqual(sorted(report['successful_submission_deltas'].values()), [0, 0, 3])
            key = measurement['surface_key'](after['surfaces'][0])
            self.assertEqual(report['presentation'][key]['samples'], 2)
            self.assertEqual(report['presentation'][key]['intervals'], 1)
            self.assertEqual(report['presentation_identity']['old_generation_history_observations_skipped'], 1)
            self.assertEqual(report['presentation_identity']['verified_surfaces'], 3)
            self.assertTrue(report['preservation']['all_surface_native_generations_verified'])
            self.assertFalse(harness['process'].terminated)
            self.assertEqual(harness['remote'].call_args_list[0].args, ('pgrep', '-x', 'lucent-desktop'))
            harness['popen'].assert_called_once()

    def test_legacy_and_null_generation_main_results_are_resource_only_and_explicit(self):
        policy = PresentationMeasurement()
        before, after = self.snapshot(), self.snapshot(300)
        for snapshot in [before, after]:
            snapshot['surfaces'] = [policy.surface(generation=None, samples=[policy.feedback()]),
                                    {'id': 'legacy', 'frames': 10,
                                     'presentation': {'samples': [policy.feedback()]}}]
        with self.harness([before, after], OwnedSampler(pending_polls=0)) as harness:
            harness['run']()
            report = harness['writer'].call_args.args[1]
            self.assertEqual(report['presentation'], {})
            self.assertEqual(report['stream_cadence'], {})
            self.assertEqual(report['presentation_identity']['verified_surfaces'], 0)
            self.assertEqual(report['presentation_identity']['unverified_surfaces'], 2)
            self.assertEqual(report['presentation_identity']['unverified_history_observations_skipped'], 4)
            self.assertFalse(report['preservation']['all_surface_native_generations_verified'])
            self.assertEqual(report['resources']['rss_mib_max_interval'], 90)
            self.assertIn('resource and logical submission observations only', report['presentation_identity']['scope'])


if __name__ == '__main__':
    unittest.main()
