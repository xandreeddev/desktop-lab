use lucent_api::*;
#[derive(Clone)]
pub struct ScrollHit<M> {
    pub id: String,
    pub viewport: Rect,
    pub offset: f32,
    pub maximum: f32,
    pub line_step: f32,
    pub track: Rect,
    pub thumb: Rect,
    pub callback: ScrollCallback<M>,
}
impl<M> ScrollHit<M> {
    pub(crate) fn new(
        id: String,
        viewport: Rect,
        content_height: f32,
        spec: ScrollSpec,
        style: ScrollStyle,
        callback: ScrollCallback<M>,
    ) -> Self {
        let maximum = (content_height - viewport.h).max(0.);
        let offset = spec.offset().min(maximum);
        let width = if style.width.is_finite() {
            style.width.clamp(0., viewport.w)
        } else {
            0.
        };
        let track = Rect::new(
            viewport.x + viewport.w - width,
            viewport.y,
            width,
            viewport.h,
        );
        let min_thumb = if style.minimum_thumb.is_finite() {
            style.minimum_thumb.max(0.)
        } else {
            0.
        };
        let thumb_height = if maximum > 0. {
            (viewport.h * viewport.h / content_height)
                .max(min_thumb)
                .min(viewport.h)
        } else {
            viewport.h
        };
        let y = if maximum > 0. {
            offset / maximum * (track.h - thumb_height)
        } else {
            0.
        };
        let thumb = Rect::new(track.x, track.y + y, track.w, thumb_height);
        Self {
            id,
            viewport,
            offset,
            maximum,
            line_step: spec.line_step(),
            track,
            thumb,
            callback,
        }
    }
    pub(crate) fn event(&self, offset: f32) -> M {
        (self.callback)(ScrollEvent {
            offset: offset.clamp(0., self.maximum),
        })
    }
    pub(crate) fn at(&self, y: f32, grab: f32) -> Option<f32> {
        if !y.is_finite() || self.track.h <= self.thumb.h {
            return None;
        }
        Some(
            ((y - self.track.y - grab) / (self.track.h - self.thumb.h) * self.maximum)
                .clamp(0., self.maximum),
        )
    }
}
pub(crate) fn gutter(style: ScrollStyle, width: f32) -> f32 {
    if style.gutter.is_finite() {
        style.gutter.clamp(0., width)
    } else {
        0.
    }
}

/// A clipping ancestor moves with the scroll containers outside it, but not
/// with a viewport it defines itself. This lets keyboard traversal distinguish
/// off-screen content from content intentionally clipped by an ordinary card.
#[derive(Clone)]
pub struct FocusClip {
    pub rect: Rect,
    pub scroll_depth: usize,
}

pub(crate) fn reveal<'a, M>(
    hit: &'a crate::Hit<M>,
    buffer: &std::collections::BTreeMap<String, f32>,
) -> Option<Vec<(&'a ScrollHit<M>, f32)>> {
    if hit.rect.w <= 0. || hit.rect.h <= 0. {
        return None;
    }
    if hit.scroll_ancestors.is_empty() {
        let visible = hit.rect.intersect(hit.clip.bounds);
        return (visible.w > 0. && visible.h > 0.).then(Vec::new);
    }
    let mut target = hit.rect;
    let mut deltas = vec![0.; hit.scroll_ancestors.len()];
    let mut changes = vec![];
    for (index, scroll) in hit.scroll_ancestors.iter().enumerate().rev() {
        if scroll.viewport.w <= 0. || scroll.viewport.h <= 0. {
            return None;
        }
        let previous = *buffer.get(&scroll.id).unwrap_or(&scroll.offset);
        target.y -= previous - scroll.offset;
        let shift = if target.y < scroll.viewport.y {
            target.y - scroll.viewport.y
        } else if target.y + target.h > scroll.viewport.y + scroll.viewport.h {
            (target.y + target.h - scroll.viewport.y - scroll.viewport.h)
                .min(target.y - scroll.viewport.y)
        } else {
            0.
        };
        let value = (previous + shift).clamp(0., scroll.maximum);
        target.y -= value - previous;
        deltas[index] = value - scroll.offset;
        if value != previous {
            changes.push((scroll, value));
        }
    }
    let mut visible = target;
    for clip in &hit.focus_clips {
        let mut bounds = clip.rect;
        bounds.y -= deltas.iter().take(clip.scroll_depth).sum::<f32>();
        visible = visible.intersect(bounds);
    }
    (visible.w > 0. && visible.h > 0.).then_some(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interaction, Layout, Scene};
    use std::sync::Arc;
    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Scroll(f32),
        Click(usize),
        Outer(f32),
    }
    fn content(count: usize) -> Element<Message> {
        Element::column(
            (0..count)
                .map(|n| {
                    Element::empty()
                        .size(160., 40.)
                        .on_click(Message::Click(n))
                        .id(format!("row-{n}"))
                })
                .collect(),
        )
        .width(Length::Fill)
        .gap(4.)
    }
    fn tree(offset: f32) -> Element<Message> {
        Element::stack(vec![
            Element::scroll(ScrollSpec::new(offset, 40.).unwrap(), content(10), |e| {
                Message::Scroll(e.offset)
            })
            .size(200., 100.)
            .padding(8.)
            .radius(20.)
            .at(10., 10.)
            .id("list"),
        ])
        .fill()
    }
    fn scene(offset: f32, interaction: &Interaction<Message>) -> Scene<Message> {
        Layout::new(Arc::new(vec![])).build(&tree(offset), 240., 140., interaction, 0.)
    }
    fn offset(messages: Vec<Message>) -> f32 {
        let [Message::Scroll(value)] = messages.as_slice() else {
            panic!("expected one scroll event: {messages:?}")
        };
        *value
    }
    #[test]
    fn scroll_bursts_clamp_and_clipping_matches_pointer_hits() {
        let mut input = Interaction::default();
        let scene = scene(0., &input);
        assert_eq!(offset(input.scroll(&scene, 50., 50., 1.).unwrap()), 40.);
        assert_eq!(offset(input.scroll(&scene, 50., 50., 1.).unwrap()), 80.);
        assert!(input.scroll(&scene, 235., 130., 1.).is_none());
        let last = scene.hits.iter().find(|h| h.id == "row-9").unwrap();
        assert!(!last.contains(last.rect.x + 2., last.rect.y + 2.));
        assert_eq!(offset(input.scroll(&scene, 50., 50., 1e30).unwrap()), 352.);
        assert_eq!(input.scroll(&scene, 50., 50., 1.), Some(vec![]));
        assert_eq!(input.scroll(&scene, 50., 50., f64::NAN), Some(vec![]));
    }
    #[test]
    fn tab_reveals_every_row_and_backtab_returns_to_start() {
        let mut input = Interaction::default();
        let mut value = 0.;
        for n in 0..11 {
            let current = scene(value, &input);
            input.synchronize(&current);
            let messages = input.key(&current, &Key::Tab);
            if !messages.is_empty() {
                value = offset(messages);
            }
            let updated = scene(value, &input);
            let focused = updated
                .hits
                .iter()
                .find(|h| Some(&h.id) == input.focus.as_ref())
                .unwrap();
            assert_eq!(
                focused.id,
                if n == 0 {
                    "list".into()
                } else {
                    format!("row-{}", n - 1)
                }
            );
            assert_eq!(
                focused.rect.intersect(focused.clip.bounds).h,
                focused.rect.h
            );
        }
        assert_eq!(value, 352.);
        for _ in 0..9 {
            let current = scene(value, &input);
            input.synchronize(&current);
            let messages = input.key(&current, &Key::BackTab);
            if !messages.is_empty() {
                value = offset(messages);
            }
        }
        assert_eq!(input.focus.as_deref(), Some("row-0"));
        assert_eq!(value, 0.);
    }
    #[test]
    fn scrollbar_capture_reaches_both_ends_and_cancels_when_removed() {
        let mut input = Interaction::default();
        let current = scene(0., &input);
        let scroll = current.hits.iter().find_map(|h| h.scroll.as_ref()).unwrap();
        let x = scroll.thumb.x + scroll.thumb.w / 2.;
        let y = scroll.thumb.y + scroll.thumb.h / 2.;
        assert_eq!(input.press(&current, x, y), Some(Message::Scroll(0.)));
        assert_eq!(offset(input.motion(&current, x, 1000., 0.)), 352.);
        assert_eq!(input.release(x, -100.), Some(Message::Scroll(0.)));
        input.press(&current, x, y);
        input.synchronize(&Scene::default());
        assert!(input.release(x, 1000.).is_none());
    }
    #[test]
    fn nested_keyboard_reveal_keeps_normal_clipping() {
        let layout = Layout::new(Arc::new(vec![]));
        let mut input = Interaction::default();
        let inner = Element::scroll(ScrollSpec::new(0., 40.).unwrap(), content(5), |e| {
            Message::Scroll(e.offset)
        })
        .size(180., 80.)
        .id("inner");
        let hidden = Element::stack(vec![
            Element::empty()
                .size(100., 20.)
                .at(0., 100.)
                .on_click(Message::Click(99))
                .id("hidden"),
        ])
        .size(180., 20.)
        .clip();
        let root = Element::scroll(
            ScrollSpec::new(0., 40.).unwrap(),
            Element::column(vec![Element::empty().size(180., 120.), inner, hidden]),
            |e| Message::Outer(e.offset),
        )
        .size(200., 100.)
        .id("outer");
        let scene = layout.build(&root, 200., 100., &input, 0.);
        input.key(&scene, &Key::Tab); // outer viewport
        assert_eq!(input.key(&scene, &Key::Tab), vec![Message::Outer(100.)]); // inner viewport
        for _ in 0..5 {
            input.key(&scene, &Key::Tab);
        }
        assert_eq!(input.focus.as_deref(), Some("row-4"));
        input.key(&scene, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("outer")); // clipped child is not reachable
    }
}
