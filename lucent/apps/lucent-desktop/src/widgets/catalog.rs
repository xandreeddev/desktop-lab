//! One client catalog supplies selector metadata, token geometry and compiled views.
use super::Message;
use super::view::{self, View};
use lucent_api::{Element, ViewContext, WidgetDescriptor, WidgetRegistry};
use lucent_design::component::{widget_layout as layout, widget_size as size};

pub struct Definition {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub size: (f32, f32),
    /// Horizontal default measured from the right edge when true.
    pub from_right: bool,
    pub position: (f32, f32),
    render: for<'a> fn(&View<'a>, &ViewContext) -> Element<Message>,
}

pub const WIDGETS: [Definition; 7] = [
    Definition {
        id: "calendar",
        title: "Calendar",
        description: "Your month at a glance",
        size: (size::calendar::WIDTH, size::calendar::HEIGHT),
        from_right: false,
        position: (layout::LEFT, layout::TOP),
        render: view::calendar,
    },
    Definition {
        id: "clock",
        title: "Clock",
        description: "Large, stacked time",
        size: (size::clock::WIDTH, size::clock::HEIGHT),
        from_right: true,
        position: (layout::RIGHT_INSET, layout::CLOCK_TOP),
        render: view::clock,
    },
    Definition {
        id: "weather",
        title: "Weather",
        description: "Current local conditions",
        size: (size::weather::WIDTH, size::weather::HEIGHT),
        from_right: false,
        position: (layout::WEATHER_LEFT, layout::WEATHER_TOP),
        render: view::weather,
    },
    Definition {
        id: "media",
        title: "Media",
        description: "Now playing and controls",
        size: (size::media::WIDTH, size::media::HEIGHT),
        from_right: true,
        position: (layout::RIGHT_INSET, layout::MEDIA_TOP),
        render: view::media,
    },
    Definition {
        id: "system",
        title: "System",
        description: "Live CPU and memory",
        size: (size::system::WIDTH, size::system::HEIGHT),
        from_right: false,
        position: (layout::LEFT, layout::SYSTEM_TOP),
        render: view::system,
    },
    Definition {
        id: "notes",
        title: "Notes",
        description: "A thought to keep nearby",
        size: (size::notes::WIDTH, size::notes::HEIGHT),
        from_right: false,
        position: (layout::SECOND_COLUMN, layout::TOP),
        render: view::notes,
    },
    Definition {
        id: "timer",
        title: "Focus",
        description: "A 25-minute focus timer",
        size: (size::timer::WIDTH, size::timer::HEIGHT),
        from_right: false,
        position: (layout::SECOND_COLUMN, layout::TIMER_TOP),
        render: view::timer,
    },
];
pub fn find(id: &str) -> Option<&'static Definition> {
    WIDGETS.iter().find(|entry| entry.id == id)
}

/// The framework registry borrows this frame's explicit view context. The small
/// table is rebuilt for that borrow; it never owns snapshots, adapters or artwork.
pub(super) fn registry<'a>() -> WidgetRegistry<View<'a>, Message> {
    let mut registry = WidgetRegistry::default();
    for entry in &WIDGETS {
        registry
            .register(
                WidgetDescriptor {
                    id: entry.id,
                    title: entry.title,
                    size: entry.size,
                },
                entry.render,
            )
            .expect("unique built-in widget ID");
    }
    registry
}
