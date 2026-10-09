//! Layout, hit testing and retained interaction state for declarative components.
use lucent_api::*;
pub mod text;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub enum Paint {
    Outline {
        rect: Rect,
        clip: Rect,
        color: Color,
        radius: f32,
        width: f32,
    },
    Shape {
        rect: Rect,
        clip: Rect,
        color: Color,
        radius: f32,
        shadow: bool,
    },
    Text {
        rect: Rect,
        clip: Rect,
        text: String,
        size: f32,
        face: usize,
        color: Color,
        align: Align,
    },
    Image {
        rect: Rect,
        clip: Rect,
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
    pub clip: Rect,
    pub radius: f32,
    pub click: Option<M>,
    pub hover: Option<M>,
    pub input: Option<InputCallback<M>>,
    pub value: String,
    pub drag: Option<DragCallback<M>>,
    pub autofocus: bool,
    pub hover_transition: Transition,
    pub tab_stop: bool,
}
impl<M> Hit<M> {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        if !self.rect.contains(x, y) || !self.clip.contains(x, y) {
            return false;
        }
        let r = self.radius.min(self.rect.w / 2.).min(self.rect.h / 2.);
        let dx = (x - self.rect.x - self.rect.w / 2.).abs() - (self.rect.w / 2. - r);
        let dy = (y - self.rect.y - self.rect.h / 2.).abs() - (self.rect.h / 2. - r);
        dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) <= r
    }
}
pub struct Scene<M> {
    pub paint: Vec<Paint>,
    pub hits: Vec<Hit<M>>,
    pub regions: Vec<(Rect, f32)>,
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

    fn measure<M>(&self, e: &Element<M>, available: (f32, f32)) -> (f32, f32) {
        let px = e.style.padding.horizontal * 2.;
        let py = e.style.padding.vertical * 2.;
        let gap = e.style.gap;
        let children: Vec<_> = e
            .children
            .iter()
            .map(|c| self.measure(c, ((available.0 - px).max(0.), (available.1 - py).max(0.))))
            .collect();
        let n = children.len();
        let gaps = gap * n.saturating_sub(1) as f32;
        let content = match &e.kind {
            Kind::Text(t) => (
                self.text_width(t, e.style.font_size, e.style.font_face),
                text::line_metrics(
                    self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]),
                    e.style.font_size,
                )
                .height
                    * t.lines().count().max(1) as f32,
            ),
            Kind::Input { .. } => (
                160.,
                text::line_metrics(
                    self.fonts.get(e.style.font_face).unwrap_or(&self.fonts[0]),
                    e.style.font_size,
                )
                .height,
            ),
            Kind::Image(_) => (32., 32.),
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
            Kind::Stack => (
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
        self.place(root, rect, rect, 1., &mut scene, interaction, now, "root");
        scene
    }
    #[allow(clippy::too_many_arguments)]
    fn place<M: Clone>(
        &self,
        e: &Element<M>,
        rect: Rect,
        clip: Rect,
        opacity: f32,
        scene: &mut Scene<M>,
        interaction: &Interaction<M>,
        now: f64,
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
        let own_clip = if e.style.clip {
            clip.intersect(rect)
        } else {
            clip
        };
        let interactive =
            e.click.is_some() || e.input.is_some() || e.drag.is_some() || e.hover.is_some();
        if e.style.background.3 > 0. && alpha > 0. {
            let mut color = e.style.background;
            if e.click.is_some() || e.input.is_some() {
                color = color.mix(
                    e.style.hover_color.alpha(color.3),
                    interaction.hover_amount(&id, now) * e.style.hover_color.3,
                );
            }
            color.3 *= alpha;
            if e.style.shadow {
                scene.paint.push(Paint::Shape {
                    rect: Rect::new(rect.x, rect.y + e.style.shadow_offset, rect.w, rect.h),
                    clip,
                    color: e.style.shadow_color.alpha(e.style.shadow_color.3 * alpha),
                    radius: e.style.radius,
                    shadow: true,
                });
            }
            scene.paint.push(Paint::Shape {
                rect,
                clip,
                color,
                radius: e.style.radius,
                shadow: false,
            });
        }
        if interactive {
            scene.regions.push((rect.intersect(clip), e.style.radius));
            scene.hits.push(Hit {
                id: id.clone(),
                rect,
                clip,
                radius: e.style.radius,
                click: e.click.clone(),
                hover: e.hover.clone(),
                input: e.input.clone(),
                value: if let Kind::Input { value, .. } = &e.kind {
                    value.clone()
                } else {
                    String::new()
                },
                drag: e.drag.clone(),
                autofocus: e.autofocus,
                hover_transition: e.style.hover_transition,
                tab_stop: e.style.tab_stop && (e.click.is_some() || e.input.is_some()),
            });
        }
        let p = e.style.padding;
        let inner = Rect::new(
            rect.x + p.horizontal,
            rect.y + p.vertical,
            (rect.w - 2. * p.horizontal).max(0.),
            (rect.h - 2. * p.vertical).max(0.),
        );
        let mut fg = e.style.foreground;
        fg.3 *= alpha;
        match &e.kind {
            Kind::Text(text) => scene.paint.push(Paint::Text {
                rect: inner,
                clip: own_clip.intersect(rect),
                text: text.clone(),
                size: e.style.font_size,
                face: e.style.font_face,
                color: fg,
                align: e.style.align,
            }),
            Kind::Input { value, placeholder } => {
                let text = if value.is_empty() {
                    placeholder.clone()
                } else {
                    value.clone()
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
                    let advance = self.text_width(value, e.style.font_size, e.style.font_face);
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
                        shadow: false,
                    });
                }
            }
            Kind::Image(data) => scene.paint.push(Paint::Image {
                rect: if e.style.image_contain && data.width > 0 && data.height > 0 {
                    let factor = (inner.w / data.width as f32).min(inner.h / data.height as f32);
                    let (w, h) = (data.width as f32 * factor, data.height as f32 * factor);
                    Rect::new(
                        inner.x + (inner.w - w) / 2.,
                        inner.y + (inner.h - h) / 2.,
                        w,
                        h,
                    )
                } else {
                    inner
                },
                clip: own_clip,
                data: data.clone(),
                radius: e.style.radius,
                opacity: alpha,
                tint: e.style.image_tint,
            }),
            _ => {}
        }
        let is_row = matches!(e.kind, Kind::Row);
        let is_column = matches!(e.kind, Kind::Column);
        let mut measured: Vec<_> = e
            .children
            .iter()
            .map(|c| self.measure(c, (inner.w, inner.h)))
            .collect();
        if is_row || is_column {
            let fixed: f32 = e
                .children
                .iter()
                .zip(&measured)
                .filter(|(c, _)| {
                    if is_row {
                        c.style.width != Length::Fill
                    } else {
                        c.style.height != Length::Fill
                    }
                })
                .map(|(_, s)| if is_row { s.0 } else { s.1 })
                .sum();
            let count = e
                .children
                .iter()
                .filter(|c| {
                    if is_row {
                        c.style.width == Length::Fill
                    } else {
                        c.style.height == Length::Fill
                    }
                })
                .count();
            let space = ((if is_row { inner.w } else { inner.h })
                - fixed
                - e.style.gap * e.children.len().saturating_sub(1) as f32)
                .max(0.);
            for (c, s) in e.children.iter().zip(&mut measured) {
                if is_row && c.style.width == Length::Fill {
                    s.0 = space / count.max(1) as f32;
                }
                if is_column && c.style.height == Length::Fill {
                    s.1 = space / count.max(1) as f32;
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
                    let cw = ((inner.w - e.style.gap * (cols - 1) as f32) / cols as f32).max(0.);
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
                own_clip,
                alpha,
                scene,
                interaction,
                now,
                &format!("{path}/{i}"),
            );
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

struct Press<M> {
    hit: Hit<M>,
    x: f32,
    y: f32,
    moved: bool,
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
        }
    }
}
impl<M: Clone> Interaction<M> {
    pub fn synchronize(&mut self, scene: &Scene<M>) {
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
            if press.moved
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
    pub fn press(&mut self, scene: &Scene<M>, x: f32, y: f32) {
        self.focus_visible = false;
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
                Press {
                    hit,
                    x,
                    y,
                    moved: false,
                }
            });
    }
    pub fn release(&mut self, x: f32, y: f32) -> Option<M> {
        let p = self.press.take()?;
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
    pub fn key(&mut self, scene: &Scene<M>, key: &Key) -> Option<M> {
        self.focus_visible = true;
        if matches!(key, Key::Tab | Key::BackTab) {
            let stops: Vec<_> = scene
                .hits
                .iter()
                .filter(|h| {
                    h.tab_stop && h.rect.intersect(h.clip).w > 0. && h.rect.intersect(h.clip).h > 0.
                })
                .collect();
            if !stops.is_empty() {
                let current = stops
                    .iter()
                    .position(|h| Some(&h.id) == self.focus.as_ref());
                let next = match (current, key) {
                    (Some(i), Key::BackTab) => (i + stops.len() - 1) % stops.len(),
                    (Some(i), _) => (i + 1) % stops.len(),
                    (None, Key::BackTab) => stops.len() - 1,
                    (None, _) => 0,
                };
                let hit = stops[next];
                self.focus = Some(hit.id.clone());
                self.select_all = false;
                self.input_buffer = hit
                    .input
                    .as_ref()
                    .map(|_| (hit.id.clone(), hit.value.clone()));
            }
            return None;
        }
        if (matches!(key, Key::Enter) || matches!(key, Key::Text(s) if s == " "))
            && let Some(hit) = scene
                .hits
                .iter()
                .find(|h| Some(&h.id) == self.focus.as_ref() && h.input.is_none())
        {
            return hit.click.clone();
        }
        let hit = scene
            .hits
            .iter()
            .find(|h| Some(&h.id) == self.focus.as_ref() && h.input.is_some())?;
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
    fn rounded_hit_test_leaves_corners_click_through() {
        let h: Hit<()> = Hit {
            id: "a".into(),
            rect: Rect::new(0., 0., 100., 100.),
            clip: Rect::new(0., 0., 100., 100.),
            radius: 20.,
            click: None,
            hover: None,
            input: None,
            value: String::new(),
            drag: None,
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
            clip: Rect::new(0., 0., 100., 30.),
            radius: 0.,
            click: None,
            hover: None,
            input: Some(Arc::new(|s: String| s)),
            value: String::new(),
            drag: None,
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
        assert_eq!(input.key(&scene, &Key::Text("f".into())), Some("f".into()));
        assert_eq!(
            input.key(&scene, &Key::Text("oo".into())),
            Some("foo".into())
        );
        assert_eq!(
            input.key(&scene, &Key::Text("t".into())),
            Some("foot".into())
        );
        input.key(&scene, &Key::SelectAll);
        assert_eq!(input.key(&scene, &Key::Text("é".into())), Some("é".into()));
        assert_eq!(input.key(&scene, &Key::Backspace), Some(String::new()));
    }
    #[test]
    fn drag_threshold_prevents_accidental_click() {
        let h = Hit {
            id: "a".into(),
            rect: Rect::new(0., 0., 100., 100.),
            clip: Rect::new(0., 0., 100., 100.),
            radius: 0.,
            click: Some(1),
            hover: None,
            input: None,
            value: String::new(),
            drag: Some(Arc::new(|d: DragEvent| if d.finished { 3 } else { 2 })),
            autofocus: false,
            hover_transition: Transition::default(),
            tab_stop: true,
        };
        let scene = Scene {
            hits: vec![h],
            ..Scene::default()
        };
        let mut i = Interaction::default();
        i.press(&scene, 20., 20.);
        assert_eq!(i.motion(&scene, 40., 20., 0.), vec![2]);
        assert_eq!(i.release(40., 20.), Some(3));
    }
}
