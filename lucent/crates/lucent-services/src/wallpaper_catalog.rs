//! HTTP/parser/cache work is isolated behind the domain catalog port.
use lucent_domain::*;
use std::{path::Path, time::Duration};
pub struct OnlineWallpapers;
impl OnlineWallpapers {
    fn call(&self, args: &[&str]) -> Result<String> {
        let home = std::env::var("HOME")
            .map_err(|_| DomainError::Unavailable("Missing home directory".into()))?;
        let script = format!("{home}/.local/lib/lucent/catalog.py");
        let mut argv = vec![script.as_str()];
        argv.extend_from_slice(args);
        let output = crate::command_with_timeout("python3", &argv, Duration::from_secs(30))?;
        let envelope: serde_json::Value = serde_json::from_str(&output)
            .map_err(|_| DomainError::Failed("Invalid provider response".into()))?;
        if let Some(error) = envelope.get("error").and_then(|v| v.as_str()) {
            return Err(DomainError::Unavailable(error.into()));
        }
        Ok(output)
    }
}
impl WallpaperCatalogPort for OnlineWallpapers {
    fn search(&self, provider: WallpaperProvider, query: &str, page: u32) -> Result<WallpaperPage> {
        serde_json::from_str(&self.call(&["search", provider.id(), query, &page.to_string()])?)
            .map_err(|_| DomainError::Failed("Invalid provider response".into()))
    }
    fn preview(&self, item: &RemoteWallpaper) -> Result<String> {
        let result: serde_json::Value =
            serde_json::from_str(&self.call(&["preview", &serde_json::to_string(item).unwrap()])?)
                .map_err(|_| DomainError::Failed("Invalid preview response".into()))?;
        result["path"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| DomainError::Failed("Missing preview".into()))
    }
    fn download(&self, item: &RemoteWallpaper) -> Result<Wallpaper> {
        let result: serde_json::Value =
            serde_json::from_str(&self.call(&["download", &serde_json::to_string(item).unwrap()])?)
                .map_err(|_| DomainError::Failed("Invalid download response".into()))?;
        let path = result["path"]
            .as_str()
            .ok_or_else(|| DomainError::Failed("Missing download".into()))?;
        // Validate decoding before anything can replace the current wallpaper.
        crate::images::try_load(Path::new(path), 64)
            .map_err(|error| DomainError::Invalid(format!("Downloaded wallpaper: {error}")))?;
        Ok(Wallpaper {
            path: path.into(),
            name: item.title.clone(),
        })
    }
}
pub struct ImagePalette;
impl PaletteGenerationPort for ImagePalette {
    fn seed(&self, path: &str) -> Result<Rgb> {
        let image = crate::images::try_load(Path::new(path), 128)
            .map_err(|error| DomainError::Invalid(format!("Wallpaper colors: {error}")))?;
        Ok(dominant_seed(&image.rgba))
    }
}
fn dominant_seed(rgba: &[u8]) -> Rgb {
    // Quantized histogram, weighted toward chromatic pixels. No wallpaper-size
    // dependent allocation or random sampling; identical input gives identical colors.
    let mut buckets = std::collections::BTreeMap::<[u8; 3], (u64, [u64; 3], u64)>::new();
    for pixel in rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 127) {
        let rgb = [pixel[0], pixel[1], pixel[2]];
        let chroma = u64::from(rgb.iter().max().unwrap() - rgb.iter().min().unwrap());
        let entry = buckets.entry(rgb.map(|c| c / 32)).or_default();
        entry.0 += 1 + chroma;
        entry.2 += 1;
        for (sum, value) in entry.1.iter_mut().zip(rgb) {
            *sum += u64::from(value);
        }
    }
    buckets
        .values()
        .max_by_key(|v| v.0)
        .map(|(_, sum, n)| Rgb::new((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8))
        .unwrap_or(Rgb::new(128, 128, 128))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_is_deterministic_and_ignores_transparent_pixels() {
        let pixels = [120, 40, 160, 255, 122, 42, 162, 255, 255, 0, 0, 0];
        assert_eq!(dominant_seed(&pixels), Rgb::new(121, 41, 161));
        assert_eq!(dominant_seed(&[]), Rgb::new(128, 128, 128));
    }
}
