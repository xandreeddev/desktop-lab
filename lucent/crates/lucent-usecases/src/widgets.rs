//! Widget extent policy. Catalog, design tokens and display selection stay in clients.
use lucent_domain::*;

pub fn constrain_size(
    size: WidgetSize,
    minimum: WidgetSize,
    maximum: WidgetSize,
) -> Result<WidgetSize> {
    if maximum.width() < minimum.width() || maximum.height() < minimum.height() {
        return Err(DomainError::Invalid(
            "Widget size bounds are inverted".into(),
        ));
    }
    WidgetSize::new(
        size.width().clamp(minimum.width(), maximum.width()),
        size.height().clamp(minimum.height(), maximum.height()),
    )
}
/// Quantize both edges to a supplied grid. If an axis has no grid point within
/// its bounds, retain its minimum extent rather than cropping required content.
pub fn snapped_size(
    size: WidgetSize,
    minimum: WidgetSize,
    maximum: WidgetSize,
    step: f32,
) -> Result<WidgetSize> {
    constrain_size(size, minimum, maximum)?;
    if !step.is_finite() || step <= 0. {
        return Err(DomainError::Invalid(
            "A finite positive grid step is required".into(),
        ));
    }
    let axis = |value: f32, min: f32, max: f32| {
        let step = f64::from(step);
        let low = (f64::from(min) / step).ceil();
        let high = (f64::from(max) / step).floor();
        if low > high {
            min
        } else {
            ((f64::from(value) / step).round().clamp(low, high) * step) as f32
        }
    };
    WidgetSize::new(
        axis(size.width(), minimum.width(), maximum.width()),
        axis(size.height(), minimum.height(), maximum.height()),
    )
}
pub fn resize_widget(
    settings: &mut DesktopSettings,
    id: &str,
    size: WidgetSize,
    minimum: WidgetSize,
    maximum: WidgetSize,
    step: f32,
) -> Result<bool> {
    let size = snapped_size(size, minimum, maximum, step)?;
    let changed = settings.widget_sizes.get(id) != Some(&size);
    if changed {
        settings.widget_sizes.insert(id.into(), size);
    }
    Ok(changed)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn size(w: f32, h: f32) -> WidgetSize {
        WidgetSize::new(w, h).unwrap()
    }
    #[test]
    fn snapped_extents_obey_both_bounds_and_do_not_accumulate_rounding_error() {
        let min = size(270., 160.);
        let max = size(643., 419.);
        assert_eq!(
            snapped_size(size(286., 172.), min, max, 8.).unwrap(),
            size(288., 176.)
        );
        assert_eq!(
            snapped_size(size(900., 900.), min, max, 8.).unwrap(),
            size(640., 416.)
        );
        assert_eq!(snapped_size(min, min, max, 8.).unwrap(), size(272., 160.));
        assert_eq!(snapped_size(min, min, min, 8.).unwrap(), min);
        let mut settings = DesktopSettings::default();
        assert!(resize_widget(&mut settings, "notes", size(286., 172.), min, max, 8.).unwrap());
        assert!(!resize_widget(&mut settings, "notes", size(288., 176.), min, max, 8.).unwrap());
    }
    #[test]
    fn invalid_limits_and_grid_leave_preferences_unchanged() {
        let mut settings = DesktopSettings::default();
        let before = settings.clone();
        for step in [0., f32::NAN, f32::INFINITY] {
            assert!(
                resize_widget(
                    &mut settings,
                    "notes",
                    size(300., 200.),
                    size(270., 160.),
                    size(640., 480.),
                    step
                )
                .is_err()
            );
        }
        assert!(
            resize_widget(
                &mut settings,
                "notes",
                size(300., 200.),
                size(640., 480.),
                size(270., 160.),
                8.
            )
            .is_err()
        );
        assert_eq!(settings, before);
    }
}
