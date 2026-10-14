use lucent_api::SurfaceSpec;

/// Explicit size changes can be painted before their configure reply. Commit
/// that image with the size request, never the previous image at a new origin.
/// Zero axes remain compositor-assigned; unchanged requests accept subsequent
/// configure sizes (including constraints and output changes).
pub(crate) fn buffer_extent(
    committed: &SurfaceSpec,
    requested: &SurfaceSpec,
    configured: (u32, u32),
) -> (u32, u32) {
    let axis = |old, new, current| if new != 0 && new != old { new } else { current };
    (
        axis(committed.width, requested.width, configured.0),
        axis(committed.height, requested.height, configured.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{Anchor, Keyboard, Layer, PointerInput};

    fn surface(height: u32) -> SurfaceSpec {
        SurfaceSpec {
            id: "test",
            instance: None,
            output: None,
            layer: Layer::Top,
            anchor: Anchor::Bottom,
            margins: Default::default(),
            redraw_group: None,
            width: 0,
            height,
            exclusive_zone: -1,
            keyboard: Keyboard::None,
            visible: true,
            pointer_input: PointerInput::Content,
        }
    }

    #[test]
    fn expanding_and_compacting_paint_the_requested_extent_in_the_same_commit() {
        let compact = surface(120);
        let expanded = surface(1080);
        assert_eq!(
            buffer_extent(&compact, &expanded, (1920, 120)),
            (1920, 1080)
        );
        assert_eq!(
            buffer_extent(&expanded, &compact, (1920, 1080)),
            (1920, 120)
        );
    }

    #[test]
    fn retry_or_coalesced_changes_keep_the_last_presented_request_as_baseline() {
        let committed = surface(120);
        let pending = surface(1080);
        for _ in 0..3 {
            assert_eq!(
                buffer_extent(&committed, &pending, (1920, 120)),
                (1920, 1080)
            );
        }
        // Cancelled before presentation: no resize or stale expanded buffer.
        assert_eq!(
            buffer_extent(&committed, &committed, (1920, 120)),
            (1920, 120)
        );
    }

    #[test]
    fn configure_remains_authoritative_after_presentation_and_for_automatic_axes() {
        let mut requested = surface(1080);
        assert_eq!(
            buffer_extent(&requested, &requested, (1280, 720)),
            (1280, 720)
        );
        let committed = requested.clone();
        requested.height = 0;
        requested.width = 800;
        assert_eq!(
            buffer_extent(&committed, &requested, (1280, 720)),
            (800, 720)
        );
    }
}
