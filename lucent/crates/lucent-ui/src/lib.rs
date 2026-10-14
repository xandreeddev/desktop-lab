//! Layout, hit testing and retained interaction state for declarative components.
use lucent_api::*;
mod scroll;
pub use scroll::{FocusClip, ScrollHit};
#[cfg(test)]
mod input_tests;
mod slider;
mod text_input;
pub use text_input::TextHit;
pub mod text;
pub use slider::SliderHit;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub enum Paint {
    Outline {
        rect: Rect,
        clip: Clip,
        color: Color,
        radius: f32,
        width: f32,
    },
    Shape {
        rect: Rect,
        clip: Clip,
        color: Color,
        radius: f32,
    },
    Shadow {
        rect: Rect,
        clip: Clip,
        color: Color,
        radius: f32,
        blur: f32,
    },
    Text {
        rect: Rect,
        clip: Clip,
        text: String,
        size: f32,
        face: usize,
        color: Color,
        align: Align,
    },
    Image {
        rect: Rect,
        /// Normalized source coordinates; cover fitting crops pixels, not destination geometry.
        uv: Rect,
        clip: Clip,
        data: Arc<ImageData>,
        radius: f32,
        opacity: f32,
        tint: Color,
    },
}
#[derive(Clone)]
pub struct Hit<M> {
    pub id: String,
    pub rect: Rect,
    pub clip: Clip,
    pub radius: f32,
    pub click: Option<M>,
    pub submit: Option<M>,
    pub hover: Option<M>,
    pub input: Option<InputCallback<M>>,
    pub edit: Option<EditCallback<M>>,
    pub text_edit: Option<TextEditCallback<M>>,
    pub text: Option<TextHit>,
    pub value: String,
    pub drag: Option<DragCallback<M>>,
    pub slider: Option<SliderHit<M>>,
    pub scroll: Option<Arc<ScrollHit<M>>>,
    pub scroll_ancestors: Arc<[Arc<ScrollHit<M>>]>,
    pub focus_clips: Arc<[FocusClip]>,
    pub autofocus: bool,
    pub hover_transition: Transition,
    pub tab_stop: bool,
}
impl<M> Hit<M> {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.clip.contains(x, y) && RoundedRect::new(self.rect, self.radius).contains(x, y)
    }
}
pub struct Scene<M> {
    pub paint: Vec<Paint>,
    pub hits: Vec<Hit<M>>,
    pub regions: Vec<Clip>,
}
impl<M> Default for Scene<M> {
    fn default() -> Self {
        Self {
            paint: vec![],
            hits: vec![],
            regions: vec![],
        }
    }
}

pub struct Layout {
    fonts: Arc<Vec<fontdue::Font>>,
}
impl Layout {
    pub fn new(fonts: Arc<Vec<fontdue::Font>>) -> Self {
        Self { fonts }
    }
    fn text_width(&self, s: &str, size: f32, face: usize) -> f32 {
        text::width(self.fonts.get(face).unwrap_or(&self.fonts[0]), s, size)
    }

    fn wrapped(&self, text: &str, size: f32, face: usize, width: f32, max_lines: usize) -> String {
        // Subtracting padding from an intrinsic width can lose a few ulps.
        // Preserve a measured single line without adding visible layout space.
        let width = width + width.abs().max(1.) * f32::EPSILON * 4.;
        let mut lines = vec![String::new()];
        for word in text.split_inclusive([' ', '\n']) {
            let hard_break = word.ends_with('\n');
            let word = word.trim_end_matches([' ', '\n']);
            let current = lines.last().unwrap();
            let joined = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if self.text_width(&joined, size, face) <= width {
                *lines.last_mut().unwrap() = joined;
            } else {
                if !current.is_empty() {
                    lines.push(String::new());
                }
                for c in word.chars() {
                    let line = lines.last_mut().unwrap();
                    if !line.is_empty()
                        && self.text_width(&format!("{line}{c}"), size, face) > width
                    {
                        lines.push(String::new());
                    }
                    lines.last_mut().unwrap().push(c);
                }
            }
            if hard_break {
                lines.push(String::new());
            }
            if lines.len() > max_lines {
                break;
            }
        }
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            let last = lines.last_mut().unwrap();
            while !last.is_empty() && self.text_width(&format!("{last}…"), size, face) > width {
                last.pop();
            }
            last.push('…');
        }
        lines.join("\n")
    }
    /// Measure row children at the widths they receive during placement. This
    /// keeps intrinsic height and wrapped text consistent inside clipped rows.
    fn measure_children<M>(&self, e: &Element<M>, available: (f32, f32)) -> Vec<(f32, f32)> {
        let mut sizes: Vec<_> = e
            .children
            .iter()
            .map(|c| self.measure(c, available))
            .collect();
        if matches!(e.kind, Kind::Row) {
            let fixed: f32 = e
                .children
                .iter()
                .zip(&sizes)
                .filter(|(c, _)| c.style.width != Length::Fill)
                .map(|(_, s)| s.0)
                .sum();
            let count = e
                .children
                .iter()
                .filter(|c| c.style.width == Length::Fill)
                .count();
            let width =
                (available.0 - fixed - e.style.gap * e.children.len().saturating_sub(1) as f32)
                    .max(0.)
                    / count.max(1) as f32;
            for (child, size) in e.children.iter().zip(&mut sizes) {
                if child.style.width == Length::Fill {
                    *size = self.measure(child, (width, available.1));
                }
            }
        }
        sizes
    }
    fn measure<M>(&self, e: &Element<M>, available: (f32, f32)) -> (f32, f32) {
        let px = e.style.padding.horizontal * 2.;
        let py = e.style.padding.vertical * 2.;
        let gap = e.style.gap;
        let available = (
            match e.style.width {
                Length::Fixed(w) => w,
                _ => available.0,
            },
            available.1,
        );
        let children =
            self.measure_children(e, ((available.0 - px).max(0.), (available.1 - py).max(0.)));
        let n = children.len();
        let gaps = gap * n.saturating_sub(1) as f32;
        let content = match &e.kind {
            Kind::Text(t) => {
                let wrapped = e.style.max_lines.map(|limit| {
                    self.wrapped(
                        t,
                        e.style.font_size,
                        e.style.font_face,
                        (available.0 - px).max(0.),
                        limit,
                    )
                });
                let t = wrapped.as_deref().unwrap_or(t);
                (
                    self.text_width(t, e.style.font_size, e.style.font_face),
                    text::line_metrics(
                        self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]),
                        e.style.font_size,
                    )
                    .height
                        * t.lines().count().max(1) as f32,
                )
            }
            Kind::Input { .. } | Kind::MaskedInput { .. } | Kind::TextInput { .. } => (
                160.,
                text::line_metrics(
                    self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]),
                    e.style.font_size,
                )
                .height,
            ),
            Kind::Image(_) => (32., 32.),
            Kind::Slider(_) => (160., e.style.slider.thumb_diameter),
            Kind::Row => (
                children.iter().map(|c| c.0).sum::<f32>() + gaps,
                children.iter().map(|c| c.1).fold(0., f32::max),
            ),
            Kind::Column => (
                children.iter().map(|c| c.0).fold(0., f32::max),
                children.iter().map(|c| c.1).sum::<f32>() + gaps,
            ),
            Kind::Grid(cols) => {
                let rows = n.div_ceil(*cols);
                (
                    available.0 - px,
                    rows as f32 * children.iter().map(|c| c.1).fold(0., f32::max)
                        + gap * rows.saturating_sub(1) as f32,
                )
            }
            Kind::Scroll(_) | Kind::Stack => (
                children.iter().map(|c| c.0).fold(0., f32::max),
                children.iter().map(|c| c.1).fold(0., f32::max),
            ),
            Kind::Empty => (0., 0.),
        };
        let resolve = |length: Length, intrinsic: f32, max: f32, padding: f32| match length {
            Length::Fixed(v) => v,
            Length::Fill => max,
            Length::Shrink => intrinsic + padding,
        };
        (
            resolve(e.style.width, content.0, available.0, px).max(0.),
            resolve(e.style.height, content.1, available.1, py).max(0.),
        )
    }
    pub fn build<M: Clone>(
        &self,
        root: &Element<M>,
        width: f32,
        height: f32,
        interaction: &Interaction<M>,
        now: f64,
    ) -> Scene<M> {
        let mut scene = Scene::default();
        let rect = Rect::new(0., 0., width, height);
        self.place(
            root,
            rect,
            rect.into(),
            1.,
            &mut scene,
            interaction,
            now,
            &Arc::default(),
            &Arc::from([FocusClip {
                rect,
                radius: 0.,
                scroll_depth: 0,
            }]),
            "root",
        );
        scene
    }
    #[allow(clippy::too_many_arguments)]
    fn place<M: Clone>(
        &self,
        e: &Element<M>,
        rect: Rect,
        clip: Clip,
        opacity: f32,
        scene: &mut Scene<M>,
        interaction: &Interaction<M>,
        now: f64,
        scrolls: &Arc<[Arc<ScrollHit<M>>]>,
        focus_clips: &Arc<[FocusClip]>,
        path: &str,
    ) {
        let id = if e.id.is_empty() {
            path.to_owned()
        } else {
            e.id.clone()
        };
        let alpha = opacity * e.style.opacity;
        if alpha <= 0.001 {
            return;
        }
        let paint_start = scene.paint.len();
        let own_clip = if e.style.clip {
            clip.with_rounded(rect, e.style.radius)
        } else {
            clip.clone()
        };
        let p = e.style.padding;
        let inner = Rect::new(
            rect.x + p.horizontal,
            rect.y + p.vertical,
            (rect.w - 2. * p.horizontal).max(0.),
            (rect.h - 2. * p.vertical).max(0.),
        );
        let scroll_hit = if let Kind::Scroll(spec) = e.kind {
            e.scroll.as_ref().map(|callback| {
                let width = (inner.w - scroll::gutter(e.style.scroll, inner.w)).max(0.);
                let height = e
                    .children
                    .first()
                    .map(|child| self.measure(child, (width, inner.h)).1)
                    .unwrap_or(0.);
                Arc::new(ScrollHit::new(
                    id.clone(),
                    inner,
                    height,
                    spec,
                    e.style.scroll,
                    callback.clone(),
                ))
            })
        } else {
            None
        };
        let mut child_focus_clips = focus_clips.clone();
        if e.style.clip {
            let mut clips = focus_clips.to_vec();
            clips.push(FocusClip {
                rect,
                radius: e.style.radius,
                scroll_depth: scrolls.len(),
            });
            child_focus_clips = clips.into();
        }
        let prepared_text = if let Kind::TextInput {
            value, selection, ..
        } = &e.kind
        {
            let font = self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]);
            Some(text_input::geometry(
                font,
                value,
                *selection,
                e.style.font_size,
                inner,
                e.style.caret_width,
                interaction.focus.as_deref() == Some(&id),
            ))
        } else {
            None
        };
        let interactive = e.click.is_some()
            || e.input.is_some()
            || e.edit.is_some()
            || e.text_edit.is_some()
            || e.drag.is_some()
            || e.hover.is_some()
            || e.slider.is_some()
            || e.scroll.is_some();
        if e.style.background.3 > 0. && alpha > 0. {
            let mut color = e.style.background;
            if e.click.is_some()
                || e.input.is_some()
                || e.edit.is_some()
                || e.text_edit.is_some()
                || e.slider.is_some()
            {
                color = color.mix(
                    e.style.hover_color.alpha(color.3),
                    interaction.hover_amount(&id, now) * e.style.hover_color.3,
                );
            }
            color.3 *= alpha;
            if let Some(shadow) = e.style.shadow {
                scene.paint.push(Paint::Shadow {
                    rect: Rect::new(
                        rect.x + shadow.offset.0,
                        rect.y + shadow.offset.1,
                        rect.w,
                        rect.h,
                    ),
                    clip: clip.clone(),
                    color: shadow.color.alpha(shadow.color.3 * alpha),
                    radius: e.style.radius,
                    blur: if shadow.blur.is_finite() {
                        shadow.blur.max(0.)
                    } else {
                        0.
                    },
                });
            }
            scene.paint.push(Paint::Shape {
                rect,
                clip: clip.clone(),
                color,
                radius: e.style.radius,
            });
        }
        if interactive {
            scene.regions.push(clip.with_rounded(rect, e.style.radius));
            scene.hits.push(Hit {
                id: id.clone(),
                rect,
                clip: clip.clone(),
                radius: e.style.radius,
                click: e.click.clone(),
                submit: e.submit.clone(),
                hover: e.hover.clone(),
                input: e.input.clone(),
                edit: e.edit.clone(),
                text_edit: e.text_edit.clone(),
                text: prepared_text.clone(),
                value: if let Kind::Input { value, .. } = &e.kind {
                    value.clone()
                } else {
                    String::new()
                },
                drag: e.drag.clone(),
                slider: if let Kind::Slider(spec) = e.kind {
                    e.slider.as_ref().map(|callback| {
                        let (track, _) = slider::geometry(rect, e.style.padding, e.style.slider);
                        SliderHit {
                            spec,
                            start: track.x,
                            width: track.w,
                            callback: callback.clone(),
                        }
                    })
                } else {
                    None
                },
                scroll: scroll_hit.clone(),
                scroll_ancestors: scrolls.clone(),
                focus_clips: child_focus_clips.clone(),
                autofocus: e.autofocus,
                hover_transition: e.style.hover_transition,
                tab_stop: e.style.tab_stop
                    && (e.click.is_some()
                        || e.input.is_some()
                        || e.edit.is_some()
                        || e.text_edit.is_some()
                        || e.slider.is_some()
                        || e.scroll.is_some()),
            });
        }
        let mut fg = e.style.foreground;
        fg.3 *= alpha;
        match &e.kind {
            Kind::TextInput { .. } => text_input::paint(
                scene,
                e,
                prepared_text.as_ref().expect("prepared text input"),
                own_clip.intersect(inner),
                alpha,
                interaction.focus.as_deref() == Some(&id),
                self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]),
            ),
            Kind::Text(text) => scene.paint.push(Paint::Text {
                rect: inner,
                clip: own_clip.intersect(rect),
                text: e
                    .style
                    .max_lines
                    .map(|limit| {
                        self.wrapped(text, e.style.font_size, e.style.font_face, inner.w, limit)
                    })
                    .unwrap_or_else(|| text.clone()),
                size: e.style.font_size,
                face: e.style.font_face,
                color: fg,
                align: e.style.align,
            }),
            Kind::Input { .. } | Kind::MaskedInput { .. } => {
                let (value, placeholder) = match &e.kind {
                    Kind::Input { value, placeholder } => {
                        (std::borrow::Cow::Borrowed(value.as_str()), placeholder)
                    }
                    Kind::MaskedInput {
                        length,
                        placeholder,
                    } => (
                        std::borrow::Cow::Owned("•".repeat((*length).min(4096))),
                        placeholder,
                    ),
                    _ => unreachable!(),
                };
                let text = if value.is_empty() {
                    placeholder.clone()
                } else {
                    value.to_string()
                };
                if value.is_empty() {
                    fg.3 *= 0.55;
                }
                scene.paint.push(Paint::Text {
                    rect: inner,
                    clip: own_clip.intersect(inner),
                    text,
                    size: e.style.font_size,
                    face: e.style.font_face,
                    color: fg,
                    align: Align::Start,
                });
                if interaction.focus.as_deref() == Some(&id) {
                    let font = self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]);
                    let line = text::line_metrics(font, e.style.font_size);
                    let top = inner.y + (inner.h - line.height).max(0.) / 2.;
                    let advance = self.text_width(&value, e.style.font_size, e.style.font_face);
                    let scroll = (advance + e.style.caret_width - inner.w).max(0.);
                    // Keep the end-caret visible when a single-line value exceeds the field.
                    if let Some(Paint::Text { rect, .. }) = scene.paint.last_mut() {
                        rect.x -= scroll;
                    }
                    let x =
                        (inner.x + advance - scroll).min(inner.x + inner.w - e.style.caret_width);
                    scene.paint.push(Paint::Shape {
                        rect: Rect::new(
                            x,
                            top + line.baseline - line.ascent,
                            e.style.caret_width,
                            line.ascent - line.descent,
                        ),
                        clip: own_clip.intersect(inner),
                        color: e.style.foreground.alpha(e.style.foreground.3 * alpha),
                        radius: 0.,
                    });
                }
            }
            Kind::Slider(spec) => slider::paint(
                scene,
                rect,
                own_clip.intersect(rect),
                *spec,
                &e.style,
                alpha,
            ),
            Kind::Image(data) => {
                let (width, height) = data.aspect_size();
                let (rect, uv) = image_geometry(inner, width, height, e.style.image_fit);
                scene.paint.push(Paint::Image {
                    rect,
                    uv,
                    clip: own_clip.intersect(inner),
                    data: data.clone(),
                    radius: e.style.radius,
                    opacity: alpha,
                    tint: e.style.image_tint,
                });
            }
            _ => {}
        }
        if let Some(scroll) = scroll_hit {
            if let Some(child) = e.children.first() {
                let width = (inner.w - scroll::gutter(e.style.scroll, inner.w)).max(0.);
                let height = self.measure(child, (width, inner.h)).1.max(inner.h);
                let mut ancestors = scrolls.to_vec();
                ancestors.push(scroll.clone());
                let mut clips = child_focus_clips.to_vec();
                clips.push(FocusClip {
                    rect: Rect::new(inner.x, inner.y, width, inner.h),
                    radius: 0.,
                    scroll_depth: scrolls.len(),
                });
                child_focus_clips = clips.into();
                self.place(
                    child,
                    Rect::new(inner.x, inner.y - scroll.offset, width, height),
                    own_clip.intersect(Rect::new(inner.x, inner.y, width, inner.h)),
                    alpha,
                    scene,
                    interaction,
                    now,
                    &ancestors.into(),
                    &child_focus_clips,
                    &format!("{path}/0"),
                );
            }
            if scroll.maximum > 0. && scroll.thumb.w > 0. {
                scene.paint.push(Paint::Shape {
                    rect: scroll.thumb,
                    clip: own_clip.clone(),
                    color: e.style.scroll.thumb.alpha(e.style.scroll.thumb.3 * alpha),
                    radius: scroll.thumb.w / 2.,
                });
            }
        } else {
            let mut measured = self.measure_children(e, (inner.w, inner.h));
            if matches!(e.kind, Kind::Column) {
                let fixed: f32 = e
                    .children
                    .iter()
                    .zip(&measured)
                    .filter(|(c, _)| c.style.height != Length::Fill)
                    .map(|(_, s)| s.1)
                    .sum();
                let count = e
                    .children
                    .iter()
                    .filter(|c| c.style.height == Length::Fill)
                    .count();
                let height =
                    (inner.h - fixed - e.style.gap * e.children.len().saturating_sub(1) as f32)
                        .max(0.)
                        / count.max(1) as f32;
                for (child, size) in e.children.iter().zip(&mut measured) {
                    if child.style.height == Length::Fill {
                        size.1 = height;
                    }
                }
            }
            let mut cursor = 0.;
            let grid_h = measured.iter().map(|v| v.1).fold(0., f32::max);
            for (i, (child, (w, h))) in e.children.iter().zip(measured).enumerate() {
                let (x, y, w, h) = match e.kind {
                    Kind::Row => {
                        let r = (
                            inner.x + cursor,
                            inner.y + align_offset(e.style.align, inner.h - h),
                            w,
                            h,
                        );
                        cursor += w + e.style.gap;
                        r
                    }
                    Kind::Column => {
                        let r = (
                            inner.x + align_offset(e.style.align, inner.w - w),
                            inner.y + cursor,
                            w,
                            h,
                        );
                        cursor += h + e.style.gap;
                        r
                    }
                    Kind::Grid(cols) => {
                        let cw =
                            ((inner.w - e.style.gap * (cols - 1) as f32) / cols as f32).max(0.);
                        (
                            inner.x + (i % cols) as f32 * (cw + e.style.gap),
                            inner.y + (i / cols) as f32 * (grid_h + e.style.gap),
                            cw,
                            h,
                        )
                    }
                    _ => {
                        let (x, y) = child.style.position.unwrap_or((0., 0.));
                        (inner.x + x, inner.y + y, w, h)
                    }
                };
                self.place(
                    child,
                    Rect::new(x, y, w, h),
                    own_clip.clone(),
                    alpha,
                    scene,
                    interaction,
                    now,
                    scrolls,
                    &child_focus_clips,
                    &format!("{path}/{i}"),
                );
            }
        }
        fn contains_focus<M>(e: &Element<M>, focus: &str, path: &str) -> bool {
            let id = if e.id.is_empty() { path } else { &e.id };
            id == focus
                || e.children
                    .iter()
                    .enumerate()
                    .any(|(i, child)| contains_focus(child, focus, &format!("{path}/{i}")))
        }
        let focused = interaction.focus_visible
            && interaction.focus.as_deref().is_some_and(|focus| {
                focus == id || (e.style.focus_within && contains_focus(e, focus, path))
            });
        if (e.style.selected || (e.style.focus_outline && focused)) && e.style.focus_width > 0. {
            scene.paint.push(Paint::Outline {
                rect,
                clip: own_clip,
                radius: e.style.radius,
                color: e.style.focus_color.alpha(e.style.focus_color.3 * alpha),
                width: e.style.focus_width,
            });
        }
        if !e.style.paint {
            scene.paint.truncate(paint_start);
        }
    }
}
fn align_offset(align: Align, remaining: f32) -> f32 {
    match align {
        Align::Start => 0.,
        Align::Center => remaining / 2.,
        Align::End => remaining,
    }
    .max(0.)
}

fn image_geometry(bounds: Rect, width: u32, height: u32, fit: ImageFit) -> (Rect, Rect) {
    let full = Rect::new(0., 0., 1., 1.);
    if fit == ImageFit::Fill || bounds.w <= 0. || bounds.h <= 0. {
        return (bounds, full);
    }
    let (x, y) = (bounds.w / width as f32, bounds.h / height as f32);
    let factor = if fit == ImageFit::Cover {
        x.max(y)
    } else {
        x.min(y)
    };
    let (w, h) = (width as f32 * factor, height as f32 * factor);
    if fit == ImageFit::Cover {
        let (u, v) = (bounds.w / w, bounds.h / h);
        (bounds, Rect::new((1. - u) / 2., (1. - v) / 2., u, v))
    } else {
        (
            Rect::new(
                bounds.x + (bounds.w - w) / 2.,
                bounds.y + (bounds.h - h) / 2.,
                w,
                h,
            ),
            full,
        )
    }
}

struct Press<M> {
    hit: Hit<M>,
    x: f32,
    y: f32,
    moved: bool,
    scroll_grab: Option<f32>,
    text_anchor: Option<usize>,
    text_last: Option<usize>,
}
pub struct Interaction<M> {
    pub focus: Option<String>,
    pub hovered: Option<String>,
    pub focus_visible: bool,
    press: Option<Press<M>>,
    hover_transition: Transition,
    motions: BTreeMap<String, Motion>,
    select_all: bool,
    input_buffer: Option<(String, String)>,
    slider_buffer: Option<(String, f32)>,
    scroll_buffer: BTreeMap<String, f32>,
}
impl<M> Default for Interaction<M> {
    fn default() -> Self {
        Self {
            focus: None,
            hovered: None,
            focus_visible: false,
            hover_transition: Transition::default(),
            press: None,
            motions: BTreeMap::new(),
            select_all: false,
            input_buffer: None,
            slider_buffer: None,
            scroll_buffer: BTreeMap::new(),
        }
    }
}
impl<M: Clone> Interaction<M> {
    pub fn synchronize(&mut self, scene: &Scene<M>) {
        if let Some(press) = &mut self.press
            && let Some(previous) = &press.hit.text
        {
            let current = scene.hits.iter().find(|hit| hit.id == press.hit.id);
            if let Some(current) = current
                && current.text.as_ref().is_some_and(|text| {
                    text.value == previous.value && text.inner == previous.inner
                })
                && current.clip == press.hit.clip
            {
                press.hit = current.clone();
            } else {
                self.press = None;
            }
        }
        if let Some(press) = &mut self.press
            && press.scroll_grab.is_some()
        {
            let current = scene.hits.iter().find(|h| h.id == press.hit.id);
            let compatible = current.is_some_and(|h| {
                h.scroll.as_ref().is_some_and(|new| {
                    press.hit.scroll.as_ref().is_some_and(|old| {
                        old.maximum == new.maximum
                            && old.track == new.track
                            && old.thumb.h == new.thumb.h
                            && h.clip == press.hit.clip
                    })
                })
            });
            if compatible {
                press.hit = current.unwrap().clone();
            } else {
                self.press = None;
            }
        }
        self.scroll_buffer = scene
            .hits
            .iter()
            .filter_map(|h| h.scroll.as_ref().map(|s| (s.id.clone(), s.offset)))
            .collect();
        if self.press.as_ref().is_some_and(|p| {
            p.hit.slider.is_some()
                && !scene
                    .hits
                    .iter()
                    .any(|h| h.id == p.hit.id && h.slider.is_some())
        }) {
            self.press = None;
        }
        self.slider_buffer = self.focus.as_ref().and_then(|id| {
            scene
                .hits
                .iter()
                .find(|h| &h.id == id)
                .and_then(|h| h.slider.as_ref())
                .map(|s| (id.clone(), s.spec.value()))
        });
        if self
            .focus
            .as_ref()
            .is_some_and(|id| !scene.hits.iter().any(|h| &h.id == id))
        {
            self.focus = None;
        }
        if self.focus.is_none() {
            self.focus = scene
                .hits
                .iter()
                .find(|h| h.autofocus)
                .map(|h| h.id.clone());
            if self.focus.is_some() {
                self.focus_visible = true;
            }
        }
        self.input_buffer = self.focus.as_ref().and_then(|id| {
            scene
                .hits
                .iter()
                .find(|h| &h.id == id && h.input.is_some())
                .map(|h| (id.clone(), h.value.clone()))
        });
    }
    pub fn hover_amount(&self, id: &str, now: f64) -> f32 {
        self.motions.get(id).map(|m| m.value(now)).unwrap_or(0.)
    }
    pub fn animating(&self, now: f64) -> bool {
        self.motions.values().any(|m| m.active(now))
    }
    /// A surface leave ends hover without fabricating a pointer position or dragging.
    pub fn leave(&mut self, now: f64) {
        if let Some(old) = self.hovered.take() {
            self.motions.entry(old).or_insert(Motion::fixed(1.)).target(
                0.,
                now,
                self.hover_transition.duration,
                self.hover_transition.curve,
            );
        }
    }
    pub fn motion(&mut self, scene: &Scene<M>, x: f32, y: f32, now: f64) -> Vec<M> {
        let mut out = vec![];
        let hit = scene.hits.iter().rev().find(|h| h.contains(x, y));
        let id = hit.map(|h| h.id.clone());
        if id != self.hovered {
            if let Some(old) = &self.hovered {
                self.motions
                    .entry(old.clone())
                    .or_insert(Motion::fixed(1.))
                    .target(
                        0.,
                        now,
                        self.hover_transition.duration,
                        self.hover_transition.curve,
                    );
            }
            if let Some(new) = &id {
                self.hover_transition = hit.unwrap().hover_transition;
                self.motions
                    .entry(new.clone())
                    .or_insert(Motion::fixed(0.))
                    .target(
                        1.,
                        now,
                        self.hover_transition.duration,
                        self.hover_transition.curve,
                    );
            }
            self.hovered = id;
            if let Some(msg) = hit.and_then(|h| h.hover.clone()) {
                out.push(msg);
            }
        }
        if let Some(press) = &mut self.press {
            let (dx, dy) = (x - press.x, y - press.y);
            press.moved |= dx.hypot(dy) >= 5.;
            if let (Some(anchor), Some(text), Some(callback)) =
                (press.text_anchor, &press.hit.text, &press.hit.text_edit)
                && let Some(caret) = text.at(x)
                && press.text_last != Some(caret)
            {
                press.text_last = Some(caret);
                out.push(callback(TextEdit::Select { anchor, caret }));
            }
            if let (Some(scroll), Some(grab)) = (&press.hit.scroll, press.scroll_grab) {
                if let Some(offset) = scroll.at(y, grab) {
                    self.scroll_buffer.insert(scroll.id.clone(), offset);
                    out.push(scroll.event(offset));
                }
            } else if let Some(slider) = &press.hit.slider {
                if let Some(message) = slider.event(x, false) {
                    out.push(message);
                }
            } else if press.moved
                && let Some(f) = &press.hit.drag
            {
                out.push(f(DragEvent {
                    dx,
                    dy,
                    finished: false,
                }));
            }
        }
        self.motions
            .retain(|id, m| m.active(now) || self.hovered.as_ref() == Some(id));
        out
    }
    pub fn press(&mut self, scene: &Scene<M>, x: f32, y: f32) -> Option<M> {
        self.press_with_modifiers(scene, x, y, KeyModifiers::default())
    }
    pub fn press_with_modifiers(
        &mut self,
        scene: &Scene<M>,
        x: f32,
        y: f32,
        modifiers: KeyModifiers,
    ) -> Option<M> {
        self.focus_visible = false;
        self.slider_buffer = None;
        self.press = scene
            .hits
            .iter()
            .rev()
            .find(|h| h.contains(x, y))
            .cloned()
            .map(|hit| {
                if hit.tab_stop {
                    self.focus = Some(hit.id.clone());
                    self.select_all = false;
                    self.input_buffer = hit
                        .input
                        .as_ref()
                        .map(|_| (hit.id.clone(), hit.value.clone()));
                }
                let scroll_grab = hit
                    .scroll
                    .as_ref()
                    .filter(|s| s.maximum > 0. && s.track.contains(x, y))
                    .map(|s| {
                        if s.thumb.contains(x, y) {
                            y - s.thumb.y
                        } else {
                            s.thumb.h / 2.
                        }
                    });
                Press {
                    text_anchor: hit.text.as_ref().and_then(|text| {
                        if modifiers.shift {
                            Some(text.selection.anchor())
                        } else {
                            text.at(x)
                        }
                    }),
                    text_last: hit.text.as_ref().and_then(|text| text.at(x)),
                    hit,
                    scroll_grab,
                    x,
                    y,
                    moved: false,
                }
            });
        if let Some(press) = &self.press
            && let (Some(anchor), Some(caret), Some(callback)) =
                (press.text_anchor, press.text_last, &press.hit.text_edit)
        {
            return Some(callback(TextEdit::Select { anchor, caret }));
        }
        if let Some(p) = &self.press
            && let (Some(scroll), Some(grab)) = (&p.hit.scroll, p.scroll_grab)
            && let Some(offset) = scroll.at(y, grab)
        {
            self.scroll_buffer.insert(scroll.id.clone(), offset);
            return Some(scroll.event(offset));
        }
        self.press
            .as_ref()
            .and_then(|p| p.hit.slider.as_ref())
            .and_then(|s| s.event(x, false))
    }
    pub fn release(&mut self, x: f32, y: f32) -> Option<M> {
        let p = self.press.take()?;
        if let (Some(anchor), Some(text), Some(callback)) =
            (p.text_anchor, &p.hit.text, &p.hit.text_edit)
        {
            return text
                .at(x)
                .map(|caret| callback(TextEdit::Select { anchor, caret }));
        }
        if let (Some(scroll), Some(grab)) = (&p.hit.scroll, p.scroll_grab) {
            let offset = scroll.at(y, grab)?;
            self.scroll_buffer.insert(scroll.id.clone(), offset);
            return Some(scroll.event(offset));
        }
        if let Some(slider) = &p.hit.slider {
            return slider.event(x, true);
        }
        if p.moved {
            return p.hit.drag.map(|f| {
                f(DragEvent {
                    dx: x - p.x,
                    dy: y - p.y,
                    finished: true,
                })
            });
        }
        if p.hit.contains(x, y) {
            p.hit.click
        } else {
            None
        }
    }
    /// A wheel event is consumed by the deepest scroll viewport under the
    /// pointer, chaining outward at its limits. Some(empty) still consumes it.
    pub fn scroll(&mut self, scene: &Scene<M>, x: f32, y: f32, lines: f64) -> Option<Vec<M>> {
        let hit = scene.hits.iter().rev().find(|h| h.contains(x, y))?;
        // Follow this hit's ancestry, never an unrelated viewport underneath it.
        let mut handled = false;
        for scroll in hit.scroll.iter().chain(hit.scroll_ancestors.iter().rev()) {
            handled = true;
            if !lines.is_finite() {
                return Some(vec![]);
            }
            let previous = *self.scroll_buffer.get(&scroll.id).unwrap_or(&scroll.offset);
            let value = (f64::from(previous) + lines * f64::from(scroll.line_step))
                .clamp(0., f64::from(scroll.maximum)) as f32;
            if value != previous {
                self.scroll_buffer.insert(scroll.id.clone(), value);
                return Some(vec![scroll.event(value)]);
            }
        }
        handled.then(Vec::new)
    }
    pub fn key(&mut self, scene: &Scene<M>, key: &Key) -> Vec<M> {
        self.focus_visible = true;
        let key = if scene
            .hits
            .iter()
            .any(|hit| Some(&hit.id) == self.focus.as_ref() && hit.text_edit.is_some())
        {
            key
        } else {
            key.unmodified()
        };
        if matches!(key, Key::Tab | Key::BackTab) {
            return self.traverse(scene, key);
        }
        if let Some(hit) = scene
            .hits
            .iter()
            .find(|h| Some(&h.id) == self.focus.as_ref())
        {
            let scroll = hit.scroll.as_ref().or_else(|| {
                matches!(key, Key::PageUp | Key::PageDown)
                    .then(|| hit.scroll_ancestors.last())
                    .flatten()
            });
            if let Some(scroll) = scroll {
                let previous = *self.scroll_buffer.get(&scroll.id).unwrap_or(&scroll.offset);
                let page = (scroll.viewport.h - scroll.line_step).max(scroll.line_step);
                let value = match key {
                    Key::Down => Some(previous + scroll.line_step),
                    Key::Up => Some(previous - scroll.line_step),
                    Key::PageDown => Some(previous + page),
                    Key::PageUp => Some(previous - page),
                    Key::Home => Some(0.),
                    Key::End => Some(scroll.maximum),
                    _ => None,
                };
                if let Some(value) = value {
                    let value = value.clamp(0., scroll.maximum);
                    self.scroll_buffer.insert(scroll.id.clone(), value);
                    return vec![scroll.event(value)];
                }
            }
        }
        self.control_key(scene, key).into_iter().collect()
    }
    fn traverse(&mut self, scene: &Scene<M>, key: &Key) -> Vec<M> {
        let count = scene.hits.len();
        if count == 0 {
            return vec![];
        }
        let reverse = matches!(key, Key::BackTab);
        let current = scene
            .hits
            .iter()
            .position(|h| Some(&h.id) == self.focus.as_ref());
        let start = current.unwrap_or(if reverse { 0 } else { count - 1 });
        // Inspect only candidates up to the next reachable stop; do not build
        // reveal plans for every item in a long list on each key press.
        let next = (1..=count).find_map(|step| {
            let index = if reverse {
                (start + count - step) % count
            } else {
                (start + step) % count
            };
            let hit = &scene.hits[index];
            hit.tab_stop
                .then(|| scroll::reveal(hit, &self.scroll_buffer))
                .flatten()
                .map(|plan| (hit, plan))
        });
        let Some((hit, plan)) = next else {
            return vec![];
        };
        self.focus = Some(hit.id.clone());
        self.slider_buffer = None;
        self.select_all = false;
        self.input_buffer = hit
            .input
            .as_ref()
            .map(|_| (hit.id.clone(), hit.value.clone()));
        let mut messages = vec![];
        for (scroll, value) in plan {
            self.scroll_buffer.insert(scroll.id.clone(), value);
            messages.push(scroll.event(value));
        }
        messages
    }
    fn control_key(&mut self, scene: &Scene<M>, key: &Key) -> Option<M> {
        if let Some(hit) = scene
            .hits
            .iter()
            .find(|hit| Some(&hit.id) == self.focus.as_ref())
            && let Some(callback) = &hit.text_edit
        {
            if matches!(key, Key::Enter) {
                return hit.submit.clone();
            }
            return text_input::key(key).map(|edit| callback(edit));
        }
        if let Some(hit) = scene
            .hits
            .iter()
            .find(|h| Some(&h.id) == self.focus.as_ref())
            && let Some(edit) = &hit.edit
        {
            let action = match key {
                Key::Enter => return hit.submit.clone(),
                Key::SelectAll => {
                    self.select_all = true;
                    return None;
                }
                Key::Backspace if self.select_all => InputEdit::Clear,
                Key::Backspace => InputEdit::Backspace,
                Key::ClearInput => InputEdit::Clear,
                Key::Text(text) if self.select_all => {
                    InputEdit::Replace(text.replace(['\n', '\r'], ""))
                }
                Key::Text(text) => InputEdit::Insert(text.replace(['\n', '\r'], "")),
                _ => return None,
            };
            self.select_all = false;
            return Some(edit(action));
        }
        if let Some(hit) = scene
            .hits
            .iter()
            .find(|h| Some(&h.id) == self.focus.as_ref())
            && let Some(slider) = &hit.slider
        {
            let previous = self
                .slider_buffer
                .as_ref()
                .filter(|(id, _)| id == &hit.id)
                .map_or(slider.spec.value(), |(_, value)| *value);
            let value = match key {
                Key::Left | Key::Down => (previous - slider.spec.step()).max(0.),
                Key::Right | Key::Up => (previous + slider.spec.step()).min(1.),
                Key::Home => 0.,
                Key::End => 1.,
                _ => return None,
            };
            self.slider_buffer = Some((hit.id.clone(), value));
            return Some((slider.callback)(SliderEvent {
                value,
                finished: true,
            }));
        }
        if (matches!(key, Key::Enter) || matches!(key, Key::Text(s) if s == " "))
            && let Some(hit) = scene.hits.iter().find(|h| {
                Some(&h.id) == self.focus.as_ref()
                    && h.input.is_none()
                    && h.edit.is_none()
                    && h.text_edit.is_none()
            })
        {
            return hit.click.clone();
        }
        let hit = scene
            .hits
            .iter()
            .find(|h| Some(&h.id) == self.focus.as_ref() && h.input.is_some())?;
        if matches!(key, Key::Enter) {
            return hit.submit.clone();
        }
        // Several key events can arrive before the next compositor frame. Keep
        // edits locally until the new scene reflects the updated client state.
        let mut value = self
            .input_buffer
            .as_ref()
            .filter(|(id, _)| id == &hit.id)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| hit.value.clone());
        match key {
            Key::SelectAll => {
                self.select_all = true;
                return None;
            }
            Key::Backspace => {
                if self.select_all {
                    value.clear();
                } else {
                    value.pop();
                }
            }
            Key::Text(s) => {
                if self.select_all {
                    value.clear();
                }
                if value.len() + s.len() <= 8192 {
                    value.push_str(&s.replace(['\n', '\r'], ""));
                }
            }
            _ => return None,
        }
        self.select_all = false;
        self.input_buffer = Some((hit.id.clone(), value.clone()));
        hit.input.as_ref().map(|f| f(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_only_retains_geometry_focus_and_drag_capture_without_paint() {
        let tree = Element::stack(vec![
            Element::empty()
                .size(80., 40.)
                .at(12., 10.)
                .background(Color(1., 0., 0., 1.))
                .on_click("click".to_string())
                .on_drag(|event| format!("{}:{}:{}", event.dx, event.dy, event.finished))
                .id("handle"),
        ])
        .fill();
        let layout = Layout::new(Arc::new(vec![]));
        let mut input = Interaction::default();
        let painted = layout.build(&tree, 120., 80., &input, 0.);
        assert!(!painted.paint.is_empty());
        input.press(&painted, 30., 25.);
        let only = tree.input_only().map(|message| format!("mapped-{message}"));
        let scene = layout.build(&only, 120., 80., &input, 0.);
        assert!(
            scene.paint.is_empty(),
            "focus decoration is suppressed as well"
        );
        assert_eq!(scene.regions, painted.regions);
        assert_eq!(scene.hits[0].id, painted.hits[0].id);
        assert_eq!(scene.hits[0].rect, painted.hits[0].rect);
        input.synchronize(&scene);
        assert_eq!(input.focus.as_deref(), Some("handle"));
        assert_eq!(input.motion(&scene, 160., 110., 0.), ["130:85:false"]);
        assert_eq!(input.release(170., 120.), Some("140:95:true".into()));
        input.press(&scene, 30., 25.);
        assert_eq!(input.release(30., 25.), Some("mapped-click".into()));
        assert_eq!(input.release(170., 120.), None);
    }
    #[test]
    fn trimmed_odd_image_keeps_cover_and_contain_geometry() {
        let source = ImageData::from_rgba("odd aspect", 65, 37, vec![255; 65 * 37 * 4]).unwrap();
        let trimmed = source.downsample_for_cover(16, 9).unwrap();
        let layout = Layout::new(Arc::new(vec![]));
        for fit in [ImageFit::Cover, ImageFit::Contain] {
            let geometry = |image| {
                let mut root = Element::<()>::image(Arc::new(image)).size(23., 17.);
                root.style.image_fit = fit;
                let scene = layout.build(&root, 23., 17., &Interaction::default(), 0.);
                match &scene.paint[0] {
                    Paint::Image { rect, uv, .. } => (*rect, *uv),
                    _ => panic!("expected an image primitive"),
                }
            };
            assert_eq!(geometry(source.clone()), geometry(trimmed.clone()));
        }
    }
    #[test]
    fn rounded_hit_test_leaves_corners_click_through() {
        let h: Hit<()> = Hit {
            id: "a".into(),
            rect: Rect::new(0., 0., 100., 100.),
            clip: Rect::new(0., 0., 100., 100.).into(),
            radius: 20.,
            click: None,
            hover: None,
            submit: None,
            input: None,
            edit: None,
            text_edit: None,
            text: None,
            value: String::new(),
            drag: None,
            slider: None,
            scroll: None,
            scroll_ancestors: Arc::default(),
            focus_clips: Arc::default(),
            autofocus: false,
            hover_transition: Transition::default(),
            tab_stop: true,
        };
        assert!(!h.contains(1., 1.));
        assert!(h.contains(50., 50.));
    }
    #[test]
    fn typing_between_frame_callbacks_preserves_every_character() {
        let hit = Hit {
            id: "input".into(),
            rect: Rect::new(0., 0., 100., 30.),
            clip: Rect::new(0., 0., 100., 30.).into(),
            radius: 0.,
            click: None,
            hover: None,
            submit: None,
            input: Some(Arc::new(|s: String| s)),
            edit: None,
            text_edit: None,
            text: None,
            value: String::new(),
            drag: None,
            slider: None,
            scroll: None,
            scroll_ancestors: Arc::default(),
            focus_clips: Arc::default(),
            autofocus: true,
            hover_transition: Transition::default(),
            tab_stop: true,
        };
        let scene = Scene {
            hits: vec![hit],
            ..Default::default()
        };
        let mut input = Interaction::default();
        input.synchronize(&scene);
        assert_eq!(
            input.key(&scene, &Key::Text("f".into())).into_iter().next(),
            Some("f".into())
        );
        assert_eq!(
            input
                .key(&scene, &Key::Text("oo".into()))
                .into_iter()
                .next(),
            Some("foo".into())
        );
        assert_eq!(
            input.key(&scene, &Key::Text("t".into())).into_iter().next(),
            Some("foot".into())
        );
        input.key(&scene, &Key::SelectAll);
        assert_eq!(
            input.key(&scene, &Key::Text("é".into())).into_iter().next(),
            Some("é".into())
        );
        assert_eq!(
            input.key(&scene, &Key::Backspace).into_iter().next(),
            Some(String::new())
        );
    }
    #[test]
    fn drag_threshold_prevents_accidental_click() {
        let h = Hit {
            id: "a".into(),
            rect: Rect::new(0., 0., 100., 100.),
            clip: Rect::new(0., 0., 100., 100.).into(),
            radius: 0.,
            click: Some(1),
            hover: None,
            submit: None,
            input: None,
            edit: None,
            text_edit: None,
            text: None,
            value: String::new(),
            drag: Some(Arc::new(|d: DragEvent| if d.finished { 3 } else { 2 })),
            slider: None,
            scroll: None,
            scroll_ancestors: Arc::default(),
            focus_clips: Arc::default(),
            autofocus: false,
            hover_transition: Transition::default(),
            tab_stop: true,
        };
        let scene = Scene {
            hits: vec![h],
            ..Scene::default()
        };
        let mut i = Interaction::default();
        assert_eq!(i.press(&scene, 20., 20.), None);
        assert_eq!(i.motion(&scene, 40., 20., 0.), vec![2]);
        assert_eq!(i.release(40., 20.), Some(3));
    }
}
