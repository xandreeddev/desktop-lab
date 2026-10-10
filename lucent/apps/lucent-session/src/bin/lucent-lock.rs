fn main() -> Result<(), Box<dyn std::error::Error>> {
    use lucent_domain::SettingsPort;
    // No credential in arguments, environment, scene trees or IPC.
    let user = lucent_auth::current_identity()?;
    lucent_wayland::run_locked(
        lucent_session::SessionScreen::new(
            lucent_session::Mode::Lock,
            user,
            std::sync::Arc::new(lucent_auth::PamLocker),
        )
        .with_theme(
            lucent_services::JsonSettings::default()
                .load()
                .map(|s| s.light)
                .unwrap_or(false),
        )
        .with_assets(std::sync::Arc::new(
            lucent_session::platform::WallpaperFile::current(),
        )),
    )
}
