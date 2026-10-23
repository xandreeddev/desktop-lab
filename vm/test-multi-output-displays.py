"""Guarded owned-peer rotation, enable and fractional Displays acceptance.

Default refuses UNBOUND before IO. Native controls use real QMP input; this
source is an acceptance plan until an explicit, reviewed run produces receipts.
"""
import argparse,base64,hashlib,json,os,re,secrets,shlex,signal,stat,sys,time,types
from pathlib import Path

HERE=Path(__file__).resolve().parent
ROOT=HERE.parent if HERE.name=='vm' else HERE.parents[1]
SOURCE='26d03adf476af44c97fb7dcdbfb75cbdc2112770'
DESKTOP='7de90a04d86621d7f51aa19f37f74c5a0675aaa17557dfb9255c9e437d4acb08'
RELEASE_MANIFEST_SHA='5b228fa1bbdc71c50a245edcb70e726ef4d5826f476ba261e53042023d322aec'
VM_UUID='a4691892-fa44-41d9-98cf-8f0dc4131ee3'
FRESH_OFF=('UNBOUND','UNBOUND')
NORMAL_OFF_IDENTITY=('UNBOUND','UNBOUND')
RELEASE_MANIFEST='UNBOUND'
PINS={
 'preservation_fingerprint':('vm/preservation_fingerprint.py','ecd796f7bce24e4ea153633089595c7ba4d5604959ca5f0131367832b173e2a3'),
 'display_supervision':('vm/display_supervision.py','1399eebb03af1ae2408124c0b46706f109ed99cc49f8cd00165021d935cab0be'),
 'owned_application_units':('vm/owned_application_units.py','0082d9c4c87c36a7cc65f361304a2fbda174af83f30d6713720d709a9193d5b7'),
 'native':('vm/test-native-multi-output.py','c1fc9b0aecd910cc9e7e46247bb883f86bc3cb675c44216943f5f03f12fe0829'),
 'multi_output_guard':('vm/multi_output_guard.py','05cc165f9b823becd28e8a631920e120ceb75afd73e026a3b91a64404729d765'),
 'multi_output_policy':('vm/multi_output_policy.py','dd61130d7ec00e175c69e496f830fa482167d201c5fc3481b545b20660c4f2a0')}
GUEST_SHA='2e1510e3f246b598c4f125f484c5793f327e77529d8e7b6b92dc82be707bb8f5'

def require(value,label):
    if not value: raise ValueError(label)

def digest(raw): return hashlib.sha256(raw).hexdigest()

def read(path,limit=2*1024*1024):
    require(path.is_absolute() and path.resolve(strict=True)==path,'canonical_source_required')
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(fd,'rb') as stream:
        value=os.fstat(fd);raw=stream.read(limit+1)
        stamp=lambda m:(m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        require(stat.S_ISREG(value.st_mode) and value.st_uid==os.getuid() and value.st_nlink==1
                and len(raw)<=limit and stamp(value)==stamp(os.fstat(fd))==stamp(path.lstat()),'owned_unchanged_source_required')
    return raw

def pinned(pin):
    raw=read(ROOT/pin[0]);require(digest(raw)==pin[1],'frozen_source_changed');return raw

def module(name,raw,path):
    value=types.ModuleType(name);value.__file__=str(path)
    exec(compile(raw,str(path),'exec'),value.__dict__);return value

def qualify(fresh_off=FRESH_OFF,normal_off=NORMAL_OFF_IDENTITY,release_manifest=RELEASE_MANIFEST):
    require(__debug__,'python_assertions_required')
    require(re.fullmatch('[a-f0-9]{64}',fresh_off[1]) and fresh_off[0]!='UNBOUND'
            and all(re.fullmatch('[a-f0-9]{64}',value) for value in normal_off)
            and release_manifest!='UNBOUND' and re.fullmatch('[a-f0-9]{64}',GUEST_SHA),
            'reviewed_execution_bindings_UNBOUND')
    raw={name:pinned(pin) for name,pin in PINS.items()}
    release=json.loads(pinned((release_manifest,RELEASE_MANIFEST_SHA)))
    require(release['source_commit']==SOURCE and release['passed'] is True
            and release['source_working_tree_changed'] is False
            and release['native_activated'] is False
            and release['binaries']['lucent-desktop']['sha256']==DESKTOP,
            'exact_archived_build_manifest_required')
    fresh=json.loads(pinned(fresh_off))
    require(fresh['success'] is True and fresh['read_only'] is True and fresh['source_commit']==SOURCE
            and fresh['binding_sha256']==normal_off[0]
            and fresh['helper_sha256']==normal_off[1]
            and fresh['facts']['runtime_profiling']=={'frames':False,'presentation':False,'source_pts':False},
            'exact_normal_off_receipt_required')
    require(fresh['facts']['authority']['desktop']['sha256']==DESKTOP,'exact_archived_desktop_required')
    guest=read(HERE/'multi_output_display_guard.py');require(digest(guest)==GUEST_SHA,'reviewed_guest_changed')
    return raw,fresh['facts'],guest

STAGE=r'''
import base64,json,os,stat,sys
from pathlib import Path
packet=json.loads(sys.stdin.buffer.read(2*1024*1024+1));home=Path.home();cache=home/'.cache'
assert cache.resolve(strict=True)==cache and cache.stat().st_uid==os.getuid() and not cache.stat().st_mode&0o022
path=cache/('native-two-output-displays-'+packet['tag']);os.mkdir(path,0o700)
assert path.resolve(strict=True)==path and stat.S_IMODE(path.stat().st_mode)==0o700
for name,data in packet['files'].items():
 assert name in ('guest.py','multi_output_guard.py','multi_output_policy.py','preservation_fingerprint.py',
                 'display_supervision.py','owned_application_units.py','header.json')
 raw=base64.b64decode(data,validate=True)
 with os.fdopen(os.open(path/name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600),'wb') as stream:stream.write(raw)
print(path)
'''

def run(lab_root,fresh_off=FRESH_OFF,vm_uuid=None,normal_off=NORMAL_OFF_IDENTITY,release_manifest=RELEASE_MANIFEST):
    require(vm_uuid==VM_UUID,'exact_vm_uuid_required')
    raw,fresh,guest=qualify(fresh_off,normal_off,release_manifest)
    (ROOT/'reports/local').mkdir(mode=0o700,exist_ok=True)
    # VM construction is deliberately after every local receipt/source gate.
    sys.path.insert(0,str(ROOT/'vm'))
    native=module('two_output_native',raw['native'],ROOT/PINS['native'][0])
    vm=native.VM(lab_root)
    require(vm.virsh('domuuid',vm.name).strip()==vm_uuid,'exact_vm_uuid_changed')
    policy=module('two_output_display_policy',raw['multi_output_policy'],ROOT/PINS['multi_output_policy'][0])
    tag=secrets.token_hex(12)
    output='LUCENT-TEST-'+tag
    report={'success':False,'runtime_source':SOURCE,'desktop_sha256':DESKTOP,'checks':[],
            'helper_sha256':digest(read(Path(__file__))), 'guest_sha256':GUEST_SHA,
            'fresh_off_sha256':fresh_off[1],'fresh_off_binding_sha256':normal_off[0],
            'fresh_off_helper_sha256':normal_off[1],'release_manifest_sha256':RELEASE_MANIFEST_SHA,'vm_uuid':vm_uuid,
            'physical_or_secure_output_acceptance':False,'presentation_or_performance_acceptance':False}
    unit='lucent-native-display-test-'+tag+'.service'
    header={'desktop_sha256':DESKTOP,'output':output,'fresh_off_facts':fresh,'unit':unit}
    files={name+'.py':raw[name] for name in ('multi_output_guard','multi_output_policy','preservation_fingerprint',
                                           'display_supervision','owned_application_units')}
    header['source_sha256']={name:digest(data) for name,data in {**files,'guest.py':guest}.items()}
    files.update({'guest.py':guest,'header.json':json.dumps(header).encode()})
    payload=json.dumps({'tag':tag,'files':{name:base64.b64encode(data).decode() for name,data in files.items()}})
    require(len(payload)<=2*1024*1024,'bounded_stage_required')
    directory=vm.remote('/usr/bin/python3','-I','-c',STAGE,data=payload,timeout=20)
    require(re.fullmatch(r'/home/omarchy/\.cache/native-two-output-displays-[a-f0-9]{24}',directory),'owned_stage_required')
    report['guest_directory']=directory
    started=False;held=False;input_deadline=None
    def request(operation,**options):
        args=['/usr/bin/python3','-I',directory+'/guest.py','request',directory,operation]
        for key,value in options.items(): args+=['--'+key.replace('_','-'),str(value)]
        reply=json.loads(vm.session(*args))
        require(reply['ok'] is True,'guard_admission_refused')
        return reply['value']
    def status(): return request('status')
    def phase(): return (vm.inspect()['client']['displays'].get('transaction') or {}).get('phase')
    def wait(predicate,label,timeout=15): return native.eventually(predicate,label,timeout)
    def click(identifier):
        nonlocal held
        for _ in range(45):
            current=status();panel=current['panel']
            try:
                monitor=next(m for m in current['monitors'] if m['name']==panel['output'])
                point=native.hit_center(monitor,panel,current['layer'],identifier)
            except native.Blocked:
                key(15);continue
            moved=request('warp',monitor=panel['output'],identifier=identifier,
                          generation=current['host']['generation'],global_id=current['global'])
            after=status()
            require(after['panel']['id']==panel['id'] and after['panel']['instance']==panel['instance']
                    and after['layer']==current['layer'] and after['panel']['hits']==panel['hits']
                    and tuple(moved['point'])==point and (after['cursor']['x'],after['cursor']['y'])==point,
                    'exact_current_hit_required')
            policy.require_owned_display_control(identifier,after['state']['selected'],output)
            require(input_deadline is not None and time.monotonic()<input_deadline,'bounded_native_input_window')
            held=True
            try:
                vm.button(True)
                down=status()
                require(down['panel']['id']==panel['id'] and down['panel']['output']==panel['output']
                        and down['panel']['focus']==identifier
                        and (down['cursor']['x'],down['cursor']['y'])==point,
                        'native_down_focus_on_exact_panel_required')
            finally: vm.button(False);held=False
            time.sleep(.15);return
        raise ValueError('native_display_control_not_reachable')
    def key(*codes,focus=None):
        current=status()
        require(current['panel'] is not None and (focus is None or current['panel']['focus']==focus),
                'exact_keyboard_focus_required')
        require(codes in ((15,),(1,)) or current['state']['selected']==output,
                'native_typing_must_target_only_owned_display_draft')
        require(input_deadline is not None and time.monotonic()<input_deadline,'bounded_native_input_window')
        vm.virsh('send-key',vm.name,'--codeset','linux','--holdtime','50',*[str(code) for code in codes])
        time.sleep(.08)
    def enter(index,text):
        current=status();identifier=next(hit['id'] for hit in current['panel']['hits'] if hit['id'].startswith(f'display-field-{index}-'))
        click(identifier);key(29,30,focus=identifier)
        codes={'0':11,'1':2,'2':3,'3':4,'4':5,'5':6,'6':7,'7':8,'8':9,'9':10,'.':52}
        for char in text: key(codes[char],focus=identifier)
    def open_panel(target='Virtual-1'):
        current=status()
        if current['panel_target']!=target:
            require(not current['state']['visible'],'close_panel_before_changing_native_host')
            state=vm.inspect();host=native.host(state,target);global_id=native.output(state,target)['global']
            moved=request('bar-warp',monitor=target,generation=host['generation'],global_id=global_id)
            wait(lambda:status()['active_monitor']==target,'real_bar_pointer_enter_selects_panel_host')
            require(status()['cursor']=={'x':moved['point'][0],'y':moved['point'][1]},'bar_cursor_postimage_required')
        vm.session('/home/omarchy/.local/bin/lucent-cli','displays','open')
        wait(lambda:vm.inspect()['client']['displays']['open'] and not vm.inspect()['client']['displays']['busy'],'display_panel_ready')
        require(status()['panel']['output']==target,'display_panel_must_use_requested_native_host')
        click('display-reset');wait(lambda:not vm.inspect()['client']['displays']['busy'],'fresh_draft_ready')
        click('display-select-'+output)
        require(status()['state']['selected']==output,'owned_output_selected')
    def observed():
        current=status()
        return sorted([native.monitor_signature(row) for row in current['monitors'] if row['name']!=output]
                      +[current['peer']],key=lambda row:row['name'])
    def owned_postimage(expected):
        current=status()
        return current['peer_ready'] and current['peer']==expected
    def keep():
        request('arm-keep');click('display-keep');wait(lambda:phase()=='kept','native_keep_acknowledged')
        require(request('capture-keep')['captured'],'exact_saved_monitor_postimage')
    def select_mode(expected):
        monitor=next(row for row in json.loads(vm.hypr('-j','monitors','all')) if row['name']==output)
        index=policy.display_mode_index(monitor,expected['width'],expected['height'],round(expected['refreshRate']*1000))
        click('display-mode-'+str(index))
    try:
        started=True
        vm.session('systemd-run','--user','--quiet','--collect','--unit='+unit,
            '--property=Type=exec','--property=RuntimeMaxSec=600','--property=TimeoutStopSec=20',
            '--property=KillMode=control-group','--property=Restart=no','--property=MemoryMax=192M','--property=TasksMax=32',
            '--property=UMask=0077','/usr/bin/python3','-I',directory+'/guest.py','serve',directory)
        wait(lambda:vm.remote('/usr/bin/python3','-I','-c',
            'import os,sys; print(os.path.isfile(sys.argv[1]))',directory+'/ready.json')=='True','independent_owner_ready',35)
        input_deadline=time.monotonic()+420
        owned=request('create')['owned'];baseline=observed()
        open_panel();enter(0,str(owned['x']+16))
        require(observed()==baseline,'draft_must_not_change_native_layout')
        report['checks'].append('two_output_draft_has_no_native_effect')
        click('display-apply');wait(lambda:phase()=='confirmation','position_preview_confirmation')
        require(owned_postimage({**owned,'x':owned['x']+16}),'exact_owned_position_preview')
        request('status');click('display-revert');wait(lambda:phase()=='reverted' and observed()==baseline,'explicit_whole_layout_revert')
        report['checks'].append('two_output_apply_explicit_revert')
        open_panel();enter(2,'1.25');click('display-apply');wait(lambda:phase()=='confirmation','scale_preview_confirmation')
        require(owned_postimage({**owned,'scale':1.25}),'exact_owned_fractional_preview')
        request('status');wait(lambda:phase()=='reverted' and observed()==baseline,'independent_timeout_revert',23)
        report['checks'].append('two_output_fractional_scale_timeout_revert')
        open_panel();enter(0,str(owned['x']+16));click('display-apply')
        wait(lambda:phase()=='confirmation','kept_position_preview_confirmation');keep()
        report['checks'].append('two_output_native_keep_complete_monitor_tail')

        positioned={**owned,'x':owned['x']+16};positioned_layout=observed()
        open_panel();click('display-rotation')
        require(observed()==positioned_layout,'rotation_draft_must_not_change_native_output')
        click('display-apply');wait(lambda:phase()=='confirmation','rotation_preview_confirmation')
        wait(lambda:owned_postimage({**positioned,'transform':1}),'owned_peer_rotated_natively')
        click('display-revert')
        wait(lambda:phase()=='reverted' and observed()==positioned_layout,'rotation_whole_layout_revert')
        report['checks'].append('owned_peer_rotation_apply_and_revert')

        open_panel();click('display-enabled');request('expect-disable')
        require(observed()==positioned_layout,'disabled_draft_must_not_change_native_output')
        click('display-apply');wait(lambda:phase()=='confirmation','disable_preview_confirmation')
        disabled={key:owned[key] for key in ('id','name','description')};disabled['disabled']=True
        wait(lambda:owned_postimage(disabled),'owned_peer_disabled_and_old_native_host_retired')
        keep();report['checks'].append('owned_peer_disable_and_keep')

        open_panel();click('display-enabled');select_mode(positioned)
        enter(0,str(positioned['x']));enter(1,str(positioned['y']));enter(2,'1')
        request('expect-enable');click('display-apply')
        wait(lambda:phase()=='confirmation','reenable_preview_confirmation')
        wait(lambda:owned_postimage(positioned),'owned_peer_reenabled_with_new_native_lifetime')
        keep();report['checks'].append('owned_peer_reenable_controls_apply_and_keep')

        open_panel();enter(2,'1.25');click('display-apply')
        wait(lambda:phase()=='confirmation','fractional_kept_preview_confirmation')
        fractional={**positioned,'scale':1.25}
        wait(lambda:owned_postimage(fractional),'owned_peer_fractional_scale_applied')
        keep();report['checks'].append('owned_peer_fractional_scale_keep')
        key(1);wait(lambda:not vm.inspect()['client']['displays']['visible'],'native_panel_escape_closed')
        open_panel(output)
        current=status();panel=current['panel']
        require(panel['output']==output and abs(panel['density']-1.25)<.001
                and (panel['buffer_width'],panel['buffer_height'])==(1280,960),
                'actual_fractional_panel_and_physical_buffer_required')
        report['fractional_panel']={key:panel[key] for key in
            ('id','output','width','height','density','buffer_width','buffer_height','native_frames')}
        fraction_layout=observed();enter(0,str(owned['x']+32))
        require(observed()==fraction_layout,'fractional_panel_edit_is_only_a_draft')
        click('display-apply');wait(lambda:phase()=='confirmation','fractional_panel_apply_confirmation')
        wait(lambda:owned_postimage({**fractional,'x':owned['x']+32}),'fractional_native_typing_applied_exact_position')
        click('display-revert')
        wait(lambda:phase()=='reverted' and observed()==fraction_layout,'fractional_panel_native_revert')
        report['checks'].append('fractional_owned_panel_real_pointer_keyboard_apply_revert')
        report['success']=True
    except BaseException as error:
        report['failure']=str(error) if isinstance(error,(ValueError,native.Blocked)) else type(error).__name__
    finally:
        if held:
            try: vm.button(False)
            except BaseException: report['release_pending']=True
        if started:
            try:
                def finished(): return json.loads(vm.remote('/usr/bin/python3','-I','-c',
                    'from pathlib import Path; import sys; p=Path(sys.argv[1]); print(p.read_text() if p.is_file() else "null")',
                    directory+'/finished.json'))
                done=finished()
                if done is None:
                    try: request('finish')
                    except BaseException: pass  # A terminal owner may have closed its socket already.
                    done=wait(finished,'independent_cleanup_receipt',75)
                report['owner']=done
                require(done['success'] is True and all(done['restoration'].values())
                        and done['cursor_restoration']['restored'] is True,'guarded_restoration_required')
            except BaseException as error:
                report['success']=False;report['cleanup_pending']=type(error).__name__
        path=ROOT/'reports/local'/('native-two-output-displays-'+tag+'.json')
        native.private_json(path,report)
        print(json.dumps({'success':report['success'],'report':str(path.relative_to(ROOT)),
                          'source_only_helper_until_executed':False}))
    return 0 if report['success'] else 1

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--run',action='store_true')
    parser.add_argument('--lab-root',type=Path,default=ROOT)
    parser.add_argument('--fresh-off',type=Path,help='Private normal-OFF receipt; relative paths use --lab-root')
    parser.add_argument('--fresh-off-sha256',help='Reviewed SHA-256 of that private receipt')
    parser.add_argument('--fresh-off-binding-sha256',help='Reviewed collector binding SHA-256 for this actual fresh receipt')
    parser.add_argument('--fresh-off-helper-sha256',help='Reviewed collector helper SHA-256 for this actual fresh receipt')
    parser.add_argument('--release-manifest',type=Path,help='Exact archived 26d build manifest; relative paths use --lab-root')
    parser.add_argument('--vm-uuid',help='Pinned UUID of the already-running Lucent test guest; required with --run')
    args=parser.parse_args()
    require(args.lab_root.is_absolute(),'absolute_lab_root_required')
    fresh_off=FRESH_OFF
    if args.fresh_off is not None or args.fresh_off_sha256 is not None:
        require(args.fresh_off is not None and args.fresh_off_sha256 is not None,'fresh_receipt_and_sha256_required_together')
        path=args.fresh_off if args.fresh_off.is_absolute() else args.lab_root/args.fresh_off
        fresh_off=(str(path),args.fresh_off_sha256)
    normal_off=NORMAL_OFF_IDENTITY
    if args.fresh_off_binding_sha256 is not None or args.fresh_off_helper_sha256 is not None:
        require(args.fresh_off_binding_sha256 is not None and args.fresh_off_helper_sha256 is not None,
                'fresh_collector_binding_and_helper_required_together')
        normal_off=(args.fresh_off_binding_sha256,args.fresh_off_helper_sha256)
    release_manifest=RELEASE_MANIFEST
    if args.release_manifest is not None:
        path=args.release_manifest if args.release_manifest.is_absolute() else args.lab_root/args.release_manifest
        release_manifest=str(path)
    if not args.run:
        qualify(fresh_off,normal_off,release_manifest);print(json.dumps({'qualified':True,'guest_actions':False}));return 0
    require(args.vm_uuid==VM_UUID,'exact_vm_uuid_required')
    signal.signal(signal.SIGTERM,lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    return run(args.lab_root.resolve(strict=True),fresh_off,args.vm_uuid,normal_off,release_manifest)

if __name__=='__main__':
    try: result=main()
    except ValueError as error:
        print(json.dumps({'success':False,'failure':str(error),
                          'guest_actions':'unknown' if '--run' in sys.argv[1:] else False}));result=1
    raise SystemExit(result)
