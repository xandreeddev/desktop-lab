use crate::desktop::{Action, Desktop, Message};
use lucent_api::*;
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
        "calendar" => (270., 276.),
        "clock" => (248., 264.),
        "weather" => (270., 130.),
        "media" => (260., 392.),
        "system" => (270., 180.),
        "notes" => (270., 160.),
        "timer" => (270., 190.),
        _ => (200., 150.),
    }
}
pub fn default_position(id: &str, viewport: (f32, f32)) -> Placement {
    let (x, y): (f32, f32) = match id {
        "calendar" => (20., 110.),
        "weather" => (25., 400.),
        "clock" => (viewport.0 - 280., 215.),
        "media" => (viewport.0 - 280., 495.),
        "system" => (20., 560.),
        "notes" => (320., 110.),
        "timer" => (320., 310.),
        _ => (20., 110.),
    };
    let size = widget_size(id);
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
                        .radius(30.)
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
        d.label(format!("{:02}", d.clock.hour), 110.)
            .font_face(1)
            .size(248., 119.)
            .align(Align::Center)
            .at(0., 5.),
        d.label(format!("{:02}", d.clock.minute), 110.)
            .font_face(1)
            .size(248., 119.)
            .align(Align::Center)
            .at(0., 104.),
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
            12.,
        )
        .size(248., 24.)
        .align(Align::Center)
        .at(0., 221.),
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
        d.label(format!("{} {}", months()[month as usize - 1], year), 15.)
            .at(19., 16.)
            .size(172., 30.),
        d.icon_button("calendar-previous", "left", Message::Month(-1))
            .at(189., 15.),
        d.icon_button("calendar-next", "right", Message::Month(1))
            .at(224., 15.),
    ];
    for (i, day) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        children.push(
            d.label(*day, 11.)
                .size(33., 25.)
                .align(Align::Center)
                .at(18. + i as f32 * 33., 51.)
                .opacity(0.65),
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
        let x = 19. + (index % 7) as f32 * 33.;
        let y = 79. + (index / 7) as f32 * 30.;
        if today {
            let petals = (0..6)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 6.;
                    Element::empty()
                        .size(22., 22.)
                        .at(x + 4.5 + a.cos() * 5., y + 3.5 + a.sin() * 5.)
                        .radius(8.)
                        .background(d.accent())
                })
                .collect();
            children.push(Element::stack(petals).fill());
        }
        children.push(
            d.label(displayed.to_string(), 12.)
                .size(31., 29.)
                .align(Align::Center)
                .color(if today {
                    Color::hex(0x174657)
                } else {
                    d.ink().alpha(if current { 1. } else { 0.3 })
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
            .size(82., 82.)
            .at(18., 24.)
            .radius(30.)
            .background(d.ink().alpha(0.08)),
        d.icon(symbol, 61.).at(28., 33.),
        d.label(temperature, 32.)
            .font_face(1)
            .at(116., 29.)
            .size(134., 46.),
        d.label(description, 14.).at(116., 72.).size(145., 30.),
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
            Element::stack(vec![d.icon("music", 70.).at(77., 67.)]).background(d.surface_color())
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
        artwork.size(224., 213.).at(18., 18.).radius(22.),
        d.label(title, 17.).size(224., 28.).at(18., 241.),
        d.label(&d.media.artist, 12.)
            .size(224., 24.)
            .at(18., 271.)
            .opacity(0.8),
        Element::empty()
            .size(224., 3.)
            .at(18., 310.)
            .radius(2.)
            .background(d.surface_color()),
        Element::empty()
            .size(224. * progress, 3.)
            .at(18., 310.)
            .radius(2.)
            .background(d.accent()),
        d.icon_button(
            "media-previous",
            "previous",
            Message::Action(Action::Previous),
        )
        .at(58., 337.),
        d.icon_button(
            "media-play",
            if d.media.playing { "pause" } else { "play" },
            Message::Action(Action::PlayPause),
        )
        .size(60., 44.)
        .padding(12.)
        .radius(24.)
        .background(d.accent())
        .at(100., 330.),
        d.icon_button("media-next", "next", Message::Action(Action::Next))
            .at(168., 337.),
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
        d.label("System", 17.).at(18., 14.).size(234., 28.),
        d.label(format!("CPU   {:.0}%", mem.cpu_percent), 14.)
            .at(18., 53.)
            .size(234., 24.),
        Element::empty()
            .size(234., 4.)
            .at(18., 84.)
            .radius(2.)
            .background(d.surface_color()),
        Element::empty()
            .size(234. * (mem.cpu_percent / 100.).clamp(0., 1.), 4.)
            .at(18., 84.)
            .radius(2.)
            .background(d.accent()),
        d.label(
            format!(
                "Memory   {} / {} MiB",
                mem.memory_used_mib, mem.memory_total_mib
            ),
            12.,
        )
        .at(18., 98.)
        .size(234., 24.),
        Element::empty()
            .size(234., 4.)
            .at(18., 129.)
            .radius(2.)
            .background(d.surface_color()),
        Element::empty()
            .size(234. * fraction, 4.)
            .at(18., 129.)
            .radius(2.)
            .background(d.accent()),
        d.label(&mem.network, 11.)
            .at(18., 144.)
            .size(234., 23.)
            .opacity(0.75),
    ])
    .fill()
}
fn notes(d: &Desktop, _: &ViewContext) -> Element<Message> {
    Element::stack(vec![
        d.label("Notes", 17.).size(230., 30.).at(20., 15.),
        Element::input(
            &d.settings.notes,
            "A thought to keep nearby…",
            Message::Notes,
        )
        .id("notes-input")
        .size(230., 60.)
        .at(20., 55.)
        .padding(10.)
        .font(14.)
        .color(d.ink())
        .background(d.surface_color())
        .radius(18.),
        d.label("Saved automatically", 10.)
            .at(20., 128.)
            .size(230., 18.)
            .opacity(0.65),
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
            15.,
        )
        .size(230., 28.)
        .at(20., 13.),
        d.label(format!("{:02}:{:02}", seconds / 60, seconds % 60), 51.)
            .size(230., 70.)
            .at(20., 47.)
            .align(Align::Center),
        Element::button(
            if running { "Pause" } else { "Start" },
            Message::TimerToggle,
        )
        .id("timer-toggle")
        .size(142., 37.)
        .at(20., 135.)
        .font(13.)
        .background(d.accent())
        .color(Color::hex(0x174657)),
        Element::button("Reset", Message::TimerReset)
            .id("timer-reset")
            .size(78., 37.)
            .at(172., 135.)
            .font(13.)
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
