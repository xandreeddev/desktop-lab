//! Validate a complete draft against current capabilities before any OS operation.
use lucent_domain::*;
use std::collections::BTreeSet;
pub fn validate_display_draft(
    outputs: &[DisplayDevice],
    baseline: &[DisplayConfiguration],
    draft: &[DisplayConfiguration],
) -> Result<()> {
    let invalid = |s: &str| DomainError::Invalid(s.into());
    if outputs.is_empty()
        || outputs.len() > 32
        || draft.len() != outputs.len()
        || baseline.len() != outputs.len()
    {
        return Err(invalid(
            "The connected display set changed; refresh the draft",
        ));
    }
    let observed: BTreeSet<_> = outputs.iter().map(|o| &o.current.output).collect();
    if observed.len() != outputs.len()
        || baseline.iter().map(|o| &o.output).collect::<BTreeSet<_>>() != observed
        || draft.iter().map(|o| &o.output).collect::<BTreeSet<_>>() != observed
    {
        return Err(invalid("Display identities must be current and unique"));
    }
    if !draft.iter().any(|o| o.enabled) {
        return Err(invalid("Keep at least one display enabled"));
    }
    for output in outputs {
        let before = baseline
            .iter()
            .find(|o| o.output == output.current.output)
            .expect("same identities");
        let next = draft
            .iter()
            .find(|o| o.output == output.current.output)
            .expect("same identities");
        if before != &output.current {
            return Err(invalid(
                "Display settings changed outside this draft; refresh before applying",
            ));
        }
        if next.transform > 7 {
            return Err(invalid("Display rotation is invalid"));
        }
        if before != next && !output.editable {
            return Err(invalid(
                "This display uses a native configuration that cannot safely be edited here",
            ));
        }
        if next.enabled && next.mode != before.mode && !output.modes.contains(&next.mode) {
            return Err(invalid("The selected display mode is no longer advertised"));
        }
        if next.enabled {
            // Hyprland requires integral logical pixel extents. This prevents a
            // rejected fractional scale from being mistaken for an applied mode.
            let scale = u64::from(next.scale.thousandths());
            if u64::from(next.mode.width()) * 1000 % scale != 0
                || u64::from(next.mode.height()) * 1000 % scale != 0
            {
                return Err(invalid(
                    "Choose a scale that produces whole logical pixels for this resolution",
                ));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn output(id: &str) -> DisplayDevice {
        DisplayDevice {
            current: DisplayConfiguration {
                output: MonitorId::new(id).unwrap(),
                enabled: true,
                mode: DisplayMode::new(1920, 1080, 60000).unwrap(),
                scale: DisplayScale::new(1000).unwrap(),
                position: DisplayPosition::new(0, 0).unwrap(),
                transform: 0,
            },
            modes: vec![DisplayMode::new(1280, 720, 60000).unwrap()],
            description: String::new(),
            editable: true,
        }
    }
    #[test]
    fn validates_full_layout_without_mutation() {
        let outputs = vec![output("DP-1"), output("DP-2")];
        let base = outputs
            .iter()
            .map(|o| o.current.clone())
            .collect::<Vec<_>>();
        let mut draft = base.clone();
        draft[0].enabled = false;
        assert!(validate_display_draft(&outputs, &base, &draft).is_ok());
        draft[1].enabled = false;
        assert!(validate_display_draft(&outputs, &base, &draft).is_err());
        assert_eq!(outputs[0].current, base[0]);
    }
    #[test]
    fn rejects_stale_identity_modes_transform_and_fractional_pixels() {
        let outputs = vec![output("DP-1")];
        let base = vec![outputs[0].current.clone()];
        for kind in 0..5 {
            let mut draft = base.clone();
            match kind {
                0 => draft[0].output = MonitorId::new("DP-2").unwrap(),
                1 => draft[0].mode = DisplayMode::new(800, 600, 75000).unwrap(),
                2 => draft[0].transform = 8,
                3 => draft[0].scale = DisplayScale::new(1300).unwrap(),
                _ => draft.push(draft[0].clone()),
            };
            assert!(validate_display_draft(&outputs, &base, &draft).is_err());
        }
        let mut valid_fractional = base.clone();
        valid_fractional[0].scale = DisplayScale::new(1250).unwrap();
        assert!(validate_display_draft(&outputs, &base, &valid_fractional).is_ok());
        let mut stale = base.clone();
        stale[0].position = DisplayPosition::new(5, 0).unwrap();
        assert!(validate_display_draft(&outputs, &stale, &base).is_err());
    }
}
