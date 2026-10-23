"""Pure policy for owned Display settings acceptance.

Display-specific geometry, mode/control rules and one declared disable cursor
transition stay separate from the shared native-output/input policy. Import has
no OS access or side effects.
"""
import math
import re

from multi_output_policy import (finite, host, monitor_signature, output, owned_name,
                                 require, retired, unique)

__all__ = ('display_peer_signature', 'display_layout', 'display_logical_size',
           'display_mode_index', 'require_owned_display_control', 'DisplayDisableCursor')


def display_peer_signature(value, identity):
    """Display-only observation; ordinary output leases remain enabled/unrotated.

    A disabled native monitor can have no usable mode or Wayland global. Retain
    its exact native identity rather than interpreting those inactive dimensions
    as a rendered viewport or adopting another same-name output.
    """
    owned_name(identity['name'])
    require(set(identity)=={'id','name','description'},'exact_owned_display_identity_fields_required')
    require(all(value.get(key) == identity[key] for key in ('id', 'name', 'description')),
            'owned_display_native_identity_changed')
    require(type(identity['id']) is int and identity['id'] >= 0
            and isinstance(identity['description'], str), 'owned_display_identity_required')
    require(type(value.get('disabled')) is bool, 'native_display_enabled_state_required')
    if value['disabled']:
        return {**identity, 'disabled': True}
    transform = value.get('transform')
    require(type(transform) is int and transform in (0, 1, 2, 3),
            'owned_display_rotation_outside_test_cases')
    signature = monitor_signature({**value, 'transform': 0})
    return {**signature, 'transform': transform}


def display_layout(monitors, baseline, identity):
    """Compare the physical/original output exactly, including disabled peers."""
    name = owned_name(identity['name'])
    require(type(monitors) is list and len(monitors) == len(baseline) + 1
            and len({row.get('name') for row in monitors}) == len(monitors),
            'exact_original_and_owned_display_inventory_required')
    others = [monitor_signature(row) for row in monitors if row.get('name') != name]
    require(sorted(others, key=lambda row: row['name']) == sorted(baseline, key=lambda row: row['name']),
            'foreign_layout_changed')
    peer = unique(monitors, lambda row: row.get('name') == name, 'owned_display_missing')
    return display_peer_signature(peer, identity)


def display_logical_size(monitor):
    require(monitor.get('disabled') is False and type(monitor.get('transform')) is int
            and monitor['transform'] in (0, 1, 2, 3),
            'enabled_display_viewport_required')
    width, height = finite(monitor['width']), finite(monitor['height'])
    if monitor['transform'] % 2:
        width, height = height, width
    scale = finite(monitor['scale'])
    require(width > 0 and height > 0 and scale > 0, 'positive_display_viewport_required')
    return width / scale, height / scale


def display_mode_index(monitor, width, height, millihertz):
    """Identify a real mode button from the adapter's ordered native catalogue.

    This selects no mode itself. The controller must still press the rendered
    hit, then observe the native postimage produced by Apply.
    """
    modes = []
    raw = monitor.get('availableModes', [])
    require(type(raw) is list and len(raw) <= 4096, 'bounded_native_mode_catalogue_required')
    for item in raw[:256]:
        if not isinstance(item,str) or len(item)>128:
            continue
        while item.endswith('Hz'):
            item=item[:-2]
        match = re.fullmatch(r'([+]?[0-9]+)x([+]?[0-9]+)@([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)',item)
        if match:
            hz=float(match[3]);w,h=int(match[1]),int(match[2])
            if not math.isfinite(hz) or not (0<hz<=1000.001 and 0<w<=32768 and 0<h<=32768):
                continue
            mode = (w,h,math.floor(hz * 1000 + .5))
            if not 1000<=mode[2]<=1_000_000:
                continue
            if mode not in modes:
                modes.append(mode)
    observed = (monitor.get('width'), monitor.get('height'), monitor.get('refreshRate'))
    if (all(type(value) is int and 0<value<=32768 for value in observed[:2])
            and type(observed[2]) in (int,float) and math.isfinite(observed[2]) and 0<observed[2]<=1000.001):
        current = (int(observed[0]), int(observed[1]), math.floor(observed[2] * 1000 + .5))
        if 1000<=current[2]<=1_000_000 and current not in modes:
            modes.insert(0, current)
    require((width, height, millihertz) in modes, 'required_owned_native_mode_not_advertised')
    return modes.index((width, height, millihertz))


def require_owned_display_control(identifier, selected, name):
    owned_name(name)
    require(isinstance(identifier,str) and
            (identifier in ('display-reset','display-select-'+name) or
             (selected==name and not identifier.startswith('display-select-'))),
            'native_input_must_target_only_owned_display_draft')


class DisplayDisableCursor:
    """One declared compositor relocation, with no authority before retirement.

    Reviewed efb50993780079460b0cbed1363e2166a2de1d9f source:
    src/config/shared/monitor/MonitorRuleManager.cpp:186-187 calls onDisconnect
    after applying a disabled rule. src/output/Monitor.cpp:478-480 force-warps
    to the sole backup's transformed physical center. Our backup is 1x/even.
    This does not authorize any unobserved path, re-enable warp or other point.
    """
    SOURCES = {
        'MonitorRuleManager.cpp': '17d46685c2623657f52e71b22a033c0a798cf3903f7bbeb64ceebce87527f016',
        'Monitor.cpp': '9cf88e154eb5dae676c79d37b5b055ca6134838857cecdbb89a3b747a6821927',
        'PointerController.cpp': '4aef766cf4205222ef143b1432d2598c7beab1527aee5e19aa58f17a1890d899',
    }

    def __init__(self, cursor, baseline, identity, peer_host, peer_surfaces,
                 original_host, original_global, transaction, now):
        require(not cursor.lost and cursor.pending is None, 'owned_cursor_before_disable_required')
        require(len(baseline) == 1, 'sole_backup_for_disable_required')
        backup = monitor_signature(baseline[0])
        require(backup['scale'] == 1 and all(type(backup[k]) is int for k in ('x', 'y', 'width', 'height'))
                and backup['width'] % 2 == backup['height'] % 2 == 0,
                'exact_even_1x_backup_center_required')
        owned_name(identity['name'])
        require(type(transaction) is int and transaction > 0 and bool(peer_surfaces),
                'captured_pre_disable_transaction_and_surfaces_required')
        self.preimage = cursor.expected
        self.destination = (backup['x'] + backup['width'] // 2, backup['y'] + backup['height'] // 2)
        require(self.preimage != self.destination, 'distinct_declared_disable_postimage_required')
        self.identity = dict(identity)
        self.peer_instance, self.peer_surfaces = peer_host['instance'], set(peer_surfaces)
        self.original_host, self.original_global = dict(original_host), original_global
        self.transaction = transaction
        self.started = finite(now)
        self.deadline = self.started + 5
        self.consumed = False

    def confirm(self, cursor, current, state, peer, now):
        require(not self.consumed and self.started <= finite(now) <= self.deadline,
                'declared_disable_cursor_window_expired_or_consumed')
        require(not cursor.lost and cursor.pending is None and cursor.expected == self.preimage,
                'pre_disable_cursor_authority_changed')
        require(type(current) is dict and all(type(current.get(k)) is int for k in ('x', 'y'))
                and (current['x'], current['y']) == self.destination,
                'cursor_does_not_match_declared_disable_postimage')
        require(peer == {**self.identity, 'disabled': True}, 'actual_owned_native_disable_required')
        original = host(state, self.original_host['id'])
        require(all(original[key] == value for key, value in self.original_host.items())
                and output(state, self.original_host['id'])['global'] == self.original_global,
                'original_backup_lifetime_changed')
        child = state['client']['displays'];transaction = child.get('transaction') or {}
        require(child['open'] is True and child['visible'] is True
                and child['selected'] == self.identity['name'], 'disable_transaction_must_target_owned_peer')
        if transaction.get('id') == self.transaction:
            require(child['busy'] is True, 'new_owned_disable_transaction_required')
            return False  # Native reply can precede the client's transaction event.
        require(type(transaction.get('id')) is int and transaction['id'] > self.transaction
                and transaction.get('phase') in ('preparing', 'confirmation', 'kept'),
                'new_owned_disable_transaction_required')
        if not retired(state, self.identity['name'], self.peer_instance, self.peer_surfaces):
            return False
        cursor.expected = self.destination
        self.consumed = True
        return True
