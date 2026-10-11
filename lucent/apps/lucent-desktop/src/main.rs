mod actions;
mod adapters;
mod artwork;
mod desktop;
mod launcher;
mod notifications;
mod osd;
mod palette_intent;
mod persistence;
mod platform;
mod ports;
mod previews;
mod shell_layout;
mod views;
mod wallpaper_browser;
mod wallpaper_selection;
mod widgets;
mod workspaces;
use desktop::Desktop;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Desktop::new(platform::desktop_adapters()))
}

#[cfg(test)]
mod visual_tests;

#[cfg(test)]
mod test_adapters;
