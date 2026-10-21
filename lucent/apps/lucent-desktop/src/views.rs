use crate::{
    desktop::*,
    launcher::{self, Mode},
    shell_layout,
};
use lucent_api::*;
use lucent_design::{self as design, *};
use lucent_domain::Application as App;

impl Desktop {
    pub fn theme(&self) -> design::Theme {
        design::Theme::from_palette(&design::selected_palette(&self.settings))
    }
    pub fn surface_color(&self) -> Color {
        self.theme().surface
    }
    pub fn ink(&self) -> Color {
        self.theme().on_surface
    }
    pub fn accent(&self) -> Color {
        self.theme().primary
    }
    pub fn label(&self, text: impl Into<String>, size: f32) -> Element<Message> {
        Element::text(text).font(size).color(self.ink())
    }
    pub fn artwork(&self) -> crate::artwork::Artwork<'_> {
        crate::artwork::Artwork {
            images: &self.images,
            theme: self.theme(),
        }
    }
    pub fn icon(&self, name: &str, size: f32) -> Element<Message> {
        self.artwork().icon(name, size)
    }
    pub fn app_icon(&self, app: &App, size: f32) -> Element<Message> {
        self.artwork().app_icon(app, size)
    }
    fn launcher_face(&self, cx: &ViewContext, width: f32, height: f32) -> Element<Message> {
        let palettes = if self.launcher.mode() == Mode::Themes {
            self.palette_choices()
        } else {
            Vec::new()
        };
        crate::launcher::View {
            launcher: &self.launcher,
            input: crate::launcher::Input {
                apps: &self.apps,
                application_status: self.catalog.status(),
                widgets_editing: self.active_widgets().editing(),
                settings: &self.settings,
                widget_layout: self.active_widget_layout(),
                palettes: &palettes,
                wallpapers: &self.wallpapers,
                wallpaper_index: self.wallpaper_index,
                browser: &self.browser,
            },
            artwork: self.artwork(),
            previews: self.previews.view(),
            carousel: self.carousel,
            current_wallpaper: self.wallpaper.source_path(),
            error: self.error_message(),
            theme_error: &self.theme_error,
        }
        .render(cx, width, height)
        .map(Message::Launcher)
    }
    pub fn dock(&self, cx: &ViewContext) -> Element<Message> {
        let viewport = self.dock_viewport();
        let width = self
            .launcher
            .panel_width()
            .value(cx.now)
            .max(component::dock::MINIMUM_WIDTH);
        let height = self
            .launcher
            .panel_height()
            .value(cx.now)
            .max(component::dock::MINIMUM_HEIGHT);
        let reveal = self.launcher.reveal().value(cx.now).clamp(0., 1.);
        let (x, mut y) =
            shell_layout::dock_origin(viewport, (cx.width, cx.height), (width, height));
        let visibility = if self.launcher.expanded() {
            1.
        } else {
            self.dock.progress(cx.now)
        };
        y += (1. - visibility) * crate::shell_layout::dock_slide_distance(viewport.1);
        let mut children = vec![];
        if reveal < 0.999 {
            children.push(self.dock_row().opacity((1. - reveal * 2.).max(0.)).at(
                (self.dock_width() - shell_layout::dock_content_width(self.dock_apps().len() + 1))
                    / 2.,
                (component::panel::DOCK_HEIGHT - component::dock_row::HEIGHT) / 2.,
            ));
        }
        if reveal > 0.001 {
            let target_width = self.launcher.panel_width().target_value();
            let target_height = self.launcher.panel_height().target_value();
            children.push(
                self.launcher_face(
                    cx,
                    target_width - (2. * component::dock::CONTENT_INSET),
                    target_height - (2. * component::dock::CONTENT_INSET),
                )
                .at(
                    component::dock::CONTENT_INSET,
                    component::dock::CONTENT_INSET,
                )
                .opacity(((reveal - 0.2) / 0.8).clamp(0., 1.)),
            );
        }
        // One persistent rounded surface morphs between the dock and launcher.
        let panel = Element::stack(children)
            .size(width, height)
            .at(x, y)
            .radius(radius::PANEL)
            .background(self.surface_color())
            .shadow()
            .clip()
            .hover_feedback(false)
            .tab_stop(false)
            .on_click(Message::Launcher(launcher::Message::Select(
                self.launcher.selected(),
            )))
            .id("dock-panel");
        let mut layers = vec![];
        if visibility > 0. {
            layers.push(
                panel
                    .opacity(visibility)
                    .on_hover(Message::Dock(crate::dock::Message::Pointer(true))),
            );
        }
        if self.settings.dock.auto_hide && !self.launcher.expanded() {
            layers.push(
                Element::empty()
                    .size(self.dock_width(), component::dock::HOTSPOT_HEIGHT)
                    .at(
                        (cx.width - self.dock_width()) / 2.,
                        cx.height - component::dock::HOTSPOT_HEIGHT,
                    )
                    .on_hover(Message::Dock(crate::dock::Message::Pointer(true)))
                    .on_click(Message::Launcher(launcher::Message::Toggle))
                    .id("dock-hotspot")
                    .tab_stop(false)
                    .input_only(),
            );
        }
        Element::stack(layers).fill()
    }
    fn dock_row(&self) -> Element<Message> {
        let mut items = vec![
            self.icon("apps", icon::LAUNCHER)
                .padding(space::SM)
                .size(
                    component::dock_row::ITEM_SIZE,
                    component::dock_row::ITEM_SIZE,
                )
                .radius(radius::CARD)
                .background(self.surface_color())
                .on_click(Message::Launcher(launcher::Message::Toggle))
                .id("dock-launcher"),
        ];
        let mut previews = vec![];
        for (index, app) in self.dock_apps().into_iter().enumerate() {
            let running = self
                .compositor
                .windows
                .iter()
                .any(|w| app.matches_window_class(&w.app_class));
            let mut content = vec![self.app_icon(app, icon::APP).at(
                (component::dock_row::ITEM_SIZE - icon::APP) / 2.,
                (component::dock_row::ITEM_SIZE - icon::APP) / 2.,
            )];
            if running {
                content.push(
                    Element::empty()
                        .size(
                            component::dock_row::INDICATOR_WIDTH,
                            component::dock_row::INDICATOR_HEIGHT,
                        )
                        .at(
                            component::dock_row::INDICATOR_LEFT,
                            component::dock_row::INDICATOR_TOP,
                        )
                        .radius(radius::INDICATOR)
                        .background(self.accent()),
                );
            }
            let element = Element::stack(content)
                .size(
                    component::dock_row::ITEM_SIZE,
                    component::dock_row::ITEM_SIZE,
                )
                .radius(radius::CONTROL)
                .background(self.surface_color());
            let id = app.id.clone();
            let mut source = element
                .clone()
                .on_click(Message::Launch(app.id.clone(), true))
                .on_drag(move |event| Message::Dock(crate::dock::Message::Drag(id.clone(), event)))
                .id(format!("dock-{}", app.id.as_str()));
            if !self.launcher.expanded() && !self.dock.dragging() {
                source = source.tooltip(self.theme().tooltip(&app.name, OverlaySide::Above));
            }
            if self.dock.dragging() {
                items.push(source.input_only());
                previews.push(element.at(
                    (index + 1) as f32 * (component::dock_row::ITEM_SIZE + space::SM)
                        + self.dock.offset(&app.id, index),
                    0.,
                ));
            } else {
                items.push(source);
            }
        }
        let row = Element::row(items)
            .gap(space::SM)
            .height(Length::Fixed(component::dock_row::HEIGHT));
        if self.dock.dragging() {
            let mut layers = vec![row];
            layers.extend(previews);
            Element::stack(layers).size(
                shell_layout::dock_content_width(self.dock_apps().len() + 1),
                component::dock_row::HEIGHT,
            )
        } else {
            row
        }
    }
}
