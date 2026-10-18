//! Pure desktop geometry. Views and animation targets share these calculations.
//! Grid policy belongs to this client/design system, never to the renderer.
use crate::{actions::COMMANDS, launcher::Mode, widgets::catalog::WIDGETS};
use lucent_api::{Rect, Shadow};
use lucent_design::{
    component::{bar, dock, dock_row, input, launcher, panel, pill},
    elevation, layout, space,
};
use lucent_domain::LauncherSize;

pub fn floor(value: f32, step: f32) -> f32 {
    (value.max(0.) / step).floor() * step
}
fn ceil(value: f32, step: f32) -> f32 {
    (value.max(0.) / step).ceil() * step
}
pub fn bottom(viewport: f32) -> f32 {
    floor(viewport - layout::SHELL_INSET, layout::SHELL_STEP)
}
pub fn panel_origin(viewport: (f32, f32), size: (f32, f32)) -> (f32, f32) {
    // Sizes settle on two shell cells horizontally, so both edges align while
    // sharing one stable center throughout a morph. Do not quantize animation.
    (
        floor(viewport.0 / 2., layout::SHELL_STEP) - size.0 / 2.,
        bottom(viewport.1) - size.1,
    )
}
/// The settled dock needs only its shape, shadow and existing bottom inset.
/// Floor the upper crop edge on the same display grid; retain the actual
/// output height's remainder rather than moving the dock onto another grid.
pub fn dock_surface_height(viewport_height: f32) -> u32 {
    let top = bottom(viewport_height) - panel::DOCK_HEIGHT;
    let shadow_top = top + elevation::OFFSET - Shadow::blur_outset(elevation::BLUR);
    (viewport_height - floor(top.min(shadow_top), layout::SHELL_STEP))
        .ceil()
        .max(1.) as u32
}
/// Surface-local position within a bottom-anchored crop. Display placement
/// remains authoritative; native buffer size must not change the output grid.
pub fn dock_origin(viewport: (f32, f32), surface: (f32, f32), size: (f32, f32)) -> (f32, f32) {
    let (x, y) = panel_origin((surface.0, viewport.1), size);
    (x, y - (viewport.1 - surface.1))
}
pub fn audio_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::audio;
    control_panel(viewport, (audio::WIDTH, audio::HEIGHT))
}
pub fn media_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::media_panel;
    control_panel(viewport, (media_panel::WIDTH, media_panel::HEIGHT))
}
pub fn network_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::network;
    control_panel(viewport, (network::WIDTH, network::HEIGHT))
}
pub fn bluetooth_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::bluetooth;
    control_panel(viewport, (bluetooth::WIDTH, bluetooth::HEIGHT))
}
pub fn power_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::power;
    control_panel(viewport, (power::WIDTH, power::HEIGHT))
}
pub fn tray_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::tray;
    control_panel(viewport, (tray::WIDTH, tray::HEIGHT))
}
pub fn live_wallpaper_panel(viewport: (f32, f32)) -> Rect {
    use lucent_design::component::live_wallpaper as live;
    let width = floor(
        live::PANEL_WIDTH.min((viewport.0 - 2. * layout::SHELL_INSET).max(0.)),
        layout::SHELL_STEP,
    );
    let height = floor(
        live::PANEL_HEIGHT.min((viewport.1 - panel::BAR_HEIGHT - 2. * layout::SHELL_INSET).max(0.)),
        layout::SHELL_STEP,
    );
    Rect::new(
        floor((viewport.0 - width) / 2., layout::SHELL_STEP),
        floor(
            panel::BAR_HEIGHT + (viewport.1 - panel::BAR_HEIGHT - height) / 2.,
            layout::SHELL_STEP,
        ),
        width,
        height,
    )
}
fn control_panel(viewport: (f32, f32), size: (f32, f32)) -> Rect {
    let width = floor(
        size.0.min(viewport.0 - layout::SHELL_INSET * 2.),
        layout::SHELL_STEP,
    );
    let top = panel::BAR_HEIGHT + layout::SHELL_INSET;
    let height = floor(
        size.1.min(viewport.1 - top - layout::SHELL_INSET),
        layout::SHELL_STEP,
    );
    Rect::new(
        floor(viewport.0 - layout::SHELL_INSET - width, layout::SHELL_STEP),
        top,
        width,
        height,
    )
}
pub fn dock_capacity(viewport: f32) -> usize {
    let available = floor(viewport - layout::SHELL_INSET * 2., layout::SHELL_STEP * 2.);
    ((available - dock::CONTENT_INSET * 2. + space::SM) / (dock_row::ITEM_SIZE + space::SM))
        .floor()
        .max(1.) as usize
}
pub fn dock_content_width(count: usize) -> f32 {
    count as f32 * dock_row::ITEM_SIZE + count.saturating_sub(1) as f32 * space::SM
}
pub fn dock_width(count: usize) -> f32 {
    ceil(
        dock_content_width(count) + dock::CONTENT_INSET * 2.,
        layout::SHELL_STEP * 2.,
    )
}
pub fn body_top() -> f32 {
    launcher::HEADER_HEIGHT + layout::SECTION_GAP
}
pub fn body_height(mode: Mode, content_height: f32) -> f32 {
    (content_height
        - body_top()
        - match mode {
            Mode::Apps => input::HEIGHT + layout::SECTION_GAP,
            Mode::Widgets => launcher::RESET_HEIGHT + layout::SECTION_GAP,
            _ => 0.,
        })
    .max(0.)
}
pub const LAUNCHER_PRESETS: [(&str, LauncherSize); 3] = [
    (
        "Small",
        LauncherSize {
            width: Some(launcher::SMALL_WIDTH as u32),
            max_height: Some(launcher::SMALL_MAX_HEIGHT as u32),
        },
    ),
    (
        "Default",
        LauncherSize {
            width: None,
            max_height: None,
        },
    ),
    (
        "Large",
        LauncherSize {
            width: Some(launcher::LARGE_WIDTH as u32),
            max_height: Some(launcher::LARGE_MAX_HEIGHT as u32),
        },
    ),
];

pub fn launcher_size(
    mode: Mode,
    results: usize,
    viewport: (f32, f32),
    preference: LauncherSize,
) -> (f32, f32) {
    let default_width = if mode == Mode::Wallpapers {
        panel::WALLPAPER_WIDTH
    } else {
        panel::LAUNCHER_WIDTH
    };
    let width = floor(
        preference
            .width
            .map_or(default_width, |w| w as f32)
            .max(launcher::MIN_WIDTH),
        layout::SHELL_STEP * 2.,
    );
    let width = width.min(floor(
        viewport.0 - layout::SHELL_INSET * 2.,
        layout::SHELL_STEP * 2.,
    ));
    let body = match mode {
        Mode::Apps => {
            results.max(1) as f32 * launcher::ROW_HEIGHT + layout::SECTION_GAP + input::HEIGHT
        }
        Mode::Commands => COMMANDS.len() as f32 * launcher::ROW_HEIGHT,
        Mode::Widgets => {
            WIDGETS.len() as f32 * launcher::ROW_HEIGHT
                + layout::SECTION_GAP
                + launcher::RESET_HEIGHT
        }
        Mode::Themes => {
            lucent_design::component::wallpaper_browser::PALETTE_ROW * 6.
                + layout::SECTION_GAP * 2.
                + launcher::THEME_CAPTION_HEIGHT
                + launcher::SIZE_BUTTON_HEIGHT
        }
        Mode::Wallpapers => {
            panel::WALLPAPER_BODY_HEIGHT
                + lucent_design::component::wallpaper_browser::SOURCE_HEIGHT
                + layout::SECTION_GAP
        }
    };
    let height = ceil(
        (body_top() + body + dock::CONTENT_INSET * 2.).max(panel::LAUNCHER_MIN_HEIGHT),
        layout::SHELL_STEP,
    );
    let max_height = preference
        .max_height
        .map_or(launcher::MAX_HEIGHT, |h| h as f32)
        .max(panel::LAUNCHER_MIN_HEIGHT);
    let available = (bottom(viewport.1) - panel::BAR_HEIGHT - layout::SECTION_GAP).min(max_height);
    (width, height.min(floor(available, layout::SHELL_STEP)))
}

pub fn palette_rows(content_height: f32) -> usize {
    ((body_height(Mode::Themes, content_height)
        - launcher::THEME_CAPTION_HEIGHT
        - launcher::SIZE_BUTTON_HEIGHT
        - layout::SECTION_GAP * 2.)
        / lucent_design::component::wallpaper_browser::PALETTE_ROW)
        .floor()
        .max(1.) as usize
}

pub fn visible_rows(
    mode: Mode,
    results: usize,
    viewport: (f32, f32),
    preference: LauncherSize,
) -> usize {
    let (_, height) = launcher_size(mode, results, viewport, preference);
    (body_height(mode, height - dock::CONTENT_INSET * 2.) / launcher::ROW_HEIGHT)
        .floor()
        .max(1.) as usize
}

#[derive(Debug)]
pub struct BarLayout {
    pub workspaces: Rect,
    pub workspace_count: usize,
    pub media: Option<Rect>,
    pub clock: Option<Rect>,
    pub system: Rect,
    pub show_mute: bool,
    pub show_tray: bool,
    pub tray_count: usize,
}
impl BarLayout {
    pub fn new(width: f32, workspace_count: usize) -> Self {
        let gap = layout::SECTION_GAP;
        let rect = |x, w| Rect::new(x, bar::TOP, w, pill::HEIGHT);
        let system_size = |count: f32| {
            pill::PADDING_INLINE * 2. + bar::CONTROL_SIZE * count + space::SM * (count - 1.)
        };
        let minimum_workspace = pill::PADDING_INLINE * 2. + bar::WORKSPACE_SIZE;
        let show_mute = width
            >= layout::SHELL_INSET * 2.
                + minimum_workspace
                + gap
                + system_size(bar::SYSTEM_CONTROL_COUNT);
        let without_mute = bar::SYSTEM_CONTROL_COUNT - if show_mute { 0. } else { 1. };
        let show_tray =
            width >= layout::SHELL_INSET * 2. + minimum_workspace + gap + system_size(without_mute);
        let system_width = system_size(without_mute - if show_tray { 0. } else { 1. });
        let system = rect(
            floor(
                width - layout::SHELL_INSET - system_width,
                layout::SHELL_STEP,
            ),
            system_width,
        );
        let capacity = ((system.x - gap - bar::LEFT - pill::PADDING_INLINE * 2. + space::SM)
            / (bar::WORKSPACE_SIZE + space::SM))
            .floor()
            .max(0.) as usize;
        let workspace_count = workspace_count
            .min(bar::MAX_WORKSPACES as usize)
            .min(capacity);
        let workspaces = rect(
            bar::LEFT,
            if workspace_count == 0 {
                0.
            } else {
                pill::PADDING_INLINE * 2.
                    + workspace_count as f32 * bar::WORKSPACE_SIZE
                    + workspace_count.saturating_sub(1) as f32 * space::SM
            },
        );
        let clock_width = pill::PADDING_INLINE * 2.
            + bar::DATE_WIDTH
            + bar::TIME_WIDTH
            + bar::CONTROL_SIZE * 2.
            + space::SM * 3.;
        let center = rect(
            floor((width - clock_width) / 2., layout::SHELL_STEP),
            clock_width,
        );
        let clock = (workspaces.x + workspaces.w + gap <= center.x
            && center.x + center.w + gap <= system.x)
            .then_some(center);
        let media_width = pill::PADDING_INLINE * 2.
            + bar::CONTROL_SIZE * 2.
            + bar::MEDIA_TEXT_WIDTH
            + space::SM * 2.;
        let media_rect = rect(workspaces.x + workspaces.w + gap, media_width);
        let end = clock.map_or(system.x, |c| c.x);
        let media = (media_rect.x + media_rect.w + gap <= end).then_some(media_rect);
        Self {
            workspaces,
            workspace_count,
            media,
            clock,
            system,
            show_mute,
            show_tray,
            tray_count: 0,
        }
    }

    /// Fill spare space without displacing the clock, media or workspaces.
    /// The overflow control remains available even when all icons fit.
    pub fn with_tray(mut self, items: usize) -> Self {
        if !self.show_tray {
            return self;
        }
        let neighbor = self.clock.or(self.media).unwrap_or(self.workspaces);
        let end = neighbor.x + neighbor.w + layout::SECTION_GAP;
        let step = bar::CONTROL_SIZE + space::SM;
        let capacity = ((self.system.x - end) / step).floor().max(0.) as usize;
        self.tray_count = items.min(bar::MAX_TRAY_ITEMS as usize).min(capacity);
        let added = step * self.tray_count as f32;
        self.system.x -= added;
        self.system.w += added;
        self
    }
}
