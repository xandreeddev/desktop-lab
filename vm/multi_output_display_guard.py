"""Private bounded owner; canonical native-output observers remain unchanged."""
import argparse,copy,ctypes,hashlib,json,os,signal,socket,stat,struct,sys,time
from pathlib import Path
sys.dont_write_bytecode=True
sys.path.insert(0,str(Path(__file__).resolve().parent))
import multi_output_guard as base
import multi_output_policy as p
import preservation_fingerprint as preservation
import display_supervision as supervision
import owned_application_units as units

require=p.require
BEGIN=b'-- BEGIN LUCENT DISPLAY CONFIGURATION\n'
END=b'-- END LUCENT DISPLAY CONFIGURATION\n'

def stamp(path,hashed=True,depth=0):
    path=Path(path)
    require(depth<=4 and path.is_absolute() and '..' not in path.parts
            and path.parent.resolve(strict=True)==path.parent,'canonical_security_path_required')
    meta=path.lstat()
    identity=lambda m:[m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid,m.st_size,m.st_mtime_ns,m.st_ctime_ns]
    require(meta.st_uid in (0,os.getuid()),'security_owner_required')
    if stat.S_ISLNK(meta.st_mode):
        target=os.readlink(path);resolved=path.resolve(strict=True)
        result={'identity':identity(meta),'link':target,'nlink':meta.st_nlink,
                'resolved_target':{'path':str(resolved),'stamp':stamp(resolved,hashed,depth+1)}}
    else:
        fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
        with os.fdopen(fd,'rb') as stream:
            before=os.fstat(fd)
            require(stat.S_ISREG(before.st_mode) and before.st_size<=128*1024*1024
                    and identity(before)==identity(meta),'bounded_unchanged_security_file_required')
            result={'identity':identity(before),'bytes':before.st_size,'uid':before.st_uid,'gid':before.st_gid,
                    'mode':stat.S_IMODE(before.st_mode),'nlink':before.st_nlink}
            if hashed:
                digest=hashlib.sha256();count=0
                for block in iter(lambda:stream.read(1024*1024),b''):
                    count+=len(block);require(count<=before.st_size,'security_file_bound');digest.update(block)
                require(count==before.st_size,'complete_security_file_required');result['sha256']=digest.hexdigest()
            require(identity(before)==identity(os.fstat(fd)),'security_file_changed')
    require(identity(meta)==identity(path.lstat()),'security_path_changed')
    return result

def metadata(value):
    if isinstance(value,dict): return {key:metadata(item) for key,item in value.items() if key!='sha256'}
    if isinstance(value,list): return [metadata(item) for item in value]
    return value

def keyboard_bindings(bindings):
    p.require_unbound_input(bindings)
    for row in bindings:
        modifiers=row['modmask']&~(2|16)
        if modifiers not in (0,4): continue
        keys={'a','code:30','code:38'} if modifiers==4 else {
            'tab','period','decimal','.',*[str(i) for i in range(10)],
            *['code:'+str(i) for i in (*range(2,12),*range(10,20),15,23,52,60)]}
        codes=(30,38) if modifiers==4 else (*range(2,12),*range(10,20),15,23,52,60)
        require(not row.get('catch_all') and str(row.get('key','')).lower() not in keys
                and row.get('keycode',0) not in codes,'native_display_typing_binding_intercepts_test')

def read(path):
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(fd,'rb') as stream:
        before=os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid()
                and before.st_size<=256*1024,'owned_monitor_file_required')
        raw=stream.read(256*1024+1)
        require(len(raw)==before.st_size and base.identity(before)==base.identity(os.fstat(fd))
                ==base.identity(path.lstat()),'monitor_file_changed')
    return raw,base.identity(before)

def original_file(expected,observed):
    # Creating/retaining the original-inode hardlink and Keep's own unlink may
    # change ctime; bytes/inode/device/mode/owner/size/mtime remain authoritative.
    return expected[0]==observed[0] and expected[1][:6]==observed[1][:6]

def keep_content(original,layout):
    require(original.count(BEGIN)<=1 and original.count(END)==original.count(BEGIN)
            and (BEGIN not in original or original.endswith(END)), 'unambiguous_monitor_tail_required')
    prefix=original.split(BEGIN,1)[0]
    if prefix and not prefix.endswith(b'\n'): prefix+=b'\n'
    rules=[]
    for m in sorted(layout,key=lambda row:row['name']):
        require(m['disabled'] is False and m['transform']==0,'owned_enabled_layout_required')
        rules.append('hl.monitor({ output = '+json.dumps(m['name'])+', disabled = false, mode = "'
            +str(m['width'])+'x'+str(m['height'])+'@'+format(round(m['refreshRate']*1000)/1000,'.3f')
            +'", position = "'+str(m['x'])+'x'+str(m['y'])+'", scale = '+format(m['scale'],'.3f')
            +', transform = 0 })\n')
    return prefix+BEGIN+''.join(rules).encode()+END

def exchange_restore(config,backup,original,kept,directory):
    require(original_file(original,read(backup)),'original_monitor_checkpoint_changed')
    current=read(config)
    if original_file(original,current): return
    require(kept is not None and current==kept,'independent_monitor_change_preserved')
    replacement=directory/'monitors.restore'
    os.link(backup,replacement)
    rename=ctypes.CDLL(None,use_errno=True).renameat2
    rename.argtypes=(ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint)
    rename.restype=ctypes.c_int
    def exchange():
        if rename(-100,os.fsencode(replacement),-100,os.fsencode(config),2):
            raise OSError(ctypes.get_errno(),'monitor_exchange_failed')
    exchange()
    try:
        require(original_file(current,read(replacement)),'raced_monitor_change_preserved')
        require(original_file(original,read(config)),'monitor_original_not_restored')
    except BaseException:
        require(original_file(original,read(config)),'uncertain_monitor_exchange_retained')
        exchange()
        raise
    replacement.unlink()
    fd=os.open(config.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try: os.fsync(fd)
    finally: os.close(fd)

class Owner(base.Native):
    def __init__(self,directory,header):
        self.directory,self.header=directory,header
        super().__init__(header['desktop_sha256'])
        expected=header['fresh_off_facts']['authority']['desktop']
        actual=units.process_snapshot(self.pid,hashed=True)
        require(all(actual[key]==expected[key] for key in ('pid','start','group','sha256')),
                'exact_fresh_desktop_lifetime_required')
        self.expected_sources={**header['fresh_off_facts']['sources'],**header['fresh_off_facts']['security']}
        self.security_sources(True)
        require(self.baseline[0]['name']=='Virtual-1' and self.baseline[0]['width']==1920
                and self.baseline[0]['height']==1080 and self.baseline[0]['x']==self.baseline[0]['y']==0
                and self.baseline[0]['scale']==1,'original_virtual_output_required')
        self.config=Path.home()/'.config/hypr/monitors.lua'
        self.original=read(self.config);self.kept=None;self.armed=None
        self.parent=str(self.config.parent)
        observed_host=p.host(base.inspect(),'Virtual-1')
        self.original_host={key:observed_host[key] for key in ('id','instance','generation')}
        self.original_global=p.output(base.inspect(),'Virtual-1')['global']
        os.link(self.config,directory/'monitors.original')
        require(original_file(self.original,read(self.config)),'monitor_checkpoint_changed_source')
        fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
        try: os.fsync(fd)
        finally: os.close(fd)
        self.admit_file(False)
        self.lease=base.Lease(self,header['output'],self.baseline)
        self.owned=None;self.allowed=[]

    def security_sources(self,hashed=False):
        require(len(self.expected_sources)<=128,'bounded_source_security_inventory')
        for path,expected in self.expected_sources.items():
            observed=stamp(path,hashed)
            require(observed==(expected if hashed else metadata(expected)),'source_or_security_changed')

    def admit_file(self,kept):
        current=base.preserved_files(True)
        expected=copy.deepcopy(self.files)
        actual=read(self.config)
        require((kept and self.armed is not None and actual[0]==self.armed)
                or (not kept and original_file(self.original,actual)), 'unowned_monitor_postimage')
        path=str(self.config)
        require(set(current['files'])==set(expected['files']) and set(current['directories'])==set(expected['directories']),
                'configuration_membership_changed')
        require(current['directories'][self.parent][:4]==expected['directories'][self.parent][:4],
                'monitor_parent_authority_changed')
        expected['files'][path]=current['files'][path]
        expected['directories'][self.parent]=current['directories'][self.parent]
        require(current==expected,'unrelated_configuration_or_media_changed')
        self.files=expected
        if kept: self.kept=actual

    def checked(self):
        self.preserve()
        self.security_sources()
        state=base.inspect()
        require(state['frame_profiling'] is False and state['presentation_feedback']['enabled'] is False
                and all(row['source_frames'] is None for row in state['client']['live_wallpaper']['producers']),
                'normal_profiling_must_remain_off')
        require(all(p.host(state,'Virtual-1')[key]==value for key,value in self.original_host.items())
                and p.output(state,'Virtual-1')['global']==self.original_global,'original_host_lifetime_changed')
        if self.owned is not None:
            current=p.unique(self.monitors(),lambda row:row['name']==self.lease.name,'owned_output_missing')
            p.unchanged_layout(self.monitors(),self.baseline,self.lease.name,p.monitor_signature(current))
            require(p.monitor_signature(current) in self.allowed and self.global_id(self.lease.name)==self.lease.global_id,
                    'owned_output_lifetime_or_unadmitted_layout')
        else: p.unchanged_layout(self.monitors(),self.baseline,self.lease.name,None)
        keyboards=[row for row in json.loads(base.hypr('-j','devices'))['keyboards'] if row.get('main')]
        require(len(keyboards)==1 and keyboards[0].get('layout')=='us'
                and keyboards[0].get('variant','')=='' and keyboards[0].get('capsLock') is False,'plain_us_keyboard_required')
        keyboard_bindings(json.loads(base.hypr('-j','binds')))
        return state

    def status(self):
        state=self.checked();child=state['client']['displays']
        require(not any(state['client'].get(name,{}).get(key) for name in
            ('audio','media','network','bluetooth','power','tray','weather','live_wallpaper','settings','application_actions')
            for key in ('open','visible','busy')),'no_foreign_panel_required')
        panels=[row for row in state['surfaces'] if row.get('role',row['id'])=='displays']
        panel=panels[0] if len(panels)==1 else None
        layer=None
        if child['open']:
            require(panel is not None and panel['output']=='Virtual-1' and panel['pointer_input']=='all'
                    and panel['keyboard']=='exclusive' and panel['frames']>0 and panel['native_frames']>0,
                    'owned_submitted_display_panel_required')
            require([row['id'] for row in state['surfaces'] if row['keyboard']=='exclusive']==[panel['id']],
                    'one_exclusive_display_target_required')
            layer=p.layer_for(json.loads(base.hypr('-j','layers')),'Virtual-1',panel,self.pid)
        self.input_preflight()
        return {'state':child,'panel':panel,'layer':layer,'monitors':self.monitors(),
                'host':self.original_host,'global':self.original_global,'cursor':json.loads(base.hypr('-j','cursorpos'))}

    def operation(self,request):
        operation=request['operation']
        if operation=='create':
            require(self.owned is None,'new_output_only')
            value=self.lease.create();self.owned=copy.deepcopy(self.lease.expected)
            self.allowed=[self.owned,{**self.owned,'x':self.owned['x']+16},{**self.owned,'scale':1.25}]
            return {'owned':self.owned,'allowed':self.allowed}
        if operation=='capture-keep':
            state=base.inspect()['client']['displays']
            require(self.armed is not None and state['transaction']['phase']=='kept','explicit_keep_ack_required')
            self.admit_file(True)
            base.private_json(self.directory/'kept.json',{'sha256':hashlib.sha256(self.kept[0]).hexdigest(),
                                                         'identity':self.kept[1]})
            return {'captured':True}
        value=self.status()
        if operation=='status': return value
        if operation=='arm-keep':
            require(value['state']['transaction']['phase']=='confirmation'
                    and p.monitor_signature(p.unique(self.monitors(),lambda row:row['name']==self.lease.name,'owned'))==self.allowed[1],
                    'owned_position_preview_required')
            self.armed=keep_content(self.original[0],self.monitors())
            base.private_json(self.directory/'keep-intent.json',{'expected_sha256':hashlib.sha256(self.armed).hexdigest()})
            return {'armed':True}
        if operation=='warp':
            require(value['panel'] is not None and request['monitor']=='Virtual-1'
                    and request['generation']==self.original_host['generation']
                    and request['global']==self.original_global,'exact_panel_host_required')
            point=p.hit_center(self.baseline[0],value['panel'],value['layer'],request['identifier'])
            self.cursor.begin(value['cursor'],point)
            require(base.hypr('eval',f'hl.dsp.cursor.move({{x={point[0]},y={point[1]}}})')=='ok','panel_warp_rejected')
            self.cursor.confirm(json.loads(base.hypr('-j','cursorpos')))
            return {'point':point,'surface':value['panel']['id']}
        require(operation=='finish','known_display_test_operation_required')
        return {'finishing':True}

    def cleanup(self):
        # Close only our initially-absent panel; another foreground panel blocks
        # restoration. No physical input, lock/auth operation or settings write.
        if base.inspect()['client']['displays']['open']:
            self.status();base.command(base.CLI,'displays','close')
        deadline=time.monotonic()+20
        while base.inspect()['client']['displays']['visible']:
            require(time.monotonic()<deadline,'display_close_deadline');time.sleep(.1)
        exchange_restore(self.config,self.directory/'monitors.original',self.original,self.kept,self.directory)
        self.admit_file(False)
        self.checked()
        if self.owned is not None:
            self.configure(p.case_layout(self.lease.name,self.baseline,1))
            self.wait_case(self.lease.name,p.case_layout(self.lease.name,self.baseline,1))
            self.lease.expected=self.owned
            self.lease.remove();self.owned=None
        self.preserve(full=True);self.restore_focus()
        self.checked()
        return {'original_monitor_bytes_inode':original_file(self.original,read(self.config)),
                'only_owned_output_removed':True,'original_workspace_cursor':True,
                'same_cohort_settings_media_and_profiling':True,
                'monitor_ctime_link_count_not_claimed_unchanged':True}

def serve(directory,header):
    owner=None;result={'success':False,'restoration':None}
    server=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    try:
        before={'preservation':preservation.fingerprint(base.command),
                'supervision':supervision.identity(base.command)}
        base.private_json(directory/'before-facts.json',before)
        owner_argv=['/usr/bin/python3','-I',str(directory/'guest.py'),'serve',str(directory)]
        origin=units.capture_unit_origin(header['unit'],owner_argv)
        require(origin['process']['pid']==os.getpid(),'same_owner_unit_lifetime_required')
        owner=Owner(directory,header)
        base.private_json(directory/'checkpoint.json',{'files':owner.files,'baseline':owner.baseline,
            'workspace':owner.workspace,'pointer':owner.pointer,'original_global':owner.original_global,
            'desktop_pid':owner.pid,'monitor_identity':owner.original[1],'owner_origin':origin})
        server.bind(str(directory/'control.sock'));os.chmod(directory/'control.sock',0o600)
        server.listen(1);server.settimeout(.5)
        base.private_json(directory/'ready.json',{'ready':True,'desktop_pid':owner.pid})
        deadline=time.monotonic()+200;finished=False
        while not finished and time.monotonic()<deadline:
            try: client,_=server.accept()
            except TimeoutError: continue
            with client:
                require(struct.unpack('3i',client.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))[1]==os.getuid(),'owned_peer_required')
                request_deadline=min(deadline,time.monotonic()+2);data=bytearray()
                while b'\n' not in data and len(data)<=1024:
                    remaining=request_deadline-time.monotonic()
                    require(remaining>0,'absolute_request_deadline')
                    client.settimeout(remaining)
                    block=client.recv(1024);require(bool(block),'complete_guard_request');data.extend(block)
                require(len(data)<=1024,'bounded_guard_request')
                request=json.loads(data)
                require(time.monotonic()<deadline,'owner_absolute_deadline')
                value=owner.operation(request)
                client.settimeout(2)
                client.sendall((json.dumps({'ok':True,'value':value})+'\n').encode())
                finished=request['operation']=='finish'
        require(finished,'bounded_display_test_expired')
        result['success']=True
    except BaseException as error:
        result['failure']=str(error) if isinstance(error,p.Blocked) else type(error).__name__
    finally:
        server.close()
        if owner is not None:
            try:
                result['restoration']=owner.cleanup()
                owner.security_sources(True)
                after={'preservation':preservation.fingerprint(base.command),
                       'supervision':supervision.identity(base.command)}
                base.private_json(directory/'after-facts.json',after)
                require(before==after,'final_supervision_settings_or_preservation_changed')
                result['restoration']['preservation_supervision_security_sources_and_profiles']=True
            except BaseException as error:
                result['success']=False
                result['cleanup_blocked']=str(error) if isinstance(error,p.Blocked) else type(error).__name__
        base.private_json(directory/'finished.json',result)

def main():
    require(__debug__,'python_assertions_required')
    parser=argparse.ArgumentParser();parser.add_argument('mode',choices=('serve','request'))
    parser.add_argument('directory',type=Path);parser.add_argument('operation',nargs='?')
    parser.add_argument('--identifier');parser.add_argument('--generation',type=int);parser.add_argument('--global-id',type=int)
    args=parser.parse_args();directory=args.directory;meta=directory.lstat()
    require(directory.is_absolute() and directory.resolve(strict=True)==directory
            and directory.name.startswith('native-two-output-displays-') and stat.S_ISDIR(meta.st_mode)
            and stat.S_IMODE(meta.st_mode)==0o700 and meta.st_uid==os.getuid(),'private_test_directory_required')
    if args.mode=='request':
        base.request(directory,args.operation,None,'Virtual-1',args.identifier,args.generation,args.global_id)
    else:
        signal.signal(signal.SIGTERM,lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
        header=json.loads((directory/'header.json').read_bytes())
        for name,expected in header['source_sha256'].items():
            value=directory/name;meta=value.lstat()
            require(value.name==name and not value.is_symlink() and stat.S_ISREG(meta.st_mode)
                    and meta.st_uid==os.getuid() and stat.S_IMODE(meta.st_mode)==0o600
                    and hashlib.sha256(value.read_bytes()).hexdigest()==expected,'staged_source_changed')
        serve(directory,header)

if __name__=='__main__': main()
