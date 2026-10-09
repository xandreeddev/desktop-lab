//! Lucent's design language, separate from the reusable UI and rendering engine.
//! Edit `design/tokens.json`, then run `scripts/generate-design-tokens.py`.
use lucent_api::{Color, Element, Transition};
#[rustfmt::skip]
mod tokens;
pub use tokens::*;
#[rustfmt::skip]
pub mod app_icons;

/// Semantic roles: clients describe intent without selecting palette swatches.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub surface: Color,
    pub surface_container: Color,
    pub on_surface: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub error: Color,
    pub success: Color,
    pub warning: Color,
    pub info: Color,
    pub focus: Color,
}
impl Theme {
    pub fn new(light: bool) -> Self {
        macro_rules! roles {
            ($mode:ident) => {
                Self {
                    surface: theme::$mode::SURFACE,
                    surface_container: theme::$mode::SURFACE_CONTAINER,
                    on_surface: theme::$mode::ON_SURFACE,
                    primary: theme::$mode::PRIMARY,
                    on_primary: theme::$mode::ON_PRIMARY,
                    error: theme::$mode::ERROR,
                    success: theme::$mode::SUCCESS,
                    warning: theme::$mode::WARNING,
                    info: theme::$mode::INFO,
                    focus: theme::$mode::FOCUS,
                }
            };
        }
        if light { roles!(light) } else { roles!(dark) }
    }
    /// Apply shared interaction and elevation recipes to a composed tree.
    /// Explicit component foregrounds and image colors are preserved.
    pub fn apply<M>(self, mut element: Element<M>) -> Element<M> {
        fn visit<M>(theme: Theme, e: &mut Element<M>) {
            e.style.focus_color = theme.focus;
            e.style.focus_width = component::focus::WIDTH;
            e.style.caret_width = component::input::CARET_WIDTH;
            e.style.hover_color = theme.primary.alpha(opacity::HOVER);
            e.style.hover_transition = Transition {
                duration: motion::HOVER,
                curve: motion::DECELERATE,
            };
            e.style.shadow_color = palette::BLACK.alpha(opacity::SHADOW);
            e.style.shadow_offset = space::XS;
            for child in &mut e.children {
                visit(theme, child);
            }
        }
        visit(self, &mut element);
        element
    }
    /// Visual state for a toggle row; attach the action to its enclosing control.
    pub fn switch_indicator<M>(self, checked: bool) -> Element<M> {
        use component::switch as s;
        Element::stack(vec![
            Element::empty()
                .size(s::THUMB, s::THUMB)
                .at(
                    if checked {
                        s::WIDTH - s::INSET - s::THUMB
                    } else {
                        s::INSET
                    },
                    s::INSET,
                )
                .radius(s::THUMB / 2.)
                .background(if checked {
                    self.on_primary
                } else {
                    self.on_surface
                }),
        ])
        .size(s::WIDTH, s::HEIGHT)
        .radius(s::HEIGHT / 2.)
        .background(if checked {
            self.primary
        } else {
            self.surface_container
        })
    }
    pub fn button<M>(self, label: impl Into<String>, message: M) -> Element<M> {
        Element::button(label, message)
            .padding_xy(
                component::button::PADDING_INLINE,
                component::button::PADDING_BLOCK,
            )
            .radius(component::button::RADIUS)
            .font(font::CONTROL)
            .color(self.on_surface)
            .background(self.surface_container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c: Color) -> f32 {
        fn linear(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(c.0) + 0.7152 * linear(c.1) + 0.0722 * linear(c.2)
    }
    #[test]
    fn text_roles_have_readable_contrast_in_both_modes() {
        for t in [Theme::new(false), Theme::new(true)] {
            for (fg, bg) in [
                (t.on_surface, t.surface),
                (t.on_surface, t.surface_container),
                (t.on_primary, t.primary),
                (t.error, t.surface),
            ] {
                let (a, b) = (luminance(fg), luminance(bg));
                assert!(
                    (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                    "insufficient contrast: {fg:?} on {bg:?}"
                );
            }
        }
    }
}
