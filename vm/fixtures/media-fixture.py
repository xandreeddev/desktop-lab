#!/usr/bin/env python3
"""Owned, time-limited MPRIS players for test-media.py.

Each serving process owns exactly one fixture name without replacing or queuing.
Requires guest python-dbus and python-gobject. Control commands address only the
two fixed fixture names; they never discover or command another application.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import time
import zlib

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

NAMES = {key: 'org.mpris.MediaPlayer2.lucent_fixture_' + key for key in ('a', 'b')}
PATH = '/org/mpris/MediaPlayer2'
ROOT = 'org.mpris.MediaPlayer2'
PLAYER = ROOT + '.Player'
PROPERTIES = 'org.freedesktop.DBus.Properties'
CONTROL = 'org.lucent.MediaFixture'
LENGTH = 120_000_000


def png(color):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack(
            '!I', zlib.crc32(kind + data) & 0xffffffff)
    row = b'\0' + bytes(color) * 2
    return (b'\x89PNG\r\n\x1a\n' +
            chunk(b'IHDR', struct.pack('!IIBBBBB', 2, 2, 8, 6, 0, 0, 0)) +
            chunk(b'IDAT', zlib.compress(row * 2)) + chunk(b'IEND', b''))


class Player(dbus.service.Object):
    def __init__(self, bus, loop, key, directory, run_id):
        self.bus, self.loop, self.key, self.run_id = bus, loop, key, run_id
        self.name = dbus.service.BusName(NAMES[key], bus=bus, do_not_queue=True,
                                         allow_replacement=False, replace_existing=False)
        self.identity = 'Lucent fixture ' + key.upper()
        self.track = 1
        self.status = 'Paused' if key == 'a' else 'Playing'
        self.can_seek = True
        self.position = 20_000_000 if key == 'a' else 40_000_000
        self.sampled_at = time.monotonic()
        self.artwork = directory / ('cover-' + key + '.png')
        self.events, self.counts = [], {}
        self.write_artwork()
        super().__init__(bus, PATH)
        self.control = Control(self, bus)

    def write_artwork(self):
        color = ((215, 65, 75, 255) if self.key == 'a' else (55, 130, 220, 255))
        if self.track % 2 == 0:
            color = (75, 190, 100, 255)
        with self.artwork.open('wb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(png(color))

    def record(self, action, *values):
        self.counts[action] = self.counts.get(action, 0) + 1
        self.events.append([action, *values])
        self.events = self.events[-128:]

    def current_position(self):
        elapsed = int((time.monotonic() - self.sampled_at) * 1_000_000)
        return min(LENGTH, self.position + (elapsed if self.status == 'Playing' else 0))

    def set_status(self, status):
        self.position = self.current_position()
        self.sampled_at = time.monotonic()
        self.status = status
        self.PropertiesChanged(PLAYER, {'PlaybackStatus': status}, [])

    def track_path(self):
        return '/org/lucent/MediaFixture/' + self.key + '/track/' + str(self.track)

    def metadata(self):
        return dbus.Dictionary({
            'mpris:trackid': dbus.ObjectPath(self.track_path()),
            'mpris:length': dbus.Int64(LENGTH),
            'mpris:artUrl': self.artwork.as_uri(),
            'xesam:title': self.identity + ' track ' + str(self.track),
            'xesam:artist': dbus.Array(['Lucent test artist'], signature='s'),
            'xesam:album': 'Owned native acceptance fixture',
        }, signature='sv')

    def properties(self, interface):
        if interface == ROOT:
            return {
                'Identity': self.identity, 'DesktopEntry': 'lucent-fixture-' + self.key,
                'CanQuit': dbus.Boolean(False), 'CanRaise': dbus.Boolean(True),
                'HasTrackList': dbus.Boolean(False),
                'SupportedUriSchemes': dbus.Array([], signature='s'),
                'SupportedMimeTypes': dbus.Array([], signature='s'),
            }
        if interface == PLAYER:
            return {
                'PlaybackStatus': self.status, 'Metadata': self.metadata(),
                'Position': dbus.Int64(self.current_position()), 'Rate': dbus.Double(1),
                'MinimumRate': dbus.Double(1), 'MaximumRate': dbus.Double(1),
                'Volume': dbus.Double(1), 'LoopStatus': 'None', 'Shuffle': dbus.Boolean(False),
                'CanControl': dbus.Boolean(True), 'CanPlay': dbus.Boolean(True),
                'CanPause': dbus.Boolean(True), 'CanGoNext': dbus.Boolean(True),
                'CanGoPrevious': dbus.Boolean(True), 'CanSeek': dbus.Boolean(self.can_seek),
            }
        raise dbus.exceptions.DBusException('Unknown fixture interface',
                                            name='org.freedesktop.DBus.Error.UnknownInterface')

    @dbus.service.method(PROPERTIES, in_signature='s', out_signature='a{sv}')
    def GetAll(self, interface):
        self.record('read', str(interface))
        return self.properties(str(interface))

    @dbus.service.method(PROPERTIES, in_signature='ss', out_signature='v')
    def Get(self, interface, property_name):
        return self.properties(str(interface))[str(property_name)]

    @dbus.service.method(PROPERTIES, in_signature='ssv', out_signature='')
    def Set(self, interface, property_name, value):
        raise dbus.exceptions.DBusException('Fixture properties are read-only',
                                            name='org.freedesktop.DBus.Error.PropertyReadOnly')

    @dbus.service.signal(PROPERTIES, signature='sa{sv}as')
    def PropertiesChanged(self, interface, changed, invalidated):
        pass

    @dbus.service.signal(PLAYER, signature='x')
    def Seeked(self, position):
        pass

    @dbus.service.method(PLAYER, in_signature='', out_signature='')
    def Play(self):
        self.record('play')
        self.set_status('Playing')

    @dbus.service.method(PLAYER, in_signature='', out_signature='')
    def Pause(self):
        self.record('pause')
        self.set_status('Paused')

    @dbus.service.method(PLAYER, in_signature='', out_signature='')
    def Stop(self):
        self.record('stop')
        self.position = 0
        self.status = 'Stopped'
        self.sampled_at = time.monotonic()
        self.PropertiesChanged(PLAYER, {'PlaybackStatus': self.status}, [])
        self.Seeked(dbus.Int64(0))

    def new_track(self):
        if self.track >= 16:
            raise ValueError('Fixture track limit')
        self.track += 1
        self.position = 10_000_000
        self.sampled_at = time.monotonic()
        self.write_artwork()  # Deliberately reuse the URI for different track pixels.
        self.PropertiesChanged(PLAYER, {'Metadata': self.metadata()}, [])
        self.Seeked(dbus.Int64(self.position))

    @dbus.service.method(PLAYER, in_signature='', out_signature='')
    def Next(self):
        self.record('next')
        self.new_track()

    @dbus.service.method(PLAYER, in_signature='', out_signature='')
    def Previous(self):
        self.record('previous')
        self.new_track()

    @dbus.service.method(PLAYER, in_signature='ox', out_signature='')
    def SetPosition(self, track, position):
        if str(track) != self.track_path():
            self.record('stale_seek', str(track), int(position))
            return
        if not self.can_seek or not 0 <= int(position) <= LENGTH:
            raise dbus.exceptions.DBusException('Invalid fixture seek',
                                                name='org.freedesktop.DBus.Error.InvalidArgs')
        self.record('set_position', str(track), int(position))
        self.position = int(position)
        self.sampled_at = time.monotonic()
        self.Seeked(dbus.Int64(self.position))

    @dbus.service.method(ROOT, in_signature='', out_signature='')
    def Raise(self):
        self.record('raise')


class Control(dbus.service.Object):
    def __init__(self, player, bus):
        self.player = player
        super().__init__(bus, '/Control')

    @dbus.service.method(CONTROL, in_signature='', out_signature='s')
    def State(self):
        p = self.player
        return json.dumps({'run_id': p.run_id, 'key': p.key, 'identity': p.identity,
                           'owner': str(p.bus.get_unique_name()), 'track': p.track_path(),
                           'status': p.status, 'counts': p.counts, 'events': p.events})

    @dbus.service.method(CONTROL, in_signature='s', out_signature='')
    def Configure(self, encoded):
        if len(encoded) > 1024:
            raise ValueError('Fixture input limit')
        options = json.loads(encoded)
        if not isinstance(options, dict) or set(options) - {'new_track', 'status', 'can_seek'}:
            raise ValueError('Unknown fixture option')
        p = self.player
        if 'status' in options:
            if options['status'] not in ('Playing', 'Paused', 'Stopped'):
                raise ValueError('Invalid fixture playback status')
            p.set_status(options['status'])
        if 'can_seek' in options:
            if type(options['can_seek']) is not bool:
                raise ValueError('Invalid fixture capability')
            p.can_seek = options['can_seek']
            p.PropertiesChanged(PLAYER, {'CanSeek': dbus.Boolean(p.can_seek)}, [])
        if 'new_track' in options:
            if type(options['new_track']) is not bool:
                raise ValueError('Invalid fixture track change')
            if options['new_track']:
                p.new_track()

    @dbus.service.method(CONTROL, in_signature='', out_signature='')
    def Quit(self):
        GLib.idle_add(self.player.loop.quit)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('serve', 'state', 'configure', 'quit'))
    parser.add_argument('key', choices=tuple(NAMES))
    parser.add_argument('arguments', nargs='*')
    args = parser.parse_args()
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus(private=True)
    if args.action != 'serve':
        api = dbus.Interface(bus.get_object(NAMES[args.key], '/Control'), CONTROL)
        if args.action == 'state' and not args.arguments:
            print(api.State(timeout=5))
        elif args.action == 'configure' and len(args.arguments) == 1:
            api.Configure(args.arguments[0], timeout=5)
        elif args.action == 'quit' and not args.arguments:
            api.Quit(timeout=5)
        else:
            parser.error('Unexpected fixture control arguments')
        return
    if len(args.arguments) != 2:
        parser.error('serve requires an existing private directory and run identity')
    directory = Path(args.arguments[0])
    if not directory.is_absolute() or not directory.is_dir():
        parser.error('serve requires an absolute existing directory')
    run_id = args.arguments[1]
    if not run_id or len(run_id) > 64 or not run_id.isalnum():
        parser.error('Invalid fixture run identity')
    loop = GLib.MainLoop()
    player = Player(bus, loop, args.key, directory, run_id)
    GLib.timeout_add_seconds(290, loop.quit)
    loop.run()
    del player
    bus.close()


if __name__ == '__main__':
    main()
