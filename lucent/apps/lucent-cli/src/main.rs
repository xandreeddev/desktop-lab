use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};
fn main() {
    if let Err(error) = run() {
        eprintln!("lucent: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!(
            "lucent-cli applications open APP_ID /absolute/file-or-URI ...\nlucent-cli launcher toggle|open|close\nlucent-cli launcher size WIDTH MAX_HEIGHT | reset\n  Dimensions use logical pixels; height is a content limit.\nlucent-cli themes open\nlucent-cli wallpaper live open|pause|resume|off\nlucent-cli wallpaper live play /absolute/file.mp4\nlucent-cli wallpaper live rate source|1..60\nlucent-cli workspace select NUMBER|name:NAME\nlucent-cli workspace toggle-special NAME|default\nlucent-cli audio open|toggle|close\nlucent-cli media open|toggle|close\nlucent-cli media play-pause|next|previous\nlucent-cli media select PLAYER\nlucent-cli media play|pause|next|previous|stop|raise PLAYER\nlucent-cli media seek PLAYER TRACK MICROSECONDS\n  Player/track IDs from inspect are transient; use current observations.\nlucent-cli network open|toggle|close\nlucent-cli palette dark|light|ocean|forest|ember|wallpaper\nlucent-cli notifications show|toggle|dismiss-last|dismiss-all|invoke-last|dnd\nlucent-cli bar show|hide\nlucent-cli widget NAME show\nlucent-cli wallpapers open|retry|search PROVIDER QUERY\nlucent-cli widgets open\nlucent-cli settings retry\nlucent-cli inspect\nlucent-cli quit"
        );
        return Ok(());
    }
    let command = encode_command(&args)?;
    if command.len() > 4000 || command.contains(['\n', '\r']) {
        return Err("Invalid command".into());
    }
    let runtime = PathBuf::from(
        env::var_os("XDG_RUNTIME_DIR")
            .ok_or("XDG_RUNTIME_DIR is missing; run inside your graphical session")?,
    );
    let mut socket = UnixStream::connect(runtime.join("lucent.sock"))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    writeln!(socket, "{command}")?;
    let mut response = String::new();
    socket.take(4 * 1024 * 1024).read_to_string(&mut response)?;
    if response.starts_with("error:") {
        return Err(response.trim().to_string().into());
    }
    print!("{response}");
    Ok(())
}
fn encode_command(args: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    if args.first().is_some_and(|arg| arg == "applications")
        && args.get(1).is_some_and(|arg| arg == "open")
    {
        if args.len() < 4 {
            return Err("Use applications open APP_ID /absolute/file-or-URI ...".into());
        }
        return Ok(format!(
            "applications open {}",
            serde_json::json!({"application":args[2],"targets":args[3..]})
        ));
    }
    Ok(args.join(" "))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_open_preserves_each_literal_cli_argument() {
        let args = [
            "applications",
            "open",
            "example.desktop",
            "/tmp/a b\"$(literal)",
            "https://example.test/",
        ]
        .map(String::from);
        let encoded = encode_command(&args).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(encoded.strip_prefix("applications open ").unwrap()).unwrap();
        assert_eq!(value["targets"][0], "/tmp/a b\"$(literal)");
        assert_eq!(value["targets"].as_array().unwrap().len(), 2);
    }
}
