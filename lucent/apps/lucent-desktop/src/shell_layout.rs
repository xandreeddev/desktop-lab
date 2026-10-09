//! Pure desktop geometry. Views and animation targets share these calculations.
//! Grid policy belongs to this client/design system, never to the renderer.
use crate::desktop::{COMMANDS, Mode, WIDGETS};
use lucent_api::Rect;
use lucent_design::{
    component::{bar, dock, dock_row, input, launcher, panel, pill},
    layout, space,
};

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
pub fn launcher_size(mode: Mode, results: usize, viewport: (f32, f32)) -> (f32, f32) {
    let width = if mode == Mode::Wallpapers {
        panel::WALLPAPER_WIDTH
    } else {
        panel::LAUNCHER_WIDTH
    };
    let width = width.min(floor(
        viewport.0 - layout::SHELL_INSET * 2.,
        layout::SHELL_STEP * 2.,
    ));
    let body = match mode {
        Mode::Apps => {
            results.min(launcher::VISIBLE_ROWS as usize).max(1) as f32 * launcher::ROW_HEIGHT
                + layout::SECTION_GAP
                + input::HEIGHT
        }
        Mode::Commands => COMMANDS.len() as f32 * launcher::ROW_HEIGHT,
        Mode::Widgets => {
            WIDGETS.len() as f32 * launcher::ROW_HEIGHT
                + layout::SECTION_GAP
                + launcher::RESET_HEIGHT
        }
        Mode::Themes => {
            launcher::THEME_HEIGHT + layout::SECTION_GAP + launcher::THEME_CAPTION_HEIGHT
        }
        Mode::Wallpapers => panel::WALLPAPER_BODY_HEIGHT,
    };
    let height = ceil(
        (body_top() + body + dock::CONTENT_INSET * 2.).max(panel::LAUNCHER_MIN_HEIGHT),
        layout::SHELL_STEP,
    );
    let available = bottom(viewport.1) - panel::BAR_HEIGHT - layout::SECTION_GAP;
    (width, height.min(floor(available, layout::SHELL_STEP)))
}
pub fn visible_rows(mode: Mode, results: usize, viewport: (f32, f32)) -> usize {
    let (_, height) = launcher_size(mode, results, viewport);
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
}
impl BarLayout {
    pub fn new(width: f32, workspace_count: usize) -> Self {
        let gap = layout::SECTION_GAP;
        let rect = |x, w| Rect::new(x, bar::TOP, w, pill::HEIGHT);
        let system_width = pill::PADDING_INLINE * 2. + bar::CONTROL_SIZE * 5. + space::SM * 4.;
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
        }
    }
}
