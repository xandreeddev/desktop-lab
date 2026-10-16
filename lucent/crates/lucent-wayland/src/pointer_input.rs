//! Protocol coordinates and button codes end at the native input adapter.
use lucent_api::MouseButton;
use smithay_client_toolkit::seat::pointer::AxisScroll;

pub(super) fn button(code: u32) -> MouseButton {
    match code {
        0x110 => MouseButton::Primary,
        0x111 => MouseButton::Secondary,
        0x112 => MouseButton::Middle,
        other => MouseButton::Other(other),
    }
}

pub(super) fn lines(axis: AxisScroll) -> f64 {
    // value120 and the deprecated discrete event describe the same movement;
    // do not add them. Touchpads keep fractional logical line displacement.
    if axis.value120 != 0 {
        f64::from(axis.value120) / 120.
    } else if axis.discrete != 0 {
        f64::from(axis.discrete)
    } else {
        axis.absolute / 15.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn high_resolution_wheel_does_not_duplicate_legacy_or_absolute_movement() {
        assert_eq!(
            lines(AxisScroll {
                value120: -30,
                discrete: -1,
                absolute: -15.,
                ..Default::default()
            }),
            -0.25
        );
        assert_eq!(
            lines(AxisScroll {
                discrete: 2,
                absolute: 30.,
                ..Default::default()
            }),
            2.
        );
        assert_eq!(
            lines(AxisScroll {
                absolute: 7.5,
                ..Default::default()
            }),
            0.5
        );
        assert_eq!(
            lines(AxisScroll {
                stop: true,
                ..Default::default()
            }),
            0.
        );
    }
    #[test]
    fn unknown_buttons_retain_their_protocol_identity() {
        assert_eq!(button(0x110), MouseButton::Primary);
        assert_eq!(button(0x111), MouseButton::Secondary);
        assert_eq!(button(0x112), MouseButton::Middle);
        assert_eq!(button(0x113), MouseButton::Other(0x113));
    }
}
