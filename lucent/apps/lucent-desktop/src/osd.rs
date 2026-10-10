//! Transient feedback is client presentation, not a notification protocol event.
use lucent_api::*;
use lucent_design::{Theme, component::osd as token, font, motion, radius, space};
use serde::Deserialize;

#[derive(Clone, Default, Deserialize)]
pub struct Feedback {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, rename = "progressText")]
    pub progress_text: String,
    #[serde(default)]
    pub max: String,
}
pub struct Osd {
    pub feedback: Option<Feedback>,
    expires: f64,
    reveal: Motion,
}
impl Default for Osd {
    fn default() -> Self {
        Self {
            feedback: None,
            expires: 0.,
            reveal: Motion::fixed(0.),
        }
    }
}
impl Osd {
    pub fn show(&mut self, feedback: Feedback, now: f64) {
        self.feedback = Some(feedback);
        self.expires = now + 1.5;
        self.reveal
            .target(1., now, motion::ENTER, motion::DECELERATE);
    }
    pub fn tick(&mut self, now: f64) {
        if now >= self.expires {
            if self.reveal.target_value() != 0. {
                self.reveal
                    .target(0., now, motion::ENTER, motion::DECELERATE);
            }
            if !self.reveal.active(now) {
                self.feedback = None;
            }
        }
    }
    pub fn animating(&self, now: f64) -> bool {
        self.reveal.active(now)
    }
    pub fn view<M: Clone>(&self, cx: &ViewContext, theme: Theme) -> Element<M> {
        let Some(value) = &self.feedback else {
            return Element::empty();
        };
        let width = token::WIDTH.min(cx.width - token::MARGIN * 2.);
        let mut parts = vec![
            Element::text(&value.message)
                .font(font::BODY)
                .color(theme.on_surface)
                .width(Length::Fill)
                .wrap(2),
        ];
        if let Ok(progress) = value.value.parse::<f32>() {
            let max = value
                .max
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.)
                .unwrap_or(100.);
            if progress.is_finite() {
                parts.push(
                    Element::stack(vec![
                        Element::empty()
                            .size(
                                (width - token::PADDING * 2.) * (progress / max).clamp(0., 1.),
                                token::TRACK,
                            )
                            .radius(radius::CONTROL)
                            .background(theme.primary),
                    ])
                    .width(Length::Fill)
                    .height(Length::Fixed(token::TRACK))
                    .radius(radius::CONTROL)
                    .background(theme.surface_container),
                );
            }
        }
        if !value.progress_text.is_empty() {
            parts.push(
                Element::text(&value.progress_text)
                    .font(font::CAPTION)
                    .color(theme.on_surface),
            );
        }
        theme.apply(
            Element::stack(vec![
                Element::column(parts)
                    .gap(space::SM)
                    .padding(token::PADDING)
                    .width(Length::Fixed(width))
                    .background(theme.surface)
                    .radius(radius::PANEL)
                    .shadow()
                    .opacity(self.reveal.value(cx.now))
                    .at((cx.width - width) / 2., token::MARGIN),
            ])
            .fill(),
        )
    }
}
