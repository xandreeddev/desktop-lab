"""Pure lease policy for one temporary, resource-only 4K VM output profile.

No IO, clocks, rendering, configuration writes or input synthesis lives here.
Presentation tracing is deliberately outside this owner's restoration authority.
"""
import copy
import math
import re

from multi_output_policy import Blocked, monitor_signature, require, unique

LEASE_SECONDS = 900
CLEANUP_RESERVE = 120
SOURCE = '9eb41939d47ced5be2012bff41a1f2f5768d6ee7'
RELEASE_SHA = 'c669b9d53879d504f0a4efa5e131c93f57cc77bc8c26c18dacef54565fe2703f'
DESKTOP_SHA = 'eb1980a5c81a60512218eb69fcee80fd1124a1112c317f7dbb66ef95e311a1a5'
MEDIA_SHA = '9bfaece1a34f02067005190a26c9dc5ba0e83e74875653c8336c022d133ebc35'
PROFILE = dict(name='Virtual-1', width=3840, height=2160, x=0, y=0,
               scale=1.5, transform=0, refreshRate=60.0)
BASELINE = {**PROFILE, 'width': 1920, 'height': 1080, 'scale': 1}
INVARIANTS = ('compositor', 'service', 'supervisor', 'process', 'global', 'host')


def resource_mode(value):
    require(value == 'resources', 'presentation_requires_nested_restore_authority')


def tag(value):
    require(isinstance(value, str) and re.fullmatch('[a-f0-9]{24}', value), 'exact_output_lease_tag_required')
    return value


def profile(monitor):
    value = monitor_signature(monitor)
    return {key: value[key] for key in PROFILE}


def original(monitors):
    require(type(monitors) is list and len(monitors) == 1, 'one_original_vm_output_required')
    value = monitor_signature(monitors[0])
    require(profile(value) == BASELINE, 'original_1080p60_one_scale_required')
    modes = monitors[0].get('availableModes')
    require(type(modes) is list and len(modes) <= 512 and any(
        isinstance(mode, str) and re.fullmatch(r'3840x2160@60(?:\.0+)?Hz', mode) for mode in modes),
        'advertised_exact_4k60_mode_required')
    return value


def same_output(value, baseline, expected):
    current = monitor_signature(value)
    require({key: current[key] for key in ('id', 'name', 'description')} ==
            {key: baseline[key] for key in ('id', 'name', 'description')}, 'original_native_output_replaced')
    require(current == expected, 'foreign_or_uncertain_native_profile_preserved')


def normal_off(state):
    require(state.get('frame_profiling') is False
            and state.get('presentation_feedback', {}).get('enabled') is False
            and all(row.get('source_frames') is None
                    for row in state['client']['live_wallpaper']['producers']),
            'resource_owner_requires_all_runtime_tracing_off')


def ready_geometry(state):
    normal_off(state)
    host = unique(state['client']['monitor_hosts'], lambda row: row['id'] == PROFILE['name'],
                  'one_profile_monitor_host_required')
    background = unique(state['surfaces'], lambda row:
        row.get('role', row['id']) == 'background' and row.get('output') == PROFILE['name']
        and row.get('instance') == host['instance'], 'one_profile_background_required')
    require(all(background.get(key) == value for key, value in {
        'width': 2560, 'height': 1440, 'density': 1.5,
        'buffer_width': 3840, 'buffer_height': 2160}.items())
        and background.get('frames', 0) > 0 and background.get('native_frames', 0) > 0,
        'physical_4k_fractional_background_not_ready')
    live = state['client']['live_wallpaper']
    require(live['players'] == live['running'] == 1 and not live.get('awaiting_first_frame')
            and len(live['producers']) == 1, 'one_running_original_live_producer_required')
    producer = live['producers'][0]
    require(producer.get('target') == [3840, 2160] and producer.get('frame_size') == [3840, 2160]
            and producer.get('source_aspect') == [3840, 2160]
            and type(producer.get('revision')) is int and producer['revision'] > 0,
            'actual_source_sized_4k_frame_required')
    return {'background': {key: background[key] for key in
            ('id', 'instance', 'width', 'height', 'density', 'buffer_width', 'buffer_height')},
            'producer': {key: producer[key] for key in ('revision', 'target', 'frame_size', 'source_aspect')}}


def same_authority(current, baseline):
    require(all(current[key] == baseline[key] for key in INVARIANTS),
            'original_compositor_output_or_runtime_lifetime_changed')
    require(current['off'] is True, 'resource_owner_requires_all_runtime_tracing_off')
    require(current['source_proof'] == baseline['source_proof'], 'source_config_security_or_settings_changed')


def mode_code(monitor):
    """Only the captured baseline or the fixed owned profile can be submitted."""
    value = profile(monitor)
    require(value in (BASELINE, PROFILE), 'undeclared_output_configuration_refused')
    mode = f"{value['width']}x{value['height']}@60"
    return ('hl.monitor({output="Virtual-1",disabled=false,mode="' + mode
            + '",position="0x0",scale=' + str(value['scale']) + ',transform=0})')


class OutputLease:
    """One attempt, with independently executable cleanup after a lost reply."""
    def __init__(self, backend, now):
        require(type(now) in (int, float) and math.isfinite(now), 'finite_monotonic_clock_required')
        self.backend = backend
        self.deadline = now + LEASE_SECONDS
        self.before = copy.deepcopy(backend.observe(full=True, idle=True))
        self.original = original([self.before['monitor']])
        require(self.before['off'] is True, 'resource_owner_requires_all_runtime_tracing_off')
        require(0 <= self.before['cursor']['x'] < 1920 and 0 <= self.before['cursor']['y'] < 1080,
                'initial_cursor_inside_both_profiles_required')
        self.target = {**self.original, **PROFILE}
        self.state = 'prepared'
        self.attempted = False
        self.restore_attempted = False
        self.geometry = None
        self.backend.record('prepared', {**self.summary(), 'before': self.before})

    def summary(self):
        return {'state': self.state, 'deadline': self.deadline, 'attempted': self.attempted,
                'restore_attempted': self.restore_attempted, 'original': self.original,
                'target': self.target, 'geometry': self.geometry}

    def checked(self, expected, *, idle=False):
        current = self.backend.observe(full=True, idle=idle)
        same_authority(current, self.before)
        same_output(current['monitor'], self.original, expected)
        return current

    def apply(self, now):
        require(self.state == 'prepared' and not self.attempted, 'output_apply_is_single_use')
        require(now < self.deadline - CLEANUP_RESERVE, 'output_lease_cleanup_reserve_required')
        self.checked(self.original, idle=True)
        require(max(now, self.backend.now()) < self.deadline - CLEANUP_RESERVE,
                'output_lease_cleanup_reserve_consumed_by_proof')
        self.attempted = True
        self.state = 'apply-intent'
        self.backend.record('apply-intent', self.summary())
        # A lost reply is not permission to repeat this command. Cleanup can
        # reconcile only the exact original or declared postimage, independently.
        self.backend.configure(self.target, expected=self.original)
        self.backend.settle(self.target, self.deadline - CLEANUP_RESERVE)
        self.checked(self.target, idle=True)
        self.geometry = self.backend.geometry()
        self.checked(self.target, idle=True)
        self.state = 'active'
        self.backend.record('active', self.summary())
        return self.summary()

    def facts(self, now):
        require(now < self.deadline - CLEANUP_RESERVE and self.state == 'active',
                'active_unexpired_resource_output_required')
        current = self.checked(self.target, idle=True)
        geometry = self.backend.geometry()
        current = self.checked(self.target, idle=True)
        observed_at = max(now, self.backend.now())
        require(observed_at < self.deadline - CLEANUP_RESERVE, 'resource_facts_expired_during_proof')
        return {'read_only': True, 'measured': False, 'native_or_performance_accepted': False,
                'profile': '4k60-1p5x', 'expected_monitor': PROFILE,
                'authority': current, 'geometry': geometry, 'remaining_seconds': self.deadline - observed_at}

    def cleanup(self):
        # Source/layout authority is independent of cursor/focus preservation.
        # Never synthesize input or overwrite a concurrent native profile.
        current = self.backend.observe(full=True, idle=False)
        same_authority(current, self.before)
        same_output(current['monitor'], self.original,
                    self.target if profile(current['monitor']) == PROFILE else self.original)
        if monitor_signature(current['monitor']) == self.target:
            require(self.attempted and not self.restore_attempted, 'unowned_or_repeated_restore_refused')
            self.restore_attempted = True
            self.state = 'restore-intent'
            self.backend.record('restore-intent', self.summary())
            self.backend.configure(self.original, expected=self.target)
            self.backend.settle(self.original, self.deadline + CLEANUP_RESERVE)
        # A retry after an uncertain restore only observes its original postimage.
        restored = self.checked(self.original)
        self.state = 'restored'
        result = {**self.summary(), 'output_restored': True, 'configuration_security_runtime_preserved': True,
                  'cursor_unchanged': restored['cursor'] == self.before['cursor'],
                  'workspace_unchanged': restored['workspace'] == self.before['workspace'],
                  'input_synthesized': False, 'windows_closed': False, 'runtime_restarted': False}
        self.backend.record('restored', result)
        return result
