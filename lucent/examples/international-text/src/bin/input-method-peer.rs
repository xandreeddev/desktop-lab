//! Owned native acceptance peer; never grabs keyboard input or replaces another IME.
use calloop::{EventLoop, channel};
use calloop_wayland_source::WaylandSource;
use serde_json::{Value, json};
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::os::fd::AsFd;
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_region::WlRegion,
        wl_registry,
        wl_seat::WlSeat,
        wl_shm::{Format, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
    zwp_input_popup_surface_v2::{self, ZwpInputPopupSurfaceV2},
};
struct State {
    method: ZwpInputMethodV2,
    popup: ZwpInputPopupSurfaceV2,
    surface: WlSurface,
    marker_buffer: WlBuffer,
    popup_buffer_committed: bool,
    manager: ZwpInputMethodManagerV2,
    active: bool,
    pending_active: bool,
    unavailable: bool,
    serial: u32,
    text: String,
    cursor: u32,
    anchor: u32,
    rect: Option<(i32, i32, i32, i32)>,
    rectangle_updates: u64,
    exit: bool,
}
// Acceptance barcode only: opaque 8×8 pixels, not a shell component or IME UI.
const MARKER_ROWS: [u8; 8] = [0xa7, 0xd2, 0x19, 0xec, 0x56, 0x83, 0x6d, 0xb8];
fn marker_pixels() -> [u8; 256] {
    let mut pixels = [0; 256];
    for (y, row) in MARKER_ROWS.into_iter().enumerate() {
        for x in 0..8 {
            let argb: u32 = if row & (1 << (7 - x)) != 0 {
                0xffff_ffff
            } else {
                0xff00_0000
            };
            pixels[(y * 8 + x) * 4..(y * 8 + x + 1) * 4].copy_from_slice(&argb.to_ne_bytes());
        }
    }
    pixels
}
fn emit(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}
impl State {
    fn command(&mut self, command: Value) {
        let id = command.get("id").and_then(Value::as_u64).unwrap_or(0);
        if command["action"] == "quit" {
            self.exit = true;
            emit(json!({"id":id,"ok":true}));
            return;
        }
        if command["action"] == "status" {
            emit(
                json!({"id":id,"ok":true,"active":self.active,"unavailable":self.unavailable,"serial":self.serial,"bytes":self.text.len(),"cursor":self.cursor,"anchor":self.anchor,"rectangle":self.rect,"rectangle_updates":self.rectangle_updates,"popup_buffer_committed":self.popup_buffer_committed,"matches":command.get("expected").and_then(Value::as_str).is_some_and(|expected|expected==self.text)}),
            );
            return;
        }
        let guarded = self.active
            && !self.unavailable
            && command.get("expected").and_then(Value::as_str) == Some(&self.text)
            && command.get("cursor").and_then(Value::as_u64) == Some(u64::from(self.cursor))
            && command.get("anchor").and_then(Value::as_u64) == Some(u64::from(self.anchor));
        if !guarded {
            emit(json!({"id":id,"ok":false,"error":"current_owned_fixture_text_required"}));
            return;
        }
        let preedit = command.get("preedit").and_then(Value::as_str);
        let commit = command.get("commit").and_then(Value::as_str);
        if preedit.is_some_and(|text| text.len() > 4000)
            || commit.is_some_and(|text| text.len() > 4000)
        {
            emit(json!({"id":id,"ok":false,"error":"bounded_native_text_required"}));
            return;
        }
        let endpoint = |key: &str, text: &str| -> Option<i32> {
            let value = command
                .get(key)
                .and_then(Value::as_i64)
                .unwrap_or(text.len() as i64);
            if value == -1 {
                return Some(-1);
            }
            let value = usize::try_from(value).ok()?;
            (value <= text.len() && text.is_char_boundary(value)).then_some(value as i32)
        };
        let endpoints = if let Some(text) = preedit {
            endpoint("begin", text).zip(endpoint("end", text))
        } else {
            Some((0, 0))
        };
        let delete = if let Some(delete) = command.get("delete") {
            delete
                .get(0)
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .zip(
                    delete
                        .get(1)
                        .and_then(Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok()),
                )
        } else {
            Some((0, 0))
        };
        let Some((begin, end)) = endpoints else {
            emit(json!({"id":id,"ok":false,"error":"valid_preedit_cursor_required"}));
            return;
        };
        let Some((before, after)) = delete.filter(|(before, after)| {
            *before <= self.cursor.min(self.anchor)
                && *after as usize <= self.text.len() - self.cursor.max(self.anchor) as usize
        }) else {
            emit(json!({"id":id,"ok":false,"error":"bounded_delete_required"}));
            return;
        };
        if let Some(text) = preedit {
            self.method.set_preedit_string(text.to_owned(), begin, end);
        }
        if let Some(text) = commit {
            self.method.commit_string(text.to_owned());
        }
        if before != 0 || after != 0 {
            self.method.delete_surrounding_text(before, after);
        }
        self.method.commit(self.serial);
        emit(json!({"id":id,"ok":true,"posted":true,"serial":self.serial}));
    }
}
impl Dispatch<ZwpInputMethodV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_input_method_v2::Event::Activate => {
                state.pending_active = true;
                state.text.clear();
                state.cursor = 0;
                state.anchor = 0;
            }
            zwp_input_method_v2::Event::Deactivate => state.pending_active = false,
            zwp_input_method_v2::Event::SurroundingText {
                text,
                cursor,
                anchor,
            } => {
                state.text = text;
                state.cursor = cursor;
                state.anchor = anchor;
            }
            zwp_input_method_v2::Event::Done => {
                state.serial = state.serial.wrapping_add(1);
                state.active = state.pending_active;
                let map_popup = state.active && !state.unavailable;
                if map_popup != state.popup_buffer_committed {
                    state
                        .surface
                        .attach(map_popup.then_some(&state.marker_buffer), 0, 0);
                    state.surface.damage(0, 0, 8, 8);
                    state.surface.commit();
                    state.popup_buffer_committed = map_popup;
                    if !map_popup {
                        state.rect = None;
                    }
                }
                emit(
                    json!({"event":"state","active":state.active,"serial":state.serial,"bytes":state.text.len(),"cursor":state.cursor,"anchor":state.anchor}),
                );
            }
            zwp_input_method_v2::Event::Unavailable => {
                state.unavailable = true;
                state.exit = true;
                emit(json!({"event":"unavailable","error":"existing_input_method_preserved"}));
            }
            _ => {}
        }
    }
}
impl Dispatch<ZwpInputPopupSurfaceV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpInputPopupSurfaceV2,
        event: zwp_input_popup_surface_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_input_popup_surface_v2::Event::TextInputRectangle {
            x,
            y,
            width,
            height,
        } = event
        {
            state.rect = Some((x, y, width, height));
            state.rectangle_updates = state.rectangle_updates.saturating_add(1);
        }
    }
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The fixture deliberately binds one static seat. A registry retirement
        // revokes admission rather than silently rebinding to another owner.
        if matches!(event, wl_registry::Event::GlobalRemove { .. }) {
            state.active = false;
            state.unavailable = true;
            state.exit = true;
            emit(json!({"event":"retired","error":"native_registry_changed"}));
        }
    }
}
wayland_client::delegate_noop!(State:WlCompositor);
// Seat capabilities and surface enter/leave are legitimate fixture events;
// the eventless form of delegate_noop! would panic instead of ignoring them.
wayland_client::delegate_noop!(State:ignore WlSurface);
wayland_client::delegate_noop!(State:ignore WlSeat);
wayland_client::delegate_noop!(State:ignore WlShm);
wayland_client::delegate_noop!(State:ignore WlBuffer);
wayland_client::delegate_noop!(State:WlShmPool);
wayland_client::delegate_noop!(State:WlRegion);
wayland_client::delegate_noop!(State:ZwpInputMethodManagerV2);
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init::<State>(&connection)?;
    let qh = queue.handle();
    let seats = globals.contents().with_list(|list| {
        list.iter()
            .filter(|global| global.interface == "wl_seat")
            .count()
    });
    if seats != 1 {
        return Err("one explicit fixture seat required".into());
    }
    let seat: WlSeat = globals.bind(&qh, 1..=1, ())?;
    let manager: ZwpInputMethodManagerV2 = globals.bind(&qh, 1..=1, ())?;
    let compositor: WlCompositor = globals.bind(&qh, 1..=1, ())?;
    let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
    let method = manager.get_input_method(&seat, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let popup = method.get_input_popup_surface(&surface, &qh, ());
    let mut marker_memory = File::from(rustix::fs::memfd_create(
        "lucent-owned-ime-marker",
        rustix::fs::MemfdFlags::CLOEXEC,
    )?);
    marker_memory.write_all(&marker_pixels())?;
    let pool = shm.create_pool(marker_memory.as_fd(), 256, &qh, ());
    let marker_buffer = pool.create_buffer(0, 8, 8, 32, Format::Argb8888, &qh, ());
    let empty_input = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&empty_input));
    empty_input.destroy();
    let mut state = State {
        method,
        popup,
        surface,
        marker_buffer,
        popup_buffer_committed: false,
        manager,
        active: false,
        pending_active: false,
        unavailable: false,
        serial: 0,
        text: String::new(),
        cursor: 0,
        anchor: 0,
        rect: None,
        rectangle_updates: 0,
        exit: false,
    };
    let mut event_loop = EventLoop::<State>::try_new()?;
    WaylandSource::new(connection, queue).insert(event_loop.handle())?;
    let (sender, receiver) = channel::channel::<Value>();
    event_loop
        .handle()
        .insert_source(receiver, |event, _, state| match event {
            channel::Event::Msg(value) => state.command(value),
            channel::Event::Closed => state.exit = true,
        })?;
    std::thread::spawn(move || {
        let mut reader = io::stdin().lock();
        loop {
            let mut line = Vec::new();
            let result = std::io::Read::take(&mut reader, 16_385).read_until(b'\n', &mut line);
            let Ok(count) = result else {
                break;
            };
            if count == 0 || line.len() > 16_384 {
                break;
            }
            if let Ok(value) = serde_json::from_slice(&line)
                && sender.send(value).is_err()
            {
                break;
            }
        }
    });
    emit(json!({"event":"ready","scope":"controlled_input_method_fixture_only"}));
    while !state.exit {
        event_loop.dispatch(None, &mut state)?;
    }
    state.popup.destroy();
    state.surface.destroy();
    state.marker_buffer.destroy();
    pool.destroy();
    state.method.destroy();
    state.manager.destroy();
    Ok(())
}
