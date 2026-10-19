//! Shared desktop selector composition. Callers own sources, preview workers,
//! selection and Apply policy; this component only places their prepared cards.
use crate::artwork::Artwork;
use lucent_api::{Align, Element, ImageData, Rect};
use lucent_design::{component::wallpaper as token, font, layout, radius, space};
use std::sync::Arc;

/// The same range drives painting and worker demand; never scan the full library.
pub fn visible_wallpapers(count: usize, center: f32) -> std::ops::Range<usize> {
    if !center.is_finite() {
        return 0..0;
    }
    let start = ((center - token::VISIBLE_DISTANCE).floor() + 1.).max(0.) as usize;
    let end = (center + token::VISIBLE_DISTANCE).ceil().max(0.) as usize;
    start.min(count)..end.min(count)
}

pub struct Card<'a, M> {
    pub index: usize,
    pub caption: &'a str,
    pub image: Option<Arc<ImageData>>,
    pub unavailable: bool,
    pub select: M,
    pub id: String,
}
pub struct Action<'a, M> {
    pub label: &'a str,
    pub message: M,
    pub enabled: bool,
    pub id: &'static str,
}
pub struct Controls<'a, M> {
    pub previous: (&'static str, M),
    pub next: (&'static str, M),
    pub apply: Action<'a, M>,
}
pub struct Carousel<'a, M> {
    pub cards: Vec<Card<'a, M>>,
    pub center: f32,
    pub artwork: Artwork<'a>,
    pub controls: Controls<'a, M>,
    pub empty: &'a str,
}

pub fn preferred_height() -> f32 {
    token::STAGE_HEIGHT + token::FOOTER_HEIGHT + space::SECTION
}

pub fn hero_size(width: f32) -> (f32, f32) {
    let width = token::HERO_WIDTH.min((width - space::LG * 2.).max(0.));
    (width, width * token::ASPECT_RATIO)
}

fn card_bounds(width: f32, center: f32, index: usize) -> Rect {
    let distance = index as f32 - center;
    let abs = distance.abs();
    // Preserve the original wide selector. Narrow selectors retain token
    // padding around the central image instead of clipping its rounded edges.
    let (hero, _) = hero_size(width);
    let scale = hero / token::HERO_WIDTH;
    let size = (hero
        - abs.min(1.) * token::NEAR_SHRINK * scale
        - (abs - 1.).clamp(0., 1.) * token::FAR_SHRINK * scale)
        .max(0.);
    let height = size * token::ASPECT_RATIO;
    let x = width / 2.
        + distance.signum()
            * (abs.min(1.) * token::NEAR_STEP + (abs - 1.).max(0.) * token::FAR_STEP)
            * scale;
    Rect::new(
        x - size / 2.,
        (token::STAGE_HEIGHT - height) / 2.,
        size,
        height,
    )
}

impl<M: Clone + 'static> Carousel<'_, M> {
    pub fn render(mut self, width: f32, height: f32) -> Element<M> {
        let theme = self.artwork.theme;
        if self.cards.is_empty() {
            return Element::text(self.empty)
                .font(font::LABEL)
                .color(theme.on_surface)
                .size(width, height)
                .align(Align::Center);
        }
        // Distant cards paint first so the central hero remains on top.
        self.cards.sort_by(|a, b| {
            ((b.index as f32 - self.center).abs()).total_cmp(&(a.index as f32 - self.center).abs())
        });
        let mut elements = Vec::new();
        for card in self.cards {
            let rect = card_bounds(width, self.center, card.index);
            let image = card
                .image
                .map(|data| Element::image(data).contain())
                .unwrap_or_else(|| {
                    if card.unavailable {
                        Element::text("Preview unavailable")
                            .font(font::SMALL)
                            .color(theme.on_surface)
                            .align(Align::Center)
                            .background(theme.surface_container)
                    } else {
                        Element::empty().background(theme.surface_container)
                    }
                });
            elements.push(
                Element::stack(vec![
                    image.size(rect.w, rect.h).radius(radius::CARD),
                    Element::text(card.caption)
                        .font(font::SMALL)
                        .color(theme.on_surface)
                        .size(
                            (rect.w - token::CAPTION_INSETS).max(0.),
                            token::CAPTION_HEIGHT,
                        )
                        .at(token::CAPTION_INSET, rect.h + token::CAPTION_GAP)
                        .align(Align::Center),
                ])
                .size(rect.w, rect.h + token::FOOTER_HEIGHT)
                .at(rect.x, rect.y)
                .on_click(card.select)
                .id(card.id),
            );
        }
        let footer_y = height - token::FOOTER_HEIGHT;
        elements.push(
            self.artwork
                .icon_button(self.controls.previous.0, "left", self.controls.previous.1)
                .size(token::CONTROL_SIZE, token::CONTROL_SIZE)
                .padding(space::MD)
                .at(0., footer_y),
        );
        let apply_width = token::BUTTON_WIDTH
            .min((width - token::CONTROL_SIZE * 2. - layout::SECTION_GAP * 2.).max(0.));
        let mut apply = theme
            .button(self.controls.apply.label, self.controls.apply.message)
            .size(apply_width, token::BUTTON_HEIGHT)
            .background(theme.primary)
            .color(theme.on_primary)
            .id(self.controls.apply.id)
            .at((width - apply_width) / 2., footer_y);
        if !self.controls.apply.enabled {
            apply.click = None;
            apply = apply.tab_stop(false).hover_feedback(false);
        }
        elements.push(apply);
        elements.push(
            self.artwork
                .icon_button(self.controls.next.0, "right", self.controls.next.1)
                .size(token::CONTROL_SIZE, token::CONTROL_SIZE)
                .padding(space::MD)
                .at(width - token::CONTROL_SIZE, footer_y),
        );
        Element::stack(elements).size(width, height).clip()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Message {
        Select(usize),
        Previous,
        Next,
        Apply,
    }
    #[test]
    fn card_selection_is_separate_from_apply_and_unwritable_apply_has_no_action() {
        let images = std::collections::BTreeMap::new();
        let artwork = Artwork {
            images: &images,
            theme: lucent_design::Theme::from_palette(&lucent_design::palettes()[0]),
        };
        for enabled in [true, false] {
            let tree = Carousel {
                cards: vec![Card {
                    index: 0,
                    caption: "Owned fixture",
                    image: None,
                    unavailable: false,
                    select: Message::Select(0),
                    id: "live-library-0".into(),
                }],
                center: 0.,
                artwork,
                controls: Controls {
                    previous: ("live-previous", Message::Previous),
                    next: ("live-next", Message::Next),
                    apply: Action {
                        label: "Apply wallpaper",
                        message: Message::Apply,
                        enabled,
                        id: "live-apply",
                    },
                },
                empty: "Empty library",
            }
            .render(252., preferred_height());
            let card = tree
                .children
                .iter()
                .find(|child| child.id == "live-library-0")
                .unwrap();
            assert_eq!(card.click, Some(Message::Select(0)));
            let apply = tree
                .children
                .iter()
                .find(|child| child.id == "live-apply")
                .unwrap();
            assert_eq!(apply.click, enabled.then_some(Message::Apply));
            if !enabled {
                assert!(!apply.style.tab_stop);
            }
        }
    }
    #[test]
    fn wide_geometry_matches_original_and_narrow_hero_retains_padding() {
        for width in [600., 800.] {
            let center = card_bounds(width, 2., 2);
            assert_eq!(center.w, token::HERO_WIDTH);
            assert_eq!(center.x, (width - token::HERO_WIDTH) / 2.);
            let near = card_bounds(width, 2., 3);
            assert_eq!(near.w, token::HERO_WIDTH - token::NEAR_SHRINK);
            assert_eq!(near.x + near.w / 2., width / 2. + token::NEAR_STEP);
        }
        for width in [180., 252., 320.] {
            let center = card_bounds(width, 1., 1);
            assert!(center.x >= space::LG);
            assert!(center.x + center.w <= width - space::LG);
            assert!(center.w > 0. && center.h > 0.);
            assert!(
                center.y + center.h + token::CAPTION_GAP + token::CAPTION_HEIGHT
                    < preferred_height() - token::FOOTER_HEIGHT
            );
        }
    }
    #[test]
    fn painting_and_demand_share_bounded_fractional_neighbors() {
        for center in [0., 0.5, 8., 8.5, 31.] {
            let visible = visible_wallpapers(32, center);
            assert!(visible.len() <= 7);
            for index in visible {
                assert!((index as f32 - center).abs() < token::VISIBLE_DISTANCE);
            }
        }
        assert!(visible_wallpapers(32, f32::NAN).is_empty());
        assert!(visible_wallpapers(0, 0.).is_empty());
    }
}
