#!/usr/bin/env python3
"""Reversible compositor-pixel acceptance for wallpaper diagonal reveals.

Requires an unlocked single-output Lucent guest at 1x, wf-recorder, grim and
ffmpeg. Temporary solid images make a spatial cut distinguishable from a
global crossfade. Selector samples compare the same stationary panel before,
during and after opening/closing. Recording and decoding use one worker thread;
frames are streamed, never accumulated in memory. No credentials or providers.

The original selected snapshot is checkpointed before any wallpaper change.
Exact preferences, links, theme/login exports, workspace/focus and idle policy
are restored. Captures and rollback material remain private; failed restoration
retains the guest checkpoint. This checks compositor pixels, not GPU timings.
"""
import argparse
import json
from pathlib import Path
import runpy
import shlex
import sys
import time

import lab


ROOT = Path(__file__).resolve().parents[1]
h = runpy.run_path(str(ROOT / 'vm/test-live-wallpapers.py'))
remote, session, cli, inspect, client = [h[key] for key in
                                       ('remote', 'session', 'cli', 'inspect', 'client')]
require, eventually = h['require'], h['eventually']


ENCODE = r'''import struct, sys, zlib
from pathlib import Path
root=Path(sys.argv[1])
def chunk(kind, value):
    return struct.pack('!I',len(value))+kind+value+struct.pack('!I',zlib.crc32(kind+value))
for name,color in [('red',(255,0,0)),('blue',(0,0,255))]:
    pixels=(b'\0'+bytes(color)*256)*160
    (root/(name+'.png')).write_bytes(b'\x89PNG\r\n\x1a\n'+
        chunk(b'IHDR',struct.pack('!IIBBBBB',256,160,8,2,0,0,0))+
        chunk(b'IDAT',zlib.compress(pixels))+chunk(b'IEND',b''))
'''


ISOLATE = '''import json, sys
from pathlib import Path
path=Path.home()/'.local/state/lucent/desktop.json'
value=json.loads(path.read_text())
value['visible_widgets']=[]
for profile in value.get('monitor_layouts',{}).values(): profile['visible_widgets']=[]
live=value.setdefault('live_wallpapers',{})
live.update(default=None,monitors={},paused=False)
temporary=path.with_name('desktop.cut-test.new')
temporary.write_text(json.dumps(value)); temporary.replace(path)
'''


# Runs entirely inside the session, avoiding SSH startup latency between frames.
# Only sanitized counts/checks leave stdout; recordings remain in private scratch.
PIXELS = r'''import json, os, resource, shutil, signal, socket, subprocess, sys, time
from pathlib import Path
scratch=Path(sys.argv[1]); width,height=map(int,sys.argv[2:4])
MAX_CAPTURE=64*1024*1024
def require(value,label):
    if not value: raise RuntimeError(label)
def request(command):
    with socket.socket(socket.AF_UNIX) as peer:
        peer.settimeout(5)
        peer.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'lucent.sock'))
        peer.sendall((command+'\n').encode())
        chunks=[]; size=0
        while chunk:=peer.recv(65536):
            size+=len(chunk); require(size<=4*1024*1024,'bounded_inspector')
            chunks.append(chunk)
    text=b''.join(chunks).decode()
    require(not text.startswith('error:'),'command_accepted')
    return json.loads(text) if command=='inspect' else text
def state(): return request('inspect')
def live_surface(value):
    found=[s for s in value['surfaces'] if s.get('role',s.get('id'))=='live-wallpaper']
    require(len(found)==1,'one_live_surface')
    return found[0]
def panel(value):
    found=[hit for hit in live_surface(value)['hits'] if hit['id']=='live-scroll']
    require(len(found)==1,'one_live_panel')
    return tuple(found[0][key] for key in ('x','y','width','height'))
def wait(predicate,label,timeout=20):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        value=state()
        if predicate(value): return value
        time.sleep(.06)
    raise RuntimeError(label)
def capture(geometry):
    output=subprocess.run(['grim','-g',geometry,'-t','ppm','-'],
                          capture_output=True,timeout=8,check=True).stdout
    require(len(output)<=MAX_CAPTURE,'bounded_still')
    tokens=[]; offset=0
    while len(tokens)<4:
        while output[offset:offset+1].isspace(): offset+=1
        if output[offset:offset+1]==b'#': offset=output.index(b'\n',offset)+1; continue
        end=offset
        while end<len(output) and not output[end:end+1].isspace(): end+=1
        tokens.append(output[offset:end]); offset=end
    require(tokens[0]==b'P6' and tokens[3]==b'255','ppm_format')
    offset+=2 if output[offset:offset+2]==b'\r\n' else 1
    w,h=map(int,tokens[1:3]); frame=output[offset:]
    require(len(frame)==w*h*3,'ppm_storage')
    return w,h,frame
def pixel(frame,w,x,y):
    at=(int(y)*w+int(x))*3
    return tuple(frame[at:at+3])
def near(a,b): return max(abs(x-y) for x,y in zip(a,b))<=7
def limit_capture(): resource.setrlimit(resource.RLIMIT_FSIZE,(MAX_CAPTURE,MAX_CAPTURE))
def record(label,geometry,w,h,command,points,before,after,expected_panel=None,reverse=False):
    destination=scratch/(label+'.mkv'); process=None; checked_bounds=0
    with (scratch/(label+'.log')).open('wb') as log:
        try:
            process=subprocess.Popen(['wf-recorder','-y','--no-dmabuf','-D',
                '-g',geometry,'-r','30','-c','ffv1','-p','threads=1',
                '-f',str(destination)],stdin=subprocess.DEVNULL,stdout=log,stderr=log,
                preexec_fn=limit_capture)
            time.sleep(.5)
            require(process.poll() is None,'recorder_started')
            request(command)
            end=time.monotonic()+3.2
            while time.monotonic()<end:
                value=state()
                if expected_panel is not None:
                    surfaces=[s for s in value['surfaces']
                              if s.get('role',s.get('id'))=='live-wallpaper']
                    if surfaces and any(hit['id']=='live-scroll' for hit in surfaces[0]['hits']):
                        require(all(abs(a-b)<.01 for a,b in zip(panel(value),expected_panel)),
                                'selector_stationary_layout')
                        require(surfaces[0]['width']==width and surfaces[0]['height']==height,
                                'selector_stationary_native_extent')
                        checked_bounds+=1
                time.sleep(.06)
        finally:
            if process is not None and process.poll() is None:
                process.send_signal(signal.SIGINT)
                try: process.wait(timeout=12)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait(timeout=5)
            require(process is not None and process.returncode==0,'recorder_completed')
    require(destination.is_file() and destination.stat().st_size<=MAX_CAPTURE,'bounded_recording')
    decoder=subprocess.Popen(['ffmpeg','-nostdin','-v','error','-threads','1',
        '-filter_threads','1','-filter_complex_threads','1',
        '-i',str(destination),'-threads','1','-f','rawvideo','-pix_fmt','rgb24','-'],
        stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
    frames=0; mixed=[]; first_before=None; first_after=None; size=w*h*3
    require(size<=MAX_CAPTURE,'bounded_frame')
    try:
        while True:
            frame=decoder.stdout.read(size)
            if not frame: break
            require(len(frame)==size and frames<400,'bounded_decoded_frames')
            values=[pixel(frame,w,x,y) for x,y in points]
            old=[index for index,(value,color) in enumerate(zip(values,before)) if near(value,color)]
            new=[index for index,(value,color) in enumerate(zip(values,after)) if near(value,color)]
            if len(old)==len(points) and first_before is None: first_before=frames
            if len(new)==len(points) and first_after is None: first_after=frames
            # In the same actual compositor frame, low x+y positions have the
            # new opaque pixels while high x+y positions retain the old ones.
            # A global fade cannot satisfy both endpoint-color tests.
            projection=lambda index: sum(points[index])/(w+h)
            if old and new and max(map(projection,new))+.03<min(map(projection,old)):
                mixed.append(frames)
                if len(mixed)==1:
                    (scratch/(label+'-cut.ppm')).write_bytes(
                        f'P6\n{w} {h}\n255\n'.encode()+frame)
            frames+=1
        require(decoder.wait(timeout=8)==0,'decoder_completed')
    finally:
        if decoder.poll() is None: decoder.kill(); decoder.wait(timeout=5)
        decoder.stdout.close()
    require(frames>=10 and first_before is not None and first_after is not None,
            'recorded_both_endpoints')
    require(first_after<first_before if reverse else first_before<first_after,
            'recorded_endpoint_order')
    require(bool(mixed),'opaque_spatial_diagonal_in_compositor_pixels')
    if expected_panel is not None: require(checked_bounds>=2,'selector_bounds_observed')
    return {'frames':frames,'diagonal_frames':len(mixed),'stationary_bounds_samples':checked_bounds}

def main():
    require(all(shutil.which(name) for name in ('grim','wf-recorder','ffmpeg')),'recording_tools')
    full_geometry=f'0,0 {width}x{height}'
    red=(255,0,0); blue=(0,0,255)
    points=[(int(width*x),int(height*y)) for y in (.16,.32,.5,.68,.84)
            for x in (.12,.3,.5,.7,.88)]
    _,_,initial=capture(full_geometry)
    require(all(near(pixel(initial,width,x,y),red) for x,y in points),'red_background_unobstructed')
    report={'checks':{},'recordings':{}}
    for label,target,before,after in [('red-to-blue','blue',red,blue),('blue-to-red','red',blue,red)]:
        report['recordings'][label]=record(label,full_geometry,width,height,
            'wallpaper set '+str(scratch/(target+'.png')),points,[before]*len(points),[after]*len(points))
        wait(lambda value:not value['client']['wallpaper_busy'] and
             all(not item['presentation']['transition_pending']
                 for item in value['client']['background_transitions']),'wallpaper_transition_retired')
        report['checks'][label+'_opaque_spatial_cut']=True

    request('wallpaper live open')
    wait(lambda value:value['client']['live_wallpaper']['open'] and
         not value['client']['live_wallpaper']['awaiting_first_frame'],'selector_presented')
    time.sleep(1.5)
    bounds=panel(state()); x,y,w,h=map(round,bounds)
    geometry=f'{x},{y} {w}x{h}'
    actual_w,actual_h,opened=capture(geometry)
    require((actual_w,actual_h)==(w,h),'selector_capture_extent')
    request('wallpaper live close'); time.sleep(1.5)
    _,_,closed=capture(geometry)
    panel_points=[(int(w*fx),int(h*fy)) for fy in (.12,.28,.5,.72,.88)
                  for fx in (.12,.28,.5,.72,.88)]
    panel_points=[(px,py) for px,py in panel_points
                  if near(pixel(closed,w,px,py),red) and
                  max(abs(a-b) for a,b in zip(pixel(opened,w,px,py),red))>=64]
    require(len(panel_points)>=12,'selector_reference_contrast')
    panel_before=[pixel(closed,w,px,py) for px,py in panel_points]
    panel_after=[pixel(opened,w,px,py) for px,py in panel_points]
    report['recordings']['selector-open']=record('selector-open',geometry,w,h,
        'wallpaper live open',panel_points,panel_before,panel_after,bounds)
    report['checks']['selector_open_stationary_diagonal_cut']=True
    # Closing shrinks the same mask: the panel remains opaque at low x+y positions
    # while the lower-right already shows the background. Endpoint order reverses.
    report['recordings']['selector-close']=record('selector-close',geometry,w,h,
        'wallpaper live close',panel_points,panel_before,panel_after,bounds,reverse=True)
    report['checks']['selector_close_stationary_diagonal_cut']=True
    print(json.dumps(report))
try:
    main()
except Exception as error:
    label=str(error) if isinstance(error,RuntimeError) else type(error).__name__
    print(json.dumps({'failure':label}))
'''


CLEANUP = '''import json, shutil, sys
from pathlib import Path
scratch=Path(sys.argv[1]); state=Path.home()/'.local/state/lucent'
selected=(state/'wallpaper').resolve()
for directory in (state/'wallpaper-assets').glob('asset-*'):
    if directory.is_symlink() or not (directory/'source.json').is_file(): continue
    asset=json.loads((directory/'source.json').read_text())
    if Path(asset['source']['path']).parent!=scratch: continue
    path=Path(asset['path'])
    if directory.parent!=(state/'wallpaper-assets').resolve() or path.parent!=directory or path.is_symlink() or path==selected:
        raise ValueError('test_asset_cleanup_guard')
    shutil.rmtree(directory)
shutil.rmtree(scratch)
'''


def dispatch(kind, selector):
    require(bool(selector) and all(ord(char) >= 32 and ord(char) != 127 for char in selector),
            'bounded_compositor_selector')
    expression = 'hl.dsp.focus({ ' + kind + ' = ' + json.dumps(selector, ensure_ascii=False) + ' })'
    require(session('hyprctl', 'dispatch', expression).strip() == 'ok', 'native_focus_dispatch')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-report', type=Path,
                        help='Optional private release report to verify the running executable identity.')
    args = parser.parse_args()
    directory = lab.ROOT / 'reports/local' / ('wallpaper-cut-' + time.strftime('%Y%m%d-%H%M%S'))
    directory.mkdir(parents=True, exist_ok=False)
    report = {'checks': {}, 'restoration': {}, 'passed': False}
    scratch = checkpoint = original = original_idle = original_bar = workspace = focus = None
    backup_ready = False
    phase = 'preflight'
    try:
        require(session('omarchy-shell', 'lock', 'isLocked').strip() == 'false', 'guest_unlocked')
        eventually(lambda: client()['background_ready'] and h['saved']() and not client()['wallpaper_busy'],
                   'initial_desktop_ready')
        before = inspect()
        require('Vulkan' in before['adapter'], 'native_vulkan')
        require(before['client']['settings_writable'] and bool(before['client']['wallpaper']),
                'writable_preferences_and_original_wallpaper')
        require(not before['client']['launcher'] and not before['client']['live_wallpaper']['open']
                and not before['client']['notification_history_open']
                and not any(before['client'][name]['open'] for name in ('audio','network','bluetooth','power','tray')),
                'initial_panels_closed')
        monitors = json.loads(session('hyprctl', '-j', 'monitors'))
        require(len(monitors) == 1 and monitors[0]['scale'] == 1
                and monitors[0]['x'] == 0 and monitors[0]['y'] == 0, 'single_origin_output_at_1x')
        monitor = monitors[0]
        require(640 <= monitor['width'] <= 2560 and 580 <= monitor['height'] <= 1440,
                'bounded_native_capture_extent')
        require(not monitor.get('specialWorkspace', {}).get('id', 0), 'ordinary_initial_workspace')
        active = monitor['activeWorkspace']
        workspace = active.get('address') or (str(active['id']) if active.get('id', 0) > 0
                                               else 'name:' + active['name'])
        focus = json.loads(session('hyprctl', '-j', 'activewindow')).get('address')
        if args.build_report:
            build = json.loads(args.build_report.read_text())
            pid = remote('pgrep', '-x', 'lucent-desktop').strip()
            expected = build['binaries']['lucent-desktop']['sha256']
            require(remote('sha256sum', f'/proc/{pid}/exe').split()[0] == expected, 'running_binary_identity')
            report['tested_implementation_commit'] = build['source_commit']
            report['desktop_sha256'] = expected
        original = before['client']['wallpaper']
        original_bar = before['client']['bar_visible']
        original_idle = json.loads(session('omarchy-toggle-idle', 'status'))['enabled']
        scratch = remote('mktemp', '-d', '/tmp/lucent-wallpaper-cut.XXXXXX').strip()
        remote('python3', '-c', h['BACKUP'], scratch)
        backup_ready = True
        checkpoint = h['GuestWallpaperCheckpoint'](remote, original)
        session('omarchy-toggle-idle', 'stay-awake')
        dispatch('workspace', 'name:lucent-wallpaper-cut-' + str(time.monotonic_ns()))
        session('systemctl', '--user', 'stop', 'lucent.service')
        remote('python3', '-c', ISOLATE)
        session('systemctl', '--user', 'start', 'lucent.service')
        eventually(lambda: client()['background_ready'] and client()['live_wallpaper']['players'] == 0,
                   'isolated_static_desktop')
        remote('python3', '-c', ENCODE, scratch)
        cli('wallpaper', 'set', scratch + '/red.png')
        eventually(lambda: client()['wallpaper']['source']['path'] == scratch + '/red.png'
                   and not client()['wallpaper_busy'] and h['saved']()
                   and all(not item['presentation']['transition_pending']
                           for item in client()['background_transitions']), 'red_fixture_ready', timeout=45)
        name = lab.inventory()['vms']['lucent']['name']
        h['move'](name, monitor, 8, 80)
        time.sleep(.15)
        phase = 'native_compositor_recordings'
        result = json.loads(session('python3', '-c', PIXELS, scratch,
                                    str(monitor['width']), str(monitor['height']), timeout=95))
        report.update(result)
        metadata = json.loads(remote('python3', '-c', '''import json, sys
from pathlib import Path
root=Path(sys.argv[1]); files=[]
for label in ('red-to-blue','blue-to-red','selector-open','selector-close'):
    for suffix in ('.mkv','-cut.ppm','.log'):
        path=root/(label+suffix)
        if path.is_file() and not path.is_symlink(): files.append((path.name,path.stat().st_size))
print(json.dumps(files))
''', scratch))
        for filename, size in metadata:
            limit = 128 * 1024 if filename.endswith('.log') else 64 * 1024 * 1024
            require(0 <= size <= limit, 'bounded_private_capture_transfer')
            capture = h['run'](lab.ssh_args('lucent') + [shlex.join(['cat', scratch + '/' + filename])])
            require(len(capture) == size, 'complete_private_capture_transfer')
            (directory / filename).write_bytes(capture)
        require('failure' not in result, 'native_pixel_acceptance')
        report['passed'] = bool(report['checks']) and all(report['checks'].values())
    except BaseException:
        report['failed_phase'] = phase
    finally:
        def restore(label, operation):
            try:
                operation()
                report['restoration'][label] = True
            except BaseException:
                report['restoration'][label] = False
                report['passed'] = False
        if backup_ready:
            if checkpoint is not None:
                restore('selected_snapshot_checkpoint', checkpoint.restore)
            def public_restore():
                cli('wallpaper', 'live', 'close')
                cli('wallpaper', 'set', original['path'])
                eventually(lambda: client()['background_ready'] and not client()['wallpaper_busy']
                           and client()['wallpaper']['source'] == original['source'],
                           'original_wallpaper_transaction', timeout=45)
            restore('original_wallpaper_transaction', public_restore)
            def replace_files():
                session('systemctl', '--user', 'stop', 'lucent.service')
                remote('python3', '-c', h['RESTORE_FILES'], scratch)
            restore('exact_files_replaced_while_stopped', replace_files)
            def restart():
                session('systemctl', '--user', 'start', 'lucent.service')
                eventually(lambda: client()['background_ready'] and client()['wallpaper'] == original,
                           'original_desktop_ready', timeout=45)
                eventually(lambda: all(json.loads(remote('python3', '-c', h['VERIFY_RESTORED'], scratch)).values()),
                           'exact_settings_theme_login_exports', timeout=45)
            restore('exact_settings_wallpaper_theme_login_exports', restart)
            def restore_bar():
                cli('bar', 'show' if original_bar else 'hide')
                eventually(lambda: client()['bar_visible'] == original_bar, 'original_bar_visibility')
            restore('bar_visibility', restore_bar)
            def restore_idle():
                if original_idle is False:
                    session('omarchy-toggle-idle', 'allow-idle')
                require(json.loads(session('omarchy-toggle-idle', 'status'))['enabled'] == original_idle,
                        'original_idle_policy')
            restore('idle_policy', restore_idle)
            def restore_workspace():
                dispatch('workspace', workspace)
                if focus:
                    dispatch('window', focus)
            restore('workspace_and_focus', restore_workspace)
            if report['restoration'].get('exact_settings_wallpaper_theme_login_exports'):
                def cleanup():
                    remote('python3', '-c', CLEANUP, scratch)
                    if checkpoint is not None:
                        checkpoint.discard()
                restore('test_owned_assets_removed', cleanup)
        (directory / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        (lab.ROOT / 'reports/local/wallpaper-cut-native.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2), flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
