//! Widget presentation borrows snapshots and prepared artwork, never the desktop root.
use super::{Host, Message, surfaces, widget_size};
use crate::{artwork::Artwork, persistence::Status};
use lucent_api::*;
use lucent_design::*;
use lucent_domain::{ClockSnapshot, Date, SystemSnapshot, TimerPhase, WidgetLayout};

#[derive(Clone, Copy)]
pub struct Input<'a> {
    pub layout: &'a WidgetLayout,
    pub notes: &'a str,
    pub save_status: Status,
    pub clock: &'a ClockSnapshot,
    pub system: &'a SystemSnapshot,
    pub network_status: &'a str,
    pub media: &'a crate::media::Media,
    pub weather: &'a crate::weather::Weather,
}
pub struct View<'a> {
    pub host: &'a Host,
    pub input: Input<'a>,
    pub artwork: Artwork<'a>,
}
impl View<'_> {
    fn theme(&self) -> Theme {
        self.artwork.theme
    }
    fn surface_color(&self) -> Color {
        self.theme().surface
    }
    fn widget_color(&self) -> Color {
        self.theme().surface_container
    }
    fn ink(&self) -> Color {
        self.theme().on_surface
    }
    fn accent(&self) -> Color {
        self.theme().primary
    }
    fn label(&self, text: impl Into<String>, size: f32) -> Element<Message> {
        Element::text(text).font(size).color(self.ink())
    }
    fn button(&self, text: impl Into<String>, message: Message) -> Element<Message> {
        self.theme().button(text, message)
    }
    fn icon(&self, name: &str, size: f32) -> Element<Message> {
        self.artwork.icon(name, size)
    }
    fn icon_button(&self, id: &str, name: &str, message: Message) -> Element<Message> {
        self.artwork.icon_button(id, name, message)
    }
}
impl View<'_> {
    /// Decorative geometry only: grid lines never enter the pointer input region.
    fn drag_grid(&self, cx: &ViewContext) -> Vec<Element<Message>> {
        if !self.host.grid_visible() {
            return vec![];
        }
        let step = component::widget_layout::GRID_STEP;
        let thickness = component::widget_grid::LINE_WIDTH;
        let mut lines = Vec::new();
        for (extent, vertical) in [(cx.width, true), (cx.height, false)] {
            for index in 0..(extent / step).ceil() as usize {
                let position = index as f32 * step;
                let major = index.is_multiple_of(component::widget_grid::MAJOR_EVERY as usize);
                let color = self.theme().on_surface.alpha(if major {
                    component::widget_grid::MAJOR_OPACITY
                } else {
                    component::widget_grid::MINOR_OPACITY
                });
                lines.push(
                    Element::empty()
                        .size(
                            if vertical { thickness } else { cx.width },
                            if vertical { cx.height } else { thickness },
                        )
                        .at(
                            if vertical { position } else { 0. },
                            if vertical { 0. } else { position },
                        )
                        .background(color),
                );
            }
        }
        lines
    }
    fn card(
        &self,
        id: &str,
        cx: &ViewContext,
        pos: (f32, f32),
        viewport: (f32, f32),
        source: bool,
    ) -> Element<Message> {
        let size = self
            .host
            .size(id, self.input.layout, viewport, source)
            .tuple();
        let editor_height = super::sizing::editor_height(self.host.editing());
        let base = widget_size(id);
        // Notes uses the actual resized card, rather than a fixed editor
        // centered inside a larger decorative shell. Other catalog views keep
        // their established base layouts.
        let content_size = if matches!(id, "notes" | "weather") {
            size
        } else {
            base
        };
        let content_context = ViewContext {
            width: content_size.0,
            height: content_size.1,
            ..cx.clone()
        };
        let content = super::catalog::registry()
            .view(id, self, &content_context)
            .expect("catalog members have a registered view");
        let mut children = vec![content.size(content_size.0, content_size.1).at(
            (size.0 - content_size.0) / 2.,
            (size.1 - content_size.1) / 2.,
        )];
        if self.host.editing() {
            let grip = component::widget_resize::HANDLE_SIZE;
            let inset = component::widget_resize::INSET;
            let resize_id = id.to_owned();
            children.push(
                self.icon("resize", icon::SMALL)
                    .padding(space::XS)
                    .size(grip, grip)
                    .align(Align::Center)
                    .at(size.0 - grip - inset, size.1 + inset)
                    .background(self.theme().surface_container)
                    .radius(radius::CONTROL)
                    .on_drag(move |event| Message::Resize(resize_id.clone(), event))
                    .tab_stop(false)
                    .id(format!("widget-resize-{id}")),
            );
            for (index, (label, delta)) in [("−", -1), ("+", 1)].into_iter().enumerate() {
                children.push(
                    self.button(label, Message::SizeStep(id.into(), delta))
                        .size(grip, grip)
                        .padding(component::widget_resize::BUTTON_PADDING)
                        .at(inset + index as f32 * (grip + space::XS), size.1 + inset)
                        .id(format!("widget-size-{id}-{delta}")),
                );
            }
        }
        let drag_id = id.to_owned();
        let press_id = id.to_owned();
        Element::stack(children)
            .size(size.0, size.1 + editor_height)
            .at(pos.0, pos.1)
            .background(if id == "calendar" || id == "media" {
                self.surface_color()
            } else {
                self.widget_color()
            })
            .radius(radius::WIDGET)
            .clip()
            .shadow()
            .on_pointer_press(move |press| Message::PressMember(press_id.clone(), press))
            .on_drag(move |drag| Message::Move(drag_id.clone(), drag))
            .id(format!("widget-{id}"))
    }
    /// Full display preview while dragging; the same composition is used by
    /// offscreen snapshots. Its native pointer policy is pass-through.
    pub fn render(&self, cx: &ViewContext) -> Element<Message> {
        let mut children = self.drag_grid(cx);
        children.extend(surfaces::visible(self.input.layout).iter().map(|id| {
            let pos = self.host.preview_position(id).unwrap_or_else(|| {
                surfaces::position(
                    id,
                    self.input.layout,
                    (cx.width, cx.height),
                    self.host.editing(),
                )
            });
            self.card(id, cx, (pos.x(), pos.y()), (cx.width, cx.height), false)
        }));
        Element::stack(children).fill()
    }
    /// Bounded source buffers use display positions translated into their own
    /// local coordinates. During capture they retain hits but suppress paint.
    pub fn render_native(&self, cx: &ViewContext, viewport: (f32, f32)) -> Element<Message> {
        let Some(source) = self
            .host
            .surfaces(self.input.layout, viewport)
            .into_iter()
            .find(|source| source.id == cx.surface)
        else {
            return Element::empty();
        };
        let children = source
            .members
            .iter()
            .map(|id| {
                let pos = self.host.source_position(id, self.input.layout, viewport);
                self.card(
                    id,
                    cx,
                    (pos.x() - source.rect.x, pos.y() - source.rect.y),
                    viewport,
                    true,
                )
            })
            .collect();
        let tree = Element::stack(children).fill();
        if self.host.grid_visible() {
            tree.input_only()
        } else {
            tree
        }
    }
}
pub(super) fn clock(d: &View<'_>, _: &ViewContext) -> Element<Message> {
    Element::stack(vec![
        d.label(format!("{:02}", d.input.clock.hour), font::CLOCK)
            .font_face(1)
            .size(component::clock::WIDTH, component::clock::DIGIT_HEIGHT)
            .align(Align::Center)
            .at(0., component::clock::HOUR_TOP),
        d.label(format!("{:02}", d.input.clock.minute), font::CLOCK)
            .font_face(1)
            .size(component::clock::WIDTH, component::clock::DIGIT_HEIGHT)
            .align(Align::Center)
            .at(0., component::clock::MINUTE_TOP),
        d.label(
            format!(
                "{}  ·  {} {}",
                [
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                    "Sunday"
                ][d.input.clock.date.weekday() as usize],
                d.input.clock.date.day(),
                months()[d.input.clock.date.month() as usize - 1]
            ),
            font::SMALL,
        )
        .size(component::clock::WIDTH, component::clock::DATE_HEIGHT)
        .align(Align::Center)
        .at(0., component::clock::DATE_TOP),
    ])
    .fill()
}
fn months() -> [&'static str; 12] {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
}
/// Advance a calendar month while preserving the real weekday of its first day.
fn calendar_month(date: Date, offset: i32) -> (i32, u32, u32) {
    let month = date.month_start(offset).unwrap_or(date);
    (month.year(), month.month(), month.first_weekday())
}
pub(super) fn calendar(d: &View<'_>, _: &ViewContext) -> Element<Message> {
    let date = d.input.clock.date;
    let (year, month, first) = calendar_month(date, d.host.month_offset);
    let mut children = vec![
        d.label(
            format!("{} {}", months()[month as usize - 1], year),
            font::LABEL,
        )
        .at(component::calendar::LEFT, component::calendar::HEADING_TOP)
        .size(
            component::calendar::HEADING_WIDTH,
            component::calendar::ROW_HEIGHT,
        ),
        d.icon_button("calendar-previous", "left", Message::Month(-1))
            .at(
                component::calendar::PREVIOUS_LEFT,
                component::calendar::NAVIGATION_TOP,
            ),
        d.icon_button("calendar-next", "right", Message::Month(1))
            .at(
                component::calendar::NEXT_LEFT,
                component::calendar::NAVIGATION_TOP,
            ),
    ];
    for (i, day) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        children.push(
            d.label(*day, font::CAPTION)
                .size(
                    component::calendar::COLUMN_WIDTH,
                    component::calendar::WEEKDAY_HEIGHT,
                )
                .align(Align::Center)
                .at(
                    component::calendar::WEEKDAY_LEFT
                        + i as f32 * component::calendar::COLUMN_WIDTH,
                    component::calendar::WEEKDAY_TOP,
                )
                .opacity(opacity::SECONDARY),
        );
    }
    let days = Date::days_in_month(year, month);
    let prev_month = if month == 1 { 12 } else { month - 1 };
    let prev_days = if month == 1 {
        31
    } else {
        Date::days_in_month(year, prev_month)
    };
    for index in 0..42_u32 {
        let day = index as i32 - first as i32 + 1;
        let current = day > 0 && day <= days as i32;
        let displayed = if day <= 0 {
            prev_days as i32 + day
        } else if day > days as i32 {
            day - days as i32
        } else {
            day
        };
        let today = current && d.host.month_offset == 0 && day == date.day() as i32;
        let x = component::calendar::LEFT + (index % 7) as f32 * component::calendar::COLUMN_WIDTH;
        let y =
            component::calendar::DAYS_TOP + (index / 7) as f32 * component::calendar::ROW_HEIGHT;
        if today {
            let petals = (0..6)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 6.;
                    Element::empty()
                        .size(
                            component::calendar::PETAL_SIZE,
                            component::calendar::PETAL_SIZE,
                        )
                        .at(
                            x + component::calendar::PETAL_LEFT
                                + a.cos() * component::calendar::PETAL_ORBIT,
                            y + component::calendar::PETAL_TOP
                                + a.sin() * component::calendar::PETAL_ORBIT,
                        )
                        .radius(radius::SMALL)
                        .background(d.accent())
                })
                .collect();
            children.push(Element::stack(petals).fill());
        }
        children.push(
            d.label(displayed.to_string(), font::SMALL)
                .size(
                    component::calendar::DAY_WIDTH,
                    component::calendar::DAY_HEIGHT,
                )
                .align(Align::Center)
                .color(if today {
                    d.theme().on_primary
                } else {
                    d.ink().alpha(if current { 1. } else { opacity::DISABLED })
                })
                .at(x, y),
        );
    }
    Element::stack(children).fill()
}
pub(super) fn weather(d: &View<'_>, cx: &ViewContext) -> Element<Message> {
    d.input
        .weather
        .widget(cx, d.theme(), |name, size| {
            d.icon(name, size).map(|_| crate::weather::Message::Ignore)
        })
        .map(Message::Weather)
}

pub(super) fn media(d: &View<'_>, _: &ViewContext) -> Element<Message> {
    let artwork = d
        .input
        .media
        .artwork()
        .cloned()
        .map(Element::image)
        .unwrap_or_else(|| {
            Element::stack(vec![d.icon("music", icon::ARTWORK).at(
                component::media::FALLBACK_LEFT,
                component::media::FALLBACK_TOP,
            )])
            .background(d.surface_color())
        });
    let title = d.input.media.label();
    let progress = d
        .input
        .media
        .progress()
        .map_or(0., |(fraction, _, _)| fraction);
    let command = |which: &str| {
        let player = d.input.media.selected()?;
        match which {
            "previous" => Some(lucent_domain::MediaCommand::Previous(player.id)),
            "next" => Some(lucent_domain::MediaCommand::Next(player.id)),
            _ => d.input.media.toggle_command(),
        }
    };
    let button = |id: &str, icon: &str, command: Option<lucent_domain::MediaCommand>| {
        if let Some(command) = command.filter(|command| d.input.media.command_available(command)) {
            d.icon_button(
                id,
                icon,
                Message::Media(crate::media::Message::Request(command)),
            )
        } else {
            d.icon(icon, lucent_design::icon::SMALL)
                .size(
                    component::icon_button::EXTENT,
                    component::icon_button::EXTENT,
                )
                .opacity(opacity::DISABLED)
        }
    };
    Element::stack(vec![
        artwork
            .size(
                component::media::CONTENT_WIDTH,
                component::media::ARTWORK_HEIGHT,
            )
            .at(component::media::INSET, component::media::INSET)
            .radius(radius::CARD)
            .on_click(Message::Media(crate::media::Message::Open))
            .id("media-open"),
        d.label(title, font::HEADING)
            .size(
                component::media::CONTENT_WIDTH,
                component::media::TITLE_HEIGHT,
            )
            .at(component::media::INSET, component::media::TITLE_TOP),
        d.label(d.input.media.artist(), font::SMALL)
            .size(
                component::media::CONTENT_WIDTH,
                component::media::ARTIST_HEIGHT,
            )
            .at(component::media::INSET, component::media::ARTIST_TOP)
            .opacity(opacity::SUPPORTING),
        Element::empty()
            .size(
                component::media::CONTENT_WIDTH,
                component::media::PROGRESS_HEIGHT,
            )
            .at(component::media::INSET, component::media::PROGRESS_TOP)
            .radius(radius::INDICATOR)
            .background(d.surface_color()),
        Element::empty()
            .size(
                component::media::CONTENT_WIDTH * progress,
                component::media::PROGRESS_HEIGHT,
            )
            .at(component::media::INSET, component::media::PROGRESS_TOP)
            .radius(radius::INDICATOR)
            .background(d.accent()),
        button("media-previous", "previous", command("previous")).at(
            component::media::PREVIOUS_LEFT,
            component::media::NAVIGATION_TOP,
        ),
        button(
            "media-play",
            if d.input
                .media
                .playback()
                .is_some_and(|playback| playback.status == lucent_domain::PlaybackStatus::Playing)
            {
                "pause"
            } else {
                "play"
            },
            command("toggle"),
        )
        .size(component::media::PLAY_WIDTH, component::media::PLAY_HEIGHT)
        .padding(space::MD)
        .radius(radius::SEARCH)
        .background(d.accent())
        .tint(d.theme().on_primary)
        .at(component::media::PLAY_LEFT, component::media::PLAY_TOP),
        button("media-next", "next", command("next")).at(
            component::media::NEXT_LEFT,
            component::media::NAVIGATION_TOP,
        ),
    ])
    .fill()
}
pub(super) fn system(d: &View<'_>, _: &ViewContext) -> Element<Message> {
    let mem = &d.input.system;
    let fraction = if mem.memory_total_mib == 0 {
        0.
    } else {
        mem.memory_used_mib as f32 / mem.memory_total_mib as f32
    };
    Element::stack(vec![
        d.label("System", font::HEADING)
            .at(component::system::INSET, component::system::HEADING_TOP)
            .size(
                component::system::CONTENT_WIDTH,
                component::system::HEADING_HEIGHT,
            ),
        d.label(format!("CPU   {:.0}%", mem.cpu_percent), font::BODY)
            .at(component::system::INSET, component::system::CPU_TOP)
            .size(
                component::system::CONTENT_WIDTH,
                component::system::LABEL_HEIGHT,
            ),
        Element::empty()
            .size(
                component::system::CONTENT_WIDTH,
                component::system::PROGRESS_HEIGHT,
            )
            .at(
                component::system::INSET,
                component::system::CPU_PROGRESS_TOP,
            )
            .radius(radius::INDICATOR)
            .background(d.surface_color()),
        Element::empty()
            .size(
                component::system::CONTENT_WIDTH * (mem.cpu_percent / 100.).clamp(0., 1.),
                component::system::PROGRESS_HEIGHT,
            )
            .at(
                component::system::INSET,
                component::system::CPU_PROGRESS_TOP,
            )
            .radius(radius::INDICATOR)
            .background(d.accent()),
        d.label(
            format!(
                "Memory   {} / {} MiB",
                mem.memory_used_mib, mem.memory_total_mib
            ),
            font::SMALL,
        )
        .at(component::system::INSET, component::system::MEMORY_TOP)
        .size(
            component::system::CONTENT_WIDTH,
            component::system::LABEL_HEIGHT,
        ),
        Element::empty()
            .size(
                component::system::CONTENT_WIDTH,
                component::system::PROGRESS_HEIGHT,
            )
            .at(
                component::system::INSET,
                component::system::MEMORY_PROGRESS_TOP,
            )
            .radius(radius::INDICATOR)
            .background(d.surface_color()),
        Element::empty()
            .size(
                component::system::CONTENT_WIDTH * fraction,
                component::system::PROGRESS_HEIGHT,
            )
            .at(
                component::system::INSET,
                component::system::MEMORY_PROGRESS_TOP,
            )
            .radius(radius::INDICATOR)
            .background(d.accent()),
        d.label(d.input.network_status, font::CAPTION)
            .at(component::system::INSET, component::system::NETWORK_TOP)
            .size(
                component::system::CONTENT_WIDTH,
                component::system::NETWORK_HEIGHT,
            )
            .opacity(opacity::MUTED),
    ])
    .fill()
}
pub(super) fn notes(d: &View<'_>, cx: &ViewContext) -> Element<Message> {
    let width = (cx.width - 2. * component::notes::INSET).max(0.);
    let caption_top = (cx.height
        - (component::widget_size::notes::HEIGHT - component::notes::CAPTION_TOP))
        .max(component::notes::INPUT_TOP);
    let input_height = (caption_top - space::SM - component::notes::INPUT_TOP).max(0.);
    let caption = match d.input.save_status {
        Status::Saved => "Saved",
        Status::Saving => "Saving…",
        Status::Retrying => "Retrying save…",
        Status::Failed => "Save failed · Retry",
        Status::Unavailable => "Settings unavailable",
    };
    let status = d
        .label(caption, font::MICRO)
        .at(component::notes::INSET, caption_top)
        .size(width, component::notes::CAPTION_HEIGHT);
    let status = if d.input.save_status == Status::Failed {
        status
            .color(d.theme().error)
            .on_click(Message::RetrySave)
            .id("notes-save-retry")
    } else if d.input.save_status == Status::Retrying {
        status.color(d.theme().error)
    } else {
        status.opacity(opacity::SECONDARY)
    };
    Element::stack(vec![
        d.label("Notes", font::HEADING)
            .size(width, component::notes::HEADING_HEIGHT)
            .at(component::notes::INSET, component::notes::HEADING_TOP),
        d.theme()
            .text_area(
                d.input.notes,
                d.host.notes_field.selection(),
                "A thought to keep nearby…",
                ScrollSpec::new(d.host.notes_scroll, component::scroll::LINE_STEP)
                    .expect("controlled notes offset"),
                Message::EditNotes,
                Message::ScrollNotes,
            )
            .id("notes-input")
            .size(width, input_height)
            .at(component::notes::INSET, component::notes::INPUT_TOP)
            .background(d.surface_color()),
        status,
    ])
    .fill()
}
pub(super) fn timer(d: &View<'_>, _: &ViewContext) -> Element<Message> {
    let seconds = d.host.timer.remaining;
    let running = d.host.timer.phase == TimerPhase::Running;
    Element::stack(vec![
        d.label(
            if d.host.timer.phase == TimerPhase::Finished {
                "Session complete"
            } else {
                "Focus"
            },
            font::LABEL,
        )
        .size(
            component::timer::CONTENT_WIDTH,
            component::timer::HEADING_HEIGHT,
        )
        .at(component::timer::INSET, component::timer::HEADING_TOP),
        d.label(
            format!("{:02}:{:02}", seconds / 60, seconds % 60),
            font::TIMER,
        )
        .size(
            component::timer::CONTENT_WIDTH,
            component::timer::TIME_HEIGHT,
        )
        .at(component::timer::INSET, component::timer::TIME_TOP)
        .align(Align::Center),
        d.button(
            if running { "Pause" } else { "Start" },
            Message::TimerToggle,
        )
        .id("timer-toggle")
        .size(
            component::timer::TOGGLE_WIDTH,
            component::timer::BUTTON_HEIGHT,
        )
        .at(component::timer::INSET, component::timer::BUTTONS_TOP)
        .font(font::CONTROL)
        .background(d.accent())
        .color(d.theme().on_primary),
        d.button("Reset", Message::TimerReset)
            .id("timer-reset")
            .size(
                component::timer::RESET_WIDTH,
                component::timer::BUTTON_HEIGHT,
            )
            .at(component::timer::RESET_LEFT, component::timer::BUTTONS_TOP)
            .font(font::CONTROL)
            .background(d.surface_color()),
    ])
    .fill()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn month_navigation_crosses_year_and_leap_day() {
        let date = Date::new(2024, 3, 1).expect("valid calendar date");
        assert_eq!(calendar_month(date, -1), (2024, 2, 3));
        assert_eq!(calendar_month(date, -3), (2023, 12, 4));
        assert_eq!(calendar_month(date, 10), (2025, 1, 2));
    }
}
