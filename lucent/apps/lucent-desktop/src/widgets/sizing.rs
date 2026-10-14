//! One pointer resize preview. Preferences change only after release/explicit action.
use super::catalog;
use lucent_api::DragEvent;
use lucent_design::component;
use lucent_domain::*;

pub(super) fn minimum(id: &str) -> WidgetSize {
    let (width, height) = super::widget_size(id);
    WidgetSize::new(width, height).expect("positive widget design tokens")
}
fn maximum(
    id: &str,
    viewport: (f32, f32),
    position: Placement,
    editing: bool,
) -> Result<WidgetSize> {
    if !viewport.0.is_finite() || !viewport.1.is_finite() || viewport.0 < 0. || viewport.1 < 0. {
        return Err(DomainError::Invalid(
            "Widget output dimensions must be finite and non-negative".into(),
        ));
    }
    let min = minimum(id);
    WidgetSize::new(
        (viewport.0 - position.x()).max(min.width()),
        (viewport.1 - position.y() - editor_height(editing)).max(min.height()),
    )
}
/// Configured preference survives output changes; presentation alone is constrained.
pub fn configured(
    id: &str,
    settings: &WidgetLayout,
    viewport: (f32, f32),
    editing: bool,
) -> WidgetSize {
    let min = minimum(id);
    let max = maximum(id, viewport, Placement::new(0., 0.).unwrap(), editing).unwrap_or(min);
    lucent_usecases::widgets::constrain_size(
        settings.widget_sizes.get(id).copied().unwrap_or(min),
        min,
        max,
    )
    .unwrap_or(min)
}
#[derive(Clone, Copy)]
struct Gesture {
    origin: WidgetSize,
    preview: WidgetSize,
    position: Placement,
}
#[derive(Default)]
pub(super) struct Resizer {
    editing: bool,
    cancelled: bool,
    active: Option<(String, Gesture)>,
}
impl Resizer {
    pub fn editing(&self) -> bool {
        self.editing
    }
    pub fn active(&self) -> bool {
        self.active.is_some()
    }
    pub fn position(&self, id: &str) -> Option<Placement> {
        self.active
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, g)| g.position)
    }
    pub fn size(&self, id: &str, source: bool) -> Option<WidgetSize> {
        self.active
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, g)| if source { g.origin } else { g.preview })
    }
    pub fn press(&mut self) {
        self.active = None;
        self.cancelled = false;
    }
    pub fn cancel(&mut self) {
        self.active = None;
        self.cancelled = true;
    }
    pub fn cancel_widget(&mut self, id: &str) {
        if self.active.as_ref().is_some_and(|(key, _)| key == id) {
            self.cancel();
        }
    }
    pub fn edit(&mut self, enabled: bool) {
        self.editing = enabled;
        self.cancel();
    }
    fn eligible(&self, id: &str, settings: &WidgetLayout) -> bool {
        self.editing
            && catalog::find(id).is_some()
            && settings.visible_widgets.iter().any(|key| key == id)
    }
    pub fn drag(
        &mut self,
        id: &str,
        event: DragEvent,
        settings: &mut WidgetLayout,
        viewport: (f32, f32),
    ) -> Result<bool> {
        if self.cancelled || !self.eligible(id, settings) {
            return Ok(false);
        }
        if let Err(error) = maximum(id, viewport, Placement::new(0., 0.).unwrap(), self.editing) {
            self.cancel();
            return Err(error);
        }
        if self.active.as_ref().is_some_and(|(key, _)| key != id) {
            return Ok(false);
        }
        if event.finished && self.active.is_none() {
            return Ok(false);
        }
        if !event.dx.is_finite() || !event.dy.is_finite() {
            self.cancel();
            return Err(DomainError::Invalid(
                "Resize movement must be finite".into(),
            ));
        }
        let mut gesture = self
            .active
            .as_ref()
            .map(|(_, g)| *g)
            .unwrap_or_else(|| Gesture {
                origin: configured(id, settings, viewport, self.editing),
                preview: configured(id, settings, viewport, self.editing),
                position: super::surfaces::position(id, settings, viewport, self.editing),
            });
        let result = (|| {
            let requested = WidgetSize::new(
                (gesture.origin.width() + event.dx).max(minimum(id).width()),
                (gesture.origin.height() + event.dy).max(minimum(id).height()),
            )?;
            let max = maximum(id, viewport, gesture.position, self.editing)?;
            gesture.preview =
                lucent_usecases::widgets::constrain_size(requested, minimum(id), max)?;
            if event.finished {
                lucent_usecases::widgets::resize_widget(
                    settings,
                    id,
                    gesture.preview,
                    minimum(id),
                    max,
                    component::widget_layout::GRID_STEP,
                )
            } else {
                self.active = Some((id.into(), gesture));
                Ok(false)
            }
        })();
        if event.finished {
            self.active = None;
        }
        if result.is_err() {
            self.cancel();
        }
        result
    }
    pub fn set(
        &mut self,
        id: &str,
        size: WidgetSize,
        settings: &mut WidgetLayout,
        viewport: (f32, f32),
    ) -> Result<bool> {
        if catalog::find(id).is_none() {
            return Err(DomainError::Invalid("Unknown widget".into()));
        }
        self.cancel_widget(id);
        let max = maximum(id, viewport, Placement::new(0., 0.).unwrap(), self.editing)?;
        lucent_usecases::widgets::resize_widget(
            settings,
            id,
            size,
            minimum(id),
            max,
            component::widget_layout::GRID_STEP,
        )
    }
    pub fn step(
        &mut self,
        id: &str,
        delta: i32,
        settings: &mut WidgetLayout,
        viewport: (f32, f32),
    ) -> Result<bool> {
        if !self.eligible(id, settings) {
            return Ok(false);
        }
        let current = configured(id, settings, viewport, self.editing);
        let step = delta.clamp(-1, 1) as f32 * component::widget_layout::GRID_STEP;
        self.set(
            id,
            WidgetSize::new(
                (current.width() + step).max(minimum(id).width()),
                (current.height() + step).max(minimum(id).height()),
            )?,
            settings,
            viewport,
        )
    }
}

pub(super) fn editor_height(editing: bool) -> f32 {
    if editing {
        component::widget_resize::HANDLE_SIZE + 2. * component::widget_resize::INSET
    } else {
        0.
    }
}
pub(super) fn visual_size(
    id: &str,
    settings: &WidgetLayout,
    viewport: (f32, f32),
    editing: bool,
) -> (f32, f32) {
    let size = configured(id, settings, viewport, editing);
    (size.width(), size.height() + editor_height(editing))
}
