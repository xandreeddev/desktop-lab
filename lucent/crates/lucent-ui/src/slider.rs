use crate::{Paint, Scene};
use lucent_api::*;

#[derive(Clone)]
pub struct SliderHit<M> {
    pub spec: SliderSpec,
    pub start: f32,
    pub width: f32,
    pub callback: SliderCallback<M>,
}
impl<M> SliderHit<M> {
    pub fn event(&self, x: f32, finished: bool) -> Option<M> {
        if self.width <= 0. {
            return None;
        }
        let value = self.spec.at((x - self.start) / self.width)?;
        Some((self.callback)(SliderEvent { value, finished }))
    }
}

pub(crate) fn geometry(rect: Rect, padding: Padding, style: SliderStyle) -> (Rect, f32) {
    let inner = Rect::new(
        rect.x + padding.horizontal,
        rect.y + padding.vertical,
        rect.w - padding.horizontal * 2.,
        rect.h - padding.vertical * 2.,
    );
    let diameter = style.thumb_diameter.max(0.).min(inner.w).min(inner.h);
    let height = style.track_height.max(0.).min(inner.h);
    (
        Rect::new(
            inner.x + diameter / 2.,
            inner.y + (inner.h - height) / 2.,
            inner.w - diameter,
            height,
        ),
        diameter,
    )
}

pub(crate) fn paint<M>(
    scene: &mut Scene<M>,
    rect: Rect,
    clip: Clip,
    spec: SliderSpec,
    style: &Style,
    alpha: f32,
) {
    let (track, diameter) = geometry(rect, style.padding, style.slider);
    let filled = track.w * spec.value();
    for (rect, color, radius) in [
        (track, style.slider.track, track.h / 2.),
        (
            Rect::new(track.x, track.y, filled, track.h),
            style.slider.fill,
            track.h / 2.,
        ),
        (
            Rect::new(
                track.x + filled - diameter / 2.,
                track.y + (track.h - diameter) / 2.,
                diameter,
                diameter,
            ),
            style.slider.thumb,
            diameter / 2.,
        ),
    ] {
        scene.paint.push(Paint::Shape {
            rect,
            clip: clip.clone(),
            color: color.alpha(color.3 * alpha),
            radius,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interaction, Layout};
    use std::sync::Arc;

    fn element(value: f32) -> Element<SliderEvent> {
        Element::slider(SliderSpec::new(value, 0.1).unwrap(), |event| event)
            .id("level")
            .size(200., 40.)
            .padding_xy(8., 0.)
            .at(10., 10.)
    }
    fn scene(value: f32, interaction: &Interaction<SliderEvent>) -> Scene<SliderEvent> {
        Layout::new(Arc::new(vec![])).build(
            &Element::stack(vec![element(value)]),
            300.,
            80.,
            interaction,
            0.,
        )
    }
    fn value(event: Option<SliderEvent>, expected: f32, finished: bool) {
        let event = event.expect("slider event");
        assert!((event.value - expected).abs() < 0.00001, "{event:?}");
        assert_eq!(event.finished, finished);
    }
    #[test]
    fn pointer_capture_uses_painted_track_and_commits_outside_bounds() {
        let mut interaction = Interaction::default();
        let scene = scene(0.2, &interaction);
        let track = scene.hits[0].slider.as_ref().unwrap();
        value(
            interaction.press(&scene, track.start + track.width / 2., 30.),
            0.5,
            false,
        );
        assert_eq!(interaction.focus.as_deref(), Some("level"));
        let motion = interaction.motion(&scene, 1000., -100., 0.);
        value(motion.last().copied(), 1., false);
        value(interaction.release(-100., 30.), 0., true);
        assert!(interaction.release(100., 30.).is_none());
    }
    #[test]
    fn keyboard_repeats_accumulate_before_a_new_frame_and_tab_focuses() {
        let mut interaction = Interaction::default();
        let first = scene(0.2, &interaction);
        interaction.key(&first, &Key::Tab);
        assert_eq!(interaction.focus.as_deref(), Some("level"));
        assert!(interaction.focus_visible);
        value(
            interaction.key(&first, &Key::Right).into_iter().next(),
            0.3,
            true,
        );
        value(
            interaction.key(&first, &Key::Up).into_iter().next(),
            0.4,
            true,
        );
        value(
            interaction.key(&first, &Key::Left).into_iter().next(),
            0.3,
            true,
        );
        value(
            interaction.key(&first, &Key::Home).into_iter().next(),
            0.,
            true,
        );
        value(
            interaction.key(&first, &Key::End).into_iter().next(),
            1.,
            true,
        );
        let next = scene(0.6, &interaction);
        interaction.synchronize(&next);
        value(
            interaction.key(&next, &Key::Down).into_iter().next(),
            0.5,
            true,
        );
    }
    #[test]
    fn hidden_control_cancels_capture_and_clipped_area_cannot_start_it() {
        let mut interaction = Interaction::default();
        let mut first = scene(0.2, &interaction);
        first.hits[0].clip = Rect::new(100., 10., 110., 40.).into();
        assert!(interaction.press(&first, 20., 30.).is_none());
        assert!(interaction.press(&first, 110., 30.).is_some());
        interaction.synchronize(&Scene::default());
        assert!(interaction.release(140., 30.).is_none());
    }
    #[test]
    fn mapped_callbacks_and_thumb_fit_the_padded_control_at_both_ends() {
        for fraction in [0., 1.] {
            let root = Element::stack(vec![element(fraction).map(|e| (e.value, e.finished))]);
            let layout = Layout::new(Arc::new(vec![]));
            let mut interaction = Interaction::default();
            let scene = layout.build(&root, 300., 80., &interaction, 0.);
            assert_eq!(interaction.press(&scene, 110., 30.), Some((0.5, false)));
            let hit = &scene.hits[0];
            for paint in &scene.paint {
                if let Paint::Shape { rect, .. } = paint {
                    assert!(rect.x >= hit.rect.x + 8.);
                    assert!(rect.x + rect.w <= hit.rect.x + hit.rect.w - 8.);
                    assert!(rect.y >= hit.rect.y && rect.y + rect.h <= hit.rect.y + hit.rect.h);
                }
            }
        }
    }
    #[test]
    fn zero_width_track_and_nonfinite_pointer_do_not_emit_values() {
        let mut interaction = Interaction::default();
        let mut scene = scene(0., &interaction);
        scene.hits[0].slider.as_mut().unwrap().width = 0.;
        assert!(interaction.press(&scene, 110., 30.).is_none());
        assert!(interaction.release(110., 30.).is_none());
        assert!(
            scene.hits[0]
                .slider
                .as_ref()
                .unwrap()
                .event(f32::NAN, true)
                .is_none()
        );
    }
}
