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
pub fn palettes() -> &'static [lucent_domain::ThemePalette] {
    static PALETTES: std::sync::OnceLock<Vec<lucent_domain::ThemePalette>> =
        std::sync::OnceLock::new();
    PALETTES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../configs/lucent/palettes.json"))
            .expect("generated palette catalog")
    })
}
pub fn selected_palette(settings: &lucent_domain::DesktopSettings) -> lucent_domain::ThemePalette {
    settings
        .palette
        .clone()
        .unwrap_or_else(|| palettes()[usize::from(settings.light)].clone())
}
/// Map a wallpaper seed into the same semantic role graph as named presets.
pub fn wallpaper_palette(seed: lucent_domain::Rgb) -> lucent_domain::ThemePalette {
    use lucent_domain::Rgb;
    let black = Rgb::new(0, 0, 0);
    let white = Rgb::new(255, 255, 255);
    let mut value = palettes()[0].clone();
    value.id = format!("wallpaper-{}", seed.hex().trim_start_matches('#'));
    value.name = format!("Wallpaper {}", seed.hex());
    let c = &mut value.colors;
    c.surface = black.mix(seed, palette_recipe::SURFACE_SEED);
    c.surface_container = black.mix(seed, palette_recipe::CONTAINER_SEED);
    c.on_surface = seed.mix(white, palette_recipe::FOREGROUND_WHITE);
    c.primary = seed.mix(white, palette_recipe::ACCENT_WHITE);
    while c.primary.contrast(c.surface) < 4.5 {
        c.primary = c.primary.mix(white, palette_recipe::CONTRAST_STEP);
    }
    c.on_primary = if black.contrast(c.primary) >= white.contrast(c.primary) {
        black
    } else {
        white
    };
    c.focus = c.primary;
    value
}
impl Theme {
    pub fn from_palette(palette: &lucent_domain::ThemePalette) -> Self {
        fn color(rgb: lucent_domain::Rgb) -> Color {
            let [r, g, b] = rgb.channels();
            Color(
                f32::from(r) / 255.,
                f32::from(g) / 255.,
                f32::from(b) / 255.,
                1.,
            )
        }
        let c = &palette.colors;
        Self {
            surface: color(c.surface),
            surface_container: color(c.surface_container),
            on_surface: color(c.on_surface),
            primary: color(c.primary),
            on_primary: color(c.on_primary),
            error: color(c.error),
            success: color(c.success),
            warning: color(c.warning),
            info: color(c.info),
            focus: color(c.focus),
        }
    }

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
            if let Some(shadow) = &mut e.style.shadow {
                shadow.color = palette::BLACK.alpha(opacity::SHADOW);
                shadow.offset = (0., elevation::OFFSET);
                shadow.blur = elevation::BLUR;
            }
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
    /// Checkbox/radio state without depending on a font's symbol coverage.
    /// The enclosing control owns input and the application's checked value.
    pub fn selection_indicator<M>(self, checked: bool, radio: bool) -> Element<M> {
        use component::selection_indicator as token;
        let radius = if radio {
            token::SIZE / 2.
        } else {
            token::RADIUS
        };
        let mut layers = vec![
            Element::empty()
                .size(
                    token::SIZE - token::STROKE * 2.,
                    token::SIZE - token::STROKE * 2.,
                )
                .at(token::STROKE, token::STROKE)
                .radius((radius - token::STROKE).max(0.))
                .background(self.surface_container),
        ];
        if checked {
            layers.push(
                Element::empty()
                    .size(token::FILL_SIZE, token::FILL_SIZE)
                    .at(
                        (token::SIZE - token::FILL_SIZE) / 2.,
                        (token::SIZE - token::FILL_SIZE) / 2.,
                    )
                    .radius(if radio {
                        token::FILL_SIZE / 2.
                    } else {
                        token::RADIUS
                    })
                    .background(self.primary),
            );
        }
        Element::stack(layers)
            .size(token::SIZE, token::SIZE)
            .radius(radius)
            .background(self.on_surface)
    }
    /// The design-system recipe for a framework scalar control.
    pub fn slider<M>(
        self,
        spec: lucent_api::SliderSpec,
        on_change: impl Fn(lucent_api::SliderEvent) -> M + Send + Sync + 'static,
    ) -> Element<M> {
        use component::slider as token;
        Element::slider(spec, on_change)
            .width(lucent_api::Length::Fill)
            .height(lucent_api::Length::Fixed(token::HEIGHT))
            .padding_xy(token::PADDING_INLINE, 0.)
            .radius(radius::CONTROL)
            .slider_style(lucent_api::SliderStyle {
                track: self.surface_container,
                fill: self.primary,
                thumb: self.primary,
                track_height: token::TRACK_HEIGHT,
                thumb_diameter: token::THUMB_DIAMETER,
            })
    }
    /// A rounded, padded viewport; the caller chooses its bounded height.
    pub fn scroll<M>(
        self,
        spec: lucent_api::ScrollSpec,
        content: Element<M>,
        on_change: impl Fn(lucent_api::ScrollEvent) -> M + Send + Sync + 'static,
    ) -> Element<M> {
        use component::scroll as token;
        Element::scroll(spec, content, on_change)
            .fill()
            .padding(token::PADDING)
            .radius(radius::CONTROL)
            .background(self.surface_container)
            .scroll_style(lucent_api::ScrollStyle {
                thumb: self.primary,
                width: token::THUMB_WIDTH,
                gutter: token::GUTTER,
                minimum_thumb: token::MINIMUM_THUMB,
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

#[cfg(test)]
mod palette_tests {
    use super::*;
    #[test]
    fn presets_and_wallpaper_colors_preserve_semantic_contrast() {
        for palette in palettes() {
            palette.validate().unwrap();
        }
        for r in (0..=255).step_by(51) {
            for g in (0..=255).step_by(51) {
                for b in (0..=255).step_by(51) {
                    wallpaper_palette(lucent_domain::Rgb::new(r, g, b))
                        .validate()
                        .unwrap();
                }
            }
        }
    }
    #[test]
    fn legacy_settings_and_saved_palette_round_trip_without_losing_identity() {
        let old: lucent_domain::DesktopSettings =
            serde_json::from_str(r#"{"version":1,"light":true}"#).unwrap();
        assert_eq!(selected_palette(&old).name, "Pearl");
        let palette = wallpaper_palette(lucent_domain::Rgb::new(60, 100, 170));
        let settings = lucent_domain::DesktopSettings {
            palette: Some(palette.clone()),
            ..Default::default()
        };
        let saved: lucent_domain::DesktopSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(selected_palette(&saved), palette);
        assert!(serde_json::from_str::<lucent_domain::Rgb>(r##""#zz00ff""##).is_err());
    }
}
