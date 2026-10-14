//! Desktop application policy, expressed against domain ports.
pub mod audio;
pub mod bluetooth;
pub mod dock;
pub mod menu;
pub mod notifications;
pub mod power;
pub mod tray;
pub mod widgets;
use lucent_domain::*;

/// Stable fuzzy ranking: names outrank descriptions; no filesystem work on a keystroke.
pub fn search_applications(apps: &[Application], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut matches: Vec<_> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, app)| {
            let name = app.name.to_lowercase();
            let score = if query.is_empty() {
                1000
            } else if name == query {
                0
            } else if name.starts_with(&query) {
                10 + name.len()
            } else if name.contains(&query) {
                100 + name.len()
            } else if app
                .keywords
                .iter()
                .any(|k| k.to_lowercase().contains(&query))
                || app.description.to_lowercase().contains(&query)
            {
                500 + name.len()
            } else {
                let mut chars = name.chars();
                if query.chars().all(|wanted| chars.any(|c| c == wanted)) {
                    800 + name.len()
                } else {
                    return None;
                }
            };
            Some((i, score, name))
        })
        .collect();
    matches.sort_by(|a, b| (a.1, &a.2).cmp(&(b.1, &b.2)));
    matches.into_iter().map(|m| m.0).collect()
}

/// Dock activation focuses a matching window; launcher activation always launches.
pub fn activate_application(
    apps: &dyn ApplicationPort,
    compositor: &dyn CompositorPort,
    app: &Application,
    prefer_running: bool,
) -> Result<()> {
    if prefer_running
        && let Ok(snapshot) = compositor.snapshot()
        && let Some(window) = snapshot
            .windows
            .iter()
            .find(|w| app.matches_window_class(&w.app_class))
    {
        return compositor.focus_window(&window.address);
    }
    apps.launch(app)
}

/// Align a released widget to a client-supplied logical spacing grid.
/// Screen-edge clamping remains the responsibility of `move_widget`.
pub fn snap_placement(placement: Placement, step: f32) -> Result<Placement> {
    if !step.is_finite() || step <= 0. {
        return Err(DomainError::Invalid(
            "A finite positive grid step is required".into(),
        ));
    }
    // Wider intermediates avoid overflow for a large coordinate / small grid.
    // The resulting f32 value must still satisfy Placement's finite invariant.
    let snap = |value: f32| ((f64::from(value) / f64::from(step)).round() * f64::from(step)) as f32;
    Placement::new(snap(placement.x()), snap(placement.y()))
}

/// Compute a bounded preview without mutating persisted preferences. Clients
/// supply dimensions; no grid, compositor or rendering policy lives here.
pub fn constrain_placement(
    placement: Placement,
    size: (f32, f32),
    viewport: (f32, f32),
) -> Result<Placement> {
    if [size.0, size.1, viewport.0, viewport.1]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.)
    {
        return Err(DomainError::Invalid(
            "Widget and output dimensions must be finite and non-negative".into(),
        ));
    }
    Placement::new(
        placement.x().clamp(0., (viewport.0 - size.0).max(0.)),
        placement.y().clamp(0., (viewport.1 - size.1).max(0.)),
    )
}

/// Commit a bounded position only after validation succeeds.
pub fn move_widget(
    settings: &mut WidgetLayout,
    id: &str,
    placement: Placement,
    size: (f32, f32),
    viewport: (f32, f32),
) -> Result<()> {
    let position = constrain_placement(placement, size, viewport)?;
    settings.positions.insert(id.into(), position);
    Ok(())
}
pub fn toggle_widget(settings: &mut WidgetLayout, id: &str) {
    if settings.visible_widgets.iter().any(|w| w == id) {
        settings.visible_widgets.retain(|w| w != id);
    } else {
        settings.visible_widgets.push(id.into());
    }
}
/// Initialize one persisted profile, copying only the default layout on its first
/// mutation. Transport output objects and client host state stay out.
pub fn ensure_monitor_layout(settings: &mut DesktopSettings, id: &MonitorId) -> Result<()> {
    if !settings.monitor_layouts.contains_key(id) && settings.monitor_layouts.len() >= 64 {
        return Err(DomainError::Invalid(
            "At most 64 saved monitor layouts".into(),
        ));
    }
    settings
        .monitor_layouts
        .entry(id.clone())
        .or_insert_with(|| settings.layout.clone());
    Ok(())
}
pub fn validate_settings(settings: &DesktopSettings) -> Result<()> {
    if settings.monitor_layouts.len() > 64 {
        return Err(DomainError::Invalid(
            "At most 64 saved monitor layouts".into(),
        ));
    }
    if let Some(palette) = &settings.palette {
        palette.validate()?;
    }
    if settings.saved_palettes.len() > 24 {
        return Err(DomainError::Invalid(
            "At most 24 saved wallpaper palettes".into(),
        ));
    }
    for palette in &settings.saved_palettes {
        palette.validate()?;
    }

    if settings.version != 1 {
        return Err(DomainError::Invalid("Unsupported settings version".into()));
    }
    if settings.launcher.width == Some(0) || settings.launcher.max_height == Some(0) {
        return Err(DomainError::Invalid(
            "Launcher dimensions must be positive".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn monitor_profiles_copy_only_layout_and_preserve_other_profiles() {
        let mut settings = DesktopSettings {
            notes: "Shared".into(),
            ..Default::default()
        };
        settings
            .layout
            .positions
            .insert("clock".into(), Placement::new(-12., 800.).unwrap());
        let first = MonitorId::new("DP-1").unwrap();
        let second = MonitorId::new("DP-2").unwrap();
        ensure_monitor_layout(&mut settings, &first).unwrap();
        ensure_monitor_layout(&mut settings, &second).unwrap();
        let defaults = settings.layout.clone();
        settings
            .monitor_layouts
            .get_mut(&first)
            .unwrap()
            .visible_widgets
            .clear();
        ensure_monitor_layout(&mut settings, &first).unwrap();
        assert!(settings.monitor_layouts[&first].visible_widgets.is_empty());
        assert_eq!(settings.monitor_layouts[&second], defaults);
        assert_eq!(settings.layout, defaults);
        assert_eq!(settings.notes, "Shared");
    }
    #[test]
    fn profile_limit_rejects_new_identity_without_erasing_disconnected_layouts() {
        let mut settings = DesktopSettings::default();
        for index in 0..64 {
            ensure_monitor_layout(
                &mut settings,
                &MonitorId::new(format!("display-{index}")).unwrap(),
            )
            .unwrap();
        }
        let before = settings.clone();
        assert!(
            ensure_monitor_layout(&mut settings, &MonitorId::new("overflow").unwrap()).is_err()
        );
        assert_eq!(settings, before);
        ensure_monitor_layout(&mut settings, &MonitorId::new("display-0").unwrap()).unwrap();
        assert!(validate_settings(&settings).is_ok());
        settings
            .monitor_layouts
            .insert(MonitorId::new("overflow").unwrap(), WidgetLayout::default());
        assert!(validate_settings(&settings).is_err());
    }
    fn app(name: &str) -> Application {
        Application {
            id: AppId::new(format!("{name}.desktop")).unwrap(),
            name: name.into(),
            description: String::new(),
            keywords: vec![],
            icon: String::new(),
            startup_class: name.into(),
            command: LaunchCommand {
                program: name.into(),
                args: vec![],
                directory: None,
                terminal: false,
            },
        }
    }
    #[test]
    fn search_ranks_exact_and_handles_unicode() {
        let apps = vec![
            app("Foot Server"),
            app("Foot"),
            app("Firefox"),
            app("Éditeur"),
        ];
        assert_eq!(search_applications(&apps, "foot"), vec![1, 0]);
        assert_eq!(search_applications(&apps, "frfx"), vec![2]);
        assert_eq!(search_applications(&apps, "Édi"), vec![3]);
        assert!(search_applications(&apps, "missing").is_empty());
    }
    struct Fake {
        calls: Mutex<Vec<String>>,
    }
    impl ApplicationPort for Fake {
        fn watch(&self, emit: &mut dyn FnMut(Result<Vec<Application>>), _: &dyn StopSignal) {
            emit(self.discover());
        }
        fn discover(&self) -> Result<Vec<Application>> {
            Ok(vec![app("foot")])
        }
        fn launch(&self, _: &Application) -> Result<()> {
            self.calls.lock().unwrap().push("launch".into());
            Ok(())
        }
    }
    impl CompositorPort for Fake {
        fn watch(&self, emit: &mut dyn FnMut(Result<CompositorSnapshot>), stop: &dyn StopSignal) {
            if !stop.cancelled() {
                emit(self.snapshot());
            }
        }
        fn snapshot(&self) -> Result<CompositorSnapshot> {
            Ok(CompositorSnapshot {
                monitors: vec![],
                workspaces: vec![],
                windows: vec![Window {
                    address: "0x1".into(),
                    app_class: "foot".into(),
                    title: String::new(),
                    workspace: Some(WorkspaceRef::numbered(1).unwrap()),
                    focused: false,
                }],
            })
        }
        fn workspace(&self, _: WorkspaceCommand) -> Result<()> {
            Ok(())
        }
        fn focus_window(&self, _: &str) -> Result<()> {
            self.calls.lock().unwrap().push("focus".into());
            Ok(())
        }
    }
    #[test]
    fn dock_focuses_but_launcher_starts_a_new_application() {
        let fake = Fake {
            calls: Mutex::new(vec![]),
        };
        activate_application(&fake, &fake, &app("foot"), true).unwrap();
        activate_application(&fake, &fake, &app("foot"), false).unwrap();
        assert_eq!(*fake.calls.lock().unwrap(), vec!["focus", "launch"]);
    }
    #[test]
    fn window_identity_never_matches_missing_hints_or_strips_multiple_suffixes() {
        let mut candidate = app("editor.desktop");
        candidate.startup_class.clear();
        assert!(!candidate.matches_window_class(""));
        assert!(!candidate.matches_window_class("editor"));
        assert!(candidate.matches_window_class("EDITOR.desktop"));
        candidate.startup_class = "CustomEditor".into();
        assert!(candidate.matches_window_class("customeditor"));
    }
    #[test]
    fn timer_catches_up_without_counting_callback_ticks() {
        let mut timer = FocusTimer::default();
        timer.toggle(100);
        timer.tick(110);
        assert_eq!(timer.remaining, 1490);
        timer.toggle(120);
        timer.tick(500);
        assert_eq!(timer.remaining, 1480);
        timer.toggle(500);
        timer.tick(2000);
        assert_eq!(timer.phase, TimerPhase::Finished);
    }
    #[test]
    fn leap_year_and_monday_calendar_alignment() {
        assert_eq!(Date::days_in_month(2000, 2), 29);
        assert_eq!(Date::days_in_month(2100, 2), 28);
        assert_eq!(
            Date::new(2026, 10, 8)
                .expect("valid calendar date")
                .first_weekday(),
            3
        );
    }
    #[test]
    fn moves_are_validated_and_clamped() {
        let mut s = DesktopSettings::default();
        move_widget(
            &mut s.layout,
            "clock",
            Placement::new(5000., -5.).unwrap(),
            (240., 240.),
            (1920., 1080.),
        )
        .unwrap();
        assert_eq!(
            s.layout.positions["clock"],
            Placement::new(1680., 0.).unwrap()
        );
        assert!(
            move_widget(
                &mut s.layout,
                "clock",
                Placement::new(1., 1.).unwrap(),
                (f32::NAN, 1.),
                (2., 2.)
            )
            .is_err()
        );
    }
    #[test]
    fn invalid_output_bounds_do_not_mutate_layout_and_extreme_snaps_stay_finite() {
        let point = Placement::new(10., 20.).unwrap();
        let mut settings = DesktopSettings::default();
        settings.layout.positions.insert("clock".into(), point);
        for (size, viewport) in [
            ((f32::NAN, 1.), (2., 2.)),
            ((1., 1.), (f32::INFINITY, 2.)),
            ((-1., 1.), (2., 2.)),
            ((1., 1.), (2., -1.)),
        ] {
            let before = settings.clone();
            assert!(move_widget(&mut settings.layout, "clock", point, size, viewport).is_err());
            assert_eq!(settings, before);
        }
        let extreme = Placement::new(f32::MAX, -f32::MAX).unwrap();
        assert_eq!(snap_placement(extreme, f32::from_bits(1)).unwrap(), extreme);
        assert!(snap_placement(extreme, f32::MAX * 0.65).is_err());
        for step in [0., -1., f32::NAN, f32::INFINITY] {
            assert!(snap_placement(point, step).is_err());
        }
    }
}
