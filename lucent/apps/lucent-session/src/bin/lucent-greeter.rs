fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = lucent_auth::Greetd::from_env(vec!["/usr/bin/start-hyprland".into()])?;
    lucent_wayland::run_greeter(lucent_session::SessionScreen::new(
        lucent_session::Mode::Login,
        String::new(),
        std::sync::Arc::new(backend),
    ))
}
