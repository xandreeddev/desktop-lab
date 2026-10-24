//! Generic event-driven Wayland runtime. Desktop policy lives entirely in clients.
use calloop::{
    EventLoop, LoopHandle, RegistrationToken, channel,
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
mod activation;
mod batching;
mod clipboard;
mod delivery;
mod effects;
mod fractional;
mod handoff;
mod input_method;
mod input_release;
mod ipc;
mod keyboard;
mod layer_commit;
mod outputs;
mod pacing;
mod pointer_input;
mod presentation;
mod profiling;
mod subscriptions;

enum RuntimeEvent<M> {
    Message(M),
    Subscription(subscriptions::Event<M>),
    Failure(api::RuntimeFailure),
    Command(String, mpsc::SyncSender<String>),
}
struct Native<M> {
    renderer: Renderer,                      // Drop before layer.
    fractional: Option<fractional::Surface>, // Destroy extensions before role/surface.
    role: SurfaceRole,
    spec: api::SurfaceSpec,
    committed_spec: api::SurfaceSpec,
    bound_output: Option<wl_output::WlOutput>,
    width: u32,
    height: u32,
    scale: u32,
    configured: bool,
    presented: Option<PresentationState>,
    demand: FrameDemand,
    scene: Scene<M>,
    interaction: Interaction<M>,
    regions: Vec<api::Clip>,
    history: handoff::SubmissionHistory,
    pacing: Option<pacing::Trace>,
    native_generation: Option<u64>, // Presentation-only identity; never reused.
}
#[derive(Clone, Copy)]
struct PresentationState {
    extent: (u32, u32),
    buffer_scale: i32,
}
impl<M> Native<M> {
    fn density(&self) -> api::Density {
        self.fractional
            .as_ref()
            .and_then(|scale| scale.preferred)
            .unwrap_or_else(|| {
                api::Density::integer(self.scale).expect("positive native integer scale")
            })
    }
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
    retiring: handoff::Presentations<api::SurfaceAddress, Native<A::Message>>,
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
    fractional: Option<fractional::Globals>,
    closed_targets: BTreeMap<api::SurfaceAddress, (api::OutputName, wl_output::WlOutput)>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_seat: Option<wl_seat::WlSeat>,
    pointer_seat: Option<wl_seat::WlSeat>,
    keyboard_surface: Option<api::SurfaceAddress>,
    input_serial: Option<u32>,
    clipboard: clipboard::Clipboard<A::Message>,
    input_method: input_method::InputMethod,
    activation: activation::Activation<A::Message>,
    input_release: input_release::Releases<A::Message>,
    modifiers: Modifiers,
    epoch: Instant,
    profile_frames: bool,
    pacing: pacing::Capture,
    presentation: presentation::Capture,
    exit: bool,
    error: Option<String>,
    handle: LoopHandle<'static, Self>,
    sender: delivery::Delivery<RuntimeEvent<A::Message>>,
    tasks: effects::Executor<A::Message>,
    subscriptions: subscriptions::Workers,
    interaction_timer: Option<(f64, RegistrationToken)>,
    draw_wakeup: calloop::ping::Ping,
    connection: Connection,
}
impl<A: Application> Drop for State<A> {
    fn drop(&mut self) {
        if let Some((_, token)) = self.interaction_timer.take() {
            self.handle.remove(token);
        }
        self.clipboard_drop();
        self.input_method.shutdown();
        self.presentation.clear_pending();
        // Drop GPU surfaces before their protocol roles, then send the queued
        // destroy requests while the connection is still alive. A caller such
        // as a permission agent can outlive this individual runtime invocation.
        // Teardown never authorizes a secure-session unlock.
        for native in self.surfaces.values_mut() {
            self.pacing.retire(native.pacing.take());
        }
        self.surfaces.clear();
        self.retiring.clear();
        self.fractional.take();
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
            lucent_ui::text::Font::from_bytes(bytes, fontdue::FontSettings::default())
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
            lucent_ui::text::Font::from_bytes(
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
    // A post-submission effect can dirty another configured surface after the
    // draw pass visited it. Wake the next dispatch once, without polling idle
    // clients or bypassing any surface's compositor callback admission.
    let (draw_wakeup, draw_events) = calloop::ping::make_ping()?;
    handle.insert_source(draw_events, |(), _, _| {})?;
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
    let input_method_allowed = authorize_unlock.is_none() && app.input_method_enabled();
    let activation = activation::Activation::new(
        &globals,
        &qh,
        app.activation_enabled() && authorize_unlock.is_none(),
    );
    let profile_frames =
        std::env::var_os("LUCENT_PROFILE_FRAMES").is_some_and(|value| value == "1");
    let mut state = State {
        surfaces: BTreeMap::new(),
        retiring: Default::default(),
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
        fractional: fractional::Globals::bind(&globals, &qh),
        closed_targets: BTreeMap::new(),
        pointer: None,
        keyboard: None,
        keyboard_seat: None,
        pointer_seat: None,
        keyboard_surface: None,
        input_serial: None,
        clipboard: clipboard::Clipboard::new(clipboard_manager),
        input_method: input_method::InputMethod::bind(&globals, &qh, !input_method_allowed),
        activation,
        input_release: Default::default(),
        modifiers: Modifiers::default(),
        epoch: Instant::now(),
        presentation: presentation::Capture::bind(&globals, &qh, ipc && authorize_unlock.is_none()),
        profile_frames,
        pacing: pacing::Capture::new(profile_frames),
        exit: false,
        error: None,
        handle: handle.clone(),
        sender: sender.clone(),
        tasks,
        subscriptions: subscriptions::Workers::default(),
        interaction_timer: None,
        draw_wakeup,
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
        if !state.exit {
            state.draw_all(&qh);
        }
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
                    fractional: self
                        .fractional
                        .as_ref()
                        .map(|globals| globals.surface(lock_surface.wl_surface(), qh)),
                    role: SurfaceRole::Lock(lock_surface, output),
                    committed_spec: spec.clone(),
                    spec,
                    bound_output: None,
                    width: 1,
                    height: 1,
                    scale: 1,
                    configured: false,
                    presented: None,
                    demand: FrameDemand::default(),
                    scene: Scene::default(),
                    interaction: Interaction::default(),
                    regions: vec![],
                    history: Default::default(),
                    pacing: self.pacing.start_native(),
                    native_generation: self.presentation.start_native(),
                },
            );
        }
        Ok(())
    }
    fn inspect(&self) -> String {
        let cache = self.gpu.cache_stats();
        let mut scene_images = BTreeMap::new();
        if self.profile_frames {
            for surface in self
                .surfaces
                .values()
                .chain(self.retiring.iter().map(|(_, native)| native))
            {
                for primitive in &surface.scene.paint {
                    if let lucent_ui::Paint::Image { data, .. } = primitive {
                        scene_images.insert(data.cache_id(), data.bytes() as u64);
                    }
                }
            }
        }
        let profile_memory = self.profile_frames.then(|| serde_json::json!({
            "scene_image_payload_bytes": scene_images.values().sum::<u64>(),
            "geometry_buffer_payload_bytes": self.surfaces.values().chain(self.retiring.iter().map(|(_, native)| native)).map(|s| s.renderer.buffer_payload_bytes()).sum::<u64>(),
            "one_color_buffer_per_surface_bytes": self.surfaces.values().chain(self.retiring.iter().map(|(_, native)| native)).map(|s| s.renderer.single_surface_buffer_bytes()).sum::<u64>(),
            "scope":"Prepared image payloads referenced by current and retiring scenes only; app caches excluded. GPU buffer values are requested payloads, not driver allocations. Actual surface buffer counts and total GPU memory are unknown."
        }));
        let surfaces = self.surfaces.iter().map(|(id,s)|serde_json::json!({"id":id.to_string(),"role":s.spec.id,"instance":s.spec.instance.as_ref().map(api::SurfaceInstance::as_str),"redraw_group":s.spec.redraw_group,"output":s.spec.output.as_ref().map(api::OutputName::as_str),"keyboard":match s.spec.keyboard { api::Keyboard::None=>"none", api::Keyboard::OnDemand=>"on-demand", api::Keyboard::Exclusive=>"exclusive" },"pointer_input":s.spec.pointer_input.name(),"width":s.width,"height":s.height,"scale":s.scale,"density":s.density().factor(),"buffer_scale":if s.fractional.is_some(){1}else{s.scale},"buffer_width":s.density().physical_size(s.width,s.height).ok().map(|size|size.0),"buffer_height":s.density().physical_size(s.width,s.height).ok().map(|size|size.1),"frames":s.history.frames,"native_frames":s.renderer.frames,"native_generation":s.native_generation,"paint_primitives":s.scene.paint.len(),"focus":s.interaction.focus,"focus_visible":s.interaction.focus_visible,"frame_times_ms":s.history.frame_times,"frame_profiles":s.history.profiles.json(),"frame_pacing":s.pacing.as_ref().map(pacing::Trace::json),"presentation":s.history.presentation.json(),"hits":s.scene.hits.iter().map(|h|serde_json::json!({"id":h.id,"x":h.rect.x,"y":h.rect.y,"width":h.rect.w,"height":h.rect.h,"clip":{"x":h.clip.bounds.x,"y":h.clip.bounds.y,"width":h.clip.bounds.w,"height":h.clip.bounds.h}})).collect::<Vec<_>>() })).collect::<Vec<_>>();
        let mut result = serde_json::json!({"profile_memory":profile_memory,"presentation_feedback":self.presentation.json(),"frame_profiling":self.profile_frames,"profile_scope":"Successful submissions only; CPU-side wall time includes driver waits. prepare_breakdown_ms is nested within prepare. No GPU execution or scanout timestamps.","gpu_cache":{"entries":cache.entries,"payload_bytes":cache.bytes,"active_payload_bytes":cache.active_bytes,"reuse_budget_bytes":cache.budget_bytes,"hits":cache.hits,"misses":cache.misses,"evictions":cache.evictions},"uptime_ms":self.epoch.elapsed().as_secs_f64()*1000.,"adapter":self.gpu.adapter_description,"surfaces":surfaces,"client":serde_json::from_str::<serde_json::Value>(&self.app.inspect()).unwrap_or(serde_json::Value::Null)});
        result["frame_pacing"] = self.pacing.json();
        result["outputs"]=serde_json::json!(self.output_bindings.iter().map(|(info,_)|serde_json::json!({"global":info.global,"name":info.name.as_ref().map(api::OutputName::as_str),"logical_size":info.logical_size,"integer_scale":info.integer_scale})).collect::<Vec<_>>());
        result["retiring_native_surfaces"] = serde_json::json!(self.retiring.len());
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
        self.messages_with_effects(
            messages,
            Effects {
                now: self.epoch.elapsed().as_secs_f64(),
                ..Default::default()
            },
            qh,
        );
    }
    fn messages_with_effects(
        &mut self,
        messages: impl IntoIterator<Item = A::Message>,
        mut effects: Effects<A::Message>,
        qh: &QueueHandle<Self>,
    ) {
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
        // Session-lock roles cannot be recreated: a second role for one output
        // is a protocol error, and teardown never grants an unlock. Reject the
        // whole effect batch before tasks, clipboard or native state mutate.
        if let Err(error) = handoff::validate_recreation(
            self.authorize_unlock.is_some() || self.session_lock.is_some(),
            !effects.recreate_surfaces.is_empty(),
        ) {
            self.error = Some(error.into());
            self.exit = true;
            return;
        }
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
        // Capture the real input surface before reconciliation closes a launcher.
        // The token callback is asynchronous and owns no native application policy.
        for request in effects.activation {
            self.activation_request(request, qh);
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
        let recreate = effects.recreate_surfaces.into_iter().collect();
        if let Err(e) = self.reconcile_with_recreation(qh, &recreate) {
            self.error = Some(e.to_string());
            self.exit = true;
        }
        self.input_release_revalidate(qh);
        for request in effects.input_release {
            self.input_release_request(request, qh);
        }
        for id in &effects.redraw {
            for s in self
                .surfaces
                .values_mut()
                .filter(|s| s.spec.matches_redraw(id))
            {
                s.demand.invalidate();
            }
        }
        for address in &effects.redraw_instances {
            for native in self
                .surfaces
                .values_mut()
                .filter(|native| native.spec.address() == *address)
            {
                native.demand.invalidate();
            }
        }
        // Reconciliation installs the current requested geometry before one
        // CPU-only input layout. Scope authority changes immediately, even if a
        // compositor callback still blocks GPU submission. Caret reveal remains
        // a controlled client batch; unrelated wallpaper frames skip editor IO.
        let clipboard_targets: std::collections::BTreeSet<_> = effects
            .clipboard
            .iter()
            .map(|request| request.target.address())
            .collect();
        let affected: Vec<_> = self
            .surfaces
            .iter()
            .filter(|(_, native)| {
                native.spec != native.committed_spec
                    || effects
                        .redraw
                        .iter()
                        .any(|id| native.spec.matches_redraw(id))
                    || effects.redraw_instances.contains(&native.spec.address())
                    || clipboard_targets.contains(&native.spec.address())
            })
            .map(|(address, native)| (address.clone(), native.role.wl_surface().clone()))
            .collect();
        let mut input_messages = vec![];
        for (address, origin) in affected {
            if self.id_for(&origin).as_ref() != Some(&address) {
                continue;
            }
            let refresh = if clipboard_targets.contains(&address) {
                InputRefresh::Current
            } else {
                InputRefresh::Affected
            };
            input_messages.extend(
                self.refresh_input_scene(&address, true, refresh)
                    .into_iter()
                    .map(|message| (origin.clone(), message)),
            );
        }
        self.clipboard_observe_focus(qh);
        if !input_messages.is_empty() {
            // Carry this update's clipboard requests through the bounded reveal /
            // retirement batch instead of dropping or validating them early.
            let mut carried = Effects {
                now: self.epoch.elapsed().as_secs_f64(),
                clipboard: effects.clipboard,
                ..Default::default()
            };
            for (origin, message) in input_messages {
                if self.id_for(&origin).is_some() {
                    self.app.update(message, &mut carried);
                }
            }
            self.effects(carried, qh);
            return;
        }
        // New popup targets now see their admitted scope; old editor transfers
        // have retired before a request can validate clipboard authority.
        for request in effects.clipboard {
            self.clipboard_request(request, qh);
        }
    }
    fn reconcile(&mut self, qh: &QueueHandle<Self>) -> Result<(), Error> {
        self.reconcile_with_recreation(qh, &Default::default())
    }
    fn reconcile_with_recreation(
        &mut self,
        qh: &QueueHandle<Self>,
        recreate: &std::collections::BTreeSet<api::SurfaceAddress>,
    ) -> Result<(), Error> {
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
            self.retiring.retain(|id, _| {
                specs
                    .iter()
                    .any(|spec| spec.address() == *id && spec.visible)
            });
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
                if recreate.contains(&address)
                    && let Some(native) = self.surfaces.get(&address)
                {
                    handoff::validate_layer_recreation(
                        native.committed_spec.exclusive_zone,
                        native.spec.exclusive_zone,
                        spec.exclusive_zone,
                    )?;
                }
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
                let history = if recreate.contains(&address) {
                    self.begin_recreation(&address)?
                } else {
                    None
                };
                if let Some(native) = self.surfaces.get_mut(&address) {
                    if native.spec != spec {
                        // Stage role changes until a matching buffer is ready.
                        // A geometry-only commit can move the old image before
                        // the configure response and replacement buffer arrive.
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
                        fractional: self
                            .fractional
                            .as_ref()
                            .map(|globals| globals.surface(layer.wl_surface(), qh)),
                        role: SurfaceRole::Layer(layer),
                        committed_spec: spec.clone(),
                        spec,
                        bound_output: target,
                        width: 1,
                        height: 1,
                        scale: 1,
                        configured: false,
                        presented: None,
                        demand: FrameDemand::default(),
                        scene: Scene::default(),
                        interaction: Interaction::default(),
                        regions: vec![],
                        history: history.unwrap_or_default(),
                        pacing: self.pacing.start_native(),
                        native_generation: self.presentation.start_native(),
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
        self.retiring.retire(id);
        let mut native = self.surfaces.remove(id)?;
        self.pacing.retire(native.pacing.take());
        self.presentation.retire_native(native.native_generation);
        Some(native)
    }
    fn begin_recreation(
        &mut self,
        id: &api::SurfaceAddress,
    ) -> Result<Option<handoff::SubmissionHistory>, Error> {
        let Some(native) = self.surfaces.get_mut(id) else {
            return Ok(None);
        };
        let SurfaceRole::Layer(layer) = &native.role else {
            return Err("Cannot recreate a session-lock surface".into());
        };
        let region = Region::new(&self.compositor)?;
        if let Some(presented) = native.presented {
            // A failed/retried render may have queued viewport/role state
            // without submitting its matching buffer. This input-disable commit
            // must retain the geometry and scale of the last successful image.
            configure_layer(layer, &native.committed_spec);
            if let Some(fractional) = &mut native.fractional {
                fractional.destination(presented.extent.0, presented.extent.1)?;
            }
            layer.wl_surface().set_buffer_scale(presented.buffer_scale);
        }
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
        layer.commit();
        if self.keyboard_surface.as_ref() == Some(id) {
            self.keyboard_surface = None;
            self.input_serial = None;
            // draw_all observes the retired focus after replacement insertion.
            // Cancelling here would synchronously reenter client update while
            // native replacement and logical-history transfer are incomplete.
        }
        let mut previous = self.surfaces.remove(id).expect("validated native surface");
        self.pacing.retire(previous.pacing.take());
        self.presentation.retire_native(previous.native_generation);
        let history = previous.history.continuation();
        let presented = previous.presented.is_some();
        self.retiring
            .retain_last_good(id.clone(), previous, presented);
        Ok(Some(history))
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
                    events.push((
                        surface.role.wl_surface().clone(),
                        surface.spec.scope_event(api::Event::Resize {
                            surface: surface.spec.id,
                            width: surface.width as f32,
                            height: surface.height as f32,
                            scale,
                            density: surface.density(),
                        }),
                    ));
                }
            }
        }
        for (surface, event) in events {
            if self.id_for(&surface).is_some()
                && let Some(message) = self.app.event(event)
            {
                self.message(message, qh);
            }
        }
    }
    /// One fresh CPU scene handles text geometry and declarative scope
    /// admission together. It never begins GPU demand or changes native regions.
    fn refresh_input_scene(
        &mut self,
        address: &api::SurfaceAddress,
        reveal: bool,
        refresh: InputRefresh,
    ) -> Vec<A::Message> {
        let Some(native) = self
            .surfaces
            .get_mut(address)
            .filter(|native| native.configured)
        else {
            return vec![];
        };
        let now = self.epoch.elapsed().as_secs_f64();
        let Some((_, _, scene)) = current_scene(&self.app, &self.layout, native, now, refresh)
        else {
            return vec![];
        };
        let (messages, focus_changed) =
            update_input_scene(&mut native.interaction, &mut native.scene, scene, reveal);
        if focus_changed {
            native.demand.invalidate();
        }
        messages
    }
    fn draw_all(&mut self, qh: &QueueHandle<Self>) {
        let now = self.epoch.elapsed().as_secs_f64();
        let mut submitted = Vec::new();
        let mut overlay_messages = Vec::new();
        for s in self.surfaces.values_mut() {
            if !s.configured || !s.demand.begin() {
                continue;
            }
            if let Some(trace) = &mut s.pacing {
                trace.begin_attempt(s.history.frames.saturating_add(1), pacing::now(self.epoch));
            }
            let total = self.profile_frames.then(Instant::now);
            let started = self.profile_frames.then(Instant::now);
            let (width, height, tree) = current_tree(&self.app, s, now);
            let view_ms = profiling::elapsed(started);
            let started = self.profile_frames.then(Instant::now);
            let scene = self
                .layout
                .build(&tree, width as f32, height as f32, &s.interaction, now);
            let layout_ms = profiling::elapsed(started);
            let started = self.profile_frames.then(Instant::now);
            let focus = s.interaction.focus.clone();
            s.interaction.synchronize(&scene);
            while let Some(message) = s.interaction.take_overlay_dismissal() {
                overlay_messages.push((s.role.wl_surface().clone(), message));
            }
            if focus != s.interaction.focus {
                s.demand.invalidate();
            }
            let regions = pointer_regions(s.spec.pointer_input, width, height, &scene);
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
            let density = s.density();
            let surface = s.role.wl_surface();
            if let Some(fractional) = &mut s.fractional {
                if let Err(error) = fractional.destination(width, height) {
                    if let Some(trace) = &mut s.pacing {
                        trace.finish_attempt(
                            pacing::now(self.epoch),
                            pacing::Outcome::PreRenderAbort,
                        );
                    }
                    self.error = Some(error.to_string());
                    self.exit = true;
                    return;
                }
                surface.set_buffer_scale(1);
            } else {
                surface.set_buffer_scale(s.scale as i32);
            }
            let input_ms = profiling::elapsed(started);
            s.renderer.enable_profiling(self.profile_frames);
            s.renderer
                .enable_stream_tracing(self.presentation.tracing());
            let mut input_release_frame = None;
            let outcome =
                s.renderer
                    .render_scaled(width, height, density, &scene.paint, |streams| {
                        if s.committed_spec != s.spec
                            && let SurfaceRole::Layer(layer) = &s.role
                        {
                            configure_layer(layer, &s.spec);
                        }
                        // WSI presentation commits the role state, viewport,
                        // input region and newly rendered buffer together.
                        self.presentation.request(
                            surface,
                            qh,
                            s.native_generation
                                .map(|generation| (generation, s.spec.address())),
                            s.history.frames.saturating_add(1),
                            self.epoch.elapsed().as_secs_f64() * 1000.,
                            streams.copied(),
                        );
                        if let Some(trace) = &mut s.pacing {
                            trace.request(
                                s.history.frames.saturating_add(1),
                                pacing::now(self.epoch),
                            );
                        }
                        surface.frame(qh, FrameCallbackData(surface.clone()));
                        input_release_frame = self.input_release.frame(
                            surface,
                            s.spec.keyboard == api::Keyboard::None
                                && s.spec.pointer_input == api::PointerInput::None,
                            qh,
                        );
                    });
            self.input_release.submitted(
                input_release_frame,
                matches!(&outcome, Ok(RenderOutcome::Presented)),
            );
            if let Some(trace) = &mut s.pacing {
                let observed = match &outcome {
                    Ok(RenderOutcome::Presented) => pacing::Outcome::Presented,
                    Ok(RenderOutcome::RetryAfter(delay)) => pacing::Outcome::RetryAfter(*delay),
                    Err(_) => pacing::Outcome::Error,
                };
                trace.finish_attempt(pacing::now(self.epoch), observed);
            }
            self.retiring.finish_submission(&s.spec.address(), &outcome);
            match outcome {
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
            s.committed_spec = s.spec.clone();
            s.width = width;
            s.height = height;
            let started = self.profile_frames.then(Instant::now);
            if self.app.animating_instance(&s.spec.address(), now) || s.interaction.animating(now) {
                s.demand.invalidate();
            }
            let submitted_ms = self.epoch.elapsed().as_secs_f64() * 1000.;
            s.history.record(submitted_ms);
            s.presented = Some(PresentationState {
                extent: (width, height),
                buffer_scale: if s.fractional.is_some() {
                    1
                } else {
                    s.scale as i32
                },
            });
            s.scene = scene;
            submitted.push((
                s.role.wl_surface().clone(),
                s.spec.scope_event(api::Event::FrameSubmitted {
                    surface: s.spec.id,
                    width: s.width as f32,
                    height: s.height as f32,
                    now,
                }),
            ));
            if let Some(render) = s.renderer.last_profile {
                s.history.profiles.push(profiling::FrameProfile {
                    frame: s.history.frames,
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
        for (surface, message) in overlay_messages {
            if self.id_for(&surface).is_some() {
                self.message(message, qh);
            }
        }
        for (surface, event) in submitted {
            // Clipboard completions and preceding frame messages may renew or
            // hide an address. An old frame cannot authorize its replacement.
            if self.id_for(&surface).is_some()
                && let Some(message) = self.app.event(event)
            {
                self.message(message, qh);
            }
        }
        self.schedule_interaction_wakeup();
        if !self.exit
            && batching::needs_wake(
                self.surfaces
                    .values()
                    .map(|native| (native.configured, &native.demand)),
            )
        {
            self.draw_wakeup.ping();
        }
    }
    /// Dwell is a single cancellable event-loop deadline across live surfaces.
    /// It creates neither a polling subscription nor frames while waiting.
    fn schedule_interaction_wakeup(&mut self) {
        let now = self.epoch.elapsed().as_secs_f64();
        let due = self
            .surfaces
            .values()
            .filter_map(|native| native.interaction.next_wakeup(&native.scene, now))
            .min_by(f64::total_cmp);
        if self.interaction_timer.as_ref().map(|(due, _)| *due) == due {
            return;
        }
        if let Some((_, token)) = self.interaction_timer.take() {
            self.handle.remove(token);
        }
        let Some(due) = due else {
            return;
        };
        let Ok(delay) = Duration::try_from_secs_f64((due - now).max(0.)) else {
            return;
        };
        match self
            .handle
            .insert_source(Timer::from_duration(delay), move |_, _, state| {
                if state
                    .interaction_timer
                    .as_ref()
                    .is_some_and(|(expected, _)| *expected == due)
                {
                    state.interaction_timer = None;
                    // A retired/recreated native has fresh interaction state. The
                    // exact pending deadline must still belong to each live target.
                    for native in state.surfaces.values_mut() {
                        if native
                            .interaction
                            .next_wakeup(&native.scene, due.next_down())
                            == Some(due)
                        {
                            native.demand.invalidate();
                        }
                    }
                }
                TimeoutAction::Drop
            }) {
            Ok(token) => self.interaction_timer = Some((due, token)),
            Err(error) => {
                self.error = Some(error.to_string());
                self.exit = true;
            }
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
        let Some(key) = keyboard::translate(event, self.modifiers) else {
            return;
        };
        let Some(origin) = self
            .surfaces
            .get(&id)
            .map(|native| native.role.wl_surface().clone())
        else {
            return;
        };
        for message in self.refresh_input_scene(&id, false, InputRefresh::Current) {
            if self.id_for(&origin).as_ref() != Some(&id) {
                return;
            }
            self.message(message, qh);
        }
        self.clipboard_observe_focus(qh);
        if self.id_for(&origin).as_ref() != Some(&id) {
            return;
        }
        if let Some(s) = self.surfaces.get_mut(&id) {
            if !s.interaction.focus_visible {
                s.demand.invalidate();
            }
            s.interaction.focus_visible = true;
        }
        let Some((logical_id, native_surface)) = self
            .surfaces
            .get(&id)
            .map(|s| (s.spec.id, s.role.wl_surface().clone()))
        else {
            self.keyboard_surface = None;
            return;
        };
        let overlay = self
            .surfaces
            .get_mut(&id)
            .and_then(|native| native.interaction.popover_key(&native.scene, &key));
        if let Some(messages) = overlay {
            if let Some(native) = self.surfaces.get_mut(&id) {
                native.demand.invalidate();
            }
            self.clipboard_observe_focus(qh);
            for message in messages {
                if self.id_for(&native_surface).as_ref() != Some(&id) {
                    break;
                }
                self.message(message, qh);
            }
            return;
        }
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
            s.demand.invalidate();
            if before != s.interaction.focus {
                s.demand.invalidate();
            }
            self.clipboard_observe_focus(qh);
            for message in messages {
                if self.id_for(&native_surface).as_ref() != Some(&id) {
                    break;
                }
                self.message(message, qh);
            }
        }
    }
}
// The CPU scene path is shared by presentation and input refresh. It never
// begins frame demand, touches a renderer/native region, or emits FrameSubmitted.
#[derive(Clone, Copy)]
enum InputRefresh {
    /// State changed: refresh native interactive surfaces and existing editors.
    Affected,
    /// Native input or a delayed transfer must resolve the current tree now.
    Current,
}
fn update_input_scene<M: Clone>(
    interaction: &mut Interaction<M>,
    current: &mut Scene<M>,
    next: Scene<M>,
    reveal: bool,
) -> (Vec<M>, bool) {
    let focus = interaction.focus.clone();
    interaction.synchronize(&next);
    let mut messages = if reveal {
        interaction.text_caret_reveal(current, &next)
    } else {
        vec![]
    };
    while let Some(message) = interaction.take_overlay_dismissal() {
        messages.push(message);
    }
    let changed = focus != interaction.focus;
    *current = next;
    (messages, changed)
}
fn current_tree<A: Application>(
    app: &A,
    native: &Native<A::Message>,
    now: f64,
) -> (u32, u32, api::Element<A::Message>) {
    let (width, height) = layer_commit::buffer_extent(
        &native.committed_spec,
        &native.spec,
        (native.width, native.height),
    );
    let tree = app.view(&ViewContext {
        surface: native.spec.id,
        instance: native.spec.instance.clone(),
        width: width as f32,
        height: height as f32,
        now,
        density: native.density(),
    });
    (width, height, tree)
}
fn current_scene<A: Application>(
    app: &A,
    layout: &Layout,
    native: &Native<A::Message>,
    now: f64,
    refresh: InputRefresh,
) -> Option<(u32, u32, Scene<A::Message>)> {
    if matches!(refresh, InputRefresh::Affected)
        && matches!(native.spec.pointer_input, api::PointerInput::None)
        && matches!(native.spec.keyboard, api::Keyboard::None)
        && !Interaction::has_text_area(&native.scene)
        && !native.scene.overlays.has_popover()
    {
        return None;
    }
    let (width, height, tree) = current_tree(app, native, now);
    let scene = layout.build(&tree, width as f32, height as f32, &native.interaction, now);
    Some((width, height, scene))
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

    fn input_layout() -> Layout {
        Layout::new(Arc::new(vec![
            lucent_ui::text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ]))
    }

    #[test]
    fn queued_controls_use_current_actions_and_retire_removed_primary_capture() {
        let layout = input_layout();
        let button = |id, action| {
            api::Element::empty()
                .size(100., 40.)
                .id(id)
                .on_click(action)
                .autofocus()
        };
        let mut interaction = Interaction::<u32>::default();
        let mut scene = layout.build(&button("action", 1), 240., 120., &interaction, 0.);
        interaction.synchronize(&scene);
        assert!(interaction.press(&scene, 20., 20.).is_none());
        let latest = layout.build(&button("action", 2), 240., 120., &interaction, 0.);
        update_input_scene(&mut interaction, &mut scene, latest, false);
        assert_eq!(interaction.release(20., 20.), Some(2));

        assert!(interaction.press(&scene, 20., 20.).is_none());
        let replacement = layout.build(&button("replacement", 3), 240., 120., &interaction, 0.);
        let (_, changed) = update_input_scene(&mut interaction, &mut scene, replacement, false);
        assert!(changed);
        assert_eq!(interaction.focus.as_deref(), Some("replacement"));
        assert_eq!(interaction.release(20., 20.), None);
        assert_eq!(interaction.key(&scene, &api::Key::Enter), vec![3]);
    }

    #[test]
    fn ordinary_input_value_is_current_for_the_next_key_without_painting() {
        let layout = input_layout();
        let field = |value| {
            api::Element::input(value, "", std::convert::identity)
                .size(200., 48.)
                .id("field")
                .autofocus()
        };
        let mut interaction = Interaction::<String>::default();
        let mut scene = layout.build(&field("a"), 240., 120., &interaction, 0.);
        interaction.synchronize(&scene);
        let latest = layout.build(&field("server"), 240., 120., &interaction, 0.);
        update_input_scene(&mut interaction, &mut scene, latest, false);
        assert_eq!(
            interaction.key(&scene, &api::Key::Text("!".into())),
            vec!["server!"]
        );
    }

    #[test]
    fn moved_primary_target_and_new_popover_retire_queued_release() {
        let layout = input_layout();
        let button = |x| {
            api::Element::stack(vec![
                api::Element::empty()
                    .size(60., 40.)
                    .at(x, 0.)
                    .id("action")
                    .on_click(1_u32)
                    .autofocus(),
            ])
        };
        let mut interaction = Interaction::default();
        let mut scene = layout.build(&button(0.), 240., 160., &interaction, 0.);
        interaction.synchronize(&scene);
        assert!(interaction.press(&scene, 20., 20.).is_none());
        let moved = layout.build(&button(80.), 240., 160., &interaction, 0.);
        update_input_scene(&mut interaction, &mut scene, moved, false);
        assert_eq!(interaction.release(20., 20.), None);

        assert!(interaction.press(&scene, 100., 20.).is_none());
        let popover = button(80.).popover(api::Popover {
            id: "menu".into(),
            anchor: "action".into(),
            placement: api::AnchoredSpec {
                side: api::OverlaySide::Below,
                align: api::OverlayAlign::Start,
                gap: 0.,
                inset: 0.,
            },
            dismiss: 0,
            content: Box::new(
                api::Element::empty()
                    .size(80., 40.)
                    .id("menu-action")
                    .on_click(2),
            ),
        });
        let scoped = layout.build(&popover, 240., 160., &interaction, 0.);
        update_input_scene(&mut interaction, &mut scene, scoped, false);
        assert_eq!(interaction.release(100., 20.), None);
        assert_eq!(interaction.focus.as_deref(), Some("menu-action"));
    }

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
                    density: s.density(),
                });
                if let Some(message) = self.app.event(event) {
                    self.message(message, qh);
                }
            }
        }
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
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        callback_data_ms: u32,
    ) {
        let callback_at = self.profile_frames.then(|| pacing::now(self.epoch));
        if let Some(id) = self.id_for(surface)
            && let Some(s) = self.surfaces.get_mut(&id)
        {
            if let (Some(trace), Some(at)) = (&mut s.pacing, callback_at) {
                trace.callback(at, callback_data_ms);
            }
            s.demand.ready();
        } else {
            self.pacing.retired_callback();
        }
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
                density: s.density(),
            });
            let mut effects = Effects {
                now: self.epoch.elapsed().as_secs_f64(),
                ..Default::default()
            };
            // The runtime owns this configure invalidation even when the client
            // consumes Resize without requesting redraw. Route it through the
            // same post-reconcile CPU reveal path before any new presentation.
            effects.redraw_surface(id);
            self.messages_with_effects(self.app.event(event), effects, qh);
        }
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
        self.retiring
            .retain(|_, native| native.bound_output.as_ref() != Some(&output));
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
                self.input_method.seat(&seat, qh);
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
            self.input_serial = None;
            self.activation.input = None;
            self.input_method_remove_seat(qh);
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
            // A wl_pointer.frame may contain several distinct native events.
            // Reconciliation for the next event must not borrow a preceding
            // button's unused authority (including when it is Motion/Leave).
            self.input_serial = None;
            self.activation.input = None;
            let Some(id) = self.id_for(&event.surface) else {
                continue;
            };
            if matches!(event.kind, PointerEventKind::Press { .. }) {
                self.input_method_cancel_preedit(qh);
            }
            for message in self.refresh_input_scene(&id, false, InputRefresh::Current) {
                if self.id_for(&event.surface).as_ref() != Some(&id) {
                    break;
                }
                self.message(message, qh);
            }
            self.clipboard_observe_focus(qh);
            if self.id_for(&event.surface).as_ref() != Some(&id) {
                continue;
            }
            let now = self.epoch.elapsed().as_secs_f64();
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let mut messages = vec![];
            let mut outside = false;
            let mut scroll = None;
            self.input_serial = match event.kind {
                PointerEventKind::Press { serial, .. } => Some(serial),
                _ => None,
            };
            self.activation.input = match event.kind {
                PointerEventKind::Press { serial, .. }
                | PointerEventKind::Release { serial, .. } => {
                    self.pointer_seat.as_ref().map(|seat| activation::Input {
                        surface: event.surface.clone(),
                        seat: seat.clone(),
                        serial,
                    })
                }
                _ => None,
            };
            if let Some(s) = self.surfaces.get_mut(&id) {
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        messages.extend(s.interaction.motion(&s.scene, x, y, now))
                    }
                    PointerEventKind::Leave { .. } => s.interaction.leave(now),
                    PointerEventKind::Press { button, .. } => {
                        let button = pointer_input::button(button);
                        outside = button == api::MouseButton::Primary
                            && !s.scene.hits.iter().any(|h| h.contains(x, y));
                        if let Some(message) = s.interaction.press_button(
                            &s.scene,
                            x,
                            y,
                            button,
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
                    PointerEventKind::Release { button, .. } => {
                        if let Some(m) =
                            s.interaction
                                .release_button(x, y, pointer_input::button(button))
                        {
                            messages.push(m);
                        }
                    }
                    PointerEventKind::Axis {
                        horizontal,
                        vertical,
                        ..
                    } => {
                        let wheel = api::WheelEvent {
                            horizontal: pointer_input::lines(horizontal),
                            vertical: pointer_input::lines(vertical),
                        };
                        if let Some(emitted) = s.interaction.wheel(&s.scene, x, y, wheel) {
                            messages.extend(emitted);
                        } else if wheel.vertical.is_finite() && wheel.vertical != 0. {
                            scroll = Some(wheel.vertical);
                        }
                    }
                }
                s.demand.invalidate();
            }
            self.clipboard_observe_focus(qh);
            if self.id_for(&event.surface).as_ref() != Some(&id) {
                continue;
            }
            let Some(logical_id) = self.surfaces.get(&id).map(|native| native.spec.id) else {
                continue;
            };
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
                let button = pointer_input::button(button);
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
                if self.id_for(&event.surface).as_ref() != Some(&id) {
                    break;
                }
                self.message(m, qh);
            }
        }
        self.input_serial = None;
        self.activation.input = None;
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
        let Some(id) = self.id_for(surface) else {
            return;
        };
        self.clipboard_cancel(qh, false);
        // Cancellation completes client requests synchronously. Those updates
        // may hide or renew this address; only this exact native object's Enter
        // can grant authority to the still-current lifetime.
        if self.id_for(surface).as_ref() == Some(&id) {
            self.keyboard_surface = Some(id);
            self.input_method_observe();
        }
    }
    fn leave(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        let Some(id) = self.id_for(surface) else {
            return;
        };
        if self.keyboard_surface.as_ref() != Some(&id) {
            return;
        }
        self.keyboard_surface = None;
        self.input_serial = None;
        self.input_method_observe();
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
        self.activation.input = self
            .keyboard_surface
            .as_ref()
            .and_then(|id| self.surfaces.get(id))
            .zip(self.keyboard_seat.as_ref())
            .map(|(native, seat)| activation::Input {
                surface: native.role.wl_surface().clone(),
                seat: seat.clone(),
                serial,
            });
        self.key(event, qh);
        self.input_serial = None;
        self.activation.input = None;
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
        _: &QueueHandle<Self>,
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
    }
}
