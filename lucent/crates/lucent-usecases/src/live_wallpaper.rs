//! Pure live wallpaper preference changes. Native storage and producer lifetime
//! remain in the injected adapter and the desktop coordinator.
use lucent_domain::{
    DesktopSettings, DomainError, LiveWallpaperSettings, LiveWallpaperTarget, LiveWallpapers,
    MonitorId, Result, WallpaperFps, WallpaperPauseCondition, WallpaperPauseReason,
    WallpaperPlaybackContext,
};

pub enum LiveWallpaperChange {
    Apply(LiveWallpaperTarget, LiveWallpaperSettings),
    Pause(LiveWallpaperTarget, bool),
    PauseAll(bool),
    Policy(WallpaperPauseCondition, bool),
    Off(LiveWallpaperTarget),
    Rate(LiveWallpaperTarget, WallpaperFps),
    Inherit(MonitorId),
}

/// Evaluate one output without changing saved preferences. Explicit pauses win;
/// unavailable observations never masquerade as battery or fullscreen state.
pub fn wallpaper_pause_reason(
    settings: &LiveWallpapers,
    profile: &LiveWallpaperSettings,
    target: &LiveWallpaperTarget,
    context: &WallpaperPlaybackContext,
) -> Option<WallpaperPauseReason> {
    if settings.paused {
        Some(WallpaperPauseReason::All)
    } else if profile.paused {
        Some(WallpaperPauseReason::Profile)
    } else if context.preparing_for_sleep {
        Some(WallpaperPauseReason::Sleep)
    } else if settings.policy.pause_on_battery && context.on_battery == Some(true) {
        Some(WallpaperPauseReason::Battery)
    } else if settings.policy.pause_when_fullscreen
        && target
            .monitor()
            .is_some_and(|id| context.fullscreen_monitors.contains(id))
    {
        Some(WallpaperPauseReason::Fullscreen)
    } else {
        None
    }
}

/// Reject cheap invalid demands before copying or decoding native media.
pub fn validate_live_wallpaper_import(
    settings: &DesktopSettings,
    target: &LiveWallpaperTarget,
    path: &str,
) -> Result<()> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(DomainError::Invalid(
            "Live wallpaper paths must be bounded and contain no controls".into(),
        ));
    }
    let live = &settings.live_wallpapers;
    let already_enabled = match target {
        LiveWallpaperTarget::Default => live.default.as_ref(),
        LiveWallpaperTarget::Monitor(id) => live.monitors.get(id),
    }
    .is_some_and(|profile| profile.enabled);
    let enabled = live
        .monitors
        .values()
        .filter(|profile| profile.enabled)
        .count()
        + usize::from(live.default.as_ref().is_some_and(|profile| profile.enabled));
    if enabled + usize::from(!already_enabled) > 4
        || target
            .monitor()
            .is_some_and(|id| !live.monitors.contains_key(id) && live.monitors.len() >= 32)
    {
        return Err(DomainError::Invalid(
            "At most four live wallpaper profiles and 32 monitor entries are supported".into(),
        ));
    }
    Ok(())
}

fn selected_mut<'a>(
    live: &'a mut LiveWallpapers,
    target: &LiveWallpaperTarget,
) -> Result<&'a mut LiveWallpaperSettings> {
    match target {
        LiveWallpaperTarget::Default => live.default.as_mut(),
        LiveWallpaperTarget::Monitor(id) => {
            if !live.monitors.contains_key(id)
                && let Some(default) = live.default.clone()
            {
                live.monitors.insert(id.clone(), default);
            }
            live.monitors.get_mut(id)
        }
    }
    .ok_or_else(|| DomainError::Invalid("Choose a live wallpaper for this target first".into()))
}

/// Validate a complete candidate before mutating any saved preference.
pub fn change_live_wallpaper(
    settings: &mut DesktopSettings,
    change: LiveWallpaperChange,
) -> Result<()> {
    let mut next = settings.clone();
    let live = &mut next.live_wallpapers;
    match change {
        LiveWallpaperChange::Apply(target, value) => match target {
            LiveWallpaperTarget::Default => live.default = Some(value),
            LiveWallpaperTarget::Monitor(id) => {
                live.monitors.insert(id, value);
            }
        },
        LiveWallpaperChange::Pause(target, value) => selected_mut(live, &target)?.paused = value,
        LiveWallpaperChange::PauseAll(value) => live.paused = value,
        LiveWallpaperChange::Policy(condition, value) => match condition {
            WallpaperPauseCondition::Battery => live.policy.pause_on_battery = value,
            WallpaperPauseCondition::Fullscreen => live.policy.pause_when_fullscreen = value,
        },
        LiveWallpaperChange::Off(target) => match target {
            LiveWallpaperTarget::Default => live.default = None,
            LiveWallpaperTarget::Monitor(_) => {
                if live.selected(&target).is_some() {
                    selected_mut(live, &target)?.enabled = false;
                }
            }
        },
        LiveWallpaperChange::Rate(target, value) => selected_mut(live, &target)?.fps = value,
        LiveWallpaperChange::Inherit(id) => {
            live.monitors.remove(&id);
        }
    }
    crate::validate_settings(&next)?;
    *settings = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::{LiveWallpaperAsset, Wallpaper};

    fn profile() -> LiveWallpaperSettings {
        LiveWallpaperSettings {
            asset: LiveWallpaperAsset {
                source: Wallpaper {
                    path: "/original.mp4".into(),
                    name: "Original".into(),
                },
                path: "/owned/source.mp4".into(),
                poster: Wallpaper {
                    path: "/owned/poster.png".into(),
                    name: "Poster".into(),
                },
            },
            fps: WallpaperFps::default(),
            paused: false,
            enabled: true,
        }
    }
    #[test]
    fn automatic_pause_is_per_output_and_never_clears_an_explicit_pause() {
        use WallpaperPauseReason::*;
        let id = MonitorId::new("DP-1").unwrap();
        let target = LiveWallpaperTarget::Monitor(id.clone());
        let other = LiveWallpaperTarget::Monitor(MonitorId::new("DP-2").unwrap());
        let mut settings = LiveWallpapers::default();
        let mut selected = profile();
        let mut context = WallpaperPlaybackContext::default();
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &target, &context),
            None
        );
        context.fullscreen_monitors.insert(id);
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &target, &context),
            Some(Fullscreen)
        );
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &other, &context),
            None
        );
        assert_eq!(
            wallpaper_pause_reason(
                &settings,
                &selected,
                &LiveWallpaperTarget::Default,
                &context
            ),
            None
        );
        context.on_battery = Some(true);
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &other, &context),
            Some(Battery)
        );
        context.preparing_for_sleep = true;
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &other, &context),
            Some(Sleep)
        );
        selected.paused = true;
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &other, &context),
            Some(Profile)
        );
        settings.paused = true;
        context = WallpaperPlaybackContext::default();
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &target, &context),
            Some(All)
        );
        settings.paused = false;
        assert_eq!(
            wallpaper_pause_reason(&settings, &selected, &target, &context),
            Some(Profile)
        );
    }
    #[test]
    fn policy_controls_change_only_the_requested_preference_and_allow_unavailable_observations() {
        let id = MonitorId::new("DP-1").unwrap();
        let target = LiveWallpaperTarget::Monitor(id.clone());
        let mut settings = DesktopSettings::default();
        let selected = profile();
        let mut context = WallpaperPlaybackContext {
            on_battery: Some(true),
            fullscreen_monitors: [id].into(),
            ..Default::default()
        };
        change_live_wallpaper(
            &mut settings,
            LiveWallpaperChange::Policy(WallpaperPauseCondition::Battery, false),
        )
        .unwrap();
        assert!(settings.live_wallpapers.policy.pause_when_fullscreen);
        assert_eq!(
            wallpaper_pause_reason(&settings.live_wallpapers, &selected, &target, &context),
            Some(WallpaperPauseReason::Fullscreen)
        );
        change_live_wallpaper(
            &mut settings,
            LiveWallpaperChange::Policy(WallpaperPauseCondition::Fullscreen, false),
        )
        .unwrap();
        assert_eq!(
            wallpaper_pause_reason(&settings.live_wallpapers, &selected, &target, &context),
            None
        );
        settings.live_wallpapers.policy = Default::default();
        context.fullscreen_monitors.clear();
        for power in [None, Some(false)] {
            context.on_battery = power;
            assert_eq!(
                wallpaper_pause_reason(&settings.live_wallpapers, &selected, &target, &context),
                None
            );
        }
        // Sleep is an observed lifecycle pause, independent of optional policies.
        settings.live_wallpapers.policy.pause_on_battery = false;
        settings.live_wallpapers.policy.pause_when_fullscreen = false;
        context.preparing_for_sleep = true;
        assert_eq!(
            wallpaper_pause_reason(&settings.live_wallpapers, &selected, &target, &context),
            Some(WallpaperPauseReason::Sleep)
        );
    }
    #[test]
    fn monitor_controls_copy_default_without_changing_other_targets_or_global_pause() {
        let id = MonitorId::new("DP-1").unwrap();
        let target = LiveWallpaperTarget::Monitor(id.clone());
        let mut settings = DesktopSettings {
            notes: "Keep these notes".into(),
            ..Default::default()
        };
        settings.live_wallpapers.default = Some(profile());
        settings.live_wallpapers.paused = true;
        change_live_wallpaper(
            &mut settings,
            LiveWallpaperChange::Pause(target.clone(), true),
        )
        .unwrap();
        assert!(settings.live_wallpapers.monitors[&id].paused);
        assert!(!settings.live_wallpapers.default.as_ref().unwrap().paused);
        assert!(settings.live_wallpapers.paused);
        change_live_wallpaper(
            &mut settings,
            LiveWallpaperChange::Rate(target.clone(), WallpaperFps::new(15).unwrap()),
        )
        .unwrap();
        assert_eq!(settings.live_wallpapers.monitors[&id].fps.value(), 15);
        assert_eq!(
            settings
                .live_wallpapers
                .default
                .as_ref()
                .unwrap()
                .fps
                .value(),
            30
        );
        change_live_wallpaper(&mut settings, LiveWallpaperChange::Off(target)).unwrap();
        assert!(!settings.live_wallpapers.monitors[&id].enabled);
        change_live_wallpaper(&mut settings, LiveWallpaperChange::Inherit(id.clone())).unwrap();
        assert!(!settings.live_wallpapers.monitors.contains_key(&id));
        assert_eq!(settings.notes, "Keep these notes");
    }
    #[test]
    fn invalid_changes_are_atomic_and_replacing_an_admitted_profile_still_succeeds() {
        let mut settings = DesktopSettings::default();
        settings.live_wallpapers.default = Some(profile());
        for index in 0..3 {
            settings
                .live_wallpapers
                .monitors
                .insert(MonitorId::new(format!("DP-{index}")).unwrap(), profile());
        }
        let target = LiveWallpaperTarget::Monitor(MonitorId::new("DP-extra").unwrap());
        let before = settings.clone();
        assert!(validate_live_wallpaper_import(&settings, &target, "/clip.mp4").is_err());
        assert!(
            change_live_wallpaper(&mut settings, LiveWallpaperChange::Apply(target, profile()))
                .is_err()
        );
        assert_eq!(settings, before);
        validate_live_wallpaper_import(&settings, &LiveWallpaperTarget::Default, "/clip.mp4")
            .unwrap();
        change_live_wallpaper(
            &mut settings,
            LiveWallpaperChange::Apply(LiveWallpaperTarget::Default, profile()),
        )
        .unwrap();
        assert!(
            validate_live_wallpaper_import(&settings, &LiveWallpaperTarget::Default, "bad\npath")
                .is_err()
        );
    }
    #[test]
    fn applying_one_target_preserves_pause_all_until_explicit_resume_all() {
        let mut settings = DesktopSettings::default();
        settings.live_wallpapers.paused = true;
        change_live_wallpaper(
            &mut settings,
            LiveWallpaperChange::Apply(LiveWallpaperTarget::Default, profile()),
        )
        .unwrap();
        assert!(settings.live_wallpapers.paused);
        change_live_wallpaper(&mut settings, LiveWallpaperChange::PauseAll(false)).unwrap();
        assert!(!settings.live_wallpapers.paused);
    }
}
