//! Native catalog client. All provider work uses injected ports on a cancellable stream.
use crate::ports::AssetPort;
use lucent_api::*;
use lucent_design::{Theme, component::wallpaper_browser as t, font, layout, radius, space};
use lucent_domain::{self as domain, *};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub enum Message {
    Ignore,
    Controls(u64, Box<Message>),
    Source(Option<WallpaperProvider>),
    Query(String),
    EditQuery(TextEdit),
    Search,
    Page {
        revision: u64,
        page: u32,
    },
    Select(ResultChoice),
    Navigate(i32),
    Results(u64, domain::Result<WallpaperPage>),
    Preview(u64, String, Arc<ImageData>),
    Finished(u64),
    Download {
        choice: ResultChoice,
        selection: u64,
        colors: bool,
    },
    Ready(u64, domain::Result<(Wallpaper, bool)>),
}
/// A rendered result belongs to one presentation, including its exact URLs.
#[derive(Clone)]
pub struct ResultChoice {
    revision: u64,
    item: RemoteWallpaper,
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
    query_field: TextFieldState,
    pub items: Vec<RemoteWallpaper>,
    pub selected: usize,
    pub page: u32,
    pub has_more: bool,
    pub status: String,
    pub previews: BTreeMap<String, Arc<ImageData>>,
    pub generation: u64,
    job: Option<Job>,
    pub submitted: String,
    results_revision: u64,
    selection_revision: u64,
    controls_revision: u64,
}
impl Browser {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    /// Dismissing the launcher retires controls without cancelling accepted IO.
    pub fn retire_controls(&mut self) {
        self.controls_revision = self.controls_revision.saturating_add(1);
        self.results_revision = self.results_revision.saturating_add(1);
        self.query_field.cancel();
    }
    pub fn admit(&self, message: Message) -> Option<Message> {
        if let Message::Controls(revision, message) = message {
            return (revision == self.controls_revision && revision < u64::MAX)
                .then(|| self.admit(*message))
                .flatten();
        }
        Some(message)
    }
    pub fn accepts_download(&self, generation: u64) -> bool {
        generation == self.generation && matches!(self.job, Some(Job::Download(..)))
    }
    fn choice(&self, index: usize) -> Option<ResultChoice> {
        (self.results_revision < u64::MAX).then_some(())?;
        Some(ResultChoice {
            revision: self.results_revision,
            item: self.items.get(index)?.clone(),
        })
    }
    fn resolve(&self, choice: &ResultChoice) -> Option<usize> {
        if choice.revision != self.results_revision
            || choice.revision == u64::MAX
            || self.source != Some(choice.item.provider)
        {
            return None;
        }
        self.items.iter().position(|item| item == &choice.item)
    }
    pub fn select_message(&self, index: usize) -> Message {
        self.choice(index).map_or(Message::Ignore, Message::Select)
    }
    pub fn download_message(&self, colors: bool) -> Message {
        if matches!(self.job, Some(Job::Download(..))) || self.generation >= u64::MAX - 1 {
            return Message::Ignore;
        }
        self.choice(self.selected)
            .filter(|choice| self.selection_revision < u64::MAX && self.resolve(choice).is_some())
            .map_or(Message::Ignore, |choice| Message::Download {
                choice,
                selection: self.selection_revision,
                colors,
            })
    }
    fn page_message(&self, page: u32) -> Message {
        Message::Page {
            revision: self.results_revision,
            page,
        }
    }
    fn select(&mut self, index: usize) {
        if self.selected != index {
            self.selection_revision = self.selection_revision.saturating_add(1);
            self.selected = index;
        }
    }
    fn can_search(&self) -> bool {
        self.source.is_some()
            && self.generation < u64::MAX - 1
            && !matches!(self.job, Some(Job::Download(..)))
            && !(self.busy() && self.items.is_empty())
    }
    fn can_page(&self, page: u32) -> bool {
        self.can_search()
            && self.results_revision < u64::MAX
            && self.page > 0
            && page > 0
            && ((self.page > 1 && page == self.page - 1)
                || (self.has_more && self.page.checked_add(1) == Some(page)))
    }
    fn search(&mut self, query: String, page: u32) {
        if !self.can_search() {
            return;
        }
        if let Some(provider) = self.source
            && let Some(generation) = self.generation.checked_add(1).filter(|id| *id < u64::MAX)
        {
            self.query_field.cancel();
            self.generation = generation;
            self.job = Some(Job::Search(provider, query.clone(), page));
            self.submitted = query;
            self.status = "Searching…".into();
            self.results_revision = self.results_revision.saturating_add(1);
            self.items.clear();
            self.previews.clear();
            self.selected = 0;
            self.page = 0;
            self.has_more = false;
        }
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        let Some(message) = self.admit(message) else {
            return;
        };
        match message {
            Message::Source(source) => {
                self.results_revision = self.results_revision.saturating_add(1);
                self.query_field.cancel();
                self.generation = self.generation.saturating_add(1);
                self.job = None;
                self.source = source;
                self.items.clear();
                self.previews.clear();
                self.selected = 0;
                self.page = 0;
                self.has_more = false;
                self.status.clear();
                self.submitted.clear();
            }
            Message::Query(query) => {
                self.query = query;
                self.query_field.reset(&self.query);
            }
            Message::EditQuery(edit) => {
                if let Err(error) = self.query_field.update(
                    &mut self.query,
                    edit,
                    4096,
                    ClipboardTarget::new("dock", "wallpaper-search"),
                    effects,
                    Message::EditQuery,
                ) {
                    self.status = error.to_string();
                }
            }
            Message::Search => self.search(self.query.clone(), 1),
            Message::Page { revision, page } => {
                if revision == self.results_revision && self.can_page(page) {
                    self.search(self.submitted.clone(), page);
                }
            }
            Message::Results(id, result)
                if id == self.generation && matches!(self.job, Some(Job::Search(..))) =>
            {
                match result {
                    Ok(page) => {
                        let Some(Job::Search(provider, _, requested)) = &self.job else {
                            unreachable!()
                        };
                        if page.page != *requested
                            || page.items.iter().any(|item| item.provider != *provider)
                        {
                            self.status = "Wallpaper provider returned an inconsistent page".into();
                            return;
                        }
                        self.results_revision = self.results_revision.saturating_add(1);
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
                }
            }
            Message::Preview(id, key, image)
                if id == self.generation
                    && matches!(self.job, Some(Job::Search(..)))
                    && self.items.iter().any(|item| item.id == key) =>
            {
                self.previews.insert(key, image);
            }
            Message::Finished(id)
                if id == self.generation && matches!(self.job, Some(Job::Search(..))) =>
            {
                self.job = None;
                if !self.items.is_empty() {
                    self.status = format!("{} results · page {}", self.items.len(), self.page);
                }
            }
            Message::Select(choice) => {
                if let Some(index) = self.resolve(&choice) {
                    self.select(index);
                }
            }
            Message::Navigate(delta) => {
                self.select(
                    self.selected
                        .saturating_add_signed(delta as isize)
                        .min(self.items.len().saturating_sub(1)),
                );
            }
            Message::Download {
                choice,
                selection,
                colors,
            } => {
                if matches!(self.job, Some(Job::Download(..)))
                    || selection != self.selection_revision
                    || selection == u64::MAX
                    || self.resolve(&choice) != Some(self.selected)
                {
                    return;
                }
                if let Some(generation) = self.generation.checked_add(1).filter(|id| *id < u64::MAX)
                {
                    self.generation = generation;
                    self.selection_revision = self.selection_revision.saturating_add(1);
                    self.job = Some(Job::Download(choice.item, colors));
                    self.status = "Downloading original…".into();
                }
            }
            Message::Ready(id, result) if self.accepts_download(id) => {
                self.job = None;
                self.status = match result {
                    Ok(_) => "Downloaded · applying…".into(),
                    Err(error) => error.to_string(),
                };
            }
            _ => {}
        }
    }
    pub fn subscriptions(
        &self,
        catalog: &Arc<dyn WallpaperCatalogPort>,
        assets: &Arc<dyn AssetPort>,
    ) -> Vec<Subscription<Message>> {
        let Some(job) = self.job.clone() else {
            return vec![];
        };
        let id = self.generation;
        let catalog = catalog.clone();
        let assets = assets.clone();
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
                            && let Ok(image) = assets.thumbnail(&path)
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
                    let result = catalog.download(&item).map(|wall| (wall, colors));
                    if !cancel.cancelled() {
                        out.send(Message::Ready(id, result));
                    }
                }
            },
        )]
    }
    /// Static-provider choices, composed with the live-library entry by the launcher.
    pub fn source_choices(&self, theme: Theme) -> Vec<Element<Message>> {
        [
            (None, "Local"),
            (Some(WallpaperProvider::Wallhaven), "Wallhaven"),
            (Some(WallpaperProvider::AlphaCoders), "Alpha"),
        ]
        .into_iter()
        .map(|(source, label)| {
            theme
                .button(label, Message::Source(source))
                .height(Length::Fixed(t::SOURCE_HEIGHT))
                .width(Length::Fill)
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
                .clip()
                .id(format!("source-{}", source.map_or("local", |s| s.id())))
                .map({
                    let revision = self.controls_revision;
                    move |message| Message::Controls(revision, Box::new(message))
                })
        })
        .collect()
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
        let search_height = lucent_design::component::input::HEIGHT;
        let search = theme
            .input_group(vec![
                theme
                    .text_input_content(
                        &self.query,
                        self.query_field.selection(),
                        "Search wallpapers",
                        Message::EditQuery,
                    )
                    .id("wallpaper-search")
                    .autofocus(),
                theme
                    .button_enabled("Search", Message::Search, self.can_search())
                    .id("wallpaper-search-submit")
                    .height(Length::Fill)
                    .font(font::CAPTION),
            ])
            .size(width, search_height);
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
                    .wrap(t::METADATA_LINES as usize)
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
            .on_click(self.select_message(index))
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
        let download = matches!(self.download_message(false), Message::Download { .. });
        let previous = self.page.saturating_sub(1);
        let next = self.page.checked_add(1).unwrap_or(0);
        let actions = Element::row(vec![
            theme
                .button_enabled("Wallpaper only", self.download_message(false), download)
                .id("download-wallpaper")
                .size(half, control)
                .font(font::CAPTION),
            theme
                .button_enabled("Wallpaper + colors", self.download_message(true), download)
                .id("download-theme")
                .size(half, control)
                .font(font::CAPTION)
                .background(theme.primary)
                .color(theme.on_primary),
        ])
        .gap(gap);
        let pages = Element::row(vec![
            theme
                .button_enabled("‹", self.page_message(previous), self.can_page(previous))
                .id("catalog-previous")
                .size(control, control),
            Element::text(status)
                .font(font::CAPTION)
                .color(theme.on_surface)
                .wrap(t::STATUS_LINES as usize)
                .width(Length::Fill)
                .height(Length::Fixed(control)),
            theme
                .button_enabled("›", self.page_message(next), self.can_page(next))
                .id("catalog-next")
                .size(control, control),
        ])
        .gap(space::SM)
        .size(width, control);
        content.push(pages.at(0., height - footer));
        content.push(actions.at(0., height - control));
        Element::stack(content).size(width, height).clip().map({
            let revision = self.controls_revision;
            move |message| Message::Controls(revision, Box::new(message))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod lifetimes;
    #[test]
    fn shared_edits_and_clipboard_do_not_search_until_submit_and_retire_old_requests() {
        let mut browser = Browser::default();
        let mut effects = Effects::default();
        browser.update(
            Message::Source(Some(WallpaperProvider::Wallhaven)),
            &mut effects,
        );
        for text in ["ca", "fé"] {
            browser.update(
                Message::EditQuery(TextEdit::Insert(text.into())),
                &mut effects,
            );
        }
        assert_eq!(browser.query, "café");
        assert!(!browser.busy());
        browser.update(Message::EditQuery(TextEdit::SelectAll), &mut effects);
        browser.update(Message::EditQuery(TextEdit::Paste), &mut effects);
        let request = effects.clipboard.pop().unwrap();
        assert_eq!(
            request.target,
            ClipboardTarget::new("dock", "wallpaper-search")
        );
        browser.update(
            request.finish(ClipboardOutcome::Text("landscape".into())),
            &mut effects,
        );
        assert_eq!(browser.query, "landscape");
        assert!(!browser.busy());
        browser.update(Message::EditQuery(TextEdit::Paste), &mut effects);
        let request = effects.clipboard.pop().unwrap();
        browser.update(Message::Search, &mut effects);
        browser.update(
            request.finish(ClipboardOutcome::Text("late".into())),
            &mut effects,
        );
        assert_eq!(browser.query, "landscape");
        assert_eq!(browser.submitted, "landscape");
        browser.update(Message::EditQuery(TextEdit::Paste), &mut effects);
        let request = effects.clipboard.pop().unwrap();
        browser.update(
            Message::Source(Some(WallpaperProvider::AlphaCoders)),
            &mut effects,
        );
        browser.update(
            request.finish(ClipboardOutcome::Text("old provider".into())),
            &mut effects,
        );
        assert_eq!(browser.query, "landscape");
        browser.update(Message::EditQuery(TextEdit::Paste), &mut effects);
        let request = effects.clipboard.pop().unwrap();
        browser.update(
            request.finish(ClipboardOutcome::Error(ClipboardError::Timeout)),
            &mut effects,
        );
        assert_eq!(browser.query, "landscape");
        assert_eq!(browser.status, "Clipboard transfer timed out");
    }
    #[test]
    fn changing_source_discards_stale_network_responses() {
        let mut browser = Browser::default();
        browser.update(
            Message::Source(Some(WallpaperProvider::Wallhaven)),
            &mut Effects::default(),
        );
        browser.update(Message::Query("mountains".into()), &mut Effects::default());
        browser.update(Message::Search, &mut Effects::default());
        let old = browser.generation;
        browser.update(
            Message::Source(Some(WallpaperProvider::AlphaCoders)),
            &mut Effects::default(),
        );
        browser.update(Message::Search, &mut Effects::default());
        browser.update(
            Message::Results(old, Err(DomainError::Unavailable("old failure".into()))),
            &mut Effects::default(),
        );
        browser.update(Message::Finished(old), &mut Effects::default());
        assert!(browser.busy());
        assert_eq!(browser.status, "Searching…");
        assert!(browser.items.is_empty());
    }
}
