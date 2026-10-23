"""Private bounded owner; canonical native-output observers remain unchanged."""
import argparse,contextlib,copy,ctypes,hashlib,json,math,os,signal,socket,stat,struct,subprocess,sys,time
from pathlib import Path
sys.dont_write_bytecode=True
sys.path.insert(0,str(Path(__file__).resolve().parent))
import multi_output_guard as base
import multi_output_policy as p
import multi_output_display_policy as d
import multi_output_display_timing as timing
import preservation_fingerprint as preservation
import display_supervision as supervision
import owned_application_units as units

require=p.require
BEGIN=b'-- BEGIN LUCENT DISPLAY CONFIGURATION\n'
END=b'-- END LUCENT DISPLAY CONFIGURATION\n'


class PhaseDeadline(TimeoutError):
    """A fixed test-owner phase exhausted its own finite observation budget."""


@contextlib.contextmanager
def phase_deadline(seconds):
    previous=signal.getsignal(signal.SIGALRM)
    require(signal.getitimer(signal.ITIMER_REAL)==(0.0,0.0),'no_preexisting_display_phase_timer')
    def expired(*_):
        raise PhaseDeadline('bounded_display_phase_expired')
    try:
        signal.signal(signal.SIGALRM,expired)
        # An existing cleanup catch may handle one exception while recording
        # lost cursor authority. A short repeat keeps the phase bound active.
        signal.setitimer(signal.ITIMER_REAL,seconds,.1)
        yield
    finally:
        try:
            signal.setitimer(signal.ITIMER_REAL,0)
        finally:
            signal.signal(signal.SIGALRM,previous)


def run_phase(directory,diagnostics,phase,action):
    require(phase in timing.PHASE_SECONDS,'known_display_observation_phase_required')
    budget=timing.PHASE_SECONDS[phase];started=time.monotonic()
    record={'phase':phase,'status':'started','budget_seconds':budget,'elapsed_seconds':0.0}
    try:
        with phase_deadline(budget):
            base.private_json(directory/(phase+'-started.json'),record)
            value=action()
            if time.monotonic()-started>=budget:
                raise PhaseDeadline('bounded_display_phase_expired')
    except BaseException as error:
        known=('TimeoutExpired','PhaseDeadline','Blocked','ValueError','OSError','PermissionError',
               'FileNotFoundError','FileExistsError','KeyboardInterrupt','JSONDecodeError')
        record={**record,'status':'failed','elapsed_seconds':max(0.0,time.monotonic()-started),
                'error_class':type(error).__name__ if type(error).__name__ in known else 'OtherError'}
        diagnostics.append(record)
        # Never expose raw argv, script text, stdout/stderr or exception payloads.
        try:base.private_json(directory/(phase+'-result.json'),record)
        except BaseException:pass
        raise
    record={**record,'status':'passed','elapsed_seconds':max(0.0,time.monotonic()-started)}
    diagnostics.append(record)
    base.private_json(directory/(phase+'-result.json'),record)
    return value


def snapshot(directory,diagnostics,phase):
    require(phase in ('before_preservation','after_preservation','before_supervision','after_supervision'),
            'fixed_read_only_snapshot_phase_required')
    preserved=phase.endswith('_preservation')
    expected=('python3','-c',preservation.SCRIPT) if preserved else ('python3','-c',supervision.SCRIPT,'identity')
    def observed(*argv):
        require(argv==expected,'fixed_read_only_snapshot_command_required')
        result=subprocess.run(argv,capture_output=True,text=True,timeout=timing.PHASE_SECONDS[phase],check=False)
        require(result.returncode==0 and len(result.stdout)<=8*1024*1024,'bounded_read_only_snapshot_failed')
        return result.stdout.strip()
    action=(lambda:preservation.fingerprint(observed)) if preserved else (lambda:supervision.identity(observed))
    return run_phase(directory,diagnostics,phase,action)


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

def keep_content(original,layout,owned=None):
    require(original.count(BEGIN)<=1 and original.count(END)==original.count(BEGIN)
            and (BEGIN not in original or original.endswith(END)), 'unambiguous_monitor_tail_required')
    prefix=original.split(BEGIN,1)[0]
    if prefix and not prefix.endswith(b'\n'): prefix+=b'\n'
    rules=[]
    for m in sorted(layout,key=lambda row:row['name']):
        editable=owned is not None and m['name']==p.owned_name(owned)
        require(type(m.get('disabled')) is bool,'native_display_enabled_state_required')
        if m['disabled']:
            require(editable,'original_display_must_remain_enabled')
            rules.append('hl.monitor({ output = '+json.dumps(m['name'])+', disabled = true })\n')
            continue
        require(m['transform']==0 or editable and m['transform'] in (1,2,3),
                'original_display_rotation_must_remain_unchanged')
        rules.append('hl.monitor({ output = '+json.dumps(m['name'])+', disabled = false, mode = "'
            +str(m['width'])+'x'+str(m['height'])+'@'+format(round(m['refreshRate']*1000)/1000,'.3f')
            +'", position = "'+str(m['x'])+'x'+str(m['y'])+'", scale = '+format(m['scale'],'.3f')
            +', transform = '+str(m['transform'])+' })\n')
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
        require([p.monitor_signature(row) for row in self.all_monitors()]==self.baseline,
                'no_preexisting_disabled_or_additional_output_required')
        self.config=Path.home()/'.config/hypr/monitors.lua'
        self.original=read(self.config);self.kept=None;self.armed=None
        self.keep_index=0;self.captured_keeps=0
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
        self.owned=None;self.allowed=[];self.peer_identity=None
        self.peer_global=None;self.peer_disabled=False;self.peer_retired=False;self.peer_ready=False
        self.peer_host=None;self.peer_surfaces=None;self.lifecycle=None;self.lifecycle_transaction=None
        self.disable_cursor=None
        self.panel_target='Virtual-1'

    def all_monitors(self):
        return json.loads(base.hypr('-j','monitors','all'))

    def peer_submitted(self,state,current):
        """Wait for actual per-output geometry; submission is not presentation."""
        width,height=d.display_logical_size(current)
        layers=json.loads(base.hypr('-j','layers'))
        try:
            host=p.host(state,self.lease.name)
            require(len(host['viewport'])==2
                    and all(math.isclose(a,b,abs_tol=.01) for a,b in zip(host['viewport'],(width,height)))
                    and math.isclose(host['density'],current['scale'],abs_tol=.001),
                    'owned_host_viewport_or_density_not_current')
            for role in ('background','bar'):
                surface=p.surface(state,self.lease.name,role)
                require(surface['native_frames']>0 and surface['frames']>0
                        and math.isclose(surface['density'],current['scale'],abs_tol=.001)
                        and math.isclose(surface['width'],width,abs_tol=1)
                        and (math.isclose(surface['height'],height,abs_tol=1) if role=='background'
                             else 0<surface['height']<height)
                        and (surface['buffer_width'],surface['buffer_height'])==
                            (math.ceil(surface['width']*surface['density']),math.ceil(surface['height']*surface['density'])),
                        'owned_native_surface_geometry_not_submitted')
                p.layer_for(layers,self.lease.name,surface,self.pid)
        except p.Blocked:
            return False
        return True

    def peer_state(self,state):
        current=d.display_layout(self.all_monitors(),self.baseline,self.peer_identity)
        require(current in self.allowed,'unadmitted_owned_display_layout')
        globals=[row for row in state['outputs'] if row['name']==self.lease.name]
        hosts=[row for row in state['client']['monitor_hosts'] if row['id']==self.lease.name]
        require(len(globals)<=1 and len(hosts)<=1,'ambiguous_owned_display_lifetime')
        transaction=state['client']['displays'].get('transaction') or {}
        requested=(state['client']['displays']['busy'] or
                   (transaction.get('id') is not None and transaction['id']!=self.lifecycle_transaction
                    and transaction.get('phase') in ('preparing','confirmation','kept')))
        if current['disabled']:
            require(self.peer_disabled or (self.lifecycle=='disable' and requested),
                    'owned_disable_without_admitted_native_transaction')
            self.peer_disabled=True
            require(not globals or globals[0]['global']==self.peer_global,
                    'unexpected_global_while_disabling_owned_display')
            self.peer_ready=(not globals and not hosts and
                p.retired(state,self.lease.name,self.peer_host['instance'],self.peer_surfaces))
            self.peer_retired|=self.peer_ready
        elif self.peer_disabled:
            require(self.lifecycle=='enable' and self.peer_retired and requested,
                    'owned_reenable_without_retired_peer_and_new_transaction')
            self.peer_ready=False
            if globals and hosts:
                require(globals[0]['global']!=self.peer_global
                        and hosts[0]['generation']>self.peer_host['generation'],
                        'reenabled_peer_requires_new_wayland_and_host_lifetimes')
                if not self.peer_submitted(state,current):
                    return current
                self.peer_global=globals[0]['global'];self.lease.global_id=self.peer_global
                self.peer_disabled=False;self.peer_retired=False;self.peer_ready=True;self.lifecycle=None
        else:
            require(len(globals)==1 and globals[0]['global']==self.peer_global,
                    'owned_wayland_lifetime_changed_outside_disable_cycle')
            self.peer_ready=self.peer_submitted(state,current)
        return current

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

    def resource_state(self):
        """Observe preserved resources without granting any cursor/input authority."""
        self.preserve()
        self.security_sources()
        state=base.inspect()
        require(state['frame_profiling'] is False and state['presentation_feedback']['enabled'] is False
                and all(row['source_frames'] is None for row in state['client']['live_wallpaper']['producers']),
                'normal_profiling_must_remain_off')
        require(all(p.host(state,'Virtual-1')[key]==value for key,value in self.original_host.items())
                and p.output(state,'Virtual-1')['global']==self.original_global,'original_host_lifetime_changed')
        return state

    def checked(self):
        state=self.resource_state()
        if self.owned is not None:
            self.peer_state(state)
        else: p.unchanged_layout(self.monitors(),self.baseline,self.lease.name,None)
        keyboards=[row for row in json.loads(base.hypr('-j','devices'))['keyboards'] if row.get('main')]
        require(len(keyboards)==1 and keyboards[0].get('layout')=='us'
                and keyboards[0].get('variant','')=='' and keyboards[0].get('capsLock') is False,'plain_us_keyboard_required')
        keyboard_bindings(json.loads(base.hypr('-j','binds')))
        return state

    def input_preflight(self):
        p.require_unbound_input(json.loads(base.hypr('-j','binds')))
        current=json.loads(base.hypr('-j','cursorpos'))
        declaration=self.disable_cursor
        if (declaration is not None and not self.cursor.lost
                and (current.get('x'),current.get('y'))!=self.cursor.expected):
            # No input is admitted while the native disable and SDK retirement
            # settle. The declared point alone never grants cursor authority.
            try:
                while True:
                    state=self.checked();self.panel_observation(state)
                    peer=self.peer_state(state)
                    # Source/panel/peer probes can take time. Only this fresh
                    # native sample may commit the declared cursor postimage.
                    current=json.loads(base.hypr('-j','cursorpos'))
                    if declaration.confirm(self.cursor,current,state,peer,time.monotonic()):
                        base.private_json(self.directory/'cursor-disable-confirmed.json',{
                            'point':list(declaration.destination),
                            'transaction':state['client']['displays']['transaction'],
                            'old_host_fully_retired':True})
                        self.disable_cursor=None
                        return state
                    time.sleep(.05)
                    current=json.loads(base.hypr('-j','cursorpos'))
            except BaseException:
                self.cursor.lost=True;self.disable_cursor=None
                raise
        self.cursor.check(current)
        return None

    def panel_observation(self,state):
        child=state['client']['displays']
        require(not any(state['client'].get(name,{}).get(key) for name in
            ('audio','media','network','bluetooth','power','tray','weather','live_wallpaper','settings','application_actions')
            for key in ('open','visible','busy')),'no_foreign_panel_required')
        panels=[row for row in state['surfaces'] if row.get('role',row['id'])=='displays']
        panel=panels[0] if len(panels)==1 else None
        layer=None
        if child['open']:
            require(panel is not None and panel['output']==self.panel_target and panel['pointer_input']=='all'
                    and panel['keyboard']=='exclusive' and panel['frames']>0 and panel['native_frames']>0,
                    'owned_submitted_display_panel_required')
            require([row['id'] for row in state['surfaces'] if row['keyboard']=='exclusive']==[panel['id']],
                    'one_exclusive_display_target_required')
            monitor=p.unique(self.monitors(),lambda row:row['name']==self.panel_target,'panel_native_output_missing')
            width,height=d.display_logical_size(monitor)
            require(math.isclose(panel['density'],monitor['scale'],abs_tol=.001)
                    and math.isclose(panel['width'],width,abs_tol=1)
                    and math.isclose(panel['height'],height,abs_tol=1)
                    and (panel['buffer_width'],panel['buffer_height'])==
                        (math.ceil(panel['width']*panel['density']),math.ceil(panel['height']*panel['density'])),
                    'panel_actual_viewport_density_and_buffer_required')
            layer=p.layer_for(json.loads(base.hypr('-j','layers')),self.panel_target,panel,self.pid)
        target=p.host(state,self.panel_target)
        return {'state':child,'panel':panel,'layer':layer,'monitors':self.monitors(),
                'host':{key:target[key] for key in ('id','instance','generation')},
                'global':p.output(state,self.panel_target)['global'],
                'active_monitor':state['client']['active_monitor'],'panel_target':self.panel_target,
                'cursor':json.loads(base.hypr('-j','cursorpos'))}

    def status(self):
        state=self.checked()
        settled=self.input_preflight()
        if settled is not None:state=settled
        value=self.panel_observation(state)
        value.update(peer=self.peer_state(state) if self.owned is not None else None,peer_ready=self.peer_ready)
        return value

    def cleanup_state(self):
        state=self.resource_state()
        neutral=copy.deepcopy(state['client']);neutral['displays']={}
        base.idle(neutral)
        if self.owned is None:
            require([p.monitor_signature(row) for row in self.all_monitors()]==self.baseline,
                    'only_original_native_output_expected')
        else:
            peer=d.display_layout(self.all_monitors(),self.baseline,self.peer_identity)
            require(peer in self.allowed,'cleanup_must_only_remove_admitted_owned_peer')
            if peer['disabled']:
                require(self.peer_disabled or self.lifecycle=='disable',
                        'cleanup_cannot_adopt_unadmitted_disabled_peer')
        return state

    def operation(self,request):
        operation=request['operation']
        if operation=='create':
            require(self.owned is None,'new_output_only')
            value=self.lease.create();self.owned=copy.deepcopy(self.lease.expected)
            self.peer_identity={key:self.owned[key] for key in ('id','name','description')}
            self.peer_global=self.lease.global_id;self.peer_ready=True
            self.allowed=[self.owned,{**self.owned,'x':self.owned['x']+16},{**self.owned,'scale':1.25},
                {**self.owned,'x':self.owned['x']+16,'transform':1},
                {**self.peer_identity,'disabled':True},
                {**self.owned,'x':self.owned['x']+16,'scale':1.25},
                {**self.owned,'x':self.owned['x']+32,'scale':1.25}]
            return {'owned':self.owned,'allowed':self.allowed}
        if operation=='capture-keep':
            state=base.inspect()['client']['displays']
            require(self.armed is not None and state['transaction']['phase']=='kept'
                    and self.keep_index>self.captured_keeps,'explicit_fresh_keep_ack_required')
            self.admit_file(True)
            base.private_json(self.directory/f'kept-{self.keep_index:02d}.json',{
                'sha256':hashlib.sha256(self.kept[0]).hexdigest(),'identity':self.kept[1],
                'transaction':state['transaction']})
            self.captured_keeps=self.keep_index
            return {'captured':True}
        value=self.status()
        if operation=='status': return value
        if operation in ('expect-disable','expect-enable'):
            require(self.owned is not None and value['state']['open'] and value['state']['dirty']
                    and not value['state']['busy'] and value['state']['selected']==self.lease.name
                    and self.panel_target=='Virtual-1' and value['peer_ready'],
                    'owned_peer_draft_on_original_panel_required')
            disabling=operation=='expect-disable'
            require(value['peer']['disabled'] is not disabling,'peer_enable_transition_must_change_state')
            if disabling:
                state=base.inspect();self.peer_host=copy.deepcopy(p.host(state,self.lease.name))
                self.peer_surfaces=p.host_surface_ids(state,self.lease.name,self.peer_host['instance'])
            else:require(self.peer_retired,'disabled_peer_host_must_be_fully_retired')
            self.lifecycle='disable' if disabling else 'enable'
            self.lifecycle_transaction=(value['state'].get('transaction') or {}).get('id')
            return {'admitted':self.lifecycle,'peer':value['peer']}
        if operation=='bar-warp':
            name=request['monitor']
            require(not value['state']['visible'] and name in ('Virtual-1',self.lease.name)
                    and (name=='Virtual-1' or self.peer_ready and not self.peer_disabled),
                    'closed_panel_and_enabled_owned_target_required')
            state=base.inspect();target=p.host(state,name);global_id=p.output(state,name)['global']
            require(target['generation']==request['generation'] and global_id==request['global'],
                    'exact_bar_host_lifetime_required')
            source=p.surface(state,name,'bar')
            layer=p.layer_for(json.loads(base.hypr('-j','layers')),name,source,self.pid)
            monitor=p.unique(self.monitors(),lambda row:row['name']==name,'bar_native_output_missing')
            require(monitor['transform']==0,'panel_input_on_unrotated_target_only')
            point=p.hit_center(monitor,source,layer,'bar-audio')
            self.cursor.begin(value['cursor'],point)
            require(base.hypr('dispatch',f'hl.dsp.cursor.move({{x={point[0]},y={point[1]}}})')=='ok','bar_warp_rejected')
            self.cursor.confirm(json.loads(base.hypr('-j','cursorpos')));self.panel_target=name
            return {'point':point,'surface':source['id'],'generation':target['generation'],'global':global_id}
        if operation=='arm-keep':
            require(value['state']['transaction']['phase']=='confirmation'
                    and value['peer'] in self.allowed and value['peer_ready']
                    and self.keep_index==self.captured_keeps,'owned_preview_required')
            self.armed=keep_content(self.original[0],self.all_monitors(),self.lease.name)
            self.keep_index+=1
            base.private_json(self.directory/f'keep-intent-{self.keep_index:02d}.json',{
                'expected_sha256':hashlib.sha256(self.armed).hexdigest(),'transaction':value['state']['transaction']})
            return {'armed':True}
        if operation=='warp':
            d.require_owned_display_control(request['identifier'],value['state']['selected'],self.lease.name)
            require(value['panel'] is not None and request['monitor']==self.panel_target
                    and request['generation']==value['host']['generation']
                    and request['global']==value['global'],'exact_panel_host_required')
            monitor=p.unique(value['monitors'],lambda row:row['name']==self.panel_target,'panel_native_output_missing')
            point=p.hit_center(monitor,value['panel'],value['layer'],request['identifier'])
            self.cursor.begin(value['cursor'],point)
            require(base.hypr('dispatch',f'hl.dsp.cursor.move({{x={point[0]},y={point[1]}}})')=='ok','panel_warp_rejected')
            self.cursor.confirm(json.loads(base.hypr('-j','cursorpos')))
            if request['identifier']=='display-apply' and self.lifecycle=='disable':
                require(self.disable_cursor is None,'one_bounded_disable_cursor_declaration_required')
                base.require_reviewed_hyprland(self.hyprland_version)
                self.disable_cursor=d.DisplayDisableCursor(self.cursor,self.baseline,self.peer_identity,
                    self.peer_host,self.peer_surfaces,self.original_host,self.original_global,
                    (value['state'].get('transaction') or {}).get('id'),time.monotonic())
                base.private_json(self.directory/'cursor-disable-intent.json',{
                    'preimage':list(self.disable_cursor.preimage),'postimage':list(self.disable_cursor.destination),
                    'deadline_monotonic':self.disable_cursor.deadline,'before_transaction':self.disable_cursor.transaction,
                    'hyprland_commit':base.REVIEWED_HYPRLAND_COMMIT,'source_sha256':d.DisplayDisableCursor.SOURCES})
            else:self.disable_cursor=None
            return {'point':point,'surface':value['panel']['id']}
        require(operation=='finish','known_display_test_operation_required')
        return {'finishing':True}

    def cleanup(self):
        # Owned-resource cleanup must not call status/input_preflight. Cursor
        # loss never grants new input, and cannot strand our config/output.
        state=self.cleanup_state();child=state['client']['displays']
        if child['open']:
            self.panel_observation(state)
            require(not (child['busy'] or (child.get('transaction') or {}).get('phase')
                         in ('preparing','confirmation')) or child['selected']==self.lease.name,
                    'cleanup_cannot_close_foreign_display_transaction')
            base.command(base.CLI,'displays','close')
        deadline=time.monotonic()+20
        while True:
            state=self.cleanup_state();child=state['client']['displays']
            phase=(child.get('transaction') or {}).get('phase')
            if not child['open'] and not child['visible'] and not child['busy'] and phase not in ('preparing','confirmation'):
                break
            require(time.monotonic()<deadline,'display_transaction_retirement_deadline');time.sleep(.1)
        exchange_restore(self.config,self.directory/'monitors.original',self.original,self.kept,self.directory)
        self.admit_file(False)
        state=self.cleanup_state()
        if self.owned is not None:
            # Retire only this owned native identity. A guardian rollback may
            # have renewed its SDK host; that grants removal, never test input.
            hosts=[row for row in state['client']['monitor_hosts'] if row['id']==self.lease.name]
            require(len(hosts)<=1,'ambiguous_cleanup_peer_host')
            captured=(hosts[0]['instance'],p.host_surface_ids(state,self.lease.name,hosts[0]['instance'])) if hosts else None
            base.Native.remove(self,self.lease.name)
            deadline=time.monotonic()+15
            while True:
                state=self.resource_state()
                rows=self.all_monitors()
                absent=not any(row['name']==self.lease.name for row in rows)
                if absent:
                    require([p.monitor_signature(row) for row in rows]==self.baseline,'foreign_layout_changed_during_cleanup')
                    retired=(p.retired(state,self.lease.name,*captured) if captured else
                        not any(row['name']==self.lease.name for row in state['outputs'])
                        and not any(row['id']==self.lease.name for row in state['client']['monitor_hosts'])
                        and not any(row.get('output')==self.lease.name for row in state['surfaces'])
                        and state.get('retiring_native_surfaces')==0)
                    if retired:break
                else:
                    require(d.display_layout(rows,self.baseline,self.peer_identity) in self.allowed,
                            'owned_identity_changed_during_cleanup')
                require(time.monotonic()<deadline,'owned_cleanup_retirement_deadline');time.sleep(.1)
            self.owned=None;self.lease.expected=None;self.lease.global_id=None
        self.preserve(full=True);self.security_sources(True);self.cleanup_state()
        cursor={'restored':False,'authority_retained':False,'at_original_position':False}
        try:
            cursor['authority_retained']=self.cursor.owns(json.loads(base.hypr('-j','cursorpos')))
            if cursor['authority_retained']:
                self.restore_focus();cursor['restored']=True
            else:cursor['reason']='cursor_authority_lost_no_input_or_restore'
        except BaseException as error:
            cursor['reason']=str(error) if isinstance(error,p.Blocked) else type(error).__name__
        final_cursor=json.loads(base.hypr('-j','cursorpos'))
        cursor['at_original_position']=final_cursor==self.pointer
        cursor['authority_retained']=self.cursor.owns(final_cursor)
        cursor['restored'] &= cursor['authority_retained'] and cursor['at_original_position']
        if not cursor['restored']:cursor.setdefault('reason','final_cursor_restoration_not_proven')
        self.cleanup_cursor=cursor
        current=self.monitors()
        workspace=(len(current)==1 and current[0]['activeWorkspace']['id']==self.workspace)
        self.cleanup_state()
        return {'original_monitor_bytes_inode':original_file(self.original,read(self.config)),
                'only_owned_output_removed':True,'original_workspace_preserved':workspace,
                'same_cohort_settings_media_and_profiling':True,
                'monitor_ctime_link_count_not_claimed_unchanged':True}

def serve(directory,header):
    owner=None;diagnostics=[];result={'success':False,'restoration':None,'phases':diagnostics}
    server=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    try:
        before={'preservation':snapshot(directory,diagnostics,'before_preservation'),
                'supervision':snapshot(directory,diagnostics,'before_supervision')}
        owner_argv=['/usr/bin/python3','-I',str(directory/'guest.py'),'serve',str(directory)]
        def authority():
            base.private_json(directory/'before-facts.json',before)
            value=units.capture_unit_origin(header['unit'],owner_argv)
            require(value['process']['pid']==os.getpid(),'same_owner_unit_lifetime_required')
            return value
        origin=run_phase(directory,diagnostics,'startup_authority',authority)
        owner=run_phase(directory,diagnostics,'owner_initialization',lambda:Owner(directory,header))
        def publish_ready():
            base.private_json(directory/'checkpoint.json',{'files':owner.files,'baseline':owner.baseline,
                'workspace':owner.workspace,'pointer':owner.pointer,'original_global':owner.original_global,
                'desktop_pid':owner.pid,'monitor_identity':owner.original[1],'owner_origin':origin})
            server.bind(str(directory/'control.sock'));os.chmod(directory/'control.sock',0o600)
            server.listen(1);server.settimeout(.5)
            base.private_json(directory/'ready.json',{'ready':True,'desktop_pid':owner.pid})
        run_phase(directory,diagnostics,'publish_ready',publish_ready)
        deadline=time.monotonic()+timing.OWNER_LEASE_SECONDS;finished=False
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
                def resource_cleanup():
                    restored=owner.cleanup()
                    owner.security_sources(True)
                    return restored
                result['restoration']=run_phase(directory,diagnostics,'resource_cleanup',resource_cleanup)
                result['cursor_restoration']=owner.cleanup_cursor
                # Retain the independent final inventory and supervision checks.
                after={'preservation':snapshot(directory,diagnostics,'after_preservation'),
                       'supervision':snapshot(directory,diagnostics,'after_supervision')}
                def final_validation():
                    base.private_json(directory/'after-facts.json',after)
                    require(before==after,'final_supervision_settings_or_preservation_changed')
                    result['restoration']['preservation_supervision_security_sources_and_profiles']=True
                    result['resource_cleanup_completed']=True
                    if not owner.cleanup_cursor['restored']:
                        result['success']=False
                        result['cleanup_blocked']=owner.cleanup_cursor.get('reason','cursor_restoration_not_proven')
                run_phase(directory,diagnostics,'finish_validation',final_validation)
            except BaseException as error:
                result['success']=False
                result['cleanup_blocked']=str(error) if isinstance(error,p.Blocked) else type(error).__name__
        base.private_json(directory/'finished.json',result)

def main():
    require(__debug__,'python_assertions_required')
    parser=argparse.ArgumentParser();parser.add_argument('mode',choices=('serve','request'))
    parser.add_argument('directory',type=Path);parser.add_argument('operation',nargs='?')
    parser.add_argument('--identifier');parser.add_argument('--generation',type=int);parser.add_argument('--global-id',type=int)
    parser.add_argument('--monitor',default='Virtual-1')
    args=parser.parse_args();directory=args.directory;meta=directory.lstat()
    require(directory.is_absolute() and directory.resolve(strict=True)==directory
            and directory.name.startswith('native-two-output-displays-') and stat.S_ISDIR(meta.st_mode)
            and stat.S_IMODE(meta.st_mode)==0o700 and meta.st_uid==os.getuid(),'private_test_directory_required')
    if args.mode=='request':
        base.request(directory,args.operation,None,args.monitor,args.identifier,args.generation,args.global_id)
    else:
        signal.signal(signal.SIGTERM,lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
        header=json.loads((directory/'header.json').read_bytes())
        require(set(header['source_sha256'])=={'guest.py','multi_output_guard.py','multi_output_policy.py',
                'multi_output_display_policy.py','multi_output_display_timing.py','preservation_fingerprint.py','display_supervision.py',
                'owned_application_units.py'},'complete_display_staged_source_set_required')
        for name,expected in header['source_sha256'].items():
            value=directory/name;meta=value.lstat()
            require(value.name==name and not value.is_symlink() and stat.S_ISREG(meta.st_mode)
                    and meta.st_uid==os.getuid() and stat.S_IMODE(meta.st_mode)==0o600
                    and hashlib.sha256(value.read_bytes()).hexdigest()==expected,'staged_source_changed')
        serve(directory,header)

if __name__=='__main__': main()
