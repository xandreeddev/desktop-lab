#!/usr/bin/env python3
"""Exercise native notification actions, replacement, dismissal and expiration.

Requires an unlocked Lucent guest with notification history closed. Fixtures are
transient; only their IDs are closed. This never restarts a desktop D-Bus daemon.
"""
import json
from pathlib import Path
import re
import runpy


def main():
    vm = runpy.run_path(str(Path(__file__).with_name('test-framework.py')), run_name='harness')
    session, cli, inspect = (vm[name] for name in ('session', 'cli', 'inspect'))
    vm['eventually'](lambda: inspect()['client']['notifications_ready'],
                     'Notification adapter not ready', timeout=30)
    assert not inspect()['client']['notification_history_open'], 'Close notification history before this check'
    owned = set()

    def call(method, *arguments):
        return session('gdbus', 'call', '--session', '--dest', 'org.freedesktop.Notifications',
                       '--object-path', '/org/freedesktop/Notifications',
                       '--method', 'org.freedesktop.Notifications.' + method, *arguments)

    def notify(replaces=0, timeout=0):
        value = call('Notify', 'Lucent validation', str(replaces), '',
                     'Notification delivery check', 'Transient native validation fixture',
                     "['fixture-open', 'Open fixture']", "{'transient': <true>}", str(timeout))
        match = re.fullmatch(r'\(uint32 (\d+),\)\s*', value)
        assert match, 'Unexpected notification ID reply'
        identifier = int(match.group(1))
        owned.add(identifier)
        return identifier

    def has_hit(identifier):
        return any(hit['id'] == identifier for surface in inspect()['surfaces'] for hit in surface['hits'])

    try:
        identifier = notify()
        cli('notifications', 'show')
        action = f'notification-action-{identifier}-fixture-open'
        vm['eventually'](lambda: has_hit(action), 'Native action was not rendered', timeout=15)
        assert notify(replaces=identifier) == identifier, 'Replacement changed notification identity'
        vm['click'](action)
        vm['eventually'](lambda: not has_hit(action), 'Invoked transient notification stayed active', timeout=15)

        identifier = notify()
        cli('notifications', 'show')
        dismiss = f'notification-dismiss-{identifier}'
        vm['eventually'](lambda: has_hit(dismiss), 'Dismiss control was not rendered', timeout=15)
        vm['click'](dismiss)
        vm['eventually'](lambda: not has_hit(dismiss), 'Dismiss did not remove the notification', timeout=15)

        identifier = notify(timeout=3000)
        cli('notifications', 'show')
        dismiss = f'notification-dismiss-{identifier}'
        vm['eventually'](lambda: has_hit(dismiss), 'Timed notification did not render', timeout=10)
        vm['eventually'](lambda: not has_hit(dismiss), 'Notification did not expire', timeout=15)
        assert inspect()['client']['notifications_ready']
    finally:
        for identifier in owned:
            call('CloseNotification', str(identifier))
        if inspect()['client']['notification_history_open']:
            cli('notifications', 'toggle')
        vm['eventually'](lambda: not inspect()['client']['notification_history_open'],
                         'Notification history did not close', timeout=10)

    report = {
        'protocol_ready': True,
        'replacement_preserves_identity': True,
        'native_action_click': True,
        'native_dismiss_click': True,
        'native_expiration': True,
        'transient_fixtures_cleaned': True,
        'history_closed': True,
        'desktop_bus_restarted': False,
    }
    (vm['REPORT'] / 'lucent-notification-integration.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
