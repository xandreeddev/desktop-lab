//! Generic event-driven Wayland runtime. Desktop policy lives entirely in clients.
use calloop::{
    EventLoop, LoopHandle, channel,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use lucent_api::{self as api, Application, Effects, FrameDemand, ViewContext};
use lucent_render::{Error, Gpu, RenderOutcome, Renderer};
use lucent_ui::{Interaction, Layout, Scene};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region, SurfaceData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    time::Instant,
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};
use wgpu_handle::HasDisplayHandle;
// Use the exact raw-window-handle version used by the renderer.
use lucent_render::display_handle as wgpu_handle;
mod clipboard;
mod delivery;
mod effects;
mod ipc;
mod outputs;
mod profiling;
mod subscriptions;

enum RuntimeEvent<M> {
    Message(M),
    Subscription(subscriptions::Event<M>),
    Failure(api::RuntimeFailure),
    Command(String, mpsc::SyncSender<String>),
}
struct Native<M> {
    renderer: Renderer, // Drop before layer.
    role: SurfaceRole,
    spec: api::SurfaceSpec,
    bound_output: Option<wl_output::WlOutput>,
    width: u32,
    height: u32,
    scale: u32,
    configured: bool,
    demand: FrameDemand,
    scene: Scene<M>,
    interaction: Interaction<M>,
    regions: Vec<api::Clip>,
    frame_times: std::collections::VecDeque<f64>,
    profiles: profiling::History,
}
enum SurfaceRole {
    Layer(LayerSurface),
    Lock(SessionLockSurface, wl_output::WlOutput),
}
impl SurfaceRole {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Self::Layer(s) => s.wl_surface(),
            Self::Lock(s, _) => s.wl_surface(),
        }
    }
}
struct State<A: Application> {
    surfaces: BTreeMap<api::SurfaceAddress, Native<A::Message>>,
    gpu: Rc<Gpu>,
    app: A,
    layout: Layout,
    compositor: CompositorState,
    layer_shell: LayerShell,
    session_lock: Option<SessionLock>,
    authorize_unlock: Option<fn(&A) -> bool>,
    locked: bool,
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    output_bindings: Vec<(api::DisplayOutput, wl_output::WlOutput)>,
    closed_targets: BTreeMap<api::SurfaceAddress, (api::OutputName, wl_output::WlOutput)>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_seat: Option<wl_seat::WlSeat>,
    pointer_seat: Option<wl_seat::WlSeat>,
    keyboard_surface: Option<api::SurfaceAddress>,
    input_serial: Option<u32>,
    clipboard: clipboard::Clipboard<A::Message>,
    modifiers: Modifiers,
    epoch: Instant,
    profile_frames: bool,
    exit: bool,
    error: Option<String>,
    handle: LoopHandle<'static, Self>,
    sender: delivery::Delivery<RuntimeEvent<A::Message>>,
    tasks: effects::Executor<A::Message>,
    subscriptions: subscriptions::Workers,
    connection: Connection,
}
impl<A: Application> Drop for State<A> {
    fn drop(&mut self) {
        self.clipboard_drop();
        // Drop GPU surfaces before their protocol roles, then send the queued
        // destroy requests while the connection is still alive. A caller such
        // as a permission agent can outlive this individual runtime invocation.
        // Teardown never authorizes a secure-session unlock.
        self.surfaces.clear();
        if let Err(error) = self.connection.flush() {
            eprintln!("{} surface cleanup failed: {error}", self.app.name());
        }
    }
}
/// Run any framework application, with no desktop-specific types in the backend.
pub fn run<A: Application>(app: A) -> Result<(), Error> {
    run_inner(app, None, true)
}
/// Run ordinary Wayland application surfaces without local debug/command IPC.
/// Authentication dialogs use this entry point; secure session locking uses run_locked.
pub fn run_without_ipc<A: Application>(app: A) -> Result<(), Error> {
    run_inner(app, None, false)
}
/// Authentication must finish before authorizing unlock. Ordinary exit never unlocks.
pub trait LockApplication: Application {
    fn authenticated(&self) -> bool;
}
/// Uses ext-session-lock for every output and exposes no debug or command socket.
pub fn run_locked<A: LockApplication>(app: A) -> Result<(), Error> {
    run_inner(app, Some(A::authenticated), false)
}
/// Login greeters also disable the debugging/command socket.
pub fn run_greeter<A: Application>(app: A) -> Result<(), Error> {
    run_without_ipc(app)
}
fn run_inner<A: Application>(
    app: A,
    authorize_unlock: Option<fn(&A) -> bool>,
    ipc: bool,
) -> Result<(), Error> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let mut fonts = Vec::new();
    for bytes in app.fonts() {
        fonts.push(
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(std::io::Error::other)?,
        );
    }
    if fonts.is_empty() {
        let matched = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "sans-serif"])
            .output()?;
        if !matched.status.success() {
            return Err("Fontconfig could not select a font".into());
        }
        fonts.push(
            fontdue::Font::from_bytes(
                fs::read(String::from_utf8(matched.stdout)?.trim())?,
                fontdue::FontSettings::default(),
            )
            .map_err(std::io::Error::other)?,
        );
    }
    let fonts = Arc::new(fonts);
    let gpu = Gpu::new(conn.backend(), fonts.clone())?;
    let mut event_loop: EventLoop<State<A>> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    let (sender, events, _delivery_lifetime) = delivery::Delivery::channel();
    let completion = sender.clone();
    let tasks = effects::Executor::new(move |result| {
        let event = match result {
            effects::Completion::Message(message) => RuntimeEvent::Message(message),
            effects::Completion::Failure(error) => RuntimeEvent::Failure(error),
        };
        let _ = completion.send(event);
    });
    let clipboard_manager = if app.clipboard_enabled() && authorize_unlock.is_none() {
        smithay_client_toolkit::data_device_manager::DataDeviceManagerState::bind(&globals, &qh)
            .ok()
    } else {
        None
    };
    let mut state = State {
        surfaces: BTreeMap::new(),
        gpu,
        app,
        layout: Layout::new(fonts),
        compositor,
        layer_shell,
        session_lock: None,
        authorize_unlock,
        locked: false,
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        output_bindings: vec![],
        closed_targets: BTreeMap::new(),
        pointer: None,
        keyboard: None,
        keyboard_seat: None,
        pointer_seat: None,
        keyboard_surface: None,
        input_serial: None,
        clipboard: clipboard::Clipboard::new(clipboard_manager),
        modifiers: Modifiers::default(),
        epoch: Instant::now(),
        profile_frames: std::env::var_os("LUCENT_PROFILE_FRAMES").is_some_and(|value| value == "1"),
        exit: false,
        error: None,
        handle: handle.clone(),
        sender: sender.clone(),
        tasks,
        subscriptions: subscriptions::Workers::default(),
        connection: conn.clone(),
    };
    let runtime =
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is required")?);
    if authorize_unlock.is_some() {
        state.session_lock = Some(SessionLockState::new(&globals, &qh).lock(&qh)?);
        state.create_lock_outputs(&qh)?;
    }
    let socket_path = runtime.join(format!("{}.sock", state.app.name()));
    let lock = File::create(runtime.join(format!("{}.lock", state.app.name())))?;
    lock.try_lock()
        .map_err(|_| "This framework client is already running")?;
    let _ipc = if ipc {
        if socket_path.exists() {
            fs::remove_file(&socket_path)?;
        }
        let delivery = sender.clone();
        Some(ipc::Server::bind(&socket_path, move |command, cancel| {
            delivery
                .scoped(cancel)
                .send(RuntimeEvent::Command(command.text, command.reply))
        })?)
    } else {
        None
    };
    let event_qh = qh.clone();
    handle.insert_source(events, move |event, _, state| {
        if let channel::Event::Msg(event) = event {
            match event {
                RuntimeEvent::Message(msg) => state.message(msg, &event_qh),
                RuntimeEvent::Subscription(event) => match event.into_live() {
                    Some(Ok(message)) => state.message(message, &event_qh),
                    Some(Err(error)) => state.runtime_failure(error, &event_qh),
                    None => {}
                },
                RuntimeEvent::Failure(error) => state.runtime_failure(error, &event_qh),
                RuntimeEvent::Command(command, reply) => {
                    let response = if command == "inspect" {
                        state.inspect()
                    } else {
                        match state.app.command(&command) {
                            Ok(Some(msg)) => {
                                state.message(msg, &event_qh);
                                "ok".into()
                            }
                            Ok(None) => "ok".into(),
                            Err(e) => format!("error: {e}"),
                        }
                    };
                    let _ = reply.send(response);
                }
            }
        }
    })?;
    WaylandSource::new(conn.clone(), queue).insert(handle)?;
    let mut effects = Effects::default();
    state.app.init(&mut effects);
    state.effects(effects, &qh);
    state.reconcile(&qh)?;
    while !state.exit {
        event_loop.dispatch(None, &mut state)?;
        state.draw_all(&qh);
    }
    eprintln!("{} stopped cleanly", state.app.name());
    if let Some(error) = state.error.take() {
        return Err(error.into());
    }
    Ok(())
}
impl<A: Application> State<A> {
    fn create_lock_outputs(&mut self, qh: &QueueHandle<Self>) -> Result<(), Error> {
        let Some(lock) = &self.session_lock else {
            return Ok(());
        };
        for output in self.outputs.outputs() {
            if self
                .surfaces
                .values()
                .any(|s| matches!(&s.role,SurfaceRole::Lock(_,o) if *o==output))
            {
                continue;
            }
            let surface = self.compositor.create_surface(qh);
            let lock_surface = lock.create_lock_surface(surface, &output, qh);
            // SAFETY: renderer drops before its surface; State owns the live connection.
            let renderer = unsafe {
                Renderer::new(
                    self.gpu.clone(),
                    self.connection.backend().display_handle()?.as_raw(),
                    lock_surface.wl_surface().id().as_ptr().cast(),
                )
            }?;
            let spec = api::SurfaceSpec {
                id: "lock",
                instance: None,
                output: None,
                layer: api::Layer::Overlay,
                anchor: api::Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: api::Keyboard::Exclusive,
                visible: true,
                pointer_input: lucent_api::PointerInput::All,
            };
            self.surfaces.insert(
                api::SurfaceAddress::new("lock").instance(api::SurfaceInstance::new(format!(
                    "global-{}",
                    output.id().protocol_id()
                ))?),
                Native {
                    renderer,
                    role: SurfaceRole::Lock(lock_surface, output),
                    spec,
                    bound_output: None,
                    width: 1,
                    height: 1,
                    scale: 1,
                    configured: false,
                    demand: FrameDemand::default(),
                    scene: Scene::default(),
                    interaction: Interaction::default(),
                    regions: vec![],
                    frame_times: Default::default(),
                    profiles: Default::default(),
                },
            );
        }
        Ok(())
    }
    fn inspect(&self) -> String {
        let cache = self.gpu.cache_stats();
        let mut scene_images = BTreeMap::new();
        if self.profile_frames {
            for surface in self.surfaces.values() {
                for primitive in &surface.scene.paint {
                    if let lucent_ui::Paint::Image { data, .. } = primitive {
                        scene_images.insert(data.cache_id(), data.bytes() as u64);
                    }
                }
            }
        }
        let profile_memory = self.profile_frames.then(|| serde_json::json!({
            "scene_image_payload_bytes": scene_images.values().sum::<u64>(),
            "geometry_buffer_payload_bytes": self.surfaces.values().map(|s| s.renderer.buffer_payload_bytes()).sum::<u64>(),
            "one_color_buffer_per_surface_bytes": self.surfaces.values().map(|s| s.renderer.single_surface_buffer_bytes()).sum::<u64>(),
            "scope":"Prepared image payloads referenced by current scenes only; app caches excluded. GPU buffer values are requested payloads, not driver allocations. Actual surface buffer counts and total GPU memory are unknown."
        }));
        let mut result = serde_json::json!({"profile_memory":profile_memory,"frame_profiling":self.profile_frames,"profile_scope":"Successful submissions only; CPU-side wall time includes driver waits. prepare_breakdown_ms is nested within prepare. No GPU execution or scanout timestamps.","gpu_cache":{"entries":cache.entries,"payload_bytes":cache.bytes,"active_payload_bytes":cache.active_bytes,"reuse_budget_bytes":cache.budget_bytes,"hits":cache.hits,"misses":cache.misses,"evictions":cache.evictions},"uptime_ms":self.epoch.elapsed().as_secs_f64()*1000.,"adapter":self.gpu.adapter_description,"surfaces":self.surfaces.iter().map(|(id,s)|serde_json::json!({"id":id.to_string(),"role":s.spec.id,"instance":s.spec.instance.as_ref().map(api::SurfaceInstance::as_str),"redraw_group":s.spec.redraw_group,"output":s.spec.output.as_ref().map(api::OutputName::as_str),"pointer_input":s.spec.pointer_input.name(),"width":s.width,"height":s.height,"scale":s.scale,"buffer_width":s.width*s.scale,"buffer_height":s.height*s.scale,"frames":s.renderer.frames,"paint_primitives":s.scene.paint.len(),"focus":s.interaction.focus,"focus_visible":s.interaction.focus_visible,"frame_times_ms":s.frame_times,"frame_profiles":s.profiles.json(),"hits":s.scene.hits.iter().map(|h|serde_json::json!({"id":h.id,"x":h.rect.x,"y":h.rect.y,"width":h.rect.w,"height":h.rect.h,"clip":{"x":h.clip.bounds.x,"y":h.clip.bounds.y,"width":h.clip.bounds.w,"height":h.clip.bounds.h}})).collect::<Vec<_>>() })).collect::<Vec<_>>(),"client":serde_json::from_str::<serde_json::Value>(&self.app.inspect()).unwrap_or(serde_json::Value::Null)});
        result["outputs"]=serde_json::json!(self.output_bindings.iter().map(|(info,_)|serde_json::json!({"global":info.global,"name":info.name.as_ref().map(api::OutputName::as_str),"logical_size":info.logical_size,"integer_scale":info.integer_scale})).collect::<Vec<_>>());
        result["closed_named_surfaces"] = serde_json::json!(
            self.closed_targets
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        result.to_string()
    }
    fn message(&mut self, message: A::Message, qh: &QueueHandle<Self>) {
        self.messages([message], qh);
    }
    fn messages(&mut self, messages: impl IntoIterator<Item = A::Message>, qh: &QueueHandle<Self>) {
        let mut effects = Effects {
            now: self.epoch.elapsed().as_secs_f64(),
            ..Default::default()
        };
        for message in messages {
            self.app.update(message, &mut effects);
        }
        self.effects(effects, qh);
    }
    fn runtime_failure(&mut self, failure: api::RuntimeFailure, qh: &QueueHandle<Self>) {
        eprintln!("{} runtime failure: {failure}", self.app.name());
        if let Some(message) = self.app.runtime_failure(failure.clone()) {
            self.message(message, qh);
        } else {
            self.error = Some(failure.to_string());
            self.exit = true;
        }
    }
    fn outputs_changed(&mut self, qh: &QueueHandle<Self>) {
        let mut bindings: Vec<_> = self
            .outputs
            .outputs()
            .filter_map(|output| {
                let info = self.outputs.info(&output)?;
                let logical_size = info.logical_size.and_then(|(width, height)| {
                    let width = u32::try_from(width).ok()?;
                    let height = u32::try_from(height).ok()?;
                    (width > 0 && height > 0).then_some((width, height))
                });
                Some((
                    api::DisplayOutput {
                        global: info.id,
                        name: info.name.and_then(|name| api::OutputName::new(name).ok()),
                        logical_size,
                        integer_scale: info.scale_factor.max(1) as u32,
                    },
                    output,
                ))
            })
            .collect();
        bindings.sort_by_key(|(info, _)| info.global);
        if self.output_bindings == bindings {
            return;
        }
        self.output_bindings = bindings;
        let event = api::Event::OutputsChanged(
            self.output_bindings
                .iter()
                .map(|(info, _)| info.clone())
                .collect(),
        );
        if let Some(message) = self.app.event(event) {
            self.message(message, qh);
        }
        if !self.exit
            && let Err(error) = self.reconcile(qh)
        {
            self.error = Some(error.to_string());
            self.exit = true;
        }
    }
    fn effects(&mut self, effects: Effects<A::Message>, qh: &QueueHandle<Self>) {
        if self.authorize_unlock.is_none() {
            self.exit |= effects.exit;
        } else if self.locked
            && self
                .authorize_unlock
                .is_some_and(|authorize| authorize(&self.app))
        {
            self.session_lock.as_ref().unwrap().unlock();
            if let Err(error) = self.connection.roundtrip() {
                self.error = Some(error.to_string());
            }
            self.exit = true;
            return;
        }
        for task in effects.tasks {
            if let Err(rejected) = self.tasks.submit(task) {
                match rejected.complete() {
                    Some(effects::Completion::Message(message)) => self.message(message, qh),
                    Some(effects::Completion::Failure(failure)) => {
                        self.runtime_failure(failure, qh)
                    }
                    None => {}
                }
            }
        }
        for request in effects.clipboard {
            self.clipboard_request(request, qh);
        }
        if let Err(e) = self.reconcile(qh) {
            self.error = Some(e.to_string());
            self.exit = true;
        }
        for id in effects.redraw {
            for s in self
                .surfaces
                .values_mut()
                .filter(|s| s.spec.matches_redraw(id))
            {
                s.demand.invalidate();
            }
        }
        for address in effects.redraw_instances {
            for native in self
                .surfaces
                .values_mut()
                .filter(|native| native.spec.address() == address)
            {
                native.demand.invalidate();
            }
        }
        self.draw_all(qh);
    }
    fn reconcile(&mut self, qh: &QueueHandle<Self>) -> Result<(), Error> {
        if self.session_lock.is_none() {
            let specs = self.app.surfaces();
            let mut ids = std::collections::BTreeSet::new();
            for spec in &specs {
                spec.validate()?;
                if !ids.insert(spec.address()) {
                    return Err(format!("Duplicate native surface ID: {}", spec.id).into());
                }
            }
            let removed: Vec<_> = self
                .surfaces
                .keys()
                .filter(|id| {
                    !specs
                        .iter()
                        .any(|spec| spec.address() == **id && spec.visible)
                })
                .cloned()
                .collect();
            for id in removed {
                self.retire_native(&id);
            }
            self.closed_targets.retain(|id, (name, output)| {
                specs.iter().any(|spec| {
                    spec.address() == *id && spec.visible && spec.output.as_ref() == Some(name)
                }) && self
                    .output_bindings
                    .iter()
                    .any(|(info, current)| info.name.as_ref() == Some(name) && current == output)
            });
            for spec in specs.into_iter().filter(|s| s.visible) {
                let address = spec.address();
                let target = match outputs::resolve(
                    spec.output.as_ref(),
                    &self.output_bindings,
                    self.closed_targets.get(&address),
                ) {
                    outputs::Target::Compositor => None,
                    outputs::Target::Waiting => {
                        self.retire_native(&address);
                        continue;
                    }
                    outputs::Target::Named(output) => Some(output),
                };
                if self
                    .surfaces
                    .get(&address)
                    .is_some_and(|native| native.bound_output != target)
                {
                    self.retire_native(&address);
                }
                if let Some(native) = self.surfaces.get_mut(&address) {
                    if native.spec != spec {
                        if let SurfaceRole::Layer(layer) = &native.role {
                            configure_layer(layer, &spec);
                            layer.commit();
                        }
                        native.spec = spec;
                        native.demand.invalidate();
                    }
                    continue;
                }
                let surface = self.compositor.create_surface(qh);
                let layer = self.layer_shell.create_layer_surface(
                    qh,
                    surface,
                    layer_kind(spec.layer),
                    Some(format!("{}-{}", self.app.name(), address)),
                    target.as_ref(),
                );
                configure_layer(&layer, &spec);
                let region = Region::new(&self.compositor)?;
                layer
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
                layer.commit();
                // SAFETY: Native drops its renderer before its layer; State keeps the connection alive.
                let renderer = unsafe {
                    Renderer::new(
                        self.gpu.clone(),
                        self.connection.backend().display_handle()?.as_raw(),
                        layer.wl_surface().id().as_ptr().cast(),
                    )
                }?;
                self.surfaces.insert(
                    address,
                    Native {
                        renderer,
                        role: SurfaceRole::Layer(layer),
                        spec,
                        bound_output: target,
                        width: 1,
                        height: 1,
                        scale: 1,
                        configured: false,
                        demand: FrameDemand::default(),
                        scene: Scene::default(),
                        interaction: Interaction::default(),
                        regions: vec![],
                        frame_times: std::collections::VecDeque::new(),
                        profiles: Default::default(),
                    },
                );
            }
        }
        self.subscriptions
            .reconcile(self.app.subscriptions(), &self.sender)
            .map_err(|failure| failure.to_string())?;
        Ok(())
    }
    fn retire_native(&mut self, id: &api::SurfaceAddress) -> Option<Native<A::Message>> {
        if self.keyboard_surface.as_ref() == Some(id) {
            self.keyboard_surface = None;
            self.input_serial = None;
        }
        self.surfaces.remove(id)
    }
    /// Some compositors update wl_output scale without repeating the preferred
    /// buffer-scale event on every existing layer surface. Track entered outputs
    /// as well, so an idle bar/dock does not retain a low-density buffer.
    fn refresh_output_scales(&mut self, qh: &QueueHandle<Self>) {
        let mut events = Vec::new();
        for surface in self.surfaces.values_mut() {
            let Some(data) = surface.role.wl_surface().data::<SurfaceData<()>>() else {
                continue;
            };
            let scale = data
                .outputs()
                .filter_map(|output| self.outputs.info(&output))
                .map(|info| info.scale_factor.max(1) as u32)
                .max();
            if let Some(scale) = scale
                && surface.scale != scale
            {
                surface.scale = scale;
                surface.demand.invalidate();
                if surface.configured {
                    events.push(surface.spec.scope_event(api::Event::Resize {
                        surface: surface.spec.id,
                        width: surface.width as f32,
                        height: surface.height as f32,
                        scale,
                    }));
                }
            }
        }
        for event in events {
            if let Some(message) = self.app.event(event) {
                self.message(message, qh);
            }
        }
        self.draw_all(qh);
    }
    fn draw_all(&mut self, qh: &QueueHandle<Self>) {
        let now = self.epoch.elapsed().as_secs_f64();
        let mut submitted = Vec::new();
        for s in self.surfaces.values_mut() {
            if !s.configured || !s.demand.begin() {
                continue;
            }
            let total = self.profile_frames.then(Instant::now);
            let started = self.profile_frames.then(Instant::now);
            let tree = self.app.view(&ViewContext {
                surface: s.spec.id,
                instance: s.spec.instance.clone(),
                width: s.width as f32,
                height: s.height as f32,
                now,
            });
            let view_ms = profiling::elapsed(started);
            let started = self.profile_frames.then(Instant::now);
            let scene =
                self.layout
                    .build(&tree, s.width as f32, s.height as f32, &s.interaction, now);
            let layout_ms = profiling::elapsed(started);
            let started = self.profile_frames.then(Instant::now);
            let focus = s.interaction.focus.clone();
            s.interaction.synchronize(&scene);
            if focus != s.interaction.focus {
                s.demand.invalidate();
            }
            let regions = pointer_regions(s.spec.pointer_input, s.width, s.height, &scene);
            if regions != s.regions {
                match Region::new(&self.compositor) {
                    Ok(region) => {
                        for clip in &regions {
                            add_region(&region, clip);
                        }
                        s.role
                            .wl_surface()
                            .set_input_region(Some(region.wl_region()));
                        s.regions = regions;
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.exit = true;
                    }
                }
            }
            let surface = s.role.wl_surface();
            surface.set_buffer_scale(s.scale as i32);
            let input_ms = profiling::elapsed(started);
            s.renderer.enable_profiling(self.profile_frames);
            match s
                .renderer
                .render(s.width, s.height, s.scale, &scene.paint, || {
                    surface.frame(qh, FrameCallbackData(surface.clone()));
                }) {
                Ok(RenderOutcome::Presented) => {}
                Ok(RenderOutcome::RetryAfter(delay)) => {
                    s.demand.defer();
                    let surface = surface.clone();
                    // Identify the native surface, not its reusable logical name.
                    // A removed/recreated panel must not inherit an old retry timer.
                    if let Err(error) = self.handle.insert_source(
                        Timer::from_duration(delay),
                        move |_, _, state| {
                            if let Some(id) = state.id_for(&surface)
                                && let Some(native) = state.surfaces.get_mut(&id)
                            {
                                native.demand.retry_ready();
                            }
                            TimeoutAction::Drop
                        },
                    ) {
                        self.error = Some(error.to_string());
                        self.exit = true;
                    }
                    continue;
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.exit = true;
                    continue;
                }
            }
            let started = self.profile_frames.then(Instant::now);
            if self.app.animating_instance(&s.spec.address(), now) || s.interaction.animating(now) {
                s.demand.invalidate();
            }
            let submitted_ms = self.epoch.elapsed().as_secs_f64() * 1000.;
            s.frame_times.push_back(submitted_ms);
            if s.frame_times.len() > profiling::MAX_FRAMES {
                s.frame_times.pop_front();
            }
            s.scene = scene;
            submitted.push(s.spec.scope_event(api::Event::FrameSubmitted {
                surface: s.spec.id,
                width: s.width as f32,
                height: s.height as f32,
                now,
            }));
            if let Some(render) = s.renderer.last_profile {
                s.profiles.push(profiling::FrameProfile {
                    frame: s.renderer.frames,
                    submitted_ms,
                    view_ms,
                    layout_ms,
                    input_ms,
                    bookkeeping_ms: profiling::elapsed(started),
                    total_ms: profiling::elapsed(total),
                    render,
                });
            }
        }
        self.clipboard_observe_focus(qh);
        let messages: Vec<_> = submitted
            .into_iter()
            .filter_map(|event| self.app.event(event))
            .collect();
        if !messages.is_empty() {
            self.messages(messages, qh);
        }
    }
    fn surface_event(&self, id: &api::SurfaceAddress, event: api::Event) -> Option<A::Message> {
        let native = self.surfaces.get(id)?;
        self.app.event(native.spec.scope_event(event))
    }
    fn id_for(&self, surface: &wl_surface::WlSurface) -> Option<api::SurfaceAddress> {
        self.surfaces
            .iter()
            .find(|(_, s)| s.role.wl_surface() == surface)
            .map(|(id, _)| id.clone())
    }
    fn key(&mut self, event: KeyEvent, qh: &QueueHandle<Self>) {
        let Some(id) = self.keyboard_surface.clone() else {
            return;
        };
        let shortcut = if self.modifiers.ctrl || self.modifiers.alt {
            event
                .keysym
                .key_char()
                .filter(|c| !c.is_control())
                .map(|character| api::Key::Shortcut {
                    character,
                    modifiers: api::KeyModifiers {
                        control: self.modifiers.ctrl,
                        alt: self.modifiers.alt,
                        shift: self.modifiers.shift,
                        logo: self.modifiers.logo,
                    },
                })
        } else {
            None
        };
        let key = match event.keysym {
            Keysym::Escape => api::Key::Escape,
            Keysym::Return | Keysym::KP_Enter => api::Key::Enter,
            Keysym::BackSpace => api::Key::Backspace,
            Keysym::Delete | Keysym::KP_Delete => api::Key::Delete,
            Keysym::Up => api::Key::Up,
            Keysym::Down => api::Key::Down,
            Keysym::Left => api::Key::Left,
            Keysym::Right => api::Key::Right,
            Keysym::ISO_Left_Tab => api::Key::BackTab,
            Keysym::Tab if self.modifiers.shift => api::Key::BackTab,
            Keysym::Tab => api::Key::Tab,
            Keysym::Page_Up => api::Key::PageUp,
            Keysym::Page_Down => api::Key::PageDown,
            Keysym::Home => api::Key::Home,
            Keysym::End => api::Key::End,
            Keysym::a if self.modifiers.ctrl => api::Key::SelectAll,
            Keysym::u if self.modifiers.ctrl => api::Key::ClearInput,
            _ if shortcut.is_some() => shortcut.unwrap(),
            _ => {
                let Some(text) = event
                    .utf8
                    .filter(|s| !s.is_empty() && s.chars().all(|c| !c.is_control()))
                else {
                    return;
                };
                if let Some(shortcut) = shortcut {
                    shortcut
                } else {
                    api::Key::Text(text)
                }
            }
        };
        let key = if matches!(
            key,
            api::Key::Left
                | api::Key::Right
                | api::Key::Home
                | api::Key::End
                | api::Key::Delete
                | api::Key::Backspace
        ) && (self.modifiers.ctrl
            || self.modifiers.shift
            || self.modifiers.alt
            || self.modifiers.logo)
        {
            api::Key::Modified {
                key: Box::new(key),
                modifiers: api::KeyModifiers {
                    control: self.modifiers.ctrl,
                    shift: self.modifiers.shift,
                    alt: self.modifiers.alt,
                    logo: self.modifiers.logo,
                },
            }
        } else {
            key
        };
        if let Some(s) = self.surfaces.get_mut(&id) {
            if !s.interaction.focus_visible {
                s.demand.invalidate();
            }
            s.interaction.focus_visible = true;
        }
        let Some(logical_id) = self.surfaces.get(&id).map(|s| s.spec.id) else {
            self.keyboard_surface = None;
            return;
        };
        if let Some(message) = self.surface_event(
            &id,
            api::Event::Key {
                surface: logical_id,
                key: key.clone(),
            },
        ) {
            self.message(message, qh);
            return;
        }
        if let Some(s) = self.surfaces.get_mut(&id) {
            let before = s.interaction.focus.clone();
            let messages = s.interaction.key(&s.scene, &key);
            if before != s.interaction.focus {
                s.demand.invalidate();
            }
            self.clipboard_observe_focus(qh);
            if messages.is_empty() {
                self.draw_all(qh);
            } else {
                self.messages(messages, qh);
            }
        }
    }
}
fn layer_kind(layer: api::Layer) -> Layer {
    match layer {
        api::Layer::Background => Layer::Background,
        api::Layer::Bottom => Layer::Bottom,
        api::Layer::Top => Layer::Top,
        api::Layer::Overlay => Layer::Overlay,
    }
}
fn pointer_regions<M>(
    policy: api::PointerInput,
    width: u32,
    height: u32,
    scene: &Scene<M>,
) -> Vec<api::Clip> {
    match policy {
        api::PointerInput::None => vec![],
        api::PointerInput::Content => scene.regions.clone(),
        api::PointerInput::All => vec![api::Rect::new(0., 0., width as f32, height as f32).into()],
    }
}

#[cfg(test)]
mod surface_tests {
    use super::*;
    #[test]
    fn pointer_policy_overrides_tree_coverage_independently_of_paint() {
        let scene: Scene<()> = Scene {
            regions: vec![api::Rect::new(12., 16., 80., 40.).into()],
            ..Default::default()
        };
        assert!(pointer_regions(api::PointerInput::None, 320, 240, &scene).is_empty());
        assert_eq!(
            pointer_regions(api::PointerInput::Content, 320, 240, &scene),
            scene.regions
        );
        assert_eq!(
            pointer_regions(api::PointerInput::All, 320, 240, &scene),
            [api::Clip::from(api::Rect::new(0., 0., 320., 240.))]
        );
    }
}

fn configure_layer(layer: &LayerSurface, spec: &api::SurfaceSpec) {
    layer.set_layer(layer_kind(spec.layer));
    layer.set_size(spec.width, spec.height);
    layer.set_exclusive_zone(spec.exclusive_zone);
    layer.set_margin(
        spec.margins.top,
        spec.margins.right,
        spec.margins.bottom,
        spec.margins.left,
    );
    layer.set_anchor(match spec.anchor {
        api::Anchor::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        api::Anchor::TopLeft => Anchor::TOP | Anchor::LEFT,
        api::Anchor::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        api::Anchor::Fill => Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        api::Anchor::Center => Anchor::empty(),
    });
    layer.set_keyboard_interactivity(match spec.keyboard {
        api::Keyboard::None => KeyboardInteractivity::None,
        api::Keyboard::OnDemand => KeyboardInteractivity::OnDemand,
        api::Keyboard::Exclusive => KeyboardInteractivity::Exclusive,
    });
}
fn add_region(region: &Region, clip: &api::Clip) {
    let rect = clip.bounds;
    if clip.shapes().is_empty() {
        let top = (rect.y - 0.5).ceil() as i32;
        let bottom = (rect.y + rect.h - 0.5).ceil() as i32;
        if bottom > top
            && let Some((left, right)) = clip.integer_span(top)
        {
            region.add(left, top, right - left, bottom - top);
        }
        return;
    }
    for row in rect.y.floor() as i32..(rect.y + rect.h).ceil() as i32 {
        if let Some((left, right)) = clip.integer_span(row) {
            region.add(left, row, right - left, 1);
        }
    }
}

impl<A: Application> CompositorHandler for State<A> {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(id) = self.id_for(surface)
            && let Some(s) = self.surfaces.get_mut(&id)
            && s.scale != factor.max(1) as u32
        {
            s.scale = factor.max(1) as u32;
            s.demand.invalidate();
            if s.configured {
                let event = s.spec.scope_event(api::Event::Resize {
                    surface: s.spec.id,
                    width: s.width as f32,
                    height: s.height as f32,
                    scale: s.scale,
                });
                if let Some(message) = self.app.event(event) {
                    self.message(message, qh);
                }
            }
        }
        self.draw_all(qh);
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(id) = self.id_for(surface)
            && let Some(s) = self.surfaces.get_mut(&id)
        {
            s.demand.ready();
        }
        self.draw_all(qh);
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
        self.refresh_output_scales(qh);
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
        self.refresh_output_scales(qh);
    }
}
impl<A: Application> LayerShellHandler for State<A> {
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let Some(id) = self.id_for(layer.wl_surface()) else {
            return;
        };
        let Some(native) = self.retire_native(&id) else {
            return;
        };
        if let (Some(name), Some(output)) = (&native.spec.output, &native.bound_output) {
            self.closed_targets
                .insert(native.spec.address(), (name.clone(), output.clone()));
            drop(native); // GPU surface retires before its protocol role.
            if let Err(error) = self.reconcile(qh) {
                self.error = Some(error.to_string());
                self.exit = true;
            }
        } else {
            self.exit = true;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        config: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(id) = self.id_for(layer.wl_surface()) {
            let s = self.surfaces.get_mut(&id).unwrap();
            s.width = config.new_size.0.max(1);
            s.height = config.new_size.1.max(1);
            s.configured = true;
            s.demand.invalidate();
            let event = s.spec.scope_event(api::Event::Resize {
                surface: s.spec.id,
                width: s.width as f32,
                height: s.height as f32,
                scale: s.scale,
            });
            if let Some(message) = self.app.event(event) {
                self.message(message, qh);
            }
        }
        self.draw_all(qh);
    }
}
impl<A: Application> OutputHandler for State<A> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if let Err(e) = self.create_lock_outputs(qh) {
            self.error = Some(e.to_string());
            self.exit = true;
        }
        self.outputs_changed(qh);
    }
    fn update_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.refresh_output_scales(qh);
        self.outputs_changed(qh);
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let removed: Vec<_> = self
            .surfaces
            .iter()
            .filter(|(_, native)| {
                matches!(&native.role,SurfaceRole::Lock(_,bound) if *bound==output)
                    || native.bound_output.as_ref() == Some(&output)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in removed {
            self.retire_native(&id);
        }
        self.outputs_changed(qh);
    }
}
impl<A: Application> SeatHandler for State<A> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
            if self.pointer.is_some() {
                self.pointer_seat = Some(seat.clone());
            }
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let key_qh = qh.clone();
            self.keyboard = self
                .seats
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(move |state, _, event| state.key(event, &key_qh)),
                )
                .ok();
            if self.keyboard.is_some() {
                self.keyboard_seat = Some(seat.clone());
                self.clipboard_seat(&seat, qh);
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && self.pointer_seat.as_ref() == Some(&seat)
            && let Some(p) = self.pointer.take()
        {
            p.release();
            self.pointer_seat = None;
        }
        if capability == Capability::Keyboard
            && self.keyboard_seat.as_ref() == Some(&seat)
            && let Some(k) = self.keyboard.take()
        {
            k.release();
            self.keyboard_surface = None;
            self.keyboard_seat = None;
            self.clipboard_cancel(qh, true);
            self.clipboard_drop();
        }
    }
    fn remove_seat(&mut self, conn: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.remove_capability(conn, qh, seat.clone(), Capability::Keyboard);
        self.remove_capability(conn, qh, seat, Capability::Pointer);
    }
}
impl<A: Application> PointerHandler for State<A> {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let Some(id) = self.id_for(&event.surface) else {
                continue;
            };
            let now = self.epoch.elapsed().as_secs_f64();
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let mut messages = vec![];
            let mut outside = false;
            let mut scroll = None;
            self.input_serial = match event.kind {
                PointerEventKind::Press { serial, .. } => Some(serial),
                _ => None,
            };
            if let Some(s) = self.surfaces.get_mut(&id) {
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        messages.extend(s.interaction.motion(&s.scene, x, y, now))
                    }
                    PointerEventKind::Leave { .. } => s.interaction.leave(now),
                    PointerEventKind::Press { button: 0x110, .. } => {
                        outside = !s.scene.hits.iter().any(|h| h.contains(x, y));
                        if let Some(message) = s.interaction.press_with_modifiers(
                            &s.scene,
                            x,
                            y,
                            api::KeyModifiers {
                                control: self.modifiers.ctrl,
                                shift: self.modifiers.shift,
                                alt: self.modifiers.alt,
                                logo: self.modifiers.logo,
                            },
                        ) {
                            messages.push(message);
                        }
                    }
                    PointerEventKind::Release { button: 0x110, .. } => {
                        if let Some(m) = s.interaction.release(x, y) {
                            messages.push(m);
                        }
                    }
                    PointerEventKind::Axis { vertical, .. } => {
                        let lines = if vertical.discrete != 0 {
                            vertical.discrete as f64
                        } else {
                            vertical.absolute / 15.
                        };
                        if let Some(emitted) = s.interaction.scroll(&s.scene, x, y, lines) {
                            messages.extend(emitted);
                        } else {
                            scroll = Some(lines);
                        }
                    }
                    _ => continue,
                }
                s.demand.invalidate();
            }
            self.clipboard_observe_focus(qh);
            let logical_id = self.surfaces[&id].spec.id;
            let boundary = match event.kind {
                PointerEventKind::Enter { .. } => Some(true),
                PointerEventKind::Leave { .. } => Some(false),
                _ => None,
            };
            if let Some(inside) = boundary
                && let Some(message) = self.surface_event(
                    &id,
                    api::Event::PointerInside {
                        surface: logical_id,
                        inside,
                    },
                )
            {
                messages.push(message);
            }
            let button = match event.kind {
                PointerEventKind::Press { button, .. } => Some((button, true)),
                PointerEventKind::Release { button, .. } => Some((button, false)),
                _ => None,
            };
            if let Some((button, pressed)) = button {
                let button = match button {
                    0x110 => api::MouseButton::Primary,
                    0x111 => api::MouseButton::Secondary,
                    0x112 => api::MouseButton::Middle,
                    other => api::MouseButton::Other(other),
                };
                if let Some(message) = self.surface_event(
                    &id,
                    api::Event::PointerButton {
                        surface: logical_id,
                        button,
                        pressed,
                        x,
                        y,
                    },
                ) {
                    messages.push(message);
                }
            }

            if outside
                && let Some(m) = self.surface_event(
                    &id,
                    api::Event::Outside {
                        surface: logical_id,
                    },
                )
            {
                messages.push(m);
            }
            if let Some(lines) = scroll
                && let Some(m) = self.surface_event(
                    &id,
                    api::Event::Scroll {
                        surface: logical_id,
                        lines,
                    },
                )
            {
                messages.push(m);
            }
            for m in messages {
                self.message(m, qh);
            }
        }
        self.input_serial = None;
        self.draw_all(qh);
    }
}
impl<A: Application> KeyboardHandler for State<A> {
    fn enter(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.clipboard_cancel(qh, false);
        self.keyboard_surface = self.id_for(surface);
    }
    fn leave(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.keyboard_surface = None;
        self.input_serial = None;
        self.clipboard_cancel(qh, false);
    }
    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.input_serial = Some(serial);
        self.key(event, qh);
        self.input_serial = None;
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event, qh);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}
delegate_registry!(@<A:Application> State<A>);
impl<A: Application> ProvidesRegistryState for State<A> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(@<A:Application> State<A>);

impl<A: Application> SessionLockHandler for State<A> {
    fn locked(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: SessionLock) {
        self.locked = true;
        self.effects(Effects::default(), qh);
    }
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        self.error = Some("Compositor refused the session lock".into());
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: SessionLockSurface,
        config: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        if let Some(id) = self.id_for(surface.wl_surface()) {
            let s = self.surfaces.get_mut(&id).unwrap();
            s.width = config.new_size.0;
            s.height = config.new_size.1;
            s.configured = true;
            s.demand.invalidate();
        }
        self.draw_all(qh);
    }
}
