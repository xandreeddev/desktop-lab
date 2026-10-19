"""Pure ownership/geometry policy for the guarded native multi-output test.

No subprocesses, desktop access, global environment reads or import side effects.
Hyprland layer geometry is output-local; its cursor coordinates are global.
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


def retired(state, name, native_ids):
    return (not any(h['id'] == name for h in state['client']['monitor_hosts'])
            and not any(o.get('name') == name for o in state['outputs'])
            and not any(s['id'] in native_ids or s.get('output') == name
                        for s in state['surfaces'])
            and state.get('retiring_native_surfaces') == 0)
