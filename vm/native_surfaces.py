"""Pure inspector selection for VM helpers; diagnostic IDs remain untouched."""


def role(surface):
    return surface.get('role', surface.get('id'))


def active_surfaces(state):
    client = state.get('client', {})
    if client.get('named_monitor_hosts') is True:
        active = client.get('active_monitor')
        hosts = [host for host in client.get('monitor_hosts', []) if host.get('id') == active]
        if len(hosts) != 1:
            return []
        instance = hosts[0].get('instance')
        if not isinstance(instance, str) or not instance:
            return []
        return [surface for surface in state.get('surfaces', [])
                if surface.get('instance') in (None, instance)]
    return [surface for surface in state.get('surfaces', []) if surface.get('instance') is None]


def find_surface(state, logical_role):
    candidates = [surface for surface in active_surfaces(state) if role(surface) == logical_role]
    if len(candidates) != 1:
        raise LookupError('Expected one active surface for role: ' + logical_role)
    return candidates[0]
