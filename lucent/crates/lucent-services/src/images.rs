use lucent_api::ImageData;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Source texture budgets for the desktop's current components at up to 2× output scale.
pub const ICON_PIXELS: u32 = 128;
pub const SYMBOL_PIXELS: u32 = 256;
pub const PREVIEW_PIXELS: u32 = 1024;

pub fn load(path: &Path, size: u32) -> Option<Arc<ImageData>> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 32 * 1024 * 1024 {
        return None;
    }
    let (width, height, rgba) = if path.extension().is_some_and(|s| s == "svg") {
        let options = resvg::usvg::Options {
            resources_dir: path.parent().map(Path::to_path_buf),
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_data(&bytes, &options).ok()?;
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        let width = (tree.size().width() * scale).ceil() as u32;
        let height = (tree.size().height() * scale).ceil() as u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let mut pixels = pixmap.take();
        for px in pixels.as_chunks_mut::<4>().0.iter_mut() {
            if px[3] > 0 {
                for c in 0..3 {
                    px[c] = (u32::from(px[c]) * 255 / u32::from(px[3])).min(255) as u8;
                }
            }
        }
        (width, height, pixels)
    } else {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode().ok()?;
        // Do not invent detail by enlarging small raster originals. Filter premultiplied
        // pixels so transparent borders cannot introduce dark/colored fringes.
        let image = if decoded.width() > size || decoded.height() > size {
            let mut rgba = decoded.to_rgba8();
            for p in rgba.pixels_mut() {
                for c in 0..3 {
                    p[c] = (u16::from(p[c]) * u16::from(p[3]) / 255) as u8;
                }
            }
            let mut resized = image::DynamicImage::ImageRgba8(rgba)
                .resize(size, size, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            for p in resized.pixels_mut() {
                for c in 0..3 {
                    p[c] = if p[3] == 0 {
                        0
                    } else {
                        (u32::from(p[c]) * 255 / u32::from(p[3])).min(255) as u8
                    };
                }
            }
            resized
        } else {
            decoded.to_rgba8()
        };
        (image.width(), image.height(), image.into_raw())
    };
    Some(Arc::new(ImageData {
        key: format!("{}:{size}", path.display()),
        width,
        height,
        rgba,
    }))
}
pub fn icon(name: &str) -> Option<Arc<ImageData>> {
    if Path::new(name).is_absolute() {
        return load(Path::new(name), ICON_PIXELS);
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let roots = [
        home.join(".local/share/icons"),
        home.join(".icons"),
        PathBuf::from("/usr/share/icons"),
    ];
    for root in roots {
        for theme in ["Papirus-Dark", "Papirus", "Adwaita", "hicolor"] {
            for size in [
                "scalable", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "symbolic",
            ] {
                for category in ["apps", "mimetypes", "places", "status"] {
                    for ext in ["png", "svg"] {
                        let path = root
                            .join(theme)
                            .join(size)
                            .join(category)
                            .join(format!("{name}.{ext}"));
                        if path.is_file()
                            && let Some(image) = load(&path, ICON_PIXELS)
                        {
                            return Some(image);
                        }
                    }
                }
            }
        }
    }
    for ext in ["png", "svg"] {
        if let Some(image) = load(
            &PathBuf::from("/usr/share/pixmaps").join(format!("{name}.{ext}")),
            ICON_PIXELS,
        ) {
            return Some(image);
        }
    }
    None
}
/// Official Material Symbols Rounded; pinned SVG sources and Apache-2.0 license
/// are vendored in assets/material. Monochrome tint is supplied by the UI theme.
pub fn symbol(name: &str, color: &str) -> Arc<ImageData> {
    let source = match name {
        "apps" => include_str!("../assets/material/apps.svg"),
        "search" => include_str!("../assets/material/search.svg"),
        "wallpaper" => include_str!("../assets/material/wallpaper.svg"),
        "widgets" => include_str!("../assets/material/widgets.svg"),
        "palette" => include_str!("../assets/material/palette.svg"),
        "power" => include_str!("../assets/material/power.svg"),
        "close" => include_str!("../assets/material/close.svg"),
        "left" => include_str!("../assets/material/left.svg"),
        "right" => include_str!("../assets/material/right.svg"),
        "music" => include_str!("../assets/material/music.svg"),
        "play" => include_str!("../assets/material/play.svg"),
        "pause" => include_str!("../assets/material/pause.svg"),
        "next" => include_str!("../assets/material/next.svg"),
        "previous" => include_str!("../assets/material/previous.svg"),
        "volume" => include_str!("../assets/material/volume.svg"),
        "network" => include_str!("../assets/material/network.svg"),
        "sun" => include_str!("../assets/material/sun.svg"),
        "cloud" => include_str!("../assets/material/cloud.svg"),
        "lock" => include_str!("../assets/material/lock.svg"),
        "command" => include_str!("../assets/material/command.svg"),
        _ => include_str!("../assets/material/apps.svg"),
    };
    let svg = source.replace("<path ", &format!("<path fill='{color}' "));
    let tree = resvg::usvg::Tree::from_str(&svg, &Default::default()).unwrap();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(SYMBOL_PIXELS, SYMBOL_PIXELS).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            SYMBOL_PIXELS as f32 / tree.size().width(),
            SYMBOL_PIXELS as f32 / tree.size().height(),
        ),
        &mut pixmap.as_mut(),
    );
    let mut rgba = pixmap.take();
    for p in rgba.as_chunks_mut::<4>().0.iter_mut() {
        if p[3] > 0 {
            for c in 0..3 {
                p[c] = (u32::from(p[c]) * 255 / u32::from(p[3])).min(255) as u8;
            }
        }
    }
    Arc::new(ImageData {
        key: format!("material-rounded:{name}:{color}"),
        width: SYMBOL_PIXELS,
        height: SYMBOL_PIXELS,
        rgba,
    })
}
