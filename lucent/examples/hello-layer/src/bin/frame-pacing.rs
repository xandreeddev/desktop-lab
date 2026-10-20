//! Refresh-driven geometry diagnostic using the ordinary public framework API.
//! No source clock, timer, image preparation or stream uploads drive this client.
use lucent_api::*;
use lucent_design::{Theme, radius};

const SURFACE: &str = "background";
// Diagnostic geometry/time policy; visual styles come from semantic tokens.
const MARKER_WIDTH: f32 = 320.;
const MARKER_HEIGHT: f32 = 192.;
const CYCLE_SECONDS: f64 = 4.;

#[derive(Clone)]
enum Message {
    Quit,
}

struct Diagnostic {
    output: OutputName,
}

fn marker(now: f64, width: f32, height: f32) -> Rect {
    let w = MARKER_WIDTH.min(width);
    let h = MARKER_HEIGHT.min(height);
    let phase = ((now / CYCLE_SECONDS).rem_euclid(1.) * std::f64::consts::TAU) as f32;
    Rect {
        x: (width - w) * (0.5 + 0.5 * phase.sin()),
        y: (height - h) * (0.5 + 0.5 * phase.cos()),
        w,
        h,
    }
}

impl Component for Diagnostic {
    type Message = Message;

    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        let rect = marker(cx.now, cx.width, cx.height);
        Element::stack(vec![
            Element::empty()
                .size(rect.w, rect.h)
                .at(rect.x, rect.y)
                .background(theme.primary)
                .radius(radius::PANEL),
        ])
        .fill()
        .background(theme.surface)
    }

    fn update(&mut self, Message::Quit: Message, effects: &mut Effects<Message>) {
        effects.quit();
    }
}

impl Application for Diagnostic {
    fn name(&self) -> &'static str {
        "lucent-frame-pacing"
    }

    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: SURFACE,
            instance: None,
            output: Some(self.output.clone()),
            layer: Layer::Background,
            anchor: Anchor::Fill,
            margins: Default::default(),
            redraw_group: None,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            keyboard: Keyboard::None,
            visible: true,
            pointer_input: PointerInput::None,
        }]
    }

    fn animating(&self, surface: &str, _: f64) -> bool {
        surface == SURFACE
    }

    fn command(&self, command: &str) -> Result<Option<Message>, String> {
        match command {
            "quit" => Ok(Some(Message::Quit)),
            _ => Err("Unknown diagnostic command".into()),
        }
    }

    fn inspect(&self) -> String {
        "{\"mode\":\"refresh-driven-geometry\",\"video\":false,\"target_fps\":null}".into()
    }
}

fn arguments(args: impl IntoIterator<Item = String>) -> Result<Option<OutputName>, &'static str> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("--help") if args.next().is_none() => Ok(None),
        Some("--output") => {
            let output = OutputName::new(args.next().ok_or("--output requires a name")?)?;
            if args.next().is_some() {
                return Err("Use frame-pacing --output NAME or --help");
            }
            Ok(Some(output))
        }
        _ => Err("Use frame-pacing --output NAME or --help"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(output) = arguments(std::env::args().skip(1))? else {
        println!(
            "frame-pacing --output NAME: passive animated background driven by compositor callbacks; no fixed FPS. Use a separately verified 1920x1080@60 scale-1 output for comparison."
        );
        return Ok(());
    };
    lucent_wayland::run(Diagnostic { output })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_require_one_literal_validated_output_before_startup() {
        let parse = |args: &[&str]| arguments(args.iter().map(|arg| (*arg).to_owned()));
        assert_eq!(
            parse(&["--output", "Virtual-1"]).unwrap().unwrap().as_str(),
            "Virtual-1"
        );
        assert!(parse(&["--help"]).unwrap().is_none());
        for args in [
            vec![],
            vec!["--output"],
            vec!["--output", ""],
            vec!["--output", "bad\nname"],
            vec!["--output", "Virtual-1", "--output", "Virtual-2"],
            vec!["--help", "--output", "Virtual-1"],
            vec!["--fps", "60"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn moving_geometry_stays_inside_each_viewport_and_repeats_without_a_source_clock() {
        for (width, height) in [(1920., 1080.), (3840., 2160.), (100., 80.)] {
            let initial = marker(0., width, height);
            let moved = marker(CYCLE_SECONDS / 4., width, height);
            if width > MARKER_WIDTH && height > MARKER_HEIGHT {
                assert_ne!((initial.x, initial.y), (moved.x, moved.y));
            }
            for step in 0..=400 {
                let now = f64::from(step) / 100.;
                let rect = marker(now, width, height);
                assert!(rect.x >= 0. && rect.y >= 0.);
                assert!(rect.x + rect.w <= width && rect.y + rect.h <= height);
                let next = marker(now + CYCLE_SECONDS, width, height);
                assert!((rect.x - next.x).abs() < 0.01);
                assert!((rect.y - next.y).abs() < 0.01);
            }
        }
    }

    #[test]
    fn native_contract_is_one_passive_named_background_with_no_producer() {
        let app = Diagnostic {
            output: OutputName::new("Virtual-1").unwrap(),
        };
        let surfaces = app.surfaces();
        assert_eq!(surfaces.len(), 1);
        let spec = &surfaces[0];
        assert!(spec.validate().is_ok());
        assert_eq!(spec.output, Some(app.output.clone()));
        assert!(matches!(spec.layer, Layer::Background));
        assert!(matches!(spec.anchor, Anchor::Fill));
        assert_eq!((spec.width, spec.height, spec.exclusive_zone), (0, 0, -1));
        assert!(matches!(spec.keyboard, Keyboard::None));
        assert!(matches!(spec.pointer_input, PointerInput::None));
        assert!(app.subscriptions().is_empty());
        assert!(app.animating(SURFACE, 0.) && app.animating(SURFACE, 600.));
        assert!(!app.animating("other", 0.));
        assert!(
            !app.activation_enabled() && !app.clipboard_enabled() && !app.input_method_enabled()
        );
        let mut app = app;
        let mut effects = Effects::default();
        app.update(app.command("quit").unwrap().unwrap(), &mut effects);
        assert!(effects.exit && effects.tasks.is_empty() && effects.redraw.is_empty());
    }
}
