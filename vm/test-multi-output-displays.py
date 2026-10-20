"""Guarded two-output Displays acceptance. Default refuses UNBOUND before IO."""
import argparse,base64,hashlib,json,os,re,secrets,shlex,signal,stat,sys,time,types
from pathlib import Path

HERE=Path(__file__).resolve().parent
ROOT=HERE.parent if HERE.name=='vm' else HERE.parents[1]
SOURCE='35ed87ed4c01cf62e3c8406b8abdcc0809cc155b'
DESKTOP='6ff4e06b73f1a1f68f27cb0504c67710b0a9e118b181ba8ee41eb78d6251ef78'
FRESH_OFF=('UNBOUND','UNBOUND')
PINS={
 'preservation_fingerprint':('vm/preservation_fingerprint.py','ecd796f7bce24e4ea153633089595c7ba4d5604959ca5f0131367832b173e2a3'),
 'display_supervision':('vm/display_supervision.py','1399eebb03af1ae2408124c0b46706f109ed99cc49f8cd00165021d935cab0be'),
 'owned_application_units':('vm/owned_application_units.py','0082d9c4c87c36a7cc65f361304a2fbda174af83f30d6713720d709a9193d5b7'),
 'native':('vm/test-native-multi-output.py','c1fc9b0aecd910cc9e7e46247bb883f86bc3cb675c44216943f5f03f12fe0829'),
 'multi_output_guard':('vm/multi_output_guard.py','05cc165f9b823becd28e8a631920e120ceb75afd73e026a3b91a64404729d765'),
 'multi_output_policy':('vm/multi_output_policy.py','25c0e45f87c25363220cffdfd901774856c8792f7e4298946fc9011dbb2ebb58')}
GUEST_SHA='e69a056b919d0fc29a69a62c34079481f6b3c62e9400fad1cda7a809b4b2014f'

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

def qualify(fresh_off=FRESH_OFF):
    require(__debug__,'python_assertions_required')
    require(re.fullmatch('[a-f0-9]{64}',fresh_off[1]) and fresh_off[0]!='UNBOUND'
            and re.fullmatch('[a-f0-9]{64}',GUEST_SHA),'reviewed_execution_bindings_UNBOUND')
    raw={name:pinned(pin) for name,pin in PINS.items()}
    fresh=json.loads(pinned(fresh_off))
    require(fresh['success'] is True and fresh['read_only'] is True and fresh['source_commit']==SOURCE
            and fresh['binding_sha256']=='6fed4bf66f418b2159924c23553db911946e17de400eae12415e855946095218'
            and fresh['helper_sha256']=='2bec9527e244c06be96b7f246631f8a39c67e25540c19f244f57eca13d9d6a89'
            and fresh['facts']['runtime_profiling']=={'frames':False,'presentation':False,'source_pts':False},
            'exact_normal_off_receipt_required')
    require(fresh['facts']['authority']['desktop']['sha256']==DESKTOP,'exact_35ed_release_required')
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

def run(lab_root,fresh_off=FRESH_OFF):
    raw,fresh,guest=qualify(fresh_off)
    (ROOT/'reports/local').mkdir(mode=0o700,exist_ok=True)
    # VM construction is deliberately after every local receipt/source gate.
    sys.path.insert(0,str(ROOT/'vm'))
    native=module('two_output_native',raw['native'],ROOT/PINS['native'][0])
    vm=native.VM(lab_root);tag=secrets.token_hex(12)
    output='LUCENT-TEST-'+tag
    report={'success':False,'runtime_source':SOURCE,'desktop_sha256':DESKTOP,'checks':[],
            'helper_sha256':digest(read(Path(__file__))), 'guest_sha256':GUEST_SHA,
            'fresh_off_sha256':fresh_off[1],'physical_or_secure_output_acceptance':False}
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
                point=native.hit_center(current['monitors'][0] if current['monitors'][0]['name']=='Virtual-1'
                    else next(m for m in current['monitors'] if m['name']=='Virtual-1'),panel,current['layer'],identifier)
            except native.Blocked:
                key(15);continue
            moved=request('warp',identifier=identifier,generation=current['host']['generation'],global_id=current['global'])
            after=status()
            require(after['panel']['id']==panel['id'] and after['panel']['instance']==panel['instance']
                    and after['layer']==current['layer'] and after['panel']['hits']==panel['hits']
                    and tuple(moved['point'])==point and (after['cursor']['x'],after['cursor']['y'])==point,
                    'exact_current_hit_required')
            require(input_deadline is not None and time.monotonic()<input_deadline,'bounded_native_input_window')
            held=True
            try: vm.button(True)
            finally: vm.button(False);held=False
            time.sleep(.15);return
        raise ValueError('native_display_control_not_reachable')
    def key(*codes,focus=None):
        current=status()
        require(current['panel'] is not None and (focus is None or current['panel']['focus']==focus),
                'exact_keyboard_focus_required')
        require(input_deadline is not None and time.monotonic()<input_deadline,'bounded_native_input_window')
        vm.virsh('send-key',vm.name,'--codeset','linux','--holdtime','50',*[str(code) for code in codes])
        time.sleep(.08)
    def enter(index,text):
        current=status();identifier=next(hit['id'] for hit in current['panel']['hits'] if hit['id'].startswith(f'display-field-{index}-'))
        click(identifier);key(29,30,focus=identifier)
        codes={'0':11,'1':2,'2':3,'3':4,'4':5,'5':6,'6':7,'7':8,'8':9,'9':10,'.':52}
        for char in text: key(codes[char],focus=identifier)
    def open_panel():
        vm.session('/home/omarchy/.local/bin/lucent-cli','displays','open')
        wait(lambda:vm.inspect()['client']['displays']['open'] and not vm.inspect()['client']['displays']['busy'],'display_panel_ready')
        click('display-reset');wait(lambda:not vm.inspect()['client']['displays']['busy'],'fresh_draft_ready')
        click('display-select-'+output)
        require(status()['state']['selected']==output,'owned_output_selected')
    def observed(): return [native.monitor_signature(row) for row in vm.monitors()]
    def owned_postimage(expected):
        status()
        return next(row for row in observed() if row['name']==output)==expected
    try:
        started=True
        vm.session('systemd-run','--user','--quiet','--collect','--unit='+unit,
            '--property=Type=exec','--property=RuntimeMaxSec=300','--property=TimeoutStopSec=20',
            '--property=KillMode=control-group','--property=Restart=no','--property=MemoryMax=192M','--property=TasksMax=32',
            '--property=UMask=0077','/usr/bin/python3','-I',directory+'/guest.py','serve',directory)
        wait(lambda:vm.remote('/usr/bin/python3','-I','-c',
            'import os,sys; print(os.path.isfile(sys.argv[1]))',directory+'/ready.json')=='True','independent_owner_ready',35)
        input_deadline=time.monotonic()+150
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
        wait(lambda:phase()=='confirmation','kept_position_preview_confirmation');request('arm-keep')
        click('display-keep');wait(lambda:phase()=='kept','native_keep_acknowledged')
        require(request('capture-keep')['captured'],'exact_saved_monitor_postimage')
        report['checks'].append('two_output_native_keep_complete_monitor_tail')
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
                require(done['success'] is True and all(done['restoration'].values()),'guarded_restoration_required')
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
    args=parser.parse_args()
    require(args.lab_root.is_absolute(),'absolute_lab_root_required')
    fresh_off=FRESH_OFF
    if args.fresh_off is not None or args.fresh_off_sha256 is not None:
        require(args.fresh_off is not None and args.fresh_off_sha256 is not None,'fresh_receipt_and_sha256_required_together')
        path=args.fresh_off if args.fresh_off.is_absolute() else args.lab_root/args.fresh_off
        fresh_off=(str(path),args.fresh_off_sha256)
    if not args.run:
        qualify(fresh_off);print(json.dumps({'qualified':True,'guest_actions':False}));return 0
    signal.signal(signal.SIGTERM,lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    return run(args.lab_root.resolve(strict=True),fresh_off)

if __name__=='__main__':
    try: result=main()
    except ValueError as error:
        print(json.dumps({'success':False,'failure':str(error),
                          'guest_actions':'unknown' if '--run' in sys.argv[1:] else False}));result=1
    raise SystemExit(result)
