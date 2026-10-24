"""Pure policy for one fresh, test-owned Chromium birth per notification route.

This does not extend the warm browser's blank-control contract. The first URI
is part of an absent-profile startup declaration, not a new tab in a control
window. Import performs no native operations.
"""
import re

import default_uri_policy as p

OPERATIONS = ('prepare', 'publish', 'admission', 'open', 'cold-ready',
              'cold-snapshot', 'cold-guard', 'cold-retire', 'notify', 'observe-note',
              'wait-expiry', 'state', 'security', 'browser-key-arm',
              'browser-key-read', 'retire-guardian', 'finish', 'watchdog')
PHASES = ('arguments', 'context', 'handler', 'transport')
REASONS = ('cold_uri_fixture_refused', 'unclassified_fixture_failure')


def require(value):
    if not value:
        raise ValueError('cold_uri_fixture_refused')


def bridge_timeout(operation):
    require(operation in OPERATIONS)
    return p.BRIDGE_EXPIRY_SECONDS if operation == 'wait-expiry' else p.BRIDGE_COMMAND_SECONDS


def first_event(values, uri):
    require(type(values) is list and len(values) == 1)
    event = values[0]
    require(type(event) is dict and event['operation'] == 'open'
            and event['uris'] == [uri] and event['fresh_token_present'] is True
            and event['legacy_startup_present'] is False
            and event['cgroup'] == event['origin']['unit']['group'])
    return event


def document(values, expected):
    """Missing initial page can wait; every unexpected/extra page refuses."""
    require(type(values) is list and type(expected) is list and len(expected) == 1)
    if not values:
        return None
    require(len(values) == 1)
    value = values[0]
    require(type(value) is dict and set(value) == {'id', 'url', 'title'}
            and type(value['id']) is str and re.fullmatch('[A-Za-z0-9-]{1,128}', value['id'])
            and value['url'] == expected[0]['uri'] and value['title'] == expected[0]['title'])
    return dict(value)


def case_receipts(values):
    """A second route needs a distinct profile, original PID birth and lease."""
    require(type(values) is list and 1 <= len(values) <= 2)
    for index, value in enumerate(values):
        require(value['case'] == p.NOTIFICATION_CASES[index] and value['success'] is True
                and value['cleanup'] and all(item is True for item in value['cleanup'].values()))
    if len(values) == 2:
        first, second = [value['cold_browser'] for value in values]
        require(first['lease']['declared']['profile'] != second['lease']['declared']['profile']
                and first['lease']['declared']['class'] != second['lease']['declared']['class']
                and first['lease']['origin'] != second['lease']['origin']
                and (first['lease']['process']['pid'], first['lease']['process']['start']) !=
                    (second['lease']['process']['pid'], second['lease']['process']['start']))


def error_packet(operation, error, phase='handler'):
    reason = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else None
    return {'failure': 'owned_cold_uri_fixture_refused',
            'operation': operation if operation in OPERATIONS else 'unknown',
            'phase': phase if phase in PHASES else 'arguments',
            'error_class': type(error).__name__ if type(error).__name__ in p.BRIDGE_ERROR_CLASSES else 'OtherError',
            'fixed_reason': reason if reason in REASONS else 'unclassified_fixture_failure'}


def validate_error(value, operation):
    require(type(value) is dict and set(value) ==
            {'failure', 'operation', 'phase', 'error_class', 'fixed_reason'}
            and value['failure'] == 'owned_cold_uri_fixture_refused'
            and operation in OPERATIONS and value['operation'] == operation
            and value['phase'] in PHASES and value['error_class'] in p.BRIDGE_ERROR_CLASSES
            and value['fixed_reason'] in REASONS)
    return dict(value)
