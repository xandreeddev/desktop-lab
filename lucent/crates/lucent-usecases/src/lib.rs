//! Desktop application policy, expressed against domain ports.
use lucent_domain::*;

/// Stable fuzzy ranking: names outrank descriptions; no filesystem work on a keystroke.
pub fn search_applications(apps: &[Application], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut matches: Vec<_> = apps.iter().enumerate().filter_map(|(i, app)| {
        let name = app.name.to_lowercase();
        let score = if query.is_empty() { 1000 }
        else if name == query { 0 }
        else if name.starts_with(&query) { 10 + name.len() }
        else if name.contains(&query) { 100 + name.len() }
        else if app.keywords.iter().any(|k| k.to_lowercase().contains(&query))
            || app.description.to_lowercase().contains(&query) { 500 + name.len() }
        else {
            let mut chars = name.chars();
            if query.chars().all(|wanted| chars.any(|c| c == wanted)) { 800 + name.len() }
            else { return None; }
        };
        Some((i, score, name))
    }).collect();
    matches.sort_by(|a,b| (a.1, &a.2).cmp(&(b.1, &b.2)));
    matches.into_iter().map(|m| m.0).collect()
}

/// Dock activation focuses a matching window; launcher activation always launches.
pub fn activate_application(apps: &dyn ApplicationPort, compositor: &dyn CompositorPort,
                            app: &Application, prefer_running: bool) -> Result<()> {
    if prefer_running && let Ok(snapshot) = compositor.snapshot()
        && let Some(window) = snapshot.windows.iter().find(|w| {
            let id = app.id.0.trim_end_matches(".desktop");
            w.app_class.eq_ignore_ascii_case(&app.startup_class)
                || w.app_class.eq_ignore_ascii_case(id)
        }) {
        return compositor.focus_window(&window.address);
    }
    apps.launch(app)
}

/// Reject invalid coordinates, clamp to the usable output, and retain stable widget IDs.
pub fn move_widget(settings: &mut DesktopSettings, id: &str, placement: Placement,
                   size: (f32,f32), viewport: (f32,f32)) -> Result<()> {
    if !placement.x.is_finite() || !placement.y.is_finite() {
        return Err(DomainError::Invalid("Widget coordinates must be finite".into()));
    }
    settings.positions.insert(id.into(), Placement {
        x: placement.x.clamp(0.0, (viewport.0-size.0).max(0.0)),
        y: placement.y.clamp(0.0, (viewport.1-size.1).max(0.0)),
    });
    Ok(())
}
pub fn toggle_widget(settings: &mut DesktopSettings, id: &str) {
    if settings.visible_widgets.iter().any(|w| w == id) {
        settings.visible_widgets.retain(|w| w != id);
    } else { settings.visible_widgets.push(id.into()); }
}
pub fn validate_settings(settings: &DesktopSettings) -> Result<()> {
    if settings.version != 1 { return Err(DomainError::Invalid("Unsupported settings version".into())); }
    if settings.positions.values().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(DomainError::Invalid("Non-finite widget position".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn app(name: &str) -> Application {
        Application { id: AppId(format!("{name}.desktop")), name: name.into(), description: String::new(),
            keywords: vec![], icon: String::new(), startup_class: name.into(),
            command: LaunchCommand { program: name.into(), args: vec![], directory: None, terminal: false } }
    }
    #[test] fn search_ranks_exact_and_handles_unicode() {
        let apps = vec![app("Foot Server"), app("Foot"), app("Firefox"), app("Éditeur")];
        assert_eq!(search_applications(&apps,"foot"), vec![1,0]);
        assert_eq!(search_applications(&apps,"frfx"), vec![2]);
        assert_eq!(search_applications(&apps,"Édi"), vec![3]);
        assert!(search_applications(&apps,"missing").is_empty());
    }
    struct Fake { calls: Mutex<Vec<String>> }
    impl ApplicationPort for Fake {
        fn discover(&self) -> Result<Vec<Application>> { Ok(vec![app("foot")]) }
        fn launch(&self, _: &Application) -> Result<()> { self.calls.lock().unwrap().push("launch".into()); Ok(()) }
    }
    impl CompositorPort for Fake {
        fn snapshot(&self) -> Result<CompositorSnapshot> { Ok(CompositorSnapshot { workspaces: vec![], windows: vec![Window {
            address: "0x1".into(), app_class: "foot".into(), title: String::new(), workspace: 1, focused: false }] }) }
        fn switch_workspace(&self, _: i32) -> Result<()> { Ok(()) }
        fn focus_window(&self, _: &str) -> Result<()> { self.calls.lock().unwrap().push("focus".into()); Ok(()) }
    }
    #[test] fn dock_focuses_but_launcher_starts_a_new_application() {
        let fake = Fake { calls: Mutex::new(vec![]) };
        activate_application(&fake,&fake,&app("foot"),true).unwrap();
        activate_application(&fake,&fake,&app("foot"),false).unwrap();
        assert_eq!(*fake.calls.lock().unwrap(),vec!["focus","launch"]);
    }
    #[test] fn timer_catches_up_without_counting_callback_ticks() {
        let mut timer=FocusTimer::default(); timer.toggle(100);
        timer.tick(110); assert_eq!(timer.remaining,1490);
        timer.toggle(120); timer.tick(500); assert_eq!(timer.remaining,1480);
        timer.toggle(500); timer.tick(2000); assert_eq!(timer.phase,TimerPhase::Finished);
    }
    #[test] fn leap_year_and_monday_calendar_alignment() {
        assert_eq!(Date::days_in_month(2000,2),29);
        assert_eq!(Date::days_in_month(2100,2),28);
        assert_eq!(Date {year:2026,month:10,day:8,weekday:3}.first_weekday(),3);
    }
    #[test] fn moves_are_validated_and_clamped() {
        let mut s=DesktopSettings::default();
        move_widget(&mut s,"clock",Placement{x:5000.,y:-5.},(240.,240.),(1920.,1080.)).unwrap();
        assert_eq!(s.positions["clock"],Placement{x:1680.,y:0.});
        assert!(move_widget(&mut s,"clock",Placement{x:f32::NAN,y:0.},(1.,1.),(2.,2.)).is_err());
    }
}
