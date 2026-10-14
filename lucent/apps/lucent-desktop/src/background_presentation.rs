//! Transient wallpaper presentation. Selection, decoding and persistence stay
//! with their existing owners; this child borrows already prepared images.
use lucent_api::{Color, Element, ImageData, Motion};
use lucent_design::motion;
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
    image: Arc<ImageData>,
}

pub struct BackgroundPresentation {
    current: Option<Current>,
    outgoing: Option<Arc<ImageData>>,
    reveal: Motion,
}

impl Default for BackgroundPresentation {
    fn default() -> Self {
        Self {
            current: None,
            outgoing: None,
            reveal: Motion::fixed(1.),
        }
    }
}

impl BackgroundPresentation {
    /// Admit prepared pixels for the current selection. A missing candidate
    /// retains the last good presentation while decoding/export is pending.
    /// Returns whether the root should invalidate this exact background.
    pub fn reconcile(
        &mut self,
        source: Option<Source>,
        image: Option<Arc<ImageData>>,
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
                self.reveal = Motion::fixed(1.);
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
            self.reveal = Motion::fixed(1.);
        } else {
            // A different output may still consume this live stream. Give the
            // frozen outgoing frame independent GPU identity without copying
            // its immutable CPU pixels or making the UI prepare mip levels.
            self.outgoing = Some(Arc::new(outgoing.snapshot()));
            self.reveal = Motion::fixed(0.);
            self.reveal
                .target(1., now, motion::PANEL, motion::DECELERATE);
        }
        self.current = Some(Current { source, image });
        true
    }

    pub fn view<M>(&self, now: f64, fallback: Color) -> Element<M> {
        let background = Element::empty().fill().background(fallback);
        let Some(current) = &self.current else {
            return background;
        };
        let image = |image: &Arc<ImageData>| Element::image(image.clone()).cover().fill();
        if let Some(outgoing) = &self.outgoing {
            // Source-over opacity mixes opaque wallpaper pixels while retaining
            // the old image beneath the arriving frame. No shader/codec policy.
            Element::stack(vec![
                background,
                image(outgoing),
                image(&current.image).opacity(self.reveal.value(now).clamp(0., 1.)),
            ])
            .fill()
        } else {
            Element::stack(vec![background, image(&current.image)]).fill()
        }
    }

    pub fn active(&self, now: f64) -> bool {
        self.outgoing.is_some() && self.reveal.active(now)
    }

    /// Release the outgoing frame only after the fully revealed frame submits.
    /// A successful submission is not a compositor scanout acknowledgement.
    pub fn presented(&mut self, now: f64) -> bool {
        if self.outgoing.is_some() && !self.reveal.active(now) {
            self.outgoing = None;
            self.reveal = Motion::fixed(1.);
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
        })
    }
}

fn same_stream(a: &ImageData, b: &ImageData) -> bool {
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

    fn top_opacity(presentation: &BackgroundPresentation, now: f64) -> f32 {
        let view: Element<()> = presentation.view(now, Color::hex(0));
        view.children.last().unwrap().style.opacity
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
        // Exact scoped submission releases the old snapshot without selecting a monitor.
        let message = app
            .event(Event::Scoped {
                instance,
                event: Box::new(Event::FrameSubmitted {
                    surface: "background",
                    now: 10.,
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
        assert_eq!(top_opacity(&presentation, 0.), 1.);
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
        assert_eq!(top_opacity(&presentation, 1.), 0.);
        let middle = top_opacity(&presentation, 1. + motion::PANEL / 2.);
        assert!(middle > 0. && middle < 1.);
        let view: Element<()> = presentation.view(1. + motion::PANEL / 2., Color::hex(0));
        assert!(
            matches!(&view.children[1].kind, Kind::Image(image) if image.cache_id()==old.cache_id())
        );
        assert!(
            matches!(&view.children[2].kind, Kind::Image(image) if image.cache_id()==new.cache_id())
        );
        assert!(!presentation.presented(1. + motion::PANEL / 2.));
        assert!(!presentation.active(1. + motion::PANEL));
        assert_eq!(top_opacity(&presentation, 1. + motion::PANEL), 1.);
        assert_eq!(presentation.inspect()["retained_images"], 2);
        assert!(presentation.presented(1. + motion::PANEL));
        assert_eq!(presentation.inspect()["retained_images"], 1);
        assert!(!presentation.presented(2.));
    }

    #[test]
    fn streamed_revisions_replace_pixels_without_restarting_a_source_fade() {
        let mut presentation = BackgroundPresentation::default();
        let stream = ImageStream::new();
        let frame = |color| Arc::new(stream.frame_rgba(1, 1, vec![color, 0, 0, 255]).unwrap());
        presentation.reconcile(source("old"), Some(image("old", [0, 0, 255, 255])), 0.);
        let live = Some(Source::Live("media".into()));
        presentation.reconcile(live.clone(), Some(frame(10)), 1.);
        let before = top_opacity(&presentation, 1.2);
        presentation.reconcile(live.clone(), Some(frame(20)), 1.2);
        assert_eq!(top_opacity(&presentation, 1.2), before);
        presentation.presented(1. + motion::PANEL);
        assert!(presentation.reconcile(live, Some(frame(30)), 2.));
        assert!(!presentation.active(2.));
        assert_eq!(presentation.inspect()["retained_images"], 1);
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
        // to the still-active stream can fade safely without texture aliasing.
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
        left.presented(1. + motion::PANEL);
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
        assert_eq!(outgoing.pixels().as_ptr(), first.pixels().as_ptr());
        let second = stream.frame_rgba(1, 1, vec![0, 200, 0, 255]).unwrap();
        assert_ne!(outgoing.cache_id(), second.cache_id());
        assert_eq!(outgoing.pixels(), [200, 0, 0, 255]);
    }
}
