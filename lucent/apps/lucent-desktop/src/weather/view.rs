//! Borrowed weather presentation; location edits never perform I/O.
use super::*;
use lucent_api::{Align, Element, Length, ScrollSpec, ViewContext};
use lucent_design::{Theme, component, font, opacity, radius, space};
pub fn condition_label(condition: WeatherCondition) -> &'static str {
    use WeatherCondition::*;
    match condition {
        Clear => "Clear sky",
        MostlyClear => "Mostly clear",
        PartlyCloudy => "Partly cloudy",
        Overcast => "Overcast",
        Fog => "Fog",
        Drizzle => "Drizzle",
        FreezingDrizzle => "Freezing drizzle",
        FreezingRain => "Freezing rain",
        Rain => "Rain",
        Snow => "Snow",
        Showers => "Rain showers",
        SnowShowers => "Snow showers",
        Thunderstorm => "Thunderstorm",
        Unknown => "Unknown conditions",
    }
}
pub fn condition_symbol(condition: WeatherCondition, is_day: bool) -> &'static str {
    use WeatherCondition::*;
    match condition {
        Clear | MostlyClear if is_day => "sun",
        Clear | MostlyClear => "weather-night",
        Rain | Drizzle | FreezingDrizzle | FreezingRain | Showers => "weather-rain",
        Snow | SnowShowers => "weather-snow",
        Thunderstorm => "weather-storm",
        Fog => "weather-fog",
        _ => "cloud",
    }
}
fn label(text: impl Into<String>, theme: Theme) -> Element<Message> {
    Element::text(text)
        .font(font::BODY)
        .color(theme.on_surface)
        .width(Length::Fill)
        .wrap(component::weather_panel::TEXT_LINES as usize)
}
impl Weather {
    fn age(&self) -> String {
        self.report
            .as_ref()
            .map(|report| {
                let minutes = self.now.saturating_sub(report.fetched_at) / 60;
                if minutes == 0 {
                    "Updated just now".into()
                } else {
                    format!("Updated {minutes} min ago")
                }
            })
            .unwrap_or_default()
    }
    pub fn summary(&self) -> String {
        self.report
            .as_ref()
            .map(|report| {
                format!(
                    "{:.0}{}",
                    self.unit().value(report.temperature),
                    self.unit().symbol()
                )
            })
            .unwrap_or_else(|| "—".into())
    }
    pub fn bar_summary(&self) -> String {
        self.report
            .as_ref()
            .map(|report| {
                format!(
                    "{}{:.0}°",
                    if self.stale() || !self.error.is_empty() {
                        "~"
                    } else {
                        ""
                    },
                    self.unit().value(report.temperature)
                )
            })
            .unwrap_or_else(|| "—".into())
    }
    pub fn widget(
        &self,
        cx: &ViewContext,
        theme: Theme,
        icon: impl Fn(&str, f32) -> Element<Message>,
    ) -> Element<Message> {
        let symbol = self
            .report
            .as_ref()
            .map(|report| condition_symbol(report.condition, report.is_day))
            .unwrap_or("cloud");
        let description = self
            .report
            .as_ref()
            .map(|report| condition_label(report.condition))
            .unwrap_or(self.status());
        let inset = space::LG;
        let width = (cx.width - 2. * inset).max(0.);
        Element::column(vec![
            Element::row(vec![
                icon(symbol, lucent_design::icon::WIDGET),
                theme
                    .button(
                        if self.report.is_some() {
                            format!("{} · {description}", self.summary())
                        } else {
                            "Configure weather".into()
                        },
                        Message::Open,
                    )
                    .width(Length::Fill)
                    .wrap(1)
                    .id("weather-details"),
            ])
            .gap(space::SM)
            .width(Length::Fill),
            label(
                self.preferences
                    .location
                    .as_ref()
                    .map(WeatherLocation::name)
                    .unwrap_or("Weather"),
                theme,
            )
            .font(font::CAPTION),
            label(
                if !self.error.is_empty() || self.report.is_none() {
                    self.status().to_owned()
                } else {
                    self.age()
                },
                theme,
            )
            .font(font::CAPTION)
            .color(if self.error.is_empty() {
                theme.on_surface.alpha(opacity::MUTED)
            } else {
                theme.error
            }),
        ])
        .gap(space::XS)
        .size(width, (cx.height - 2. * inset).max(0.))
        .at(inset, inset)
        .clip()
    }
    pub fn view(
        &self,
        cx: &ViewContext,
        artwork: crate::artwork::Artwork<'_>,
        save: SaveState<'_>,
    ) -> Element<Message> {
        let theme = artwork.theme;
        let mut rows = vec![
            Element::row(vec![
                label("Weather", theme).font(font::TITLE),
                theme.button("Close", Message::Close).id("weather-close"),
            ])
            .gap(space::MD),
            label(self.status(), theme),
        ];
        if let Some(report) = &self.report {
            rows.push(
                Element::row(vec![
                    artwork.icon(
                        condition_symbol(report.condition, report.is_day),
                        lucent_design::icon::WEATHER,
                    ),
                    label(
                        format!("{} · {}", self.summary(), condition_label(report.condition)),
                        theme,
                    )
                    .font(font::TITLE),
                ])
                .gap(space::MD)
                .width(Length::Fill),
            );
            rows.push(label(self.age(), theme).font(font::CAPTION));
            rows.push(label("Three-day forecast · local dates", theme));
            for day in report.days() {
                let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
                rows.push(
                    Element::row(vec![
                        label(
                            format!(
                                "{} {:02}/{:02}",
                                weekdays[day.date.weekday() as usize],
                                day.date.day(),
                                day.date.month()
                            ),
                            theme,
                        ),
                        label(condition_label(day.condition), theme),
                        label(
                            format!(
                                "{:.0} / {:.0}{}",
                                self.unit().value(day.high()),
                                self.unit().value(day.low()),
                                self.unit().symbol()
                            ),
                            theme,
                        )
                        .align(Align::End),
                    ])
                    .gap(space::SM)
                    .padding(space::SM)
                    .radius(radius::CONTROL)
                    .background(theme.surface_container),
                );
            }
        }
        if !self.error.is_empty() {
            rows.push(
                theme
                    .status_text(&self.error, true)
                    .width(Length::Fill)
                    .wrap(component::weather_panel::TEXT_LINES as usize),
            );
        }
        if self.desired().is_some() || self.legacy_allowed && !self.error.is_empty() {
            rows.push(
                theme
                    .button(
                        if self.busy {
                            "Refresh after current request"
                        } else if self.error.is_empty() {
                            "Refresh weather"
                        } else {
                            "Retry weather"
                        },
                        Message::Refresh,
                    )
                    .id("weather-refresh"),
            );
        }
        if let Some(form) = &self.form {
            rows.push(label("Location", theme).font(font::TITLE));
            rows.push(label("Use a label and explicit coordinates. Applying sends those coordinates to Open-Meteo. No location is detected automatically.",theme).font(font::CAPTION));
            for (field, title, placeholder) in [
                (Field::Name, "Location label", "Selected location"),
                (Field::Latitude, "Latitude", "−90 to 90"),
                (Field::Longitude, "Longitude", "−180 to 180"),
            ] {
                let generation = form.generation;
                let index = field.index();
                rows.push(label(title, theme).font(font::CAPTION));
                if save.writable {
                    rows.push(
                        theme
                            .text_input(
                                &form.values[index],
                                form.fields[index].selection(),
                                placeholder,
                                move |edit| Message::Edit(generation, field, edit),
                            )
                            .id(format!("weather-{}-{generation}", field.name()))
                            .on_submit(Message::Apply),
                    );
                } else {
                    rows.push(label("Preferences are unavailable", theme));
                }
            }
            rows.push(
                Element::row(
                    [TemperatureUnit::Celsius, TemperatureUnit::Fahrenheit]
                        .into_iter()
                        .map(|unit| {
                            theme
                                .choice_button(
                                    unit.symbol(),
                                    if save.writable {
                                        Message::Unit(unit)
                                    } else {
                                        Message::Ignore
                                    },
                                    form.unit == unit,
                                )
                                .id(format!("weather-unit-{}", unit.symbol()))
                        })
                        .collect(),
                )
                .gap(space::SM),
            );
            if !self.form_error.is_empty() {
                rows.push(
                    theme
                        .status_text(&self.form_error, true)
                        .wrap(component::weather_panel::TEXT_LINES as usize),
                );
            }
            if save.writable {
                rows.push(
                    Element::row(vec![
                        theme
                            .button("Apply and enable", Message::Apply)
                            .id("weather-apply"),
                        theme
                            .button("Turn weather off", Message::Disable)
                            .id("weather-disable"),
                    ])
                    .gap(space::SM),
                );
            }
        }
        rows.push(theme.status_text(
            match save.status {
                crate::persistence::Status::Saved => "Preferences saved",
                crate::persistence::Status::Saving => "Saving preferences…",
                crate::persistence::Status::Retrying => "Retrying preference save…",
                crate::persistence::Status::Failed => "Preference save failed",
                crate::persistence::Status::Unavailable => "Preferences unavailable",
            },
            matches!(
                save.status,
                crate::persistence::Status::Failed | crate::persistence::Status::Unavailable
            ),
        ));
        if !save.error.is_empty() {
            rows.push(
                theme
                    .status_text(save.error, true)
                    .wrap(component::weather_panel::TEXT_LINES as usize),
            );
        }
        if save.writable && save.status == crate::persistence::Status::Failed {
            rows.push(
                theme
                    .button("Retry saving preferences", Message::RetrySave)
                    .id("weather-save-retry"),
            );
        }
        rows.push(
            label(
                "Forecast: Open-Meteo · model data, not a live station measurement",
                theme,
            )
            .font(font::CAPTION),
        );
        let rect = crate::shell_layout::weather_panel((cx.width, cx.height));
        let content = Element::column(rows)
            .width(Length::Fill)
            .padding(space::LG)
            .gap(space::MD);
        let panel = theme
            .scroll(
                ScrollSpec::new(self.scroll, component::scroll::LINE_STEP)
                    .expect("controlled weather scroll"),
                content,
                Message::Scroll,
            )
            .size(rect.w, rect.h)
            .radius(radius::PANEL)
            .background(theme.surface)
            .shadow()
            .on_click(Message::Ignore)
            .hover_feedback(false)
            .tab_stop(false)
            .id("weather-scroll")
            .opacity(self.motion.value(cx.now))
            .at(rect.x, rect.y);
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("weather-backdrop"),
            panel,
        ])
        .fill()
    }
}
