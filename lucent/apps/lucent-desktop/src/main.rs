mod desktop;
mod notifications;
mod platform;
mod ports;
mod shell_layout;
mod views;
mod widgets;
use desktop::Desktop;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Desktop::new(platform::desktop_ports()))
}

#[cfg(test)]
mod visual_tests;

#[cfg(test)]
mod test_ports;
