use lucent_domain::MenuOutcome;
use std::io::Read;
fn main() {
    if std::env::args().any(|arg| arg == "--help") {
        println!(
            "lucent-menu: read a JSON menu request from stdin; return the selected value on stdout.\nUse omarchy-menu-keybindings for the native shortcut menu."
        );
        return;
    }
    match run() {
        Ok(Some(value)) => println!("{value}"),
        Ok(None) => std::process::exit(1),
        Err(error) => {
            eprintln!("lucent-menu: {error}");
            std::process::exit(2);
        }
    }
}
fn run() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("menu request exceeds 2 MiB".into());
    }
    let request: lucent_menu::Request = serde_json::from_slice(&bytes)?;
    if request.entries.len() > 10_000 {
        return Err("menu exceeds 10,000 entries".into());
    }
    let (menu, result) = lucent_menu::Menu::new(request);
    lucent_wayland::run(menu)?;
    Ok(match result.outcome() {
        Some(MenuOutcome::Accepted(value)) => Some(value.clone()),
        _ => None,
    })
}
