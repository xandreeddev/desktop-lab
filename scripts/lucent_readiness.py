"""Pure inspection contract shared by native activation and supervision."""


def desktop_rendered(state, required=('background', 'bar', 'dock')):
    if not isinstance(state, dict):
        return False
    client = state.get('client')
    surfaces = state.get('surfaces')
    if not isinstance(client, dict) or 'theme_error' not in client or not isinstance(surfaces, list):
        return False
    # The printable id is diagnostic text. Match the SDK's explicit role and
    # instance instead; older releases supplied only an uninstanced id.
    rendered = {(surface.get('role', surface.get('id')), surface.get('instance')) for surface in surfaces
                if isinstance(surface, dict) and isinstance(surface.get('role', surface.get('id')), str)
                and (surface.get('instance') is None or isinstance(surface['instance'], str))
                and type(surface.get('frames')) in (int, float) and surface['frames'] > 0}
    required = {(role, None) for role in required}
    if client.get('named_monitor_hosts') is True:
        hosts = client.get('monitor_hosts')
        if not isinstance(hosts, list) or not hosts:
            return False
        instances = set()
        per_output_roles = {role for role, instance in required if role in ('background', 'bar')}
        required -= {(role, None) for role in per_output_roles}
        for host in hosts:
            if not isinstance(host, dict):
                return False
            instance = host.get('instance')
            if not isinstance(instance, str) or not instance or instance in instances:
                return False
            instances.add(instance)
            identifiers = widget_ids(host.get('widget_surfaces'))
            if identifiers is None:
                return False
            required.update((role, instance) for role in per_output_roles | identifiers)
        return required <= rendered
    groups = client.get('widget_surfaces')
    if groups is None:
        required.add(('widgets', None))  # Previous releases used one widget buffer.
    else:
        identifiers = widget_ids(groups)
        if identifiers is None:
            return False
        required.update((role, None) for role in identifiers)
    return required <= rendered


def widget_ids(groups):
    if not isinstance(groups, list):
        return None
    identifiers = [group.get('id') if isinstance(group, dict) else None for group in groups]
    if any(not isinstance(name, str) or not name for name in identifiers):
        return None
    if len(set(identifiers)) != len(identifiers):
        return None
    return set(identifiers)
