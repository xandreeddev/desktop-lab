//! Native catalog client. All provider work uses injected ports on a cancellable stream.
use crate::adapters::DesktopAdapters;
use lucent_api::*;
use lucent_design::{Theme, component::wallpaper_browser as t, font, layout, radius, space};
use lucent_domain::{self as domain, *};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub enum Message {
    Source(Option<WallpaperProvider>),
    Query(String),
    Search(u32),
    Select(usize),
    Navigate(i32),
    Results(u64, domain::Result<WallpaperPage>),
    Preview(u64, String, Arc<ImageData>),
    Finished(u64),
    Download(bool),
    Ready(u64, domain::Result<(Wallpaper, Option<Rgb>)>),
}
#[derive(Clone)]
enum Job {
    Search(WallpaperProvider, String, u32),
    Download(RemoteWallpaper, bool),
}
#[derive(Default)]
pub struct Browser {
    pub source: Option<WallpaperProvider>,
    pub query: String,
    pub items: Vec<RemoteWallpaper>,
    pub selected: usize,
    pub page: u32,
    pub has_more: bool,
    pub status: String,
    pub previews: BTreeMap<String, Arc<ImageData>>,
    pub generation: u64,
    job: Option<Job>,
    pub submitted: String,
}
impl Browser {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Source(source) => {
                self.generation += 1;
                self.job = None;
                self.source = source;
                self.items.clear();
                self.previews.clear();
                self.selected = 0;
                self.page = 0;
                self.has_more = false;
                self.status.clear();
            }
            Message::Query(query) => self.query = query,
            Message::Search(page) => {
                if page == 0
                    || matches!(self.job, Some(Job::Download(..)))
                    || (self.busy() && self.items.is_empty())
                {
                    return;
                }
                if let Some(provider) = self.source {
                    self.generation += 1;
                    self.job = Some(Job::Search(provider, self.query.clone(), page));
                    self.submitted = self.query.clone();
                    self.status = "Searching…".into();
                    self.items.clear();
                    self.previews.clear();
                    self.selected = 0;
                }
            }
            Message::Results(id, result) if id == self.generation => match result {
                Ok(page) => {
                    self.items = page.items;
                    self.page = page.page;
                    self.has_more = page.has_more;
                    self.status = if self.items.is_empty() {
                        "No wallpapers found".into()
                    } else {
                        format!("{} results · loading previews…", self.items.len())
                    };
                }
                Err(error) => self.status = error.to_string(),
            },
            Message::Preview(id, key, image) if id == self.generation => {
                self.previews.insert(key, image);
            }
            Message::Finished(id) if id == self.generation => {
                self.job = None;
                if !self.items.is_empty() {
                    self.status = format!("{} results · page {}", self.items.len(), self.page);
                }
            }
            Message::Select(index) => self.selected = index.min(self.items.len().saturating_sub(1)),
            Message::Navigate(delta) => {
                self.selected = (self.selected as i32 + delta)
                    .clamp(0, self.items.len().saturating_sub(1) as i32)
                    as usize
            }
            Message::Download(colors) => {
                if matches!(self.job, Some(Job::Download(..))) {
                    return;
                }
                if let Some(item) = self.items.get(self.selected).cloned() {
                    self.generation += 1;
                    self.job = Some(Job::Download(item, colors));
                    self.status = "Downloading original…".into();
                }
            }
            Message::Ready(id, result) if id == self.generation => {
                self.job = None;
                self.status = match result {
                    Ok(_) => "Downloaded · applying…".into(),
                    Err(error) => error.to_string(),
                };
            }
            _ => {}
        }
    }
    pub fn subscriptions(&self, adapters: &DesktopAdapters) -> Vec<Subscription<Message>> {
        let Some(job) = self.job.clone() else {
            return vec![];
        };
        let id = self.generation;
        let catalog = adapters.catalog.clone();
        let assets = adapters.assets.clone();
        let palettes = adapters.palette_generation.clone();
        vec![Subscription::stream(
            format!("wallpaper-catalog-{id}"),
            move |out, cancel| match job {
                Job::Search(provider, query, page) => {
                    let result = catalog.search(provider, &query, page);
                    if cancel.cancelled() {
                        return;
                    }
                    let items = result
                        .as_ref()
                        .map(|page| page.items.clone())
                        .unwrap_or_default();
                    out.send(Message::Results(id, result));
                    for item in items {
                        if cancel.cancelled() {
                            return;
                        }
                        if let Ok(path) = catalog.preview(&item)
                            && let Some(image) = assets.background(&path)
                        {
                            if cancel.cancelled() {
                                return;
                            }
                            out.send(Message::Preview(id, item.id, image));
                        }
                    }
                    out.send(Message::Finished(id));
                }
                Job::Download(item, colors) => {
                    let result = catalog.download(&item).and_then(|wall| {
                        let seed = if colors {
                            Some(palettes.seed(&wall.path)?)
                        } else {
                            None
                        };
                        Ok((wall, seed))
                    });
                    if !cancel.cancelled() {
                        out.send(Message::Ready(id, result));
                    }
                }
            },
        )]
    }
    pub fn sources(&self, width: f32, theme: Theme) -> Element<Message> {
        Element::row(
            [
                (None, "Local"),
                (Some(WallpaperProvider::Wallhaven), "Wallhaven"),
                (Some(WallpaperProvider::AlphaCoders), "Alpha Coders"),
            ]
            .into_iter()
            .map(|(source, label)| {
                theme
                    .button(label, Message::Source(source))
                    .size((width - space::SM * 2.) / 3., t::SOURCE_HEIGHT)
                    .padding_xy(space::SM, space::XS)
                    .font(font::CAPTION)
                    .background(if self.source == source {
                        theme.primary
                    } else {
                        theme.surface_container
                    })
                    .color(if self.source == source {
                        theme.on_primary
                    } else {
                        theme.on_surface
                    })
                    .id(format!("source-{}", source.map_or("local", |s| s.id())))
            })
            .collect(),
        )
        .gap(space::SM)
        .size(width, t::SOURCE_HEIGHT)
    }
    pub fn view(&self, width: f32, height: f32, theme: Theme) -> Element<Message> {
        let compact = height < t::CARD_HEIGHT + t::SEARCH_HEIGHT + t::FOOTER_HEIGHT * 2.;
        let gap = if compact {
            space::XS
        } else {
            layout::SECTION_GAP
        };
        let control = if compact {
            lucent_design::component::launcher::TAB_HEIGHT
        } else {
            t::FOOTER_HEIGHT
        };
        let search_height = if compact { control } else { t::SEARCH_HEIGHT };
        let search = Element::row(vec![
            Element::input(&self.query, "Search wallpapers", Message::Query)
                .id("wallpaper-search")
                .autofocus()
                .focus_outline(false)
                .font(font::BODY)
                .color(theme.on_surface)
                .width(Length::Fill)
                .height(Length::Fill),
            theme
                .button("Search", Message::Search(1))
                .id("wallpaper-search-submit")
                .height(Length::Fill)
                .font(font::CAPTION),
        ])
        .gap(space::SM)
        .padding(space::SM)
        .size(width, search_height)
        .radius(radius::SEARCH)
        .background(theme.surface_container)
        .focus_within();
        let footer = control * 2. + gap;
        let available = (height - search_height - gap * 2. - footer).max(0.);
        let columns = ((width + gap) / (t::CARD_MIN_WIDTH + gap)).floor().max(1.) as usize;
        let rows = ((available + space::SM) / (t::CARD_HEIGHT + space::SM))
            .floor()
            .max(1.) as usize;
        let page_size = columns * rows;
        let start = (self.selected / page_size) * page_size;
        let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let card_height = t::CARD_HEIGHT.min(available);
        let mut content = vec![search];
        for (index, item) in self
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(if available >= control { page_size } else { 0 })
        {
            let selected = index == self.selected;
            let label_h = font::SMALL + space::SM;
            let image_h = (card_height - label_h - space::SM).max(0.);
            let image = self
                .previews
                .get(&item.id)
                .map(|image| Element::image(image.clone()).cover())
                .unwrap_or_else(|| Element::empty().background(theme.surface_container));
            let card = Element::column(vec![
                image
                    .size(card_width - space::SM * 2., (image_h - space::SM).max(0.))
                    .radius(radius::CONTROL),
                Element::text(format!("{} · {}", item.id, item.resolution))
                    .font(font::SMALL)
                    .color(theme.on_surface)
                    .size(card_width - space::SM * 2., label_h),
            ])
            .gap(space::SM)
            .padding(space::SM)
            .size(card_width, card_height)
            .radius(radius::CARD)
            .background(if selected {
                theme.surface_container
            } else {
                theme.surface
            })
            .selected(selected)
            .on_click(Message::Select(index))
            .id(format!("online-wallpaper-{index}"));
            content.push(card.at(
                ((index - start) % columns) as f32 * (card_width + gap),
                t::SEARCH_HEIGHT
                    + gap
                    + ((index - start) / columns) as f32 * (card_height + space::SM),
            ));
        }
        let status = if self.items.is_empty() && self.status.is_empty() {
            "Search to browse wallpapers"
        } else {
            &self.status
        };
        let half = (width - gap) / 2.;
        let actions = Element::row(vec![
            theme
                .button("Wallpaper only", Message::Download(false))
                .id("download-wallpaper")
                .size(half, control)
                .font(font::CAPTION),
            theme
                .button("Wallpaper + colors", Message::Download(true))
                .id("download-theme")
                .size(half, control)
                .font(font::CAPTION)
                .background(theme.primary)
                .color(theme.on_primary),
        ])
        .gap(gap);
        let pages = Element::row(vec![
            theme
                .button("‹", Message::Search(self.page.saturating_sub(1)))
                .id("catalog-previous")
                .size(control, control),
            Element::text(status)
                .font(font::CAPTION)
                .color(theme.on_surface)
                .width(Length::Fill)
                .height(Length::Fixed(control)),
            theme
                .button(
                    "›",
                    Message::Search(if self.has_more { self.page + 1 } else { 0 }),
                )
                .id("catalog-next")
                .size(control, control),
        ])
        .gap(space::SM)
        .size(width, control);
        content.push(pages.at(0., height - footer));
        content.push(actions.at(0., height - control));
        Element::stack(content).size(width, height).clip()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_source_discards_stale_network_responses() {
        let mut browser = Browser::default();
        browser.update(Message::Source(Some(WallpaperProvider::Wallhaven)));
        browser.update(Message::Query("mountains".into()));
        browser.update(Message::Search(1));
        let old = browser.generation;
        browser.update(Message::Source(Some(WallpaperProvider::AlphaCoders)));
        browser.update(Message::Search(1));
        browser.update(Message::Results(
            old,
            Err(DomainError::Unavailable("old failure".into())),
        ));
        browser.update(Message::Finished(old));
        assert!(browser.busy());
        assert_eq!(browser.status, "Searching…");
        assert!(browser.items.is_empty());
    }
}
