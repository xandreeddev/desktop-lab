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
            "lucent-cli launcher toggle|open|close\nlucent-cli launcher size WIDTH MAX_HEIGHT | reset\n  Dimensions use logical pixels; height is a content limit.\nlucent-cli themes open\nlucent-cli theme dark|light\nlucent-cli notifications show|toggle|dismiss-last|dismiss-all|invoke-last|dnd\nlucent-cli bar show|hide\nlucent-cli widget NAME show\nlucent-cli wallpapers open\nlucent-cli widgets open\nlucent-cli inspect\nlucent-cli quit"
        );
        return Ok(());
    }
    let command = args.join(" ");
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
