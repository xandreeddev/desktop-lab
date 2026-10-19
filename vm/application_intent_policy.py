"""Inert, pure input policy for the owned graphical application-intent test."""
import math


def require(condition, label):
    if not condition:
        raise ValueError(label)


def unbound_pointer(bindings):
    require(type(bindings) is list and len(bindings) <= 4096, 'bounded_actual_pointer_bindings')
    for binding in bindings:
        require(type(binding) is dict and type(binding.get('modmask')) is int,
                'complete_actual_pointer_binding')
        if binding['modmask'] & ~(2 | 16):
            continue
        require(not binding.get('catch_all', False)
                and str(binding.get('key', '')).lower() not in
                ('mouse:272', 'mouse:left', 'catchall', 'any', 'code:272', 'code:280')
                and binding.get('keycode', 0) not in (272, 280),
                'plain_pointer_binding_intercepts_owned_input')


def target(surface, layer, identifier):
    require(surface.get('id') == 'intent' and surface.get('frames', 0) > 0
            and surface.get('native_frames', 0) > 0 and surface.get('pointer_input') in ('all', 'content'),
            'submitted_owned_intent_surface_required')
    hits = [hit for hit in surface['hits'] if hit['id'] == identifier]
    require(len(hits) == 1, 'one_current_owned_intent_hit')
    hit = hits[0]
    clip = hit['clip']
    values = [source.get(key) for source, keys in (
        (surface, ('width', 'height')), (layer, ('x', 'y', 'w', 'h')),
        (hit, ('x', 'y', 'width', 'height')), (clip, ('x', 'y', 'width', 'height')))
        for key in keys]
    require(all(type(value) in (int, float) and math.isfinite(value) for value in values),
            'finite_owned_intent_geometry')
    require(abs(layer['w'] - surface['width']) <= .5
            and abs(layer['h'] - surface['height']) <= .5, 'current_owned_native_intent_geometry')
    left, top = max(hit['x'], clip['x'], 0), max(hit['y'], clip['y'], 0)
    right = min(hit['x'] + hit['width'], clip['x'] + clip['width'], surface['width'])
    bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'], surface['height'])
    require(right - left > 4 and bottom - top > 4
            and right - left >= hit['width'] - .5 and bottom - top >= hit['height'] - .5,
            'fully_visible_owned_intent_control')
    point = (round(layer['x'] + (left + right) / 2), round(layer['y'] + (top + bottom) / 2))
    require(0 <= point[0] < 1920 and 0 <= point[1] < 1080, 'owned_intent_point_on_prepared_output')
    return {'id': surface['id'], 'instance': surface.get('instance'), 'output': surface.get('output'),
            'layer': dict(layer), 'hit': {key: hit[key] for key in ('id', 'x', 'y', 'width', 'height', 'clip')},
            'point': point}


def same_target(before, after):
    require(before == after, 'current_owned_intent_hit_and_layer_lifetime')


def owned_layers(layers, desktop_pid, example_pid=None):
    require(type(desktop_pid) is int and desktop_pid > 1, 'identified_desktop_layer_owner')
    found = []
    count = 0
    def walk(value, depth=0):
        nonlocal count
        count += 1
        require(count <= 8192 and depth <= 16, 'bounded_native_layer_tree')
        if isinstance(value, dict):
            if 'namespace' in value:
                namespace = value['namespace']
                require(isinstance(namespace, str), 'concrete_native_layer_namespace')
                if namespace == 'application-intent-intent':
                    require(example_pid is not None and value.get('pid') == example_pid,
                            'exact_owned_example_native_layer_pid')
                    require(isinstance(value.get('address'), str) and value['address'],
                            'concrete_owned_example_native_layer')
                    found.append({key: value[key] for key in
                                  ('address', 'namespace', 'pid', 'x', 'y', 'w', 'h')})
                else:
                    require(namespace.startswith('lucent-') and not namespace.startswith('lucent-authorization-')
                            and value.get('pid') == desktop_pid, 'exact_desktop_native_layer_pid')
            for child in value.values():
                walk(child, depth + 1)
        elif isinstance(value, list):
            for child in value:
                walk(child, depth + 1)
    walk(layers)
    require(len(found) == (1 if example_pid else 0), 'one_exact_owned_example_layer')
    return found[0] if found else None


def pointer_destination(point):
    require(len(point) == 2 and all(type(value) is int for value in point)
            and 0 <= point[0] < 1920 and 0 <= point[1] < 1080, 'bounded_exact_pointer_destination')


def compositor_move(point):
    """Position the test cursor in the observed compositor's logical pixels.

    Positioning is not a physical-input proof. Tests still deliver button/key
    events through QMP/evdev and verify their actual native focus or effect.
    """
    pointer_destination(point)
    return f'hl.dsp.cursor.move({{x={point[0]},y={point[1]}}})'


def absolute_move(point):
    pointer_destination(point)
    return [{'type': 'abs', 'data': {'axis': axis, 'value': round(value / extent * 32767)}}
            for axis, value, extent in (('x', point[0], 1919), ('y', point[1], 1079))]


def socket_peer(peer, expected_pid, uid):
    require(len(peer) == 3 and all(type(value) is int for value in peer)
            and peer[0] == expected_pid and peer[1] == uid,
            'exact_owned_example_socket_peer')


class CursorOwnership:
    """Input never adopts independent movement, even when it later returns."""
    def __init__(self, initial):
        require(type(initial) is dict and all(type(initial.get(key)) is int for key in ('x', 'y')),
                'exact_initial_application_cursor')
        self.initial = self.expected = (initial['x'], initial['y'])
        self.pending = None
        self.lost = False

    def observe(self, current):
        point = tuple(current.get(key) for key in ('x', 'y')) if type(current) is dict else None
        if self.lost or point is None or not all(type(value) is int for value in point) \
                or point not in (self.expected, self.pending):
            self.lost = True
        require(not self.lost, 'application_cursor_authority_lost')
        return point

    def begin(self, current, destination):
        self.observe(current)
        absolute_move(destination)
        require(self.pending is None, 'one_declared_application_cursor_transition')
        self.pending = tuple(destination)

    def finish(self, current):
        point = self.observe(current)
        require(self.pending is not None and point == self.pending, 'application_cursor_transition_not_confirmed')
        self.expected, self.pending = point, None
