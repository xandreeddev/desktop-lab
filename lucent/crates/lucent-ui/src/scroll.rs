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
        let maximum = if content_height.is_finite() {
            (content_height - viewport.h).max(0.)
        } else {
            0.
        };
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
    let requested = if style.gutter.is_finite() {
        style.gutter.clamp(0., width)
    } else {
        0.
    };
    let thumb = if style.width.is_finite() {
        style.width.clamp(0., width)
    } else {
        0.
    };
    requested.max(thumb)
}

/// A clipping ancestor moves with the scroll containers outside it, but not
/// with a viewport it defines itself. This lets keyboard traversal distinguish
/// off-screen content from content intentionally clipped by an ordinary card.
#[derive(Clone)]
pub struct FocusClip {
    pub rect: Rect,
    pub radius: f32,
    pub diagonal_reveal: Option<f32>,
    pub scroll_depth: usize,
}

fn visible(clip: &Clip, rect: Rect) -> bool {
    let clip = clip.intersect(rect);
    if clip.bounds.w <= 0. || clip.bounds.h <= 0. {
        return false;
    }
    // Match native input coverage at logical pixel centers, including corners.
    let first = (clip.bounds.y - 0.5).ceil() as i32;
    let end = (clip.bounds.y + clip.bounds.h - 0.5).ceil() as i32;
    (first..end).any(|row| clip.integer_span(row).is_some())
}
fn hit_visible<M>(clip: &Clip, hit: &crate::Hit<M>, target: Rect) -> bool {
    hit.parts.as_ref().map_or_else(
        || visible(clip, target),
        |parts| {
            parts.iter().any(|part| {
                visible(
                    clip,
                    Rect::new(
                        part.x + target.x - hit.rect.x,
                        part.y + target.y - hit.rect.y,
                        part.w,
                        part.h,
                    ),
                )
            })
        },
    )
}

pub(crate) fn reveal<'a, M>(
    hit: &'a crate::Hit<M>,
    buffer: &std::collections::BTreeMap<String, f32>,
) -> Option<Vec<(&'a ScrollHit<M>, f32)>> {
    if hit.rect.w <= 0. || hit.rect.h <= 0. {
        return None;
    }
    if hit.scroll_ancestors.is_empty() {
        return hit_visible(&hit.clip, hit, hit.rect).then(Vec::new);
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
            changes.push((scroll.as_ref(), value));
        }
    }
    let mut projected = Clip::from(target);
    for clip in hit.focus_clips.iter() {
        let mut bounds = clip.rect;
        bounds.y -= deltas.iter().take(clip.scroll_depth).sum::<f32>();
        projected = projected.with_rounded(bounds, clip.radius);
        if let Some(progress) = clip.diagonal_reveal {
            projected = projected.with_diagonal_reveal(bounds, progress);
        }
    }
    hit_visible(&projected, hit, target).then_some(changes)
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

    #[test]
    fn keyboard_scroll_projection_preserves_diagonal_ancestors() {
        let layout = Layout::new(Arc::new(vec![]));
        let content = Element::stack(vec![
            Element::empty()
                .size(16., 16.)
                .at(4., 4.)
                .on_click(Message::Click(1))
                .id("reachable"),
            Element::empty()
                .size(16., 16.)
                .at(60., 100.)
                .on_click(Message::Click(2))
                .id("masked"),
        ])
        .size(80., 140.);
        let root = Element::scroll(ScrollSpec::new(0., 16.).unwrap(), content, |event| {
            Message::Scroll(event.offset)
        })
        .size(80., 60.)
        .diagonal_reveal(0.25)
        .id("viewport");
        let mut input = Interaction::default();
        let scene = layout.build(&root, 80., 60., &input, 0.);
        input.synchronize(&scene);
        input.key(&scene, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("viewport"));
        input.key(&scene, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("reachable"));
        assert!(input.key(&scene, &Key::Tab).is_empty());
        assert_eq!(input.focus.as_deref(), Some("viewport"));
    }

    #[test]
    fn wheel_chains_only_to_ancestors_and_overlays_block_underlying_lists() {
        let layout = Layout::new(Arc::new(vec![]));
        let mut input = Interaction::default();
        let inner = Element::scroll(ScrollSpec::new(136., 40.).unwrap(), content(5), |e| {
            Message::Scroll(e.offset)
        })
        .size(180., 80.)
        .id("inner");
        let outer = Element::scroll(
            ScrollSpec::new(0., 40.).unwrap(),
            Element::column(vec![inner, Element::empty().size(180., 120.)]),
            |e| Message::Outer(e.offset),
        )
        .size(200., 100.)
        .id("outer");
        let scene = layout.build(&outer, 240., 140., &input, 0.);
        assert_eq!(
            input.scroll(&scene, 50., 50., 1.),
            Some(vec![Message::Outer(40.)])
        );
        assert_eq!(
            input.scroll(&scene, 50., 50., -1.),
            Some(vec![Message::Scroll(96.)])
        );
        let overlay = Element::empty()
            .size(200., 100.)
            .on_click(Message::Click(99))
            .id("overlay");
        let root = Element::stack(vec![outer, overlay]);
        let scene = layout.build(&root, 240., 140., &input, 0.);
        assert!(input.scroll(&scene, 50., 50., 1.).is_none());
    }

    #[test]
    fn capture_uses_new_callback_and_cancels_after_geometry_or_content_changes() {
        let layout = Layout::new(Arc::new(vec![]));
        let list = |count, height, outer| {
            Element::scroll(
                ScrollSpec::new(0., 40.).unwrap(),
                content(count),
                move |e| {
                    if outer {
                        Message::Outer(e.offset)
                    } else {
                        Message::Scroll(e.offset)
                    }
                },
            )
            .size(200., height)
            .id("list")
        };
        let mut input = Interaction::default();
        let first = layout.build(
            &Element::stack(vec![list(10, 100., false)]),
            240.,
            140.,
            &input,
            0.,
        );
        let thumb = first.hits[0].scroll.as_ref().unwrap().thumb;
        input.press(&first, thumb.x + 1., thumb.y + 1.);
        let replaced = layout.build(
            &Element::stack(vec![list(10, 100., true)]),
            240.,
            140.,
            &input,
            0.,
        );
        input.synchronize(&replaced);
        assert_eq!(input.release(thumb.x, 1000.), Some(Message::Outer(336.)));
        for (count, height) in [(10, 80.), (3, 100.), (0, 100.)] {
            input.press(&first, thumb.x + 1., thumb.y + 1.);
            let changed = layout.build(
                &Element::stack(vec![list(count, height, false)]),
                240.,
                140.,
                &input,
                0.,
            );
            input.synchronize(&changed);
            assert!(input.release(thumb.x, 1000.).is_none());
        }
    }

    #[test]
    fn mapping_gutter_and_shrink_preserve_controlled_bounds() {
        let layout = Layout::new(Arc::new(vec![]));
        let mut input = Interaction::default();
        let list = Element::scroll(ScrollSpec::new(10_000., 40.).unwrap(), content(10), |e| {
            Message::Scroll(e.offset)
        })
        .scroll_style(ScrollStyle {
            width: 24.,
            gutter: 0.,
            ..ScrollStyle::default()
        })
        .size(200., 100.)
        .id("list")
        .map(|m| match m {
            Message::Scroll(n) => Message::Outer(n),
            m => m,
        });
        let scene = layout.build(&list, 200., 100., &input, 0.);
        let scroll = scene.hits[0].scroll.as_ref().unwrap();
        assert_eq!(scroll.offset, 336.);
        assert!(
            scene
                .hits
                .iter()
                .skip(1)
                .all(|h| h.clip.bounds.x + h.clip.bounds.w <= scroll.track.x)
        );
        assert_eq!(
            input.scroll(&scene, 50., 50., -1.),
            Some(vec![Message::Outer(296.)])
        );
        let empty = Element::scroll(ScrollSpec::new(10_000., 40.).unwrap(), content(0), |e| {
            Message::Scroll(e.offset)
        })
        .size(200., 100.)
        .id("list");
        let scene = layout.build(&empty, 200., 100., &input, 0.);
        input.synchronize(&scene);
        assert_eq!(scene.hits[0].scroll.as_ref().unwrap().offset, 0.);
        assert_eq!(input.scroll(&scene, 50., 50., 1.), Some(vec![]));
        for value in [f32::NAN, f32::INFINITY] {
            let hit = ScrollHit::new(
                "invalid".into(),
                Rect::new(0., 0., 200., 100.),
                value,
                ScrollSpec::new(0., 40.).unwrap(),
                ScrollStyle::default(),
                Arc::new(|e| Message::Scroll(e.offset)),
            );
            assert_eq!(hit.maximum, 0.);
            assert!(hit.at(1000., 0.).is_none());
        }
    }

    #[test]
    fn keyboard_pages_accumulate_and_rounded_corners_are_not_focusable() {
        let layout = Layout::new(Arc::new(vec![]));
        let mut input = Interaction::default();
        let current = scene(0., &input);
        input.key(&current, &Key::Tab);
        assert_eq!(
            input.key(&current, &Key::PageDown),
            vec![Message::Scroll(44.)]
        );
        assert_eq!(
            input.key(&current, &Key::PageDown),
            vec![Message::Scroll(88.)]
        );
        assert_eq!(input.key(&current, &Key::Home), vec![Message::Scroll(0.)]);
        assert_eq!(input.key(&current, &Key::End), vec![Message::Scroll(352.)]);
        input.key(&current, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("row-0"));
        assert_eq!(
            input.key(&current, &Key::PageDown),
            vec![Message::Scroll(44.)]
        );
        let root = Element::scroll(
            ScrollSpec::new(0., 40.).unwrap(),
            Element::stack(vec![
                Element::empty()
                    .size(2., 2.)
                    .on_click(Message::Click(99))
                    .id("corner"),
                Element::empty()
                    .size(40., 20.)
                    .at(20., 20.)
                    .on_click(Message::Click(1))
                    .id("visible"),
            ])
            .size(100., 100.),
            |e| Message::Scroll(e.offset),
        )
        .size(100., 100.)
        .radius(40.)
        .id("round");
        let scene = layout.build(&root, 100., 100., &input, 0.);
        input.focus = None;
        input.key(&scene, &Key::Tab);
        input.key(&scene, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("visible"));
    }

    #[test]
    fn long_lists_share_ancestor_metadata_and_reveal_the_final_control() {
        let layout = Layout::new(Arc::new(vec![]));
        let mut input = Interaction::default();
        let root = Element::scroll(ScrollSpec::new(0., 40.).unwrap(), content(10_000), |e| {
            Message::Scroll(e.offset)
        })
        .size(200., 100.)
        .id("list");
        let scene = layout.build(&root, 200., 100., &input, 0.);
        assert!(Arc::ptr_eq(
            &scene.hits[1].scroll_ancestors,
            &scene.hits[10_000].scroll_ancestors
        ));
        assert!(Arc::ptr_eq(
            &scene.hits[1].focus_clips,
            &scene.hits[10_000].focus_clips
        ));
        input.key(&scene, &Key::BackTab);
        assert_eq!(input.focus.as_deref(), Some("row-9999"));
        assert_eq!(input.scroll_buffer["list"], 439_896.);
    }

    #[test]
    #[ignore = "opt-in CPU timing; records evidence without machine-dependent thresholds"]
    fn scroll_list_cpu_measurement() {
        use std::{hint::black_box, time::Instant};
        let layout = Layout::new(Arc::new(vec![]));
        for count in [100, 1_000, 10_000] {
            let root = Element::scroll(ScrollSpec::new(0., 40.).unwrap(), content(count), |e| {
                Message::Scroll(e.offset)
            })
            .size(200., 400.)
            .id("list");
            let mut input = Interaction::default();
            let scene = layout.build(&root, 200., 400., &input, 0.);
            let start = Instant::now();
            for _ in 0..20 {
                black_box(layout.build(&root, 200., 400., &input, 0.));
            }
            let layout_ms = start.elapsed().as_secs_f64() * 1000. / 20.;
            let start = Instant::now();
            for _ in 0..500 {
                black_box(input.key(&scene, &Key::Tab));
            }
            let tab_us = start.elapsed().as_secs_f64() * 1_000_000. / 500.;
            println!(
                "SCROLL_CPU {{\"rows\":{count},\"layout_mean_ms\":{layout_ms:.4},\"tab_mean_us\":{tab_us:.4},\"hits\":{}}}",
                scene.hits.len()
            );
        }
    }
}
