//! Shared decoded icon presentation; views never load assets or choose adapters.
use lucent_api::*;
use lucent_design::*;
use lucent_domain::Application as App;
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy)]
pub struct Artwork<'a> {
    pub images: &'a BTreeMap<String, Arc<ImageData>>,
    pub theme: Theme,
}
impl Artwork<'_> {
    pub fn icon<M: Clone + 'static>(&self, name: &str, size: f32) -> Element<M> {
        self.images
            .get(&format!("symbol:{name}"))
            .cloned()
            .map(|data| Element::image(data).contain())
            .unwrap_or_else(Element::empty)
            .size(size, size)
            .tint(self.theme.on_surface)
    }
    pub fn icon_button<M: Clone + 'static>(&self, id: &str, name: &str, message: M) -> Element<M> {
        self.icon(name, icon::CONTROL)
            .padding(component::icon_button::PADDING)
            .size(
                component::icon_button::EXTENT,
                component::icon_button::EXTENT,
            )
            .radius(radius::CONTROL)
            .background(self.theme.surface)
            .on_click(message)
            .id(id)
    }
    pub fn app_icon<M: Clone + 'static>(&self, app: &App, size: f32) -> Element<M> {
        self.images
            .get(&format!("app:{}", app.id.as_str()))
            .or_else(|| self.images.get(&app.icon))
            .or_else(|| self.images.get("app:fallback"))
            .cloned()
            .map(|data| {
                let monochrome = data.key.starts_with("lucent-app:");
                let image = Element::image(data).contain();
                if monochrome {
                    image.tint(self.theme.on_surface)
                } else {
                    image
                }
            })
            .unwrap_or_else(|| {
                self.icon("apps", size)
                    .padding(space::XS)
                    .background(self.theme.surface_container)
                    .radius(radius::CARD)
            })
            .size(size, size)
    }
}
