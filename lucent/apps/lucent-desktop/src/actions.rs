//! Desktop action vocabulary shared by the bar, widgets and launcher.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Lock,
    VolumeUp,
    VolumeDown,
    Mute,
    Terminal,
    LiveWallpapers,
    DockAutoHide,
    Quit,
}
pub const COMMANDS: [(&str, Action, &str); 8] = [
    ("New terminal", Action::Terminal, "command"),
    ("Lock screen", Action::Lock, "lock"),
    ("Volume up", Action::VolumeUp, "volume"),
    ("Volume down", Action::VolumeDown, "volume"),
    ("Mute / unmute", Action::Mute, "volume"),
    ("Close Lucent", Action::Quit, "power"),
    ("Toggle dock auto-hide", Action::DockAutoHide, "apps"),
    ("Live wallpapers", Action::LiveWallpapers, "wallpaper"),
];
