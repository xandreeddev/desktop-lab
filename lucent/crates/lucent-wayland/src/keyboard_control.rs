//! Keyboard-only commits preserve the last successfully presented image.
//!
//! A retry may already have staged another viewport, role or input region.
//! Neither that pending cache nor the latest input scene is a presentation.
use lucent_api::{Clip, Keyboard, SurfaceSpec};

#[derive(Clone)]
pub(super) struct Presented {
    pub extent: (u32, u32),
    pub buffer_scale: i32,
    pub regions: Vec<Clip>,
}

pub(super) struct Plan {
    pub spec: SurfaceSpec,
    pub presented: Presented,
}
impl Plan {
    /// Reset the staging cache so the next image must resend its different
    /// input region. This records no new buffer or presentation history.
    pub fn record_commit(self, committed: &mut SurfaceSpec, regions: &mut Vec<Clip>) {
        *committed = self.spec;
        *regions = self.presented.regions;
    }
}

/// The requested next image deliberately does not participate in this plan.
pub(super) fn plan(
    committed: &SurfaceSpec,
    presented: Option<&Presented>,
    keyboard: Keyboard,
) -> Option<Plan> {
    let mut spec = committed.clone();
    spec.keyboard = keyboard;
    Some(Plan {
        spec,
        presented: presented?.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{Anchor, Layer, PointerInput, Rect};

    fn spec() -> SurfaceSpec {
        SurfaceSpec {
            id: "editor",
            instance: None,
            output: None,
            layer: Layer::Bottom,
            anchor: Anchor::TopLeft,
            margins: Default::default(),
            redraw_group: None,
            width: 304,
            height: 240,
            exclusive_zone: -1,
            keyboard: Keyboard::None,
            visible: true,
            pointer_input: PointerInput::Content,
        }
    }

    #[test]
    fn control_plan_keeps_presented_geometry_and_regions_after_another_image_retry() {
        let mut committed = spec();
        let shown = Presented {
            extent: (304, 240),
            buffer_scale: 1,
            regions: vec![Rect::new(8., 16., 200., 120.).into()],
        };
        let mut requested = committed.clone();
        requested.width = 520;
        requested.height = 480;
        requested.margins.top = 92;
        requested.keyboard = Keyboard::OnDemand;
        let pending_regions = vec![Clip::from(Rect::new(0., 0., 520., 480.))];
        let plan = plan(&committed, Some(&shown), Keyboard::OnDemand).unwrap();
        let mut expected = committed.clone();
        expected.keyboard = Keyboard::OnDemand;
        assert_eq!(plan.spec, expected);
        assert_eq!(plan.presented.extent, (304, 240));
        assert_eq!(plan.presented.buffer_scale, 1);
        assert_eq!(plan.presented.regions, shown.regions);
        assert_ne!(plan.presented.regions, pending_regions);
        let mut staged_cache = pending_regions.clone();
        plan.record_commit(&mut committed, &mut staged_cache);
        assert_eq!(committed, expected);
        assert_eq!(staged_cache, shown.regions);
        // The next draw must resend B after restoring A's cache.
        assert_ne!(staged_cache, pending_regions);
        assert_eq!(requested.width, 520);
        assert_eq!(requested.margins.top, 92);
    }

    #[test]
    fn automatic_role_axes_and_fractional_viewport_are_preserved_independently() {
        let mut committed = spec();
        committed.width = 0;
        committed.height = 0;
        let shown = Presented {
            extent: (1536, 864),
            buffer_scale: 1,
            regions: vec![],
        };
        let plan = plan(&committed, Some(&shown), Keyboard::OnDemand).unwrap();
        assert_eq!((plan.spec.width, plan.spec.height), (0, 0));
        assert_eq!(plan.presented.extent, (1536, 864));
        assert!(plan.presented.regions.is_empty());
    }

    #[test]
    fn no_successful_image_cannot_authorize_a_control_commit() {
        assert!(plan(&spec(), None, Keyboard::OnDemand).is_none());
    }

    #[test]
    fn withdrawing_keyboard_retains_integer_scale_and_full_input_region() {
        let mut committed = spec();
        committed.keyboard = Keyboard::OnDemand;
        let shown = Presented {
            extent: (304, 240),
            buffer_scale: 2,
            regions: vec![Rect::new(0., 0., 304., 240.).into()],
        };
        let plan = plan(&committed, Some(&shown), Keyboard::None).unwrap();
        assert_eq!(plan.spec.keyboard, Keyboard::None);
        assert_eq!(plan.spec.pointer_input, PointerInput::Content);
        assert_eq!(plan.presented.buffer_scale, 2);
        assert_eq!(plan.presented.regions, shown.regions);
    }
    #[test]
    fn native_interaction_starts_inactive_without_discarding_component_focus() {
        let mut input = super::super::native_interaction::<()>();
        input.focus = Some("notes-input".into());
        input.focus_visible = true;
        assert!(!input.keyboard_active());
        assert!(input.set_keyboard_active(true));
        assert!(input.keyboard_active());
        assert!(input.set_keyboard_active(false));
        assert_eq!(input.focus.as_deref(), Some("notes-input"));
        assert!(input.focus_visible);
    }
}
