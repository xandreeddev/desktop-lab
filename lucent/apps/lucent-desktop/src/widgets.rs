use crate::desktop::{Action, Desktop, Message};
use lucent_api::*;
use lucent_design::*;
use lucent_domain::{Date, Placement, TimerPhase};

pub fn registry() -> WidgetRegistry<Desktop, Message> {
    let mut registry = WidgetRegistry::default();
    let entries: [(&str, &str, WidgetView<Desktop, Message>); 7] = [
        ("calendar", "Calendar", calendar),
        ("clock", "Clock", clock),
        ("weather", "Weather", weather),
        ("media", "Media", media),
        ("system", "System", system),
        ("notes", "Notes", notes),
        ("timer", "Focus", timer),
    ];
    for (id, title, view) in entries {
        registry
            .register(
                WidgetDescriptor {
                    id,
                    title,
                    size: widget_size(id),
                },
                view,
            )
            .expect("unique built-in widget ID");
    }
    registry
}
pub fn widget_size(id: &str) -> (f32, f32) {
    match id {
        "calendar" => (
            component::widget_size::calendar::WIDTH,
            component::widget_size::calendar::HEIGHT,
        ),
        "clock" => (
            component::widget_size::clock::WIDTH,
            component::widget_size::clock::HEIGHT,
        ),
        "weather" => (
            component::widget_size::weather::WIDTH,
            component::widget_size::weather::HEIGHT,
        ),
        "media" => (
            component::widget_size::media::WIDTH,
            component::widget_size::media::HEIGHT,
        ),
        "system" => (
            component::widget_size::system::WIDTH,
            component::widget_size::system::HEIGHT,
        ),
        "notes" => (
            component::widget_size::notes::WIDTH,
            component::widget_size::notes::HEIGHT,
        ),
        "timer" => (
            component::widget_size::timer::WIDTH,
            component::widget_size::timer::HEIGHT,
        ),
        _ => (
            component::widget_size::fallback::WIDTH,
            component::widget_size::fallback::HEIGHT,
        ),
    }
}
pub fn default_position(id: &str, viewport: (f32, f32)) -> Placement {
    let (x, y): (f32, f32) = match id {
        "calendar" => (
            component::widget_layout::LEFT,
            component::widget_layout::TOP,
        ),
        "weather" => (
            component::widget_layout::WEATHER_LEFT,
            component::widget_layout::WEATHER_TOP,
        ),
        "clock" => (
            viewport.0 - component::widget_layout::RIGHT_INSET,
            component::widget_layout::CLOCK_TOP,
        ),
        "media" => (
            viewport.0 - component::widget_layout::RIGHT_INSET,
            component::widget_layout::MEDIA_TOP,
        ),
        "system" => (
            component::widget_layout::LEFT,
            component::widget_layout::SYSTEM_TOP,
        ),
        "notes" => (
            component::widget_layout::SECOND_COLUMN,
            component::widget_layout::TOP,
        ),
        "timer" => (
            component::widget_layout::SECOND_COLUMN,
            component::widget_layout::TIMER_TOP,
        ),
        _ => (
            component::widget_layout::LEFT,
            component::widget_layout::TOP,
        ),
    };
    let size = widget_size(id);
    let Placement { x, y } =
        lucent_usecases::snap_placement(Placement { x, y }, component::widget_layout::GRID_STEP)
            .expect("valid grid token and default placement");
    Placement {
        x: x.clamp(0., (viewport.0 - size.0).max(0.)),
        y: y.clamp(0., (viewport.1 - size.1).max(0.)),
    }
}
impl Desktop {
    pub fn widgets(&self, cx: &ViewContext) -> Element<Message> {
        let children = self
            .settings
            .visible_widgets
            .iter()
            .filter_map(|id| {
                let content = self.registry.view(id, self, cx)?;
                let pos = self
                    .settings
                    .positions
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| default_position(id, (cx.width, cx.height)));
                let size = widget_size(id);
                let drag_id = id.clone();
                Some(
                    Element::stack(vec![content])
                        .size(size.0, size.1)
                        .at(
                            pos.x.clamp(0., (cx.width - size.0).max(0.)),
                            pos.y.clamp(0., (cx.height - size.1).max(0.)),
                        )
                        .background(if id == "calendar" || id == "media" {
                            self.surface_color()
                        } else {
                            self.widget_color()
                        })
                        .radius(radius::WIDGET)
                        .clip()
                        .shadow()
                        .on_drag(move |drag| Message::MoveWidget(drag_id.clone(), drag))
                        .id(format!("widget-{id}")),
                )
            })
            .collect();
        Element::stack(children).fill()
    }
}
fn clock(d: &Desktop, _: &ViewContext) -> Element<Message> {
    Element::stack(vec![
        d.label(format!("{:02}", d.clock.hour), font::CLOCK)
            .font_face(1)
            .size(component::clock::WIDTH, component::clock::DIGIT_HEIGHT)
            .align(Align::Center)
            .at(0., component::clock::HOUR_TOP),
        d.label(format!("{:02}", d.clock.minute), font::CLOCK)
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
                ][d.clock.date.weekday as usize],
                d.clock.date.day,
                months()[d.clock.date.month as usize - 1]
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
    let (mut year, mut month, mut first) = (date.year, date.month, date.first_weekday());
    for _ in 0..offset.unsigned_abs() {
        if offset > 0 {
            first = (first + Date::days_in_month(year, month)) % 7;
            if month == 12 {
                year += 1;
                month = 1;
            } else {
                month += 1;
            }
        } else {
            if month == 1 {
                year -= 1;
                month = 12;
            } else {
                month -= 1;
            }
            first = (first + 7 - Date::days_in_month(year, month) % 7) % 7;
        }
    }
    (year, month, first)
}
fn calendar(d: &Desktop, _: &ViewContext) -> Element<Message> {
    let date = d.clock.date;
    let (year, month, first) = calendar_month(date, d.month_offset);
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
    let prev_days = Date::days_in_month(if month == 1 { year - 1 } else { year }, prev_month);
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
        let today = current && d.month_offset == 0 && day == date.day as i32;
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
fn weather(d: &Desktop, _: &ViewContext) -> Element<Message> {
    let (temperature, description, symbol) = d
        .weather
        .as_ref()
        .map(|w| {
            (
                format!("{:.0}°", w.temperature),
                w.description.as_str(),
                if w.code <= 1 { "sun" } else { "cloud" },
            )
        })
        .unwrap_or_else(|| ("—".into(), "Weather unavailable", "cloud"));
    Element::stack(vec![
        Element::empty()
            .size(
                component::weather::ICON_BACKGROUND_SIZE,
                component::weather::ICON_BACKGROUND_SIZE,
            )
            .at(
                component::weather::ICON_BACKGROUND_LEFT,
                component::weather::ICON_BACKGROUND_TOP,
            )
            .radius(radius::WIDGET)
            .background(d.ink().alpha(opacity::SUBTLE)),
        d.icon(symbol, icon::WEATHER)
            .at(component::weather::ICON_LEFT, component::weather::ICON_TOP),
        d.label(temperature, font::TEMPERATURE)
            .font_face(1)
            .at(
                component::weather::TEXT_LEFT,
                component::weather::TEMPERATURE_TOP,
            )
            .size(
                component::weather::TEMPERATURE_WIDTH,
                component::weather::TEMPERATURE_HEIGHT,
            ),
        d.label(description, font::BODY)
            .at(
                component::weather::TEXT_LEFT,
                component::weather::DESCRIPTION_TOP,
            )
            .size(
                component::weather::DESCRIPTION_WIDTH,
                component::weather::DESCRIPTION_HEIGHT,
            ),
    ])
    .fill()
}

fn media(d: &Desktop, _: &ViewContext) -> Element<Message> {
    let artwork = d
        .images
        .get(&d.media.art_url)
        .cloned()
        .map(Element::image)
        .unwrap_or_else(|| {
            Element::stack(vec![d.icon("music", icon::ARTWORK).at(
                component::media::FALLBACK_LEFT,
                component::media::FALLBACK_TOP,
            )])
            .background(d.surface_color())
        });
    let title = if d.media.title.is_empty() {
        "Nothing playing"
    } else {
        &d.media.title
    };
    let progress = if d.media.length == 0 {
        0.
    } else {
        (d.media.position as f32 / d.media.length as f32).clamp(0., 1.)
    };
    Element::stack(vec![
        artwork
            .size(
                component::media::CONTENT_WIDTH,
                component::media::ARTWORK_HEIGHT,
            )
            .at(component::media::INSET, component::media::INSET)
            .radius(radius::CARD),
        d.label(title, font::HEADING)
            .size(
                component::media::CONTENT_WIDTH,
                component::media::TITLE_HEIGHT,
            )
            .at(component::media::INSET, component::media::TITLE_TOP),
        d.label(&d.media.artist, font::SMALL)
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
        d.icon_button(
            "media-previous",
            "previous",
            Message::Action(Action::Previous),
        )
        .at(
            component::media::PREVIOUS_LEFT,
            component::media::NAVIGATION_TOP,
        ),
        d.icon_button(
            "media-play",
            if d.media.playing { "pause" } else { "play" },
            Message::Action(Action::PlayPause),
        )
        .size(component::media::PLAY_WIDTH, component::media::PLAY_HEIGHT)
        .padding(space::MD)
        .radius(radius::SEARCH)
        .background(d.accent())
        .tint(d.theme().on_primary)
        .at(component::media::PLAY_LEFT, component::media::PLAY_TOP),
        d.icon_button("media-next", "next", Message::Action(Action::Next))
            .at(
                component::media::NEXT_LEFT,
                component::media::NAVIGATION_TOP,
            ),
    ])
    .fill()
}
fn system(d: &Desktop, _: &ViewContext) -> Element<Message> {
    let mem = &d.system;
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
        d.label(&mem.network, font::CAPTION)
            .at(component::system::INSET, component::system::NETWORK_TOP)
            .size(
                component::system::CONTENT_WIDTH,
                component::system::NETWORK_HEIGHT,
            )
            .opacity(opacity::MUTED),
    ])
    .fill()
}
fn notes(d: &Desktop, _: &ViewContext) -> Element<Message> {
    Element::stack(vec![
        d.label("Notes", font::HEADING)
            .size(
                component::notes::CONTENT_WIDTH,
                component::notes::HEADING_HEIGHT,
            )
            .at(component::notes::INSET, component::notes::HEADING_TOP),
        Element::input(
            &d.settings.notes,
            "A thought to keep nearby…",
            Message::Notes,
        )
        .id("notes-input")
        .size(
            component::notes::CONTENT_WIDTH,
            component::notes::INPUT_HEIGHT,
        )
        .at(component::notes::INSET, component::notes::INPUT_TOP)
        .padding_xy(component::input::PADDING_INLINE, space::MD)
        .font(font::BODY)
        .color(d.ink())
        .background(d.surface_color())
        .radius(radius::CONTROL),
        d.label("Saved automatically", font::MICRO)
            .at(component::notes::INSET, component::notes::CAPTION_TOP)
            .size(
                component::notes::CONTENT_WIDTH,
                component::notes::CAPTION_HEIGHT,
            )
            .opacity(opacity::SECONDARY),
    ])
    .fill()
}
fn timer(d: &Desktop, _: &ViewContext) -> Element<Message> {
    let seconds = d.timer.remaining;
    let running = d.timer.phase == TimerPhase::Running;
    Element::stack(vec![
        d.label(
            if d.timer.phase == TimerPhase::Finished {
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
        let date = Date {
            year: 2024,
            month: 3,
            day: 1,
            weekday: 4,
        };
        assert_eq!(calendar_month(date, -1), (2024, 2, 3));
        assert_eq!(calendar_month(date, -3), (2023, 12, 4));
        assert_eq!(calendar_month(date, 10), (2025, 1, 2));
    }
}
