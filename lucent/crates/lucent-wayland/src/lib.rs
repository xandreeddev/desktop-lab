//! Generic event-driven Wayland runtime. Desktop policy lives entirely in clients.
use calloop::{EventLoop, LoopHandle, channel};
use calloop_wayland_source::WaylandSource;
use lucent_api::{self as api, Application, Effects, FrameDemand, ViewContext};
use lucent_render::{Error, Gpu, Renderer};
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
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};
use wgpu_handle::HasDisplayHandle;
// Use the exact raw-window-handle version used by the renderer.
use lucent_render::display_handle as wgpu_handle;

enum RuntimeEvent<M> {
    Message(M),
    Command(String, mpsc::SyncSender<String>),
}
struct Native<M> {
    renderer: Renderer, // Drop before layer.
    role: SurfaceRole,
    spec: api::SurfaceSpec,
    width: u32,
    height: u32,
    scale: u32,
    configured: bool,
    demand: FrameDemand,
    scene: Scene<M>,
    interaction: Interaction<M>,
    regions: Vec<(api::Rect, f32)>,
    frame_times: std::collections::VecDeque<f64>,
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
    surfaces: BTreeMap<String, Native<A::Message>>,
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
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_surface: Option<String>,
    modifiers: Modifiers,
    epoch: Instant,
    exit: bool,
    error: Option<String>,
    handle: LoopHandle<'static, Self>,
    sender: channel::Sender<RuntimeEvent<A::Message>>,
    tasks: mpsc::Sender<api::Task<A::Message>>,
    subscriptions: BTreeMap<String, api::Cancellation>,
    connection: Connection,
}
/// Run any framework application, with no desktop-specific types in the backend.
pub fn run<A: Application>(app: A) -> Result<(), Error> {
    run_inner(app, None, true)
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
    run_inner(app, None, false)
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
    let (sender, events) = channel::channel();
    let (task_sender, task_receiver) = mpsc::channel::<api::Task<A::Message>>();
    let completion = sender.clone();
    // A single ordered effects worker makes configuration writes deterministic.
    std::thread::spawn(move || {
        for task in task_receiver {
            let message = task();
            if completion.send(RuntimeEvent::Message(message)).is_err() {
                break;
            }
        }
    });
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
        pointer: None,
        keyboard: None,
        keyboard_surface: None,
        modifiers: Modifiers::default(),
        epoch: Instant::now(),
        exit: false,
        error: None,
        handle: handle.clone(),
        sender: sender.clone(),
        tasks: task_sender,
        subscriptions: BTreeMap::new(),
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
    let ipc_cancel = api::Cancellation::default();
    if ipc {
        if socket_path.exists() {
            fs::remove_file(&socket_path)?;
        }
        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
        let cancel = ipc_cancel.clone();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                if cancel.cancelled() {
                    break;
                }
                if let Ok(mut stream) = incoming {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    let mut text = String::new();
                    if BufReader::new(&stream)
                        .take(4096)
                        .read_line(&mut text)
                        .is_err()
                    {
                        continue;
                    }
                    let (tx, rx) = mpsc::sync_channel(1);
                    if sender
                        .send(RuntimeEvent::Command(text.trim().into(), tx))
                        .is_err()
                    {
                        break;
                    }
                    let result = rx
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap_or_else(|_| "error: client not responding".into());
                    let _ = writeln!(stream, "{result}");
                }
            }
        });
    }
    let event_qh = qh.clone();
    handle.insert_source(events, move |event, _, state| {
        if let channel::Event::Msg(event) = event {
            match event {
                RuntimeEvent::Message(msg) => state.message(msg, &event_qh),
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
    for cancel in state.subscriptions.values() {
        cancel.cancel();
    }
    ipc_cancel.cancel();
    if ipc {
        let _ = UnixStream::connect(&socket_path);
        let _ = fs::remove_file(socket_path);
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
                layer: api::Layer::Overlay,
                anchor: api::Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: api::Keyboard::Exclusive,
                visible: true,
                capture_all: true,
            };
            self.surfaces.insert(
                format!("lock-{}", output.id().protocol_id()),
                Native {
                    renderer,
                    role: SurfaceRole::Lock(lock_surface, output),
                    spec,
                    width: 1,
                    height: 1,
                    scale: 1,
                    configured: false,
                    demand: FrameDemand::default(),
                    scene: Scene::default(),
                    interaction: Interaction::default(),
                    regions: vec![],
                    frame_times: Default::default(),
                },
            );
        }
        Ok(())
    }
    fn inspect(&self) -> String {
        serde_json::json!({"uptime_ms":self.epoch.elapsed().as_secs_f64()*1000.,"adapter":self.gpu.adapter_description,"surfaces":self.surfaces.iter().map(|(id,s)|serde_json::json!({"id":id,"width":s.width,"height":s.height,"scale":s.scale,"buffer_width":s.width*s.scale,"buffer_height":s.height*s.scale,"frames":s.renderer.frames,"focus":s.interaction.focus,"focus_visible":s.interaction.focus_visible,"frame_times_ms":s.frame_times,"hits":s.scene.hits.iter().map(|h|serde_json::json!({"id":h.id,"x":h.rect.x,"y":h.rect.y,"width":h.rect.w,"height":h.rect.h,"clip":{"x":h.clip.x,"y":h.clip.y,"width":h.clip.w,"height":h.clip.h}})).collect::<Vec<_>>() })).collect::<Vec<_>>(),"client":serde_json::from_str::<serde_json::Value>(&self.app.inspect()).unwrap_or(serde_json::Value::Null)}).to_string()
    }
    fn message(&mut self, message: A::Message, qh: &QueueHandle<Self>) {
        let mut effects = Effects {
            now: self.epoch.elapsed().as_secs_f64(),
            ..Default::default()
        };
        self.app.update(message, &mut effects);
        self.effects(effects, qh);
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
            let _ = self.tasks.send(task);
        }
        if let Err(e) = self.reconcile(qh) {
            self.error = Some(e.to_string());
            self.exit = true;
        }
        for id in effects.redraw {
            for s in self.surfaces.values_mut().filter(|s| s.spec.id == id) {
                s.demand.invalidate();
            }
        }
        self.draw_all(qh);
    }
    fn reconcile(&mut self, qh: &QueueHandle<Self>) -> Result<(), Error> {
        if self.session_lock.is_none() {
            let specs = self.app.surfaces();
            self.surfaces
                .retain(|id, _| specs.iter().any(|s| s.id == *id && s.visible));
            for spec in specs.into_iter().filter(|s| s.visible) {
                if let Some(native) = self.surfaces.get_mut(spec.id) {
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
                    Some(format!("{}-{}", self.app.name(), spec.id)),
                    None,
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
                    spec.id.into(),
                    Native {
                        renderer,
                        role: SurfaceRole::Layer(layer),
                        spec,
                        width: 1,
                        height: 1,
                        scale: 1,
                        configured: false,
                        demand: FrameDemand::default(),
                        scene: Scene::default(),
                        interaction: Interaction::default(),
                        regions: vec![],
                        frame_times: std::collections::VecDeque::new(),
                    },
                );
            }
        }
        let wanted = self.app.subscriptions();
        self.subscriptions.retain(|id, cancel| {
            let keep = wanted.iter().any(|s| s.id == *id);
            if !keep {
                cancel.cancel();
            }
            keep
        });
        for subscription in wanted {
            if self.subscriptions.contains_key(&subscription.id) {
                continue;
            }
            let cancel = api::Cancellation::default();
            self.subscriptions.insert(subscription.id, cancel.clone());
            let sender = self.sender.clone();
            std::thread::spawn(move || {
                (subscription.run)(
                    api::Emitter::new(move |message| {
                        let _ = sender.send(RuntimeEvent::Message(message));
                    }),
                    cancel,
                )
            });
        }
        Ok(())
    }
    /// Some compositors update wl_output scale without repeating the preferred
    /// buffer-scale event on every existing layer surface. Track entered outputs
    /// as well, so an idle bar/dock does not retain a low-density buffer.
    fn refresh_output_scales(&mut self, qh: &QueueHandle<Self>) {
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
            }
        }
        self.draw_all(qh);
    }
    fn draw_all(&mut self, qh: &QueueHandle<Self>) {
        let now = self.epoch.elapsed().as_secs_f64();
        for s in self.surfaces.values_mut() {
            if !s.configured || !s.demand.begin() {
                continue;
            }
            let tree = self.app.view(&ViewContext {
                surface: s.spec.id,
                width: s.width as f32,
                height: s.height as f32,
                now,
            });
            let scene =
                self.layout
                    .build(&tree, s.width as f32, s.height as f32, &s.interaction, now);
            let focus = s.interaction.focus.clone();
            s.interaction.synchronize(&scene);
            if focus != s.interaction.focus {
                s.demand.invalidate();
            }
            let regions = if s.spec.capture_all {
                vec![(api::Rect::new(0., 0., s.width as f32, s.height as f32), 0.)]
            } else {
                scene.regions.clone()
            };
            if regions != s.regions {
                match Region::new(&self.compositor) {
                    Ok(region) => {
                        for (rect, radius) in &regions {
                            add_region(&region, *rect, *radius);
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
            surface.frame(qh, FrameCallbackData(surface.clone()));
            if let Err(e) = s.renderer.render(s.width, s.height, s.scale, &scene.paint) {
                self.error = Some(e.to_string());
                self.exit = true;
            }
            if self.app.animating(s.spec.id, now) || s.interaction.animating(now) {
                s.demand.invalidate();
            }
            s.frame_times
                .push_back(self.epoch.elapsed().as_secs_f64() * 1000.);
            if s.frame_times.len() > 120 {
                s.frame_times.pop_front();
            }
            s.scene = scene;
        }
    }
    fn id_for(&self, surface: &wl_surface::WlSurface) -> Option<String> {
        self.surfaces
            .iter()
            .find(|(_, s)| s.role.wl_surface() == surface)
            .map(|(id, _)| id.clone())
    }
    fn key(&mut self, event: KeyEvent, qh: &QueueHandle<Self>) {
        let Some(id) = self.keyboard_surface.clone() else {
            return;
        };
        let key = match event.keysym {
            Keysym::Escape => api::Key::Escape,
            Keysym::Return | Keysym::KP_Enter => api::Key::Enter,
            Keysym::BackSpace => api::Key::Backspace,
            Keysym::Up => api::Key::Up,
            Keysym::Down => api::Key::Down,
            Keysym::Left => api::Key::Left,
            Keysym::Right => api::Key::Right,
            Keysym::ISO_Left_Tab => api::Key::BackTab,
            Keysym::Tab if self.modifiers.shift => api::Key::BackTab,
            Keysym::Tab => api::Key::Tab,
            Keysym::Home => api::Key::Home,
            Keysym::End => api::Key::End,
            Keysym::a if self.modifiers.ctrl => api::Key::SelectAll,
            Keysym::u if self.modifiers.ctrl => api::Key::ClearInput,
            _ => {
                let Some(text) = event
                    .utf8
                    .filter(|s| !s.is_empty() && s.chars().all(|c| !c.is_control()))
                else {
                    return;
                };
                if self.modifiers.ctrl || self.modifiers.alt {
                    return;
                }
                api::Key::Text(text)
            }
        };
        if let Some(s) = self.surfaces.get_mut(&id) {
            if !s.interaction.focus_visible {
                s.demand.invalidate();
            }
            s.interaction.focus_visible = true;
        }
        let logical_id = self.surfaces[&id].spec.id;
        if let Some(message) = self.app.event(api::Event::Key {
            surface: logical_id,
            key: key.clone(),
        }) {
            self.message(message, qh);
            return;
        }
        if let Some(s) = self.surfaces.get_mut(&id) {
            let before = s.interaction.focus.clone();
            let message = s.interaction.key(&s.scene, &key);
            if before != s.interaction.focus {
                s.demand.invalidate();
            }
            if let Some(message) = message {
                self.message(message, qh);
            } else {
                self.draw_all(qh);
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
fn configure_layer(layer: &LayerSurface, spec: &api::SurfaceSpec) {
    layer.set_layer(layer_kind(spec.layer));
    layer.set_size(spec.width, spec.height);
    layer.set_exclusive_zone(spec.exclusive_zone);
    layer.set_anchor(match spec.anchor {
        api::Anchor::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
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
fn add_region(region: &Region, rect: api::Rect, radius: f32) {
    let radius = radius.min(rect.w / 2.).min(rect.h / 2.);
    if radius < 1. {
        region.add(
            rect.x as i32,
            rect.y as i32,
            rect.w.ceil() as i32,
            rect.h.ceil() as i32,
        );
        return;
    }
    for row in rect.y.floor() as i32..(rect.y + rect.h).ceil() as i32 {
        let edge =
            ((row as f32 + 0.5 - rect.y - rect.h / 2.).abs() - (rect.h / 2. - radius)).max(0.);
        let inset = radius - (radius * radius - edge * edge).max(0.).sqrt();
        let left = (rect.x + inset).ceil() as i32;
        let right = (rect.x + rect.w - inset).floor() as i32;
        if right > left {
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
        {
            s.scale = factor.max(1) as u32;
            s.demand.invalidate();
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
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
            let event = api::Event::Resize {
                surface: s.spec.id,
                width: s.width as f32,
                height: s.height as f32,
            };
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
    }
    fn update_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.refresh_output_scales(qh);
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.surfaces
            .retain(|_, s| !matches!(&s.role,SurfaceRole::Lock(_,o) if *o==output));
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
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
            self.keyboard_surface = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
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
            if let Some(s) = self.surfaces.get_mut(&id) {
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        messages.extend(s.interaction.motion(&s.scene, x, y, now))
                    }
                    PointerEventKind::Leave { .. } => s.interaction.leave(now),
                    PointerEventKind::Press { button: 0x110, .. } => {
                        outside = !s.scene.hits.iter().any(|h| h.contains(x, y));
                        s.interaction.press(&s.scene, x, y);
                    }
                    PointerEventKind::Release { button: 0x110, .. } => {
                        if let Some(m) = s.interaction.release(x, y) {
                            messages.push(m);
                        }
                    }
                    PointerEventKind::Axis { vertical, .. } => {
                        scroll = Some(if vertical.discrete != 0 {
                            vertical.discrete as f64
                        } else {
                            vertical.absolute / 15.
                        })
                    }
                    _ => continue,
                }
                s.demand.invalidate();
            }
            let logical_id = self.surfaces[&id].spec.id;
            if outside
                && let Some(m) = self.app.event(api::Event::Outside {
                    surface: logical_id,
                })
            {
                messages.push(m);
            }
            if let Some(lines) = scroll
                && let Some(m) = self.app.event(api::Event::Scroll {
                    surface: logical_id,
                    lines,
                })
            {
                messages.push(m);
            }
            for m in messages {
                self.message(m, qh);
            }
        }
        self.draw_all(qh);
    }
}
impl<A: Application> KeyboardHandler for State<A> {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_surface = self.id_for(surface);
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.keyboard_surface = None;
    }
    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event, qh);
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
