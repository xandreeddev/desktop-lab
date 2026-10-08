mod desktop;
mod views;
mod widgets;
use desktop::Desktop;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Desktop::new())
}
