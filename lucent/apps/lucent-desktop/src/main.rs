mod actions;
mod adapters;
mod artwork;
mod audio;
mod background_presentation;
mod bar;
mod bluetooth;
mod catalog;
mod desktop;
mod displays;
mod dock;
mod launcher;
mod live_wallpaper;
mod media;
mod monitors;
mod network;
mod notifications;
mod osd;
mod palette_intent;
mod persistence;
mod platform;
mod ports;
mod power;
mod previews;
mod shell_layout;
mod tray;
mod views;
mod wallpaper_browser;
mod wallpaper_carousel;
mod wallpaper_selection;
mod weather;
mod widgets;
mod work;
mod workspaces;
use desktop::Desktop;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--display-guard") {
        return lucent_services::displays::run_guard().map_err(Into::into);
    }
    lucent_wayland::run(Desktop::new(platform::desktop_adapters()))
}

#[cfg(test)]
mod visual_tests;

#[cfg(test)]
mod test_adapters;
