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
            e.style.placeholder_opacity = component::input::PLACEHOLDER_OPACITY;
            e.style.text_selection_color = theme.primary.alpha(component::input::SELECTION_OPACITY);
            e.style.text_underline_width = component::input::UNDERLINE_WIDTH;
            e.style.hover_color = if e.style.hover_feedback {
                theme.primary.alpha(opacity::HOVER)
            } else {
                Color::TRANSPARENT
            };
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
    /// A controlled choice with the same geometry as an ordinary button.
    /// The client owns selection and supplies the action for this choice.
    pub fn choice_button<M>(
        self,
        label: impl Into<String>,
        message: M,
        selected: bool,
    ) -> Element<M> {
        let control = self.button(label, message);
        if selected {
            control.background(self.primary).color(self.on_primary)
        } else {
            control
        }
    }
    /// One keyboard/pointer target for a label and its controlled switch.
    /// The client supplies the next-value message; rendering changes no state.
    pub fn toggle_row<M>(self, label: impl Into<String>, message: M, checked: bool) -> Element<M> {
        Element::row(vec![
            Element::text(label)
                .font(font::CONTROL)
                .color(self.on_surface)
                .width(lucent_api::Length::Fill),
            self.switch_indicator(checked),
        ])
        .width(lucent_api::Length::Fill)
        .align(lucent_api::Align::Center)
        .gap(space::SM)
        .padding_xy(
            component::button::PADDING_INLINE,
            component::button::PADDING_BLOCK,
        )
        .radius(component::button::RADIUS)
        .background(self.surface)
        .on_click(message)
    }
    /// A complete ordinary field using the shared height and content insets.
    /// Clients retain IDs, submit policy and text state; masked fields stay separate.
    pub fn text_input<M>(
        self,
        value: impl Into<String>,
        selection: lucent_api::TextSelection,
        placeholder: impl Into<String>,
        on_edit: impl Fn(lucent_api::TextEdit) -> M + Send + Sync + 'static,
    ) -> Element<M> {
        self.text_input_content(value, selection, placeholder, on_edit)
            .focus_outline(true)
            .height(lucent_api::Length::Fixed(component::input::HEIGHT))
            .padding_xy(
                component::input::PADDING_INLINE,
                component::input::PADDING_BLOCK,
            )
            .background(self.surface_container)
            .radius(radius::CONTROL)
            .clip()
    }
    /// Bounded multiline field. The client chooses actual geometry and owns
    /// text/selection/scroll; all visual roles reuse the design system recipes.
    pub fn text_area<M>(
        self,
        value: impl Into<String>,
        selection: lucent_api::TextSelection,
        placeholder: impl Into<String>,
        scroll: lucent_api::ScrollSpec,
        on_edit: impl Fn(lucent_api::TextEdit) -> M + Send + Sync + 'static,
        on_scroll: impl Fn(lucent_api::ScrollEvent) -> M + Send + Sync + 'static,
    ) -> Element<M> {
        Element::text_area(value, selection, placeholder, scroll, on_edit, on_scroll)
            .placeholder_opacity(component::input::PLACEHOLDER_OPACITY)
            .font(font::BODY)
            .color(self.on_surface)
            .fill()
            .padding_xy(
                component::input::PADDING_INLINE,
                component::input::PADDING_BLOCK,
            )
            .background(self.surface_container)
            .radius(radius::CONTROL)
            .focus_outline(true)
            .scroll_style(lucent_api::ScrollStyle {
                thumb: self.primary,
                width: component::scroll::THUMB_WIDTH,
                gutter: component::scroll::GUTTER,
                minimum_thumb: component::scroll::MINIMUM_THUMB,
            })
    }
    /// Editable content inside `input_group`; the group supplies the field
    /// height, insets, background and focus decoration exactly once.
    pub fn text_input_content<M>(
        self,
        value: impl Into<String>,
        selection: lucent_api::TextSelection,
        placeholder: impl Into<String>,
        on_edit: impl Fn(lucent_api::TextEdit) -> M + Send + Sync + 'static,
    ) -> Element<M> {
        Element::text_input(value, selection, placeholder, on_edit)
            .placeholder_opacity(component::input::PLACEHOLDER_OPACITY)
            .font(font::BODY)
            .color(self.on_surface)
            .width(lucent_api::Length::Fill)
            .height(lucent_api::Length::Fill)
            .focus_outline(false)
    }
    /// One field containing optional leading and trailing controls. Pass
    /// `text_input_content` between them; slots move the text start intentionally.
    pub fn input_group<M>(self, contents: Vec<Element<M>>) -> Element<M> {
        Element::row(contents)
            .align(lucent_api::Align::Center)
            .gap(space::SM)
            .padding_xy(
                component::input::PADDING_INLINE,
                component::input::PADDING_BLOCK,
            )
            .width(lucent_api::Length::Fill)
            .height(lucent_api::Length::Fixed(component::input::HEIGHT))
            .radius(radius::CONTROL)
            .background(self.surface_container)
            .focus_within()
            .clip()
    }
    /// Supporting status uses the semantic error role when the client reports
    /// a failure. Hosts choose wrapping without changing their input geometry.
    pub fn status_text<M>(self, label: impl Into<String>, error: bool) -> Element<M> {
        Element::text(label)
            .font(font::CAPTION)
            .color(if error { self.error } else { self.on_surface })
            .width(lucent_api::Length::Fill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_recipes_and_composed_controls_take_placeholder_opacity_from_tokens() {
        use lucent_api::{ScrollSpec, TextSelection};
        assert_eq!(component::input::PLACEHOLDER_OPACITY, opacity::PLACEHOLDER);
        assert_ne!(
            lucent_api::Style::default().placeholder_opacity,
            opacity::PLACEHOLDER
        );
        for theme in [Theme::new(false), Theme::new(true)] {
            let recipes = [
                theme.text_input("", TextSelection::default(), "Hint", |_| ()),
                theme.text_input_content("", TextSelection::default(), "Hint", |_| ()),
                theme.text_area(
                    "",
                    TextSelection::default(),
                    "Hint",
                    ScrollSpec::new(0., component::scroll::LINE_STEP).unwrap(),
                    |_| (),
                    |_| (),
                ),
            ];
            for recipe in recipes {
                assert_eq!(
                    recipe.style.placeholder_opacity,
                    component::input::PLACEHOLDER_OPACITY
                );
            }
            let bare = Element::stack(vec![
                Element::input("", "Hint", |_| ()),
                Element::masked_input(0, "Hint", |_| ()),
            ]);
            let styled = theme.apply(bare);
            assert!(
                styled
                    .children
                    .iter()
                    .all(|child| child.style.placeholder_opacity
                        == component::input::PLACEHOLDER_OPACITY)
            );
        }
    }
    #[test]
    fn plain_and_grouped_fields_share_height_caret_insets_and_vertical_center() {
        use lucent_api::{Length, TextSelection};
        use lucent_ui::{Interaction, Layout, Paint};
        use std::sync::Arc;
        let fonts = Arc::new(vec![
            fontdue::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                Default::default(),
            )
            .unwrap(),
        ]);
        let layout = Layout::new(fonts);
        for theme in [Theme::new(false), Theme::new(true)] {
            for width in [232., 640.] {
                for leading in [None, Some(icon::COMPACT)] {
                    let field = theme
                        .text_input_content("", TextSelection::default(), "Search", |_| ())
                        .id("field")
                        .autofocus();
                    let mut contents = vec![];
                    if let Some(size) = leading {
                        contents.push(Element::empty().size(size, size));
                    }
                    contents.push(field);
                    let grouped = theme.input_group(contents).width(Length::Fixed(width));
                    let plain = theme
                        .text_input("", TextSelection::default(), "Search", |_| ())
                        .id("field")
                        .autofocus()
                        .width(Length::Fixed(width));
                    let mut carets = vec![];
                    for tree in [plain, grouped] {
                        assert_eq!(tree.style.height, Length::Fixed(component::input::HEIGHT));
                        let tree = theme.apply(tree);
                        let mut input = Interaction::default();
                        let initial =
                            layout.build(&tree, width, component::input::HEIGHT, &input, 0.);
                        input.synchronize(&initial);
                        let scene =
                            layout.build(&tree, width, component::input::HEIGHT, &input, 0.);
                        let hit = scene.hits.iter().find(|hit| hit.id == "field").unwrap();
                        let text = hit.text.as_ref().unwrap();
                        assert_eq!(text.inner.y, component::input::PADDING_BLOCK);
                        assert_eq!(
                            text.inner.h,
                            component::input::HEIGHT - component::input::PADDING_BLOCK * 2.
                        );
                        let caret = scene
                            .paint
                            .iter()
                            .find_map(|paint| match paint {
                                Paint::Shape { rect, .. }
                                    if rect.w == component::input::CARET_WIDTH =>
                                {
                                    Some(*rect)
                                }
                                _ => None,
                            })
                            .expect("focused empty field paints a caret");
                        assert_eq!(caret.x, text.inner.x);
                        assert!(
                            (caret.y + caret.h / 2. - component::input::HEIGHT / 2.).abs()
                                < f32::EPSILON * component::input::HEIGHT,
                            "caret must stay vertically centered: {caret:?}"
                        );
                        carets.push(caret);
                    }
                    assert_eq!(carets[0].x, component::input::PADDING_INLINE);
                    assert_eq!(
                        carets[1].x,
                        component::input::PADDING_INLINE
                            + leading.map_or(0., |size| size + space::SM)
                    );
                    assert_eq!(carets[0].y, carets[1].y);
                    assert_eq!(carets[0].h, carets[1].h);
                }
            }
        }
    }
    #[test]
    fn theme_application_and_mapping_preserve_passive_container_feedback_policy() {
        for theme in [Theme::new(false), Theme::new(true)] {
            let panel = Element::stack(vec![theme.button("Apply", 1)])
                .background(theme.surface)
                .on_click(0)
                .hover_feedback(false)
                .tab_stop(false);
            let themed = theme.apply(panel).map(|message| message.to_string());
            let themed = theme.apply(themed);
            assert!(!themed.style.hover_feedback);
            assert_eq!(themed.style.hover_color, Color::TRANSPARENT);
            assert_eq!(themed.click.as_deref(), Some("0"));
            assert!(!themed.style.tab_stop);
            assert!(themed.children[0].style.hover_feedback);
            assert_eq!(
                themed.children[0].style.hover_color,
                theme.primary.alpha(opacity::HOVER)
            );
            assert_eq!(themed.children[0].click.as_deref(), Some("1"));
        }
    }
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
