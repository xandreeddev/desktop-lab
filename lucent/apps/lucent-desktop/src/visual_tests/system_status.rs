use super::*;
use lucent_domain::{DomainError, Placement, SystemSnapshot};

pub(super) fn prepare(app: &mut Desktop, name: &str) {
    app.settings.layout.visible_widgets = vec!["system".into()];
    app.settings
        .layout
        .positions
        .insert("system".into(), Placement::new(24., 24.).unwrap());
    let reading = SystemSnapshot {
        cpu_percent: if name == "system-warming" {
            None
        } else {
            Some(35.)
        },
        memory_used_mib: 4096,
        memory_total_mib: 16384,
    };
    let mut effects = Effects::default();
    if !matches!(name, "system-loading" | "system-unavailable") {
        app.update(Message::System(Ok(reading.clone())), &mut effects);
    }
    if matches!(
        name,
        "system-unavailable" | "system-stale" | "system-recovered"
    ) {
        app.update(
            Message::System(Err(DomainError::Unavailable(
                "Controlled system observation failure".into(),
            ))),
            &mut effects,
        );
    }
    if name == "system-recovered" {
        app.update(Message::System(Ok(reading)), &mut effects);
    }
}

#[test]
fn system_status_and_readings_fit_the_existing_widget_at_narrow_and_regular_outputs() {
    for light in [false, true] {
        for width in [320., 640.] {
            for (name, heading, cpu, memory) in [
                (
                    "system-loading",
                    "System · Loading",
                    "CPU   —",
                    "Memory   —",
                ),
                (
                    "system-unavailable",
                    "System · Unavailable",
                    "CPU   —",
                    "Memory   —",
                ),
                (
                    "system-warming",
                    "System",
                    "CPU   Collecting…",
                    "Memory   4096 / 16384 MiB",
                ),
                (
                    "system-ready",
                    "System",
                    "CPU   35%",
                    "Memory   4096 / 16384 MiB",
                ),
                (
                    "system-stale",
                    "System · Last reading",
                    "CPU   35%",
                    "Memory   4096 / 16384 MiB",
                ),
                (
                    "system-recovered",
                    "System",
                    "CPU   35%",
                    "Memory   4096 / 16384 MiB",
                ),
            ] {
                let mut app = fixture(0, light);
                resize_fixture(&mut app, width, 240.);
                prepare(&mut app, name);
                let fonts = fonts(&app);
                let layout = Layout::new(fonts.clone());
                let cx = ViewContext {
                    surface: "widgets",
                    instance: None,
                    width,
                    height: 240.,
                    now: 1.,
                    density: Density::ONE,
                };
                let scene = layout.build(&app.view(&cx), width, 240., &Interaction::default(), 1.);
                for label in [heading, cpu, memory] {
                    assert!(
                        scene.paint.iter().any(|paint| {
                            matches!(paint, Paint::Text { text, .. } if text == label)
                        }),
                        "{name}: expected complete label {label}"
                    );
                }
                for paint in &scene.paint {
                    if let Paint::Text {
                        text,
                        rect,
                        clip,
                        size,
                        face,
                        layout,
                        ..
                    } = paint
                    {
                        assert!(fully_visible(*rect, clip.bounds), "{name}: clipped {text}");
                        assert!(
                            lucent_ui::text::width_with_layout(
                                &fonts[*face],
                                text,
                                *size,
                                layout.clone()
                            ) <= rect.w + 0.1,
                            "{name}: text exceeds its available width: {text}"
                        );
                        assert!(!text.contains("NaN") && !text.contains("inf"));
                    }
                }
            }
        }
    }
}
