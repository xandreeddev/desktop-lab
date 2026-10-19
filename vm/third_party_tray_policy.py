"""Pure safety rules for genuine nm-applet tray acceptance, with no OS access."""
import hashlib
import json
import math
import re


class Blocked(RuntimeError):
    pass


class Unsupported(Blocked):
    pass


def require(condition, label):
    if not condition:
        raise Blocked(label)


def unique(values, predicate, label):
    matches = [value for value in values if predicate(value)]
    require(len(matches) == 1, label)
    return matches[0]


def unit_name(value):
    require(isinstance(value, str)
            and re.fullmatch(r'lucent-third-party-tray-[a-f0-9]{24}\.service', value),
            'invalid_owned_unit_name')
    return value


def cgroup_path(value, uid, unit):
    unit_name(unit)
    expected = f'/user.slice/user-{uid}.slice/user@{uid}.service/app.slice/{unit}'
    require(value == expected, 'exact_owned_applet_cgroup_required')
    return value


def label(value):
    require(isinstance(value, str) and len(value) <= 256, 'bounded_menu_label_required')
    result, index = [], 0
    while index < len(value):
        if value[index] == '_':
            index += 1
            if index < len(value) and value[index] == '_':
                result.append('_')
                index += 1
        else:
            result.append(value[index])
            index += 1
    return ''.join(result)


def information_entry(root):
    """Accept one exact, enabled root leaf; never a toggle or a fuzzy label."""
    count, ids = 0, set()
    def validate(node, depth):
        nonlocal count
        count += 1
        require(depth <= 8 and count <= 256 and type(node) is dict
                and type(node.get('id')) is int and node['id'] not in ids
                and type(node.get('properties')) is dict and type(node.get('children')) is list,
                'bounded_unique_menu_tree_required')
        ids.add(node['id'])
        for child in node['children']:
            validate(child, depth + 1)
    validate(root, 0)
    require(root['id'] == 0, 'root_menu_required')
    entry = unique(root['children'],
                   lambda node: label(node['properties'].get('label', '')) == 'Connection Information',
                   'one_safe_information_action_required')
    props = entry['properties']
    require(entry['id'] > 0 and props.get('enabled', True) is True
            and props.get('visible', True) is True and props.get('type', 'standard') == 'standard'
            and props.get('toggle-type', '') == '' and 'toggle-state' not in props
            and props.get('children-display', '') == '' and not entry['children'],
            'information_action_must_be_visible_enabled_nonmutating_leaf')
    return entry['id']


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                     ensure_ascii=True).encode()).hexdigest()


def current_menu(before, after):
    fields = ('owner', 'pid', 'item_path', 'menu_path', 'revision', 'layout_sha256', 'information_id')
    require(all(before.get(field) == after.get(field) for field in fields)
            and all(field in before for field in fields), 'genuine_menu_or_owner_retired_before_release')


def owned_process(intent, current):
    fields = ('pid', 'start', 'cgroup')
    require(all(intent.get(field) == current.get(field) for field in fields),
            'owned_applet_process_lifetime_changed')
    require(current.get('executable_sha256') == intent.get('applet_sha256')
            and current.get('argv') == ['/usr/bin/nm-applet', '--indicator', '--no-agent']
            and current.get('token') == intent.get('token')
            and isinstance(intent.get('token'), str) and len(intent['token']) == 24,
            'owned_genuine_no_agent_applet_required')


def visible_point(surface, layer, identifier):
    hit = unique(surface['hits'], lambda value: value['id'] == identifier, 'one_current_tray_hit_required')
    clip = hit['clip']
    values = [hit.get(k) for k in ('x', 'y', 'width', 'height')]
    values += [clip.get(k) for k in ('x', 'y', 'width', 'height')]
    require(all(type(value) in (int, float) and math.isfinite(value) for value in values),
            'finite_tray_geometry_required')
    x = max(hit['x'], clip['x'], 0)
    y = max(hit['y'], clip['y'], 0)
    right = min(hit['x'] + hit['width'], clip['x'] + clip['width'], surface['width'])
    bottom = min(hit['y'] + hit['height'], clip['y'] + clip['height'], surface['height'])
    require(right - x >= hit['width'] - .5 and bottom - y >= hit['height'] - .5
            and right - x >= 4 and bottom - y >= 4, 'fully_visible_safe_tray_action_required')
    return round(layer['x'] + (x + right) / 2), round(layer['y'] + (y + bottom) / 2)


def button_event(button, down):
    require(button in ('left', 'right') and type(down) is bool, 'typed_native_tray_button_required')
    return {'type': 'btn', 'data': {'button': button, 'down': down}}


def unbound_input(bindings):
    require(type(bindings) is list and len(bindings) <= 4096, 'bounded_native_bindings_required')
    symbols = {'escape', 'esc', 'mouse:272', 'mouse:left', 'mouse:273', 'mouse:right',
               'catchall', 'any', 'code:1', 'code:9', 'code:272', 'code:280',
               'code:273', 'code:281'}
    for binding in bindings:
        require(type(binding) is dict and type(binding.get('modmask')) is int,
                'complete_native_binding_required')
        if binding['modmask'] & ~(2 | 16) != 0:
            continue
        require(not binding.get('catch_all', False)
                and str(binding.get('key', '')).lower() not in symbols
                and binding.get('keycode', 0) not in (1, 9, 272, 280, 273, 281),
                'plain_tray_pointer_or_escape_binding_intercepts_test')


class CursorOwnership:
    """Never adopt a backend warp or user motion, even if it later returns."""
    def __init__(self, initial):
        require(type(initial) is dict and all(type(initial.get(key)) is int for key in ('x', 'y')),
                'exact_initial_cursor_required')
        self.initial = self.expected = (initial['x'], initial['y'])
        self.pending = None
        self.lost = False

    def observe(self, current):
        point = tuple(current.get(key) for key in ('x', 'y')) if type(current) is dict else None
        if self.lost or not all(type(value) is int for value in (point or (None,))):
            self.lost = True
        elif point not in (self.expected, self.pending):
            self.lost = True
        require(not self.lost, 'cursor_authority_lost_no_input_or_restore')
        return point

    def begin(self, current, destination):
        self.observe(current)
        require(self.pending is None and len(destination) == 2
                and all(type(value) is int for value in destination), 'declared_cursor_move_required')
        self.pending = tuple(destination)

    def finish(self, current):
        point = self.observe(current)
        require(self.pending is not None and point == self.pending, 'native_pointer_move_not_confirmed')
        self.expected, self.pending = point, None
