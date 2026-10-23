"""Pure ownership/geometry policy for the guarded native multi-output test.

No subprocesses, desktop access, global environment reads or import side effects.
Pinned Hyprland layer geometry and cursor coordinates are global logical values.
"""
import math
import re


class Blocked(RuntimeError):
    """A missing safety/acceptance precondition; never evidence of a pass."""


def require(condition, label):
    if not condition:
        raise Blocked(label)


def unique(values, predicate, label):
    selected = [value for value in values if predicate(value)]
    require(len(selected) == 1, label)
    return selected[0]


def owned_name(value):
    require(isinstance(value, str) and re.fullmatch(r'LUCENT-TEST-[a-f0-9]{24}', value),
            'invalid_owned_output_name')
    return value


def finite(value):
    require(type(value) in (int, float) and math.isfinite(value), 'nonfinite_geometry')
    return value


def monitor_signature(value):
    """Stable native identity/layout; focus/workspace/reservation are observations."""
    fields = ('id', 'name', 'description', 'width', 'height', 'x', 'y', 'scale',
              'transform', 'refreshRate', 'disabled', 'dpmsStatus')
    require(all(key in value for key in fields), 'incomplete_native_monitor_identity')
    for key in ('width', 'height', 'x', 'y', 'scale', 'refreshRate'):
        finite(value[key])
    require(value['width'] > 0 and value['height'] > 0 and value['scale'] > 0
            and value['transform'] == 0 and value['disabled'] is False
            and value['dpmsStatus'] is True, 'unsupported_monitor_layout')
    return {key: value[key] for key in fields}


def unchanged_layout(monitors, baseline, name, expected):
    """Allow only the exact output/lifetime currently owned by this lease."""
    owned_name(name)
    require(len({m['name'] for m in monitors}) == len(monitors), 'ambiguous_output_names')
    others = [monitor_signature(m) for m in monitors if m['name'] != name]
    require(sorted(others, key=lambda m: m['name']) ==
            sorted(baseline, key=lambda m: m['name']), 'foreign_layout_changed')
    found = [m for m in monitors if m['name'] == name]
    require(len(found) == (1 if expected is not None else 0), 'owned_output_lifetime_changed')
    if expected is not None:
        require(monitor_signature(found[0]) == expected, 'owned_output_layout_changed')
    return found[0] if found else None


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


def case_layout(name, baseline, scale):
    owned_name(name)
    require(type(scale) in (float, int) and scale in (1, 2, 1.25), 'unowned_scale_request')
    require(len(baseline) == 1, 'one_original_output_required')
    original = baseline[0]
    # Finite fixed physical workload, with an unambiguous nonoverlapping origin.
    right = finite(original['x']) + finite(original['width']) / finite(original['scale'])
    require(right.is_integer() if isinstance(right, float) else True, 'fractional_original_edge')
    return dict(name=name, width=1280, height=960, refresh=60,
                x=int(right) + 32, y=int(original['y']), scale=scale, transform=0)


def matches_case(monitor, case):
    return (all(monitor.get(k) == case[k] for k in ('name', 'width', 'height', 'x', 'y', 'transform'))
            and math.isclose(finite(monitor['scale']), case['scale'], abs_tol=.0001)
            and math.isclose(finite(monitor['refreshRate']), case['refresh'], abs_tol=.05))


def host(state, name):
    require(state.get('client', {}).get('named_monitor_hosts') is True,
            'named_native_hosts_required')
    return unique(state['client']['monitor_hosts'], lambda h: h['id'] == name,
                  'one_current_monitor_host_required')


def output(state, name):
    return unique(state.get('outputs', []), lambda o: o['name'] == name,
                  'one_current_wayland_global_required')


def surface(state, name, role):
    current = host(state, name)
    return unique(state['surfaces'],
                  lambda s: s.get('output') == name and s.get('role', s.get('id')) == role
                  and s.get('instance') == current['instance'],
                  'one_current_host_surface_required')


def layer_for(layers, name, source, pid):
    """Do not search across monitors and accidentally select the focused host."""
    require(name in layers and isinstance(layers[name], dict), 'native_layer_monitor_missing')
    found = []

    def visit(value):
        if isinstance(value, dict):
            if value.get('namespace') == 'lucent-' + source['id']:
                found.append(value)
            else:
                for child in value.values():
                    visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)

    visit(layers[name])
    result = unique(found, lambda _: True, 'one_native_layer_on_exact_output_required')
    require(result.get('pid') == pid and isinstance(result.get('address'), str)
            and result['address'], 'native_layer_owner_changed')
    require(math.isclose(finite(result['w']), finite(source['width']), abs_tol=1)
            and math.isclose(finite(result['h']), finite(source['height']), abs_tol=1),
            'framework_native_geometry_not_settled')
    return result


def hit_center(monitor, source, layer, identifier):
    hit = unique(source['hits'], lambda h: h['id'] == identifier, 'one_live_hit_required')
    require(source.get('pointer_input') != 'none', 'pointer_input_disabled')
    clip = hit['clip']
    left = max(finite(hit['x']), finite(clip['x']), 0)
    top = max(finite(hit['y']), finite(clip['y']), 0)
    right = min(finite(hit['x']) + finite(hit['width']),
                finite(clip['x']) + finite(clip['width']), finite(source['width']))
    bottom = min(finite(hit['y']) + finite(hit['height']),
                 finite(clip['y']) + finite(clip['height']), finite(source['height']))
    require(right - left >= 4 and bottom - top >= 4, 'hit_not_safely_visible')
    local = ((left + right) / 2, (top + bottom) / 2)
    # The pinned Hyprland layers JSON reports the arranged global logical box.
    # SDK hits are surface-local logical values; neither the monitor origin nor
    # its fractional density belongs in this addition a second time.
    x = round(finite(layer['x']) + local[0])
    y = round(finite(layer['y']) + local[1])
    require(monitor['x'] <= x < monitor['x'] + monitor['width'] / monitor['scale']
            and monitor['y'] <= y < monitor['y'] + monitor['height'] / monitor['scale'],
            'hit_outside_target_output')
    return x, y


def button_event(down):
    require(type(down) is bool, 'button_state_must_be_boolean')
    return {'type': 'btn', 'data': {'down': down, 'button': 'left'}}


def cursor_matches(position, expected):
    return (isinstance(position, dict) and all(type(position.get(axis)) in (int, float)
            and math.isfinite(position[axis]) and abs(position[axis] - value) <= 1
            for axis, value in zip(('x', 'y'), expected)))


def require_owned_layers(layers, pid):
    require(type(pid) is int and pid > 1, 'captured_desktop_layer_owner_required')
    def visit(value):
        if isinstance(value, dict):
            if 'namespace' in value:
                require(isinstance(value['namespace'], str) and value['namespace'].startswith('lucent-')
                        and not value['namespace'].startswith('lucent-authorization-')
                        and value.get('pid') == pid, 'foreign_native_layer_owner_or_authorization')
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)
    visit(layers)


def require_unbound_input(bindings):
    require(type(bindings) is list and len(bindings) <= 4096, 'bounded_native_bindings_required')
    symbols = {'escape', 'esc', 'mouse:272', 'mouse:left', 'catchall', 'any',
               'code:1', 'code:9', 'code:272', 'code:280'}
    for binding in bindings:
        require(type(binding) is dict and type(binding.get('modmask')) is int,
                'complete_native_binding_required')
        if binding['modmask'] & ~(2 | 16) != 0:
            continue
        require(not binding.get('catch_all', False)
                and str(binding.get('key', '')).lower() not in symbols
                and binding.get('keycode', 0) not in (1, 9, 272, 280),
                'escape_or_plain_pointer_binding_intercepts_native_test')


class CursorOwnership:
    """Only captured initial position or an exact declared transition is owned."""
    def __init__(self, initial):
        require(len(initial) == 2 and all(type(value) is int for value in initial),
                'exact_initial_native_cursor_required')
        self.expected = tuple(initial)
        self.pending = None
        self.lost = False

    def owns(self, current):
        if self.lost:
            return False
        # Exact integer cursorpos, not an arbitrary nearby point or backend warp.
        point = ((current.get('x'), current.get('y')) if isinstance(current, dict)
                 and all(type(current.get(key)) is int for key in ('x', 'y')) else None)
        if point is None or point not in (self.expected, self.pending):
            self.lost = True
            return False
        if self.pending is not None and point == self.pending:
            self.expected, self.pending = self.pending, None
        return True

    def check(self, current):
        require(self.owns(current), 'foreign_or_uncertain_cursor_movement_blocks_warp')

    def begin(self, current, destination):
        self.check(current)
        require(len(destination) == 2 and all(type(value) is int for value in destination),
                'exact_guardian_cursor_destination_required')
        self.pending = tuple(destination)

    def confirm(self, current):
        require(self.pending is not None, 'declared_cursor_transition_required')
        point = ((current.get('x'), current.get('y')) if isinstance(current, dict)
                 and all(type(current.get(key)) is int for key in ('x', 'y')) else None)
        if point != self.pending:
            self.lost = True
            raise Blocked('native_cursor_postimage_differs_from_declared_transition')
        self.expected, self.pending = self.pending, None


def host_surface_ids(state, name, instance):
    """Capture monitor-owned IDs, excluding shared surfaces retargeted on focus.

    The shared dock has no host instance. Its current output is an observation,
    whereas the nonempty captured instance identifies the host-owned surfaces.
    Keep their IDs so dropping/changing an instance after removal cannot hide a
    stale native surface from the subsequent retirement assertion.
    """
    require(isinstance(instance, str) and bool(instance)
            and host(state, name)['instance'] == instance, 'captured_monitor_host_instance_required')
    owned = [item for item in state['surfaces'] if item.get('instance') == instance]
    require(owned and all(item.get('output') == name and isinstance(item.get('id'), str)
                         and item['id'] for item in owned), 'captured_host_surfaces_required')
    result = {item['id'] for item in owned}
    require(len(result) == len(owned), 'ambiguous_captured_host_surface_ids')
    return result


def retired(state, name, instance, native_ids):
    require(isinstance(instance, str) and bool(instance) and bool(native_ids),
            'captured_host_retirement_authority_required')
    return (not any(h['id'] == name or h.get('instance') == instance
                    for h in state['client']['monitor_hosts'])
            and not any(o.get('name') == name for o in state['outputs'])
            and not any(s['id'] in native_ids or s.get('instance') == instance or s.get('output') == name
                        for s in state['surfaces'])
            and state.get('retiring_native_surfaces') == 0)
