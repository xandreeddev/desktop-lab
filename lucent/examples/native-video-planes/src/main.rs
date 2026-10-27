//! Explicit one-frame decoder→native-plane→Vulkan validation consumer.
//! Inert unless --run; no session, compositor, wallpaper preference or UI changes.
use image::ImageEncoder;
use lucent_domain::{
    NativeMediaFrame, NativePlaybackOptions, StopSignal, Wallpaper, WallpaperPlaybackPort,
};
use lucent_services::video::NativeVideoWallpapers;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
struct Request {
    source: PathBuf,
    state: PathBuf,
    output: PathBuf,
}
fn request(args: &[String]) -> Result<Option<Request>, Error> {
    if args.is_empty() || args == ["--help"] {
        return Ok(None);
    }
    if args.len() != 7
        || args[0] != "--run"
        || args[1] != "--source"
        || args[3] != "--state"
        || args[5] != "--output"
    {
        return Err("Expected --run --source ABSOLUTE_MEDIA --state NEW_ABSOLUTE_DIRECTORY --output NEW_ABSOLUTE_PNG".into());
    }
    let value = Request {
        source: PathBuf::from(&args[2]),
        state: PathBuf::from(&args[4]),
        output: PathBuf::from(&args[6]),
    };
    if [&value.source, &value.state, &value.output]
        .iter()
        .any(|p| !p.is_absolute())
    {
        return Err("All paths must be absolute".into());
    }
    if value.source == value.output
        || value.source.starts_with(&value.state)
        || value.output.starts_with(&value.state)
    {
        return Err("Source, new import store and output must be separate".into());
    }
    Ok(Some(value))
}
struct Deadline(Instant);
impl StopSignal for Deadline {
    fn cancelled(&self) -> bool {
        Instant::now() >= self.0
    }
    fn wait(&self, value: Duration) {
        std::thread::sleep(value.min(self.0.saturating_duration_since(Instant::now())));
    }
}
fn main() -> Result<(), Error> {
    let Some(request) = request(&std::env::args().skip(1).collect::<Vec<_>>())? else {
        println!(
            "Prepared only. To validate one frame: --run --source ABSOLUTE_MEDIA --state NEW_ABSOLUTE_DIRECTORY --output NEW_ABSOLUTE_PNG"
        );
        return Ok(());
    };
    if request.state.exists() || request.output.exists() {
        return Err("Store/output must be new; existing files are preserved".into());
    }
    let adapter = NativeVideoWallpapers {
        state: request.state,
        library: request
            .source
            .parent()
            .ok_or("Source needs a parent")?
            .into(),
    };
    let asset = adapter.prepare(&Wallpaper {
        path: request
            .source
            .to_str()
            .ok_or("Source path must be UTF-8")?
            .into(),
        name: "Native plane validation".into(),
    })?;
    let mut first = None;
    adapter.play_native(
        &asset,
        NativePlaybackOptions {
            looping: false,
            ..Default::default()
        },
        &mut |value| {
            first = Some(value);
            false
        },
        Arc::new(Deadline(Instant::now() + Duration::from_secs(15))),
    );
    // All decoder/session owners have returned before rendering. Retained plane
    // buffers must remain valid independently. This is not playback FPS evidence.
    let (width, height, pixels) = match first.ok_or("No frame before cancellation")?? {
        NativeMediaFrame::Yuv420 {
            frame, publication, ..
        } => {
            let (width, height) = frame.dimensions();
            let mut painter = lucent_render::PlanarPainter::new(lucent_render::Gpu::headless(
                Arc::new(Vec::new()),
            )?);
            let pixels = painter.snapshot(&frame, width, height)?;
            println!(
                "Native YUV4208: referenced={} copied={} publication bytes; {:?}. RGBA readback is validation-only.",
                publication.referenced_bytes,
                publication.copied_bytes,
                painter.stats()
            );
            (width, height, pixels)
        }
        NativeMediaFrame::Rgba(frame) => {
            println!(
                "Unsupported native metadata/format: existing RGBA fallback; no GPU-plane claim."
            );
            frame.into_rgba()
        }
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(request.output)?;
    image::codecs::png::PngEncoder::new(file).write_image(
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(source: &str, state: &str, output: &str) -> Vec<String> {
        [
            "--run", "--source", source, "--state", state, "--output", output,
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }
    #[test]
    fn native_validation_is_inert_by_default_and_requires_explicit_separate_absolute_paths() {
        assert!(request(&[]).unwrap().is_none());
        assert!(request(&["--help".into()]).unwrap().is_none());
        assert!(
            request(&args("/input.mp4", "/new-store", "/output.png"))
                .unwrap()
                .is_some()
        );
        for invalid in [
            args("relative.mp4", "/store", "/out.png"),
            args("/store/in.mp4", "/store", "/out.png"),
            args("/input.mp4", "/store", "/store/out.png"),
            args("/input.mp4", "/store", "/input.mp4"),
        ] {
            assert!(request(&invalid).is_err());
        }
        let mut invalid = args("/input.mp4", "/store", "/out.png");
        invalid[0] = "--prepare".into();
        assert!(request(&invalid).is_err());
    }
}
