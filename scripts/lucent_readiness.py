"""Pure inspection contract shared by native activation and supervision."""


def desktop_rendered(state, required=('background', 'bar', 'dock')):
    if not isinstance(state, dict):
        return False
    client = state.get('client')
    surfaces = state.get('surfaces')
    if not isinstance(client, dict) or 'theme_error' not in client or not isinstance(surfaces, list):
        return False
    rendered = {surface.get('id') for surface in surfaces
                if isinstance(surface, dict) and isinstance(surface.get('id'), str)
                and isinstance(surface.get('frames'), (int, float)) and surface['frames'] > 0}
    required = set(required)
    groups = client.get('widget_surfaces')
    if groups is None:
        required.add('widgets')  # Previous native releases used one widget buffer.
    else:
        if not isinstance(groups, list):
            return False
        identifiers = [group.get('id') if isinstance(group, dict) else None for group in groups]
        if any(not isinstance(name, str) or not name for name in identifiers):
            return False
        if len(set(identifiers)) != len(identifiers):
            return False
        required.update(identifiers)
    return required <= rendered
