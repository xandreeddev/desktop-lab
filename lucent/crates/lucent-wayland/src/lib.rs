//! Event-driven native layer surfaces. Applications do not handle raw Wayland objects.
use lucent_domain::{Counter, FrameDemand};
use lucent_render::{Error, Renderer};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
};

/// Open one centered floating prototype surface. Left-click updates it, right-click exits.
/// The initial milestone deliberately preserves the existing desktop and secure locker.
pub fn run(title: &str) -> Result<(), Error> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("lucent-prototype"),
        None,
    );
    layer.set_anchor(Anchor::empty());
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_exclusive_zone(0);
    layer.set_size(540, 190);
    layer.commit();
    let font_path = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif"])
        .output()?;
    if !font_path.status.success() {
        return Err("fontconfig could not select a font".into());
    }
    let font = std::fs::read(String::from_utf8(font_path.stdout)?.trim())?;
    // SAFETY: state drops renderer before layer; conn was declared before state and
    // remains alive until after state and its native surface have been destroyed.
    let renderer = unsafe {
        Renderer::new(
            conn.backend().display_ptr().cast(),
            layer.wl_surface().id().as_ptr().cast(),
            font,
        )
    }?;
    let mut state = State {
        renderer,
        layer,
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        pointer: None,
        exit: false,
        width: 540,
        height: 190,
        scale: 1,
        demand: FrameDemand::default(),
        counter: Counter::default(),
        title: title.into(),
        configured: false,
        error: None,
    };
    while !state.exit {
        queue.blocking_dispatch(&mut state)?;
    }
    conn.flush()?;
    if let Some(error) = state.error {
        return Err(error.into());
    }
    eprintln!(
        "lucent closed cleanly; {} frame(s), {} click(s)",
        state.renderer.frames, state.counter.clicks
    );
    Ok(())
}

// Drop order is intentional: GPU before the native Wayland layer surface.
struct State {
    renderer: Renderer,
    layer: LayerSurface,
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    pointer: Option<wl_pointer::WlPointer>,
    exit: bool,
    width: u32,
    height: u32,
    scale: u32,
    demand: FrameDemand,
    counter: Counter,
    title: String,
    configured: bool,
    error: Option<String>,
}
impl State {
    fn redraw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || !self.demand.begin() {
            return;
        }
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(self.scale as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if let Err(error) = self.renderer.render(
            self.width * self.scale,
            self.height * self.scale,
            self.scale,
            &self.title,
            self.counter.clicks,
        ) {
            self.error = Some(error.to_string());
            self.exit = true;
        }
    }
}
impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        self.scale = factor.max(1) as u32;
        self.demand.invalidate();
        self.redraw(qh);
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.demand.ready();
        self.redraw(qh);
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &LayerSurface,
        config: LayerSurfaceConfigure,
        _: u32,
    ) {
        if config.new_size.0 > 0 {
            self.width = config.new_size.0;
        }
        if config.new_size.1 > 0 {
            self.height = config.new_size.1;
        }
        self.configured = true;
        self.demand.invalidate();
        self.redraw(qh);
    }
}
impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl SeatHandler for State {
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
            match self.seats.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.exit = true;
                }
            }
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
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}
impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            if let PointerEventKind::Press { button, .. } = event.kind {
                if button == 0x111 {
                    self.exit = true;
                } else if button == 0x110 {
                    self.counter.click();
                    self.demand.invalidate();
                }
            }
        }
        self.redraw(qh);
    }
}
delegate_registry!(State);
impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(State);
