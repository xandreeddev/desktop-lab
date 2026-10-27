//! Transient wallpaper presentation. Selection, decoding and persistence stay
//! with their existing owners; this child borrows already prepared images.
use crate::presentation_frame::PresentationFrame;
#[cfg(test)]
use lucent_api::ImageData;
use lucent_api::{Color, Element, Motion};
use lucent_design::motion;
#[cfg(test)]
use std::sync::Arc;

/// Content identity comes from the selected immutable asset, never a video's
/// per-frame allocation or revision. Static and live assets have separate roles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Static(String),
    Live(String),
}

struct Current {
    source: Source,
    image: PresentationFrame,
}

enum Reveal {
    Settled,
    AwaitingSubmission { selected_at: f64 },
    Running(Motion),
}
impl Reveal {
    fn value(&self, now: f64) -> f32 {
        match self {
            Self::Settled => 1.,
            Self::AwaitingSubmission { .. } => 0.,
            Self::Running(motion) => motion.value(now).clamp(0., 1.),
        }
    }
    fn active(&self, now: f64) -> bool {
        match self {
            Self::Settled => false,
            Self::AwaitingSubmission { .. } => true,
            Self::Running(motion) => motion.active(now),
        }
    }
}

pub struct BackgroundPresentation {
    current: Option<Current>,
    outgoing: Option<PresentationFrame>,
    reveal: Reveal,
}

impl Default for BackgroundPresentation {
    fn default() -> Self {
        Self {
            current: None,
            outgoing: None,
            reveal: Reveal::Settled,
        }
    }
}

impl BackgroundPresentation {
    /// Admit prepared pixels for the current selection. A missing candidate
    /// retains the last good presentation while decoding/export is pending.
    /// Returns whether the root should invalidate this exact background.
    #[cfg(test)]
    pub fn reconcile(
        &mut self,
        source: Option<Source>,
        image: Option<Arc<ImageData>>,
        now: f64,
    ) -> bool {
        self.reconcile_frame(source, image.map(PresentationFrame::from), now)
    }
    pub fn reconcile_frame(
        &mut self,
        source: Option<Source>,
        image: Option<PresentationFrame>,
        now: f64,
    ) -> bool {
        let (Some(source), Some(image)) = (source, image) else {
            return false;
        };
        let Some(previous) = self.current.take() else {
            self.current = Some(Current { source, image });
            return true;
        };
        if previous.source == source {
            let changed = previous.image.cache_id() != image.cache_id();
            // Two revisions of one stream share a GPU texture. Never put them
            // in separate layers, even if a caller recycles an asset identity.
            if self
                .outgoing
                .as_ref()
                .is_some_and(|old| same_stream(old, &image))
            {
                self.outgoing = None;
                self.reveal = Reveal::Settled;
            }
            self.current = Some(Current { source, image });
            return changed;
        }

        // An interrupted third selection cannot preserve a two-image mixture
        // exactly without allocating a rendered snapshot. Retain only its
        // dominant visible layer, keeping the resource bound at two images.
        let outgoing = if self.reveal.value(now) < 0.5 {
            self.outgoing.take().unwrap_or(previous.image)
        } else {
            previous.image
        };
        if outgoing.cache_id() == image.cache_id() || same_stream(&outgoing, &image) {
            self.outgoing = None;
            self.reveal = Reveal::Settled;
        } else {
            // A different output may still consume this live stream. Give the
            // frozen outgoing frame independent GPU identity without copying
            // its immutable CPU pixels or making the UI prepare mip levels.
            self.outgoing = Some(outgoing.snapshot());
            self.reveal = Reveal::AwaitingSubmission { selected_at: now };
        }
        self.current = Some(Current { source, image });
        true
    }

    pub fn view<M>(&self, now: f64, fallback: Color) -> Element<M> {
        let background = Element::empty().fill().background(fallback);
        let Some(current) = &self.current else {
            return background;
        };
        let image = |image: &PresentationFrame| image.element().cover().fill();
        let mut layers = Vec::with_capacity(3);
        // The cover image spans this surface. Omit the solid underlay only
        // when the bottom image proves full coverage; transparent media and
        // a transparent outgoing frame still need the theme fallback.
        let base = self.outgoing.as_ref().unwrap_or(&current.image);
        if !base.is_opaque() {
            layers.push(background);
        }
        if let Some(outgoing) = &self.outgoing {
            // Revealed transparent pixels belong to the new wallpaper's
            // fallback, rather than the old image. Clip that composition as
            // one subtree so revealed interior pixels no longer borrow it.
            // Opaque media keeps the single-image presentation fast path.
            layers.push(image(outgoing));
            let incoming = if current.image.is_opaque() {
                image(&current.image)
            } else {
                Element::stack(vec![
                    Element::empty().fill().background(fallback),
                    image(&current.image),
                ])
                .fill()
            };
            layers.push(incoming.diagonal_reveal(self.reveal.value(now)));
        } else {
            layers.push(image(&current.image));
        }
        Element::stack(layers).fill()
    }

    pub fn active(&self, now: f64) -> bool {
        self.outgoing.is_some() && self.reveal.active(now)
    }

    /// Start only after a frame for this selection submits. Time spent waiting
    /// for a compositor callback or rendering its first frame is not animation
    /// time. Completion uses the sampled view time, so a slow submission cannot
    /// retire the outgoing image before a fully revealed frame was rendered.
    /// Neither timestamp is a compositor scanout acknowledgement.
    pub fn presented(&mut self, sampled_at: f64, submitted_at: f64) -> bool {
        if let Reveal::AwaitingSubmission { selected_at } = self.reveal {
            if sampled_at < selected_at {
                return false;
            }
            let mut reveal = Motion::fixed(0.);
            reveal.target(
                1.,
                submitted_at.max(sampled_at),
                motion::WALLPAPER_REVEAL,
                motion::CUT,
            );
            self.reveal = Reveal::Running(reveal);
            return true;
        }
        if self.outgoing.is_some() && !self.reveal.active(sampled_at) {
            self.outgoing = None;
            self.reveal = Reveal::Settled;
            true
        } else {
            false
        }
    }

    pub fn inspect(&self) -> serde_json::Value {
        let mut images = Vec::new();
        if let Some(current) = &self.current {
            images.push(&current.image);
        }
        if let Some(outgoing) = &self.outgoing
            && images
                .iter()
                .all(|image| image.cache_id() != outgoing.cache_id())
        {
            images.push(outgoing);
        }
        serde_json::json!({
            "retained_images": images.len(),
            "retained_frame_bytes": images.iter().map(|image|image.bytes()).sum::<usize>(),
            "transition_pending": self.outgoing.is_some(),
            "awaiting_first_frame": matches!(self.reveal, Reveal::AwaitingSubmission { .. }),
        })
    }
}

fn same_stream(a: &PresentationFrame, b: &PresentationFrame) -> bool {
    match (a.stream_revision(), b.stream_revision()) {
        (Some((a, _)), Some((b, _))) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{ImageStream, Kind};

    fn image(key: &str, color: [u8; 4]) -> Arc<ImageData> {
        Arc::new(ImageData::from_rgba(key, 1, 1, color.to_vec()).unwrap())
    }

    fn source(key: &str) -> Option<Source> {
        Some(Source::Static(key.into()))
    }

    fn top_progress(presentation: &BackgroundPresentation, now: f64) -> f32 {
        let view: Element<()> = presentation.view(now, Color::hex(0));
        view.children
            .last()
            .unwrap()
            .style
            .diagonal_reveal
            .unwrap_or(1.)
    }

    #[test]
    fn desktop_routes_transition_submission_and_retires_disconnected_hosts() {
        use crate::desktop::{Desktop, Message};
        use lucent_api::{
            Application, Component, DisplayOutput, Effects, Event, OutputName, SurfaceAddress,
        };
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.wallpaper.background = Some(image("initial", [255, 0, 0, 255]));
        app.update(Message::Ignore, &mut Effects::default());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&app.inspect()).unwrap()["background_transitions"].as_array().unwrap().len(), 1);
        app.update(
            Message::Outputs(vec![DisplayOutput {
                global: 1,
                name: Some(OutputName::new("DP-1").unwrap()),
                logical_size: Some((1280, 800)),
                integer_scale: 1,
            }]),
            &mut Effects::default(),
        );
        let instance = app
            .monitors
            .connected
            .values()
            .next()
            .unwrap()
            .instance
            .clone();
        let source = lucent_domain::Wallpaper {
            path: "/fixture/next.png".into(),
            name: "Next".into(),
        };
        app.wallpaper.current = Some(lucent_domain::WallpaperAsset {
            source,
            path: "/fixture/next.png".into(),
        });
        app.wallpaper.background = Some(image("next", [0, 0, 255, 255]));
        app.update(Message::Ignore, &mut Effects::default());
        let address = SurfaceAddress::new("background").instance(instance.clone());
        assert!(app.animating_instance(&address, 0.));
        assert!(!app.animating_instance(&SurfaceAddress::new("background"), 0.));
        // Only the exact output's first submission starts its timeline.
        let message = app
            .event(Event::Scoped {
                instance: instance.clone(),
                event: Box::new(Event::FrameSubmitted {
                    surface: "background",
                    now: 10.,
                    width: 1280.,
                    height: 800.,
                }),
            })
            .unwrap();
        app.update(
            message,
            &mut Effects {
                now: 11.,
                ..Effects::default()
            },
        );
        let report: serde_json::Value = serde_json::from_str(&app.inspect()).unwrap();
        assert_eq!(
            report["background_transitions"][0]["presentation"]["retained_images"],
            2
        );
        assert!(app.animating_instance(&address, 11.));
        let message = app
            .event(Event::Scoped {
                instance,
                event: Box::new(Event::FrameSubmitted {
                    surface: "background",
                    now: 11. + motion::WALLPAPER_REVEAL,
                    width: 1280.,
                    height: 800.,
                }),
            })
            .unwrap();
        app.update(message, &mut Effects::default());
        let report: serde_json::Value = serde_json::from_str(&app.inspect()).unwrap();
        assert_eq!(
            report["background_transitions"][0]["presentation"]["retained_images"],
            1
        );
        app.update(Message::Outputs(vec![]), &mut Effects::default());
        let report: serde_json::Value = serde_json::from_str(&app.inspect()).unwrap();
        assert!(
            report["background_transitions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn initial_pixels_are_immediate_and_missing_candidates_keep_last_good() {
        let mut presentation = BackgroundPresentation::default();
        let first = image("first", [255, 0, 0, 255]);
        assert!(presentation.reconcile(source("first"), Some(first.clone()), 0.));
        assert!(!presentation.active(0.));
        assert_eq!(top_progress(&presentation, 0.), 1.);
        assert!(!presentation.reconcile(source("missing"), None, 1.));
        assert_eq!(
            presentation.current.as_ref().unwrap().image.cache_id(),
            first.cache_id()
        );
        assert_eq!(presentation.inspect()["retained_images"], 1);
    }

    #[test]
    fn actual_source_change_reveals_new_pixels_and_retires_after_submission() {
        let mut presentation = BackgroundPresentation::default();
        let old = image("old", [255, 0, 0, 255]);
        let new = image("new", [0, 0, 255, 255]);
        presentation.reconcile(source("old"), Some(old.clone()), 0.);
        presentation.reconcile(source("new"), Some(new.clone()), 1.);
        assert!(presentation.active(1.));
        assert_eq!(top_progress(&presentation, 1.), 0.);
        assert!(presentation.presented(1., 1.));
        let middle = top_progress(&presentation, 1. + motion::WALLPAPER_REVEAL / 2.);
        assert!(middle > 0. && middle < 1.);
        let view: Element<()> =
            presentation.view(1. + motion::WALLPAPER_REVEAL / 2., Color::hex(0));
        assert!(
            matches!(&view.children[0].kind, Kind::Image(image) if image.cache_id()==old.cache_id())
        );
        assert!(
            matches!(&view.children[1].kind, Kind::Image(image) if image.cache_id()==new.cache_id())
        );
        assert!(!presentation.presented(
            1. + motion::WALLPAPER_REVEAL / 2.,
            1. + motion::WALLPAPER_REVEAL / 2.
        ));
        assert!(!presentation.active(1. + motion::WALLPAPER_REVEAL));
        assert_eq!(
            top_progress(&presentation, 1. + motion::WALLPAPER_REVEAL),
            1.
        );
        assert_eq!(presentation.inspect()["retained_images"], 2);
        assert!(
            presentation.presented(1. + motion::WALLPAPER_REVEAL, 1. + motion::WALLPAPER_REVEAL)
        );
        assert_eq!(presentation.inspect()["retained_images"], 1);
        assert!(!presentation.presented(2., 2.));
    }

    #[test]
    fn delayed_first_submission_does_not_consume_the_reveal_duration() {
        let mut presentation = BackgroundPresentation::default();
        presentation.reconcile(source("old"), Some(image("old", [255, 0, 0, 255])), 0.);
        presentation.reconcile(source("new"), Some(image("new", [0, 0, 255, 255])), 1.);

        // Even a long wait for the next frame keeps the old pixels visible.
        assert_eq!(top_progress(&presentation, 10.), 0.);
        assert!(presentation.active(10.));
        assert!(
            !presentation.presented(0., 11.),
            "an older view cannot start this selection"
        );
        assert_eq!(presentation.inspect()["awaiting_first_frame"], true);
        assert!(presentation.presented(10., 11.));
        assert_eq!(top_progress(&presentation, 11.), 0.);
        assert_eq!(presentation.inspect()["awaiting_first_frame"], false);

        let sampled = 11. + motion::WALLPAPER_REVEAL / 2.;
        let progress = top_progress(&presentation, sampled);
        assert!(progress > 0. && progress < 1.);
        // Submitting a partially revealed view late must not release its underlay.
        assert!(!presentation.presented(sampled, 20.));
        assert_eq!(presentation.inspect()["retained_images"], 2);
        assert_eq!(
            top_progress(&presentation, sampled),
            progress,
            "submission does not restart the timeline"
        );
        assert_eq!(top_progress(&presentation, 20.), 1.);
        assert!(presentation.presented(20., 21.));
        assert_eq!(presentation.inspect()["retained_images"], 1);
        assert!(!presentation.active(21.));
    }

    #[test]
    fn streamed_revisions_replace_pixels_without_restarting_a_source_reveal() {
        let mut presentation = BackgroundPresentation::default();
        let stream = ImageStream::new();
        let frame = |color| Arc::new(stream.frame_rgba(1, 1, vec![color, 0, 0, 255]).unwrap());
        presentation.reconcile(source("old"), Some(image("old", [0, 0, 255, 255])), 0.);
        let live = Some(Source::Live("media".into()));
        presentation.reconcile(live.clone(), Some(frame(10)), 1.);
        presentation.presented(1., 1.);
        let before = top_progress(&presentation, 1.2);
        presentation.reconcile(live.clone(), Some(frame(20)), 1.2);
        assert_eq!(top_progress(&presentation, 1.2), before);
        presentation.presented(1. + motion::WALLPAPER_REVEAL, 1. + motion::WALLPAPER_REVEAL);
        assert!(presentation.reconcile(live, Some(frame(30)), 2.));
        assert!(!presentation.active(2.));
        assert_eq!(presentation.inspect()["retained_images"], 1);
    }

    #[test]
    fn fallback_follows_coverage_of_the_current_bottom_image() {
        let mut presentation = BackgroundPresentation::default();
        let stream = ImageStream::new();
        let frame = |alpha| Arc::new(stream.frame_rgba(1, 1, vec![40, 80, 120, alpha]).unwrap());
        let layers = |p: &BackgroundPresentation| {
            let tree: Element<()> = p.view(0., Color::hex(0));
            tree.children.len()
        };
        presentation.reconcile(source("live"), Some(frame(255)), 0.);
        assert_eq!(layers(&presentation), 1);
        presentation.reconcile(source("live"), Some(frame(100)), 1.);
        assert_eq!(
            layers(&presentation),
            2,
            "a transparent revision restores fallback"
        );
        presentation.reconcile(source("opaque"), Some(image("opaque", [255; 4])), 2.);
        presentation.presented(2., 2.);
        assert_eq!(
            layers(&presentation),
            3,
            "outgoing transparency needs fallback through the reveal"
        );
        presentation.presented(2. + motion::WALLPAPER_REVEAL, 2. + motion::WALLPAPER_REVEAL);
        assert_eq!(layers(&presentation), 1);
    }

    #[test]
    #[ignore = "requires Vulkan; run explicitly with --ignored"]
    fn background_coverage_optimization_preserves_pixels_and_reveals() {
        use lucent_ui::{Interaction, Layout};
        let fonts = Arc::new(vec![]);
        let layout = Layout::new(fonts.clone());
        let gpu = lucent_render::Gpu::headless(fonts).unwrap();
        let mut canvas = lucent_render::Canvas::new(gpu);
        let fallback = Color::hex(0x18344f);
        for old_alpha in [0, 127, 255] {
            for new_alpha in [0, 127, 255] {
                let old = image("old", [200, 60, 90, old_alpha]);
                let new = image("new", [20, 150, 100, new_alpha]);
                let mut presentation = BackgroundPresentation::default();
                presentation.reconcile(source("old"), Some(old.clone()), 0.);
                presentation.reconcile(source("new"), Some(new.clone()), 1.);
                presentation.presented(1., 1.);
                for density in [
                    lucent_api::Density::ONE,
                    lucent_api::Density::new(3, 2).unwrap(),
                    lucent_api::Density::integer(2).unwrap(),
                ] {
                    for now in [
                        1.,
                        1. + motion::WALLPAPER_REVEAL / 2.,
                        1. + motion::WALLPAPER_REVEAL,
                    ] {
                        let optimized: Element<()> = presentation.view(now, fallback);
                        let incoming = Element::image(new.clone()).cover().fill();
                        let incoming = if new.is_opaque() {
                            incoming
                        } else {
                            Element::stack(vec![
                                Element::empty().fill().background(fallback),
                                incoming,
                            ])
                            .fill()
                        };
                        let reference: Element<()> = Element::stack(vec![
                            Element::empty().fill().background(fallback),
                            Element::image(old.clone()).cover().fill(),
                            incoming.diagonal_reveal(presentation.reveal.value(now)),
                        ])
                        .fill();
                        let expected =
                            layout.build(&reference, 31., 17., &Interaction::default(), now);
                        let actual =
                            layout.build(&optimized, 31., 17., &Interaction::default(), now);
                        assert_eq!(
                            canvas
                                .snapshot_scaled(31, 17, density, &actual.paint)
                                .unwrap(),
                            canvas
                                .snapshot_scaled(31, 17, density, &expected.paint)
                                .unwrap(),
                            "alpha {old_alpha}/{new_alpha}, density {density:?}, time {now}"
                        );
                    }
                }
                presentation
                    .presented(1. + motion::WALLPAPER_REVEAL, 1. + motion::WALLPAPER_REVEAL);
                let optimized: Element<()> = presentation.view(2., fallback);
                let reference: Element<()> = Element::stack(vec![
                    Element::empty().fill().background(fallback),
                    Element::image(new).cover().fill(),
                ])
                .fill();
                let actual = layout.build(&optimized, 31., 17., &Interaction::default(), 2.);
                let expected = layout.build(&reference, 31., 17., &Interaction::default(), 2.);
                assert_eq!(
                    canvas.snapshot(31, 17, 1, &actual.paint).unwrap(),
                    canvas.snapshot(31, 17, 1, &expected.paint).unwrap()
                );
                assert_eq!(actual.paint.len(), if new_alpha == 255 { 1 } else { 2 });
            }
        }
    }

    #[test]
    #[ignore = "requires Vulkan; run explicitly with --ignored"]
    fn background_coverage_optimization_revealed_transparency_matches_settled_pixels() {
        use lucent_ui::{Interaction, Layout};
        let fonts = Arc::new(vec![]);
        let layout = Layout::new(fonts.clone());
        let gpu = lucent_render::Gpu::headless(fonts).unwrap();
        let mut canvas = lucent_render::Canvas::new(gpu);
        let fallback = Color::hex(0x18344f);
        for alpha in [0, 127] {
            for scale in [1, 2] {
                let mut presentation = BackgroundPresentation::default();
                presentation.reconcile(source("old"), Some(image("old", [200, 60, 90, 255])), 0.);
                let snapshot = |canvas: &mut lucent_render::Canvas,
                                presentation: &BackgroundPresentation,
                                now| {
                    let tree: Element<()> = presentation.view(now, fallback);
                    let scene = layout.build(&tree, 31., 17., &Interaction::default(), now);
                    canvas.snapshot(31, 17, scale, &scene.paint).unwrap()
                };
                let old = snapshot(&mut canvas, &presentation, 0.);
                presentation.reconcile(
                    source("new"),
                    Some(image("new", [20, 150, 100, alpha])),
                    1.,
                );
                presentation.presented(1., 1.);
                let middle_at = 1. + motion::WALLPAPER_REVEAL * 0.4;
                let progress = presentation.reveal.value(middle_at);
                let middle = snapshot(&mut canvas, &presentation, middle_at);
                let completed_at = 1. + motion::WALLPAPER_REVEAL;
                let completed = snapshot(&mut canvas, &presentation, completed_at);
                assert!(presentation.presented(completed_at, completed_at));
                let settled = snapshot(&mut canvas, &presentation, completed_at);
                let mut revealed = 0;
                let mut retained = 0;
                let width = 31 * scale;
                let height = 17 * scale;
                for (index, actual) in middle.as_chunks::<4>().0.iter().enumerate() {
                    let x = (index as u32 % width) as f32 + 0.5;
                    let y = (index as u32 / width) as f32 + 0.5;
                    // Plain solid fills already antialias the target's outer
                    // edges, whereas axis-aligned images retain opaque edge
                    // pixels. That independent renderer behavior can expose
                    // the old underlay inside the outer fine-derivative fringe.
                    // Check exact compositing outside that two-physical-pixel
                    // neighborhood; the generic reveal endpoint test still
                    // compares every pixel for identical paint compositions.
                    if x < 2. || y < 2. || x > width as f32 - 2. || y > height as f32 - 2. {
                        continue;
                    }
                    assert_eq!(
                        completed.as_chunks::<4>().0[index],
                        settled.as_chunks::<4>().0[index],
                        "outgoing retirement must preserve interior pixels at {index}, alpha {alpha}, {scale}x"
                    );
                    let distance = x + y - progress * 48. * scale as f32;
                    if distance < -2. {
                        assert_eq!(
                            *actual,
                            settled.as_chunks::<4>().0[index],
                            "revealed transparency at {index}, alpha {alpha}, {scale}x"
                        );
                        revealed += 1;
                    } else if distance > 2. {
                        assert_eq!(
                            *actual,
                            old.as_chunks::<4>().0[index],
                            "retained old pixels at {index}, alpha {alpha}, {scale}x"
                        );
                        retained += 1;
                    }
                }
                assert!(
                    revealed > 0 && retained > 0,
                    "both sides of the cut must be exercised"
                );
                assert_eq!(presentation.inspect()["retained_images"], 1);
            }
        }
    }

    #[test]
    fn same_source_refit_updates_image_without_animation_or_retained_history() {
        let mut presentation = BackgroundPresentation::default();
        presentation.reconcile(source("same"), Some(image("small", [10, 0, 0, 255])), 0.);
        let larger = Arc::new(ImageData::from_rgba("large", 2, 1, vec![20; 8]).unwrap());
        assert!(presentation.reconcile(source("same"), Some(larger.clone()), 1.));
        assert!(!presentation.active(1.));
        assert!(!presentation.reconcile(source("same"), Some(larger), 2.));
        assert_eq!(presentation.inspect()["retained_images"], 1);
    }

    #[test]
    fn interrupted_choices_retain_only_dominant_frame_and_latest_candidate() {
        let mut presentation = BackgroundPresentation::default();
        let old = image("old", [10, 0, 0, 255]);
        presentation.reconcile(source("old"), Some(old.clone()), 0.);
        presentation.reconcile(source("middle"), Some(image("middle", [20, 0, 0, 255])), 1.);
        presentation.presented(1., 1.);
        presentation.reconcile(
            source("latest"),
            Some(image("latest", [30, 0, 0, 255])),
            1.001,
        );
        assert_eq!(
            presentation.outgoing.as_ref().unwrap().cache_id(),
            old.cache_id()
        );
        assert_eq!(presentation.inspect()["retained_images"], 2);
        for index in 0..30 {
            presentation.reconcile(
                source(&format!("choice-{index}")),
                Some(image("candidate", [40, 0, 0, 255])),
                1.002 + f64::from(index) * 0.001,
            );
            assert_eq!(presentation.inspect()["retained_images"], 2);
        }
        assert_eq!(presentation.inspect()["retained_frame_bytes"], 8);
    }

    #[test]
    fn shared_stream_identity_never_appears_as_two_texture_layers() {
        let mut presentation = BackgroundPresentation::default();
        let stream = ImageStream::new();
        let frame = |value| Arc::new(stream.frame_rgba(1, 1, vec![value, 0, 0, 255]).unwrap());
        presentation.reconcile(Some(Source::Live("first".into())), Some(frame(10)), 0.);
        presentation.reconcile(Some(Source::Live("second".into())), Some(frame(20)), 1.);
        assert!(!presentation.active(1.));
        assert_eq!(presentation.inspect()["retained_images"], 1);
        presentation.reconcile(source("static"), Some(image("still", [30, 0, 0, 255])), 2.);
        // A frozen outgoing snapshot has independent GPU identity, so returning
        // to the still-active stream can transition safely without texture aliasing.
        presentation.reconcile(Some(Source::Live("first".into())), Some(frame(40)), 2.001);
        assert!(presentation.active(2.001));
        assert_eq!(presentation.inspect()["retained_images"], 2);
        assert!(
            presentation
                .outgoing
                .as_ref()
                .unwrap()
                .stream_revision()
                .is_none()
        );
    }

    #[test]
    fn output_presentations_have_independent_transition_and_retirement() {
        let mut left = BackgroundPresentation::default();
        let mut right = BackgroundPresentation::default();
        let old = image("shared", [10, 0, 0, 255]);
        left.reconcile(source("shared"), Some(old.clone()), 0.);
        right.reconcile(source("shared"), Some(old), 0.);
        left.reconcile(source("left"), Some(image("left", [20, 0, 0, 255])), 1.);
        assert!(left.active(1.));
        assert!(!right.active(1.));
        left.presented(1., 1.);
        left.presented(1. + motion::WALLPAPER_REVEAL, 1. + motion::WALLPAPER_REVEAL);
        assert_eq!(left.inspect()["retained_images"], 1);
        assert_eq!(right.inspect()["retained_images"], 1);
    }

    #[test]
    fn outgoing_live_snapshot_shares_pixels_without_mutable_texture_identity() {
        let mut presentation = BackgroundPresentation::default();
        let stream = ImageStream::new();
        let first = Arc::new(stream.frame_rgba(1, 1, vec![200, 0, 0, 255]).unwrap());
        presentation.reconcile(Some(Source::Live("media".into())), Some(first.clone()), 0.);
        presentation.reconcile(source("still"), Some(image("still", [0, 0, 200, 255])), 1.);
        let outgoing = presentation.outgoing.as_ref().unwrap();
        assert!(outgoing.stream_revision().is_none());
        assert_eq!(outgoing.cache_id(), first.cache_id());
        assert_eq!(
            outgoing.as_image().unwrap().pixels().as_ptr(),
            first.pixels().as_ptr()
        );
        let second = stream.frame_rgba(1, 1, vec![0, 200, 0, 255]).unwrap();
        assert_ne!(outgoing.cache_id(), second.cache_id());
        assert_eq!(outgoing.as_image().unwrap().pixels(), [200, 0, 0, 255]);
    }
}

#[cfg(test)]
mod native_frame_tests {
    use super::*;
    use lucent_api::{
        ImageStream, Kind,
        video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer, Yuv420Frame},
    };
    fn native(stream: &ImageStream, value: u8) -> PresentationFrame {
        let p = |v| Plane::new(Bytes::from(vec![v]), 1, 1, 1).unwrap();
        let frame = Yuv420Frame::new(
            1,
            1,
            [p(value), p(128), p(128)],
            Bt709 {
                range: Range::Full,
                transfer: Transfer::Srgb,
                chroma: ChromaSiting::Left,
            },
        )
        .unwrap();
        Arc::new(stream.frame_yuv420(frame).unwrap()).into()
    }
    #[test]
    fn native_background_updates_without_restarting_reveal_and_freezes_only_on_source_change() {
        let stream = ImageStream::new();
        let first = native(&stream, 30);
        let second = native(&stream, 80);
        let mut presentation = BackgroundPresentation::default();
        assert!(presentation.reconcile_frame(Some(Source::Live("first".into())), Some(first), 0.));
        assert!(presentation.reconcile_frame(
            Some(Source::Live("first".into())),
            Some(second.clone()),
            1.
        ));
        assert!(!presentation.active(1.));
        assert!(presentation.outgoing.is_none());
        let view: Element<()> = presentation.view(1., Color::hex(0));
        assert!(matches!(&view.children[0].kind, Kind::Video(_)));
        let image = Arc::new(ImageData::from_rgba("next", 1, 1, vec![20, 40, 60, 255]).unwrap());
        presentation.reconcile_frame(Some(Source::Static("next".into())), Some(image.into()), 2.);
        let old = presentation.outgoing.as_ref().unwrap();
        assert_eq!(old.cache_id(), second.cache_id());
        assert!(old.stream_revision().is_none());
        assert_eq!(old.representation(), "yuv420");
        let view: Element<()> = presentation.view(2., Color::hex(0));
        assert!(matches!(&view.children[0].kind, Kind::Video(_)));
        assert_eq!(
            view.children.last().unwrap().style.diagonal_reveal,
            Some(0.)
        );
        assert!(presentation.presented(2., 3.));
        assert!(presentation.active(3.));
        assert_eq!(presentation.inspect()["retained_images"], 2);
        assert_eq!(presentation.inspect()["retained_frame_bytes"], 7);
    }
}
