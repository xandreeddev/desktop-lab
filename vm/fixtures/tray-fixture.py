#!/usr/bin/env python3
"""Owned, bounded SNI/DBusMenu fixture for test-tray.py; never replaces a name.

Requires the guest's python-dbus and python-gobject. Run on the guest session
bus only. With an argument, call this fixture's control API instead of serving.
No desktop files, settings or external application names are read or changed.
"""
import json
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

NAME = 'org.lucent.TrayFixture'
ITEM = 'org.kde.StatusNotifierItem'
CONTROL = NAME
MENU = 'com.canonical.dbusmenu'
PROPERTIES = 'org.freedesktop.DBus.Properties'


def pixmap(color):
    return dbus.Array([(16, 16, dbus.ByteArray(bytes(color) * 256))], signature='(iiay)')


class Item(dbus.service.Object):
    def __init__(self, fixture, index):
        self.fixture = fixture
        self.index = index
        self.path = '/Item' + str(index)
        self.status = 'Active'
        self.menu_only = False
        self.has_menu = True
        self.delayed_failure = False
        super().__init__(fixture.bus, self.path)

    @dbus.service.method(PROPERTIES, in_signature='s', out_signature='a{sv}')
    def GetAll(self, interface):
        if interface != ITEM:
            return {}
        self.fixture.record('read', self.index)
        return {
            'Id': 'lucent-fixture-' + str(self.index),
            'Title': 'Tray fixture ' + str(self.index),
            'Category': 'ApplicationStatus',
            'Status': self.status,
            'IconName': '',
            'IconPixmap': pixmap((255, 85, 175, 215)),
            'AttentionIconPixmap': pixmap((255, 240, 170, 65)),
            'OverlayIconPixmap': pixmap((255, 185, 85, 145)),
            'ItemIsMenu': dbus.Boolean(self.menu_only),
            'Menu': dbus.ObjectPath('/Menu' if self.has_menu else '/'),
        }

    @dbus.service.signal(ITEM, signature='s')
    def NewStatus(self, status):
        pass

    @dbus.service.method(ITEM, in_signature='ii', out_signature='',
                         async_callbacks=('reply', 'error'))
    def Activate(self, x, y, reply, error):
        self.fixture.record('activate', self.index)
        if self.delayed_failure:
            def fail():
                error(dbus.exceptions.DBusException(
                    'Controlled fixture failure', name=NAME + '.Rejected'))
                return False
            GLib.timeout_add(1200, fail)
        else:
            reply()

    @dbus.service.method(ITEM, in_signature='ii', out_signature='')
    def SecondaryActivate(self, x, y):
        self.fixture.record('secondary', self.index)

    @dbus.service.method(ITEM, in_signature='ii', out_signature='')
    def ContextMenu(self, x, y):
        self.fixture.record('context', self.index)

    @dbus.service.method(ITEM, in_signature='is', out_signature='')
    def Scroll(self, delta, orientation):
        self.fixture.record('scroll', int(delta), str(orientation))


class Menu(dbus.service.Object):
    def __init__(self, fixture):
        self.fixture = fixture
        super().__init__(fixture.bus, '/Menu')

    @dbus.service.method(MENU, in_signature='i', out_signature='b')
    def AboutToShow(self, parent):
        self.fixture.record('about', int(parent))
        return False

    @dbus.service.method(MENU, in_signature='iias', out_signature='u(ia{sv}av)')
    def GetLayout(self, parent, depth, properties):
        self.fixture.record('layout', int(parent))
        child = dbus.Struct((1, dbus.Dictionary({
            'label': '_Open fixture', 'enabled': dbus.Boolean(True),
            'visible': dbus.Boolean(True),
        }, signature='sv'), dbus.Array([], signature='v')),
            signature='ia{sv}av', variant_level=1)
        root = dbus.Struct((0, dbus.Dictionary({}, signature='sv'),
                            dbus.Array([child], signature='v')), signature='ia{sv}av')
        return dbus.UInt32(1), root

    @dbus.service.method(MENU, in_signature='isvu', out_signature='')
    def Event(self, entry, event, value, timestamp):
        self.fixture.record('event', int(entry), str(event))


class Fixture(dbus.service.Object):
    def __init__(self, bus, loop):
        self.bus = bus
        self.loop = loop
        self.name = dbus.service.BusName(NAME, bus=bus, do_not_queue=True)
        self.events = []
        self.counts = {}
        self.errors = 0
        self.items = []
        super().__init__(bus, '/Control')
        self.menu = Menu(self)

    def record(self, *event):
        self.counts[event[0]] = self.counts.get(event[0], 0) + 1
        self.events.append(event)
        self.events = self.events[-128:]

    def add(self, count):
        while len(self.items) < min(7, count):
            item = Item(self, len(self.items) + 1)
            self.items.append(item)
            watcher = dbus.Interface(self.bus.get_object(
                'org.kde.StatusNotifierWatcher', '/StatusNotifierWatcher'),
                'org.kde.StatusNotifierWatcher')
            watcher.RegisterStatusNotifierItem(item.path,
                reply_handler=lambda: None, error_handler=self.registration_error)
        return False

    def registration_error(self, error):
        self.errors += 1

    @dbus.service.method(CONTROL, in_signature='', out_signature='s')
    def State(self):
        return json.dumps({'events': self.events, 'counts': self.counts,
                           'registration_errors': self.errors,
                           'items': len(self.items)})

    @dbus.service.method(CONTROL, in_signature='s', out_signature='')
    def Configure(self, encoded):
        if len(encoded) > 512:
            raise ValueError('Fixture input limit')
        options = json.loads(encoded)
        item = self.items[0]
        if 'status' in options:
            assert options['status'] in ('Active', 'Passive', 'NeedsAttention')
            item.status = options['status']
        for key in ('menu_only', 'has_menu', 'delayed_failure'):
            if key in options:
                assert isinstance(options[key], bool)
                setattr(item, key, options[key])
        self.add(int(options.get('items', len(self.items))))
        item.NewStatus(item.status)

    @dbus.service.method(CONTROL, in_signature='', out_signature='')
    def Quit(self):
        GLib.idle_add(self.loop.quit)


def main():
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    if len(sys.argv) > 1:
        api = dbus.Interface(bus.get_object(NAME, '/Control'), CONTROL)
        if sys.argv[1] == 'state':
            print(api.State(timeout=5))
        elif sys.argv[1] == 'configure':
            api.Configure(sys.argv[2], timeout=5)
        elif sys.argv[1] == 'quit':
            api.Quit(timeout=5)
        else:
            raise ValueError('Unknown fixture command')
        return
    loop = GLib.MainLoop()
    fixture = Fixture(bus, loop)
    GLib.idle_add(fixture.add, 1)
    GLib.timeout_add_seconds(180, loop.quit)
    loop.run()


if __name__ == '__main__':
    main()
