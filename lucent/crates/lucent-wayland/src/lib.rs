//! Event-driven native layer surfaces. Applications do not handle raw Wayland objects.
use lucent_domain::{FrameDemand, Point, Release, Widget};
use lucent_render::{Error, Renderer};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
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
use std::{path::PathBuf, time::Instant};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
};

/// Open a transparent widget host with a draggable card and Vulkan animations.
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
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_exclusive_zone(-1);
    layer.set_size(0, 0);
    // Do not capture the whole desktop while the first frame is being prepared.
    let empty = Region::new(&compositor)?;
    layer.wl_surface().set_input_region(Some(empty.wl_region()));
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
            conn.backend(),
            layer.wl_surface().id().as_ptr().cast(),
            font,
        )
    }?;
    let state_file = position_file();
    let saved = state_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| parse_position(&s));
    let mut state = State {
        renderer,
        layer,
        compositor,
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        pointer: None,
        exit: false,
        width: 1,
        height: 1,
        scale: 1,
        demand: FrameDemand::default(),
        widget: Widget::new(saved),
        epoch: Instant::now(),
        state_file,
        input_bounds: None,
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
        state.renderer.frames, state.widget.clicks
    );
    Ok(())
}

// Drop order is intentional: GPU before the native Wayland layer surface.
struct State {
    renderer: Renderer,
    layer: LayerSurface,
    compositor: CompositorState,
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    pointer: Option<wl_pointer::WlPointer>,
    exit: bool,
    width: u32,
    height: u32,
    scale: u32,
    demand: FrameDemand,
    widget: Widget,
    epoch: Instant,
    state_file: Option<PathBuf>,
    input_bounds: Option<[f32; 4]>,
    title: String,
    configured: bool,
    error: Option<String>,
}
impl State {
    fn redraw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || !self.demand.begin() {
            return;
        }
        let now = self.epoch.elapsed().as_secs_f64();
        let card = self.widget.visual(now);
        let bounds = card.bounds();
        if self.input_bounds != Some(bounds) {
            if let Err(error) = self.set_input_region(bounds) {
                self.error = Some(error.to_string());
                self.exit = true;
                return;
            }
            self.input_bounds = Some(bounds);
        }
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(self.scale as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if let Err(error) = self.renderer.render(
            (self.width * self.scale, self.height * self.scale),
            self.scale,
            &self.title,
            self.widget.clicks,
            card,
        ) {
            self.error = Some(error.to_string());
            self.exit = true;
        }
        // Queue a final settled frame too. Once it arrives no further callbacks
        // are requested unless a new input/configure event invalidates the scene.
        if self.widget.animating(now) {
            self.demand.invalidate();
        }
        if self.widget.finished(now) {
            self.exit = true;
        }
    }
    fn set_input_region(&self, bounds: [f32; 4]) -> Result<(), Error> {
        let [x, y, w, h] = bounds;
        let radius = 26.0 * w / lucent_domain::CARD_WIDTH;
        let region = Region::new(&self.compositor)?;
        let top = y.floor() as i32;
        let bottom = (y + h).ceil() as i32;
        // Rounded input mask: even the transparent corners pass clicks through.
        for row in top..bottom {
            let edge = ((row as f32 + 0.5 - y - h / 2.0).abs() - (h / 2.0 - radius)).max(0.0);
            let inset = radius - (radius * radius - edge * edge).max(0.0).sqrt();
            let left = (x + inset).ceil() as i32;
            let right = (x + w - inset).floor() as i32;
            if right > left {
                region.add(left, row, right - left, 1);
            }
        }
        self.layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
        Ok(())
    }
    fn save_position(&self) {
        let p = self.widget.position;
        eprintln!("lucent position {:.1} {:.1}", p.x, p.y);
        if let Some(path) = &self.state_file {
            let save = || -> std::io::Result<()> {
                std::fs::create_dir_all(path.parent().unwrap())?;
                let temporary = path.with_extension("tmp");
                std::fs::write(&temporary, format!("v1 {} {}\n", p.x, p.y))?;
                std::fs::rename(temporary, path)
            };
            if let Err(error) = save() {
                eprintln!("lucent could not save position: {error}");
            }
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
        self.widget.configure(
            self.width as f32,
            self.height as f32,
            self.epoch.elapsed().as_secs_f64(),
        );
        eprintln!(
            "lucent configured {} {} position {:.1} {:.1}",
            self.width, self.height, self.widget.position.x, self.widget.position.y
        );
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
        qh: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
            self.widget.cancel_drag(self.epoch.elapsed().as_secs_f64());
            self.demand.invalidate();
            self.redraw(qh);
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
            let now = self.epoch.elapsed().as_secs_f64();
            let point = Point::new(event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } => self.widget.hover(true, now),
                PointerEventKind::Leave { .. } => self.widget.hover(false, now),
                PointerEventKind::Motion { .. } => self.widget.motion(point),
                PointerEventKind::Press { button: 0x110, .. } => self.widget.press(point, now),
                PointerEventKind::Release { button: 0x110, .. } => {
                    // Include the release coordinates if motion was coalesced.
                    self.widget.motion(point);
                    match self.widget.release(now) {
                        Release::Dragged => self.save_position(),
                        Release::Clicked => eprintln!("lucent click {}", self.widget.clicks),
                        Release::Ignored => {}
                    }
                }
                PointerEventKind::Press { button: 0x111, .. } => self.widget.close(now),
                _ => continue,
            }
            self.demand.invalidate();
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

fn position_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))?;
    Some(base.join("lucent/position"))
}
fn parse_position(text: &str) -> Option<Point> {
    let fields: Vec<_> = text.split_whitespace().collect();
    if fields.len() != 3 || fields[0] != "v1" {
        return None;
    }
    let p = Point::new(fields[1].parse().ok()?, fields[2].parse().ok()?);
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_format_rejects_corrupt_or_future_state() {
        assert_eq!(
            parse_position("v1 120.5 200\n"),
            Some(Point::new(120.5, 200.0))
        );
        for bad in [
            "v2 1 2", "v1 NaN 2", "v1 1 inf", "v1 1", "v1 1 2 3", "broken",
        ] {
            assert_eq!(parse_position(bad), None);
        }
    }
}
