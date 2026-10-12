//! Pure client geometry for bounded native widget buffers.
//! Overlapping paint bounds share one buffer, preserving the original tree's
//! order without relying on compositor ordering within a Wayland layer.
use super::{default_position, widget_size};
use lucent_api::{Rect, Shadow};
use lucent_design::{elevation, layout};
use lucent_domain::{DesktopSettings, Placement};

#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub id: &'static str,
    pub members: Vec<&'static str>,
    pub rect: Rect,
}

pub fn position(id: &str, settings: &DesktopSettings, viewport: (f32, f32)) -> Placement {
    let value = settings
        .positions
        .get(id)
        .copied()
        .unwrap_or_else(|| default_position(id, viewport));
    let size = widget_size(id);
    Placement::new(
        value.x().clamp(0., (viewport.0 - size.0).max(0.)),
        value.y().clamp(0., (viewport.1 - size.1).max(0.)),
    )
    .expect("validated placement, output and design tokens")
}

fn crop(id: &str, settings: &DesktopSettings, viewport: (f32, f32)) -> Rect {
    let pos = position(id, settings, viewport);
    let (width, height) = widget_size(id);
    let pad = Shadow::blur_outset(elevation::BLUR);
    let floor = |value| crate::shell_layout::floor(value, layout::SHELL_STEP);
    let ceil = |value: f32| (value.max(0.) / layout::SHELL_STEP).ceil() * layout::SHELL_STEP;
    let x = floor(pos.x() - pad);
    let y = floor(pos.y().min(pos.y() + elevation::OFFSET - pad));
    let right = ceil(pos.x() + width + pad).min(viewport.0);
    let bottom =
        ceil((pos.y() + height).max(pos.y() + height + elevation::OFFSET + pad)).min(viewport.1);
    Rect::new(x, y, (right - x).max(1.), (bottom - y).max(1.))
}

pub fn visible(settings: &DesktopSettings) -> Vec<&'static str> {
    let mut ids = Vec::new();
    for id in &settings.visible_widgets {
        if let Some(entry) = super::catalog::find(id)
            && !ids.contains(&entry.id)
        {
            ids.push(entry.id);
        }
    }
    ids
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}
fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect::new(
        x,
        y,
        (a.x + a.w).max(b.x + b.w) - x,
        (a.y + a.h).max(b.y + b.h) - y,
    )
}

pub fn layout(settings: &DesktopSettings, viewport: (f32, f32)) -> Vec<Surface> {
    let order = visible(settings);
    let mut result: Vec<_> = order
        .iter()
        .map(|id| Surface {
            id,
            members: vec![id],
            rect: crop(id, settings, viewport),
        })
        .collect();
    // Merge until even the resulting buffer bounds are disjoint. This small
    // catalog needs no spatial index; member ordering remains client ordering.
    loop {
        let pair = (0..result.len()).find_map(|a| {
            ((a + 1)..result.len())
                .find(|b| overlaps(result[a].rect, result[*b].rect))
                .map(|b| (a, b))
        });
        let Some((a, b)) = pair else { break };
        let other = result.remove(b);
        result[a].rect = union(result[a].rect, other.rect);
        result[a].members.extend(other.members);
        result[a]
            .members
            .sort_by_key(|id| order.iter().position(|candidate| candidate == id));
        result[a].id = result[a].members[0];
    }
    result
}

/// During capture, keep every existing source origin/extent stable. Newly
/// shown cards get another input source; the full preview owns their paint.
pub fn captured(
    layout: &[Surface],
    settings: &DesktopSettings,
    viewport: (f32, f32),
) -> Vec<Surface> {
    let visible = visible(settings);
    let mut result: Vec<_> = layout
        .iter()
        .filter_map(|group| {
            let mut group = group.clone();
            group.members.retain(|id| visible.contains(id));
            (!group.members.is_empty()).then_some(group)
        })
        .collect();
    for id in visible {
        if !layout.iter().any(|group| group.members.contains(&id)) {
            result.push(Surface {
                id,
                members: vec![id],
                rect: crop(id, settings, viewport),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_and_transitive_members_keep_order_in_disjoint_buffers() {
        let mut settings = DesktopSettings {
            visible_widgets: vec![
                "calendar".into(),
                "weather".into(),
                "clock".into(),
                "system".into(),
            ],
            ..Default::default()
        };
        settings
            .positions
            .insert("calendar".into(), Placement::new(64., 64.).unwrap());
        settings
            .positions
            .insert("weather".into(), Placement::new(200., 200.).unwrap());
        settings
            .positions
            .insert("clock".into(), Placement::new(400., 200.).unwrap());
        settings
            .positions
            .insert("system".into(), Placement::new(1400., 800.).unwrap());
        let groups = layout(&settings, (1920., 1080.));
        let group = groups
            .iter()
            .find(|s| s.members.contains(&"calendar"))
            .unwrap();
        assert_eq!(group.members, ["calendar", "weather", "clock"]);
        for (index, a) in groups.iter().enumerate() {
            for b in &groups[index + 1..] {
                assert!(!overlaps(a.rect, b.rect));
            }
        }
    }
    #[test]
    fn captured_sources_keep_geometry_through_unrelated_visibility_changes() {
        let mut settings = DesktopSettings::default();
        let initial = layout(&settings, (1920., 1080.));
        let source = initial
            .iter()
            .find(|s| s.members.contains(&"weather"))
            .unwrap();
        settings.visible_widgets.retain(|id| id != "calendar");
        settings.visible_widgets.push("notes".into());
        let current = captured(&initial, &settings, (1920., 1080.));
        let source_after = current.iter().find(|s| s.id == source.id).unwrap();
        assert_eq!(source.rect, source_after.rect);
        assert!(current.iter().any(|s| s.members.contains(&"notes")));
        assert!(!source_after.members.contains(&"calendar"));
    }
}
