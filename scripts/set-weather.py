#!/usr/bin/env python3
"""Apply an explicitly selected city from a private JSON file; never detect location."""
import argparse
import json
import math
from pathlib import Path
import re

from guest import HOME_DIR, STATE, backup, run, write

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('profile', choices=('lucid', 'noctalia', 'lucent'))
p.add_argument('location_json', type=Path)
args = p.parse_args()
if args.profile != 'lucent' and (STATE / 'profile').read_text().strip() != args.profile:
    p.error('Install this profile first so its original configuration is backed up.')
place = json.loads(args.location_json.read_text())
lat, lon = float(place['latitude']), float(place['longitude'])
if not (math.isfinite(lat) and math.isfinite(lon) and -90 <= lat <= 90 and -180 <= lon <= 180):
    p.error('Invalid coordinates')
if not isinstance(place['name'], str) or not place['name'].strip():
    p.error('A city name is required')

if args.profile == 'lucent':
    backup('.config/lucent/weather.json')
    write(HOME_DIR / '.config/lucent/weather.json', json.dumps(place, indent=2)+'\n')
elif args.profile == 'lucid':
    relative = '.config/quickshell/lucidprefs/prefs.json'
    path = HOME_DIR / relative
    values = json.loads(path.read_text())
    values.update(gpsEnabled=False, locationName=place['name'], locationLabel=place['name'],
                  locationLat=lat, locationLon=lon, locationTz=place.get('timezone', ''))
    write(path, json.dumps(values, indent=2)+'\n')
else:
    relative = '.config/noctalia/config.toml'
    path = HOME_DIR / relative
    text = re.sub(r'(?ms)^\[location\]\s*\n.*?(?=^\[|\Z)', '', path.read_text())
    text += '\n[location]\nauto_locate = false\naddress = ' + json.dumps(place['name']) + '\n'
    write(path, text)
    run('noctalia', 'config', 'validate')
write(STATE / 'weather.json', json.dumps(place, indent=2)+'\n')
print('Selected weather location applied. Restart the shell to load it.')
