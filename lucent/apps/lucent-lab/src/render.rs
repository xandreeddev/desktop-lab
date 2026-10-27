//! Export composition/IO lives outside the story views. One GPU, sequential scenes.
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use lucent_api::Density;
use lucent_design::palettes;
use lucent_render::{Canvas, Gpu};
use lucent_ui::{Interaction, Layout, Scene, text::Font};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{FONT_BYTES, Lab, Message, Viewport, stories::STORIES};

pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository containing app")
}
fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err("Cannot verify export source".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub fn sha256(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn sources(root: &Path) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    fn visit(
        root: &Path,
        relative: &Path,
        result: &mut BTreeMap<String, String>,
    ) -> Result<(), Box<dyn Error>> {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err("Producer source symlinks are unsupported".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                visit(root, &relative.join(entry?.file_name()), result)?;
            }
        } else if matches!(
            path.extension().and_then(|part| part.to_str()),
            Some("rs" | "wgsl" | "ttf" | "otf" | "json")
        ) {
            result.insert(
                relative.to_str().ok_or("Non-UTF8 producer source")?.into(),
                sha256(&path)?,
            );
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    for source in [
        "lucent/apps/lucent-lab/src",
        "lucent/apps/lucent-menu/src",
        "lucent/crates/lucent-api/src",
        "lucent/crates/lucent-design/src",
        "lucent/crates/lucent-ui/src",
        "lucent/crates/lucent-render/src",
        "lucent/crates/lucent-domain/src",
        "lucent/crates/lucent-usecases/src",
        "lucent/crates/lucent-video-frame/src",
        "lucent/crates/lucent-ui/assets",
    ] {
        visit(root, Path::new(source), &mut result)?;
    }
    for source in [
        "design/tokens.json",
        "configs/lucent/palettes.json",
        "lucent/Cargo.toml",
        "lucent/Cargo.lock",
        "lucent/apps/lucent-lab/Cargo.toml",
        "lucent/apps/lucent-menu/Cargo.toml",
        "lucent/crates/lucent-api/Cargo.toml",
        "lucent/crates/lucent-design/Cargo.toml",
        "lucent/crates/lucent-ui/Cargo.toml",
        "lucent/crates/lucent-render/Cargo.toml",
        "lucent/crates/lucent-domain/Cargo.toml",
        "lucent/crates/lucent-usecases/Cargo.toml",
        "lucent/crates/lucent-video-frame/Cargo.toml",
        "lucent/apps/lucent-desktop/assets/LucentSans.ttf",
        "lucent/apps/lucent-desktop/assets/LucentDisplay.ttf",
    ] {
        result.insert(source.into(), sha256(&root.join(source))?);
    }
    Ok(result)
}

fn committed_inputs(root: &Path, inputs: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let paths = inputs.keys().map(String::as_str).collect::<Vec<_>>();
    let mut tracked = vec!["ls-files", "--error-unmatch", "--"];
    tracked.extend(&paths);
    git(root, &tracked)
        .map_err(|_| "Commit every producing input before exporting the catalogue")?;
    let mut unchanged = vec!["diff", "--quiet", "HEAD", "--"];
    unchanged.extend(&paths);
    git(root, &unchanged)
        .map_err(|_| "Producing inputs differ from the committed export source")?;
    Ok(())
}

/// Export a new directory; never overwrite an older catalogue or its evidence.
pub fn export(output: &Path, expected_source: Option<&str>) -> Result<(), Box<dyn Error>> {
    let root = repository();
    let source = git(&root, &["rev-parse", "HEAD"])?;
    if source.len() != 40
        || !source.bytes().all(|byte| byte.is_ascii_hexdigit())
        || expected_source.is_some_and(|expected| expected != source)
    {
        return Err("Export source must match the full current Git commit".into());
    }
    let inputs = sources(&root)?;
    committed_inputs(&root, &inputs)?;
    fs::create_dir(output)?;
    fs::create_dir(output.join("images"))?;
    let fonts: Arc<Vec<Font>> = Arc::new(
        FONT_BYTES
            .iter()
            .map(|bytes| {
                Font::from_bytes(*bytes, Default::default()).expect("bundled production font")
            })
            .collect(),
    );
    let layout = Layout::new(fonts.clone());
    let mut canvas = Canvas::new(Gpu::headless(fonts)?);
    let mut stories = Vec::new();
    for story in STORIES {
        let mut variants = Vec::new();
        for palette in palettes() {
            for state in story.states {
                for viewport in Viewport::ALL {
                    for scale in [1, 2] {
                        let lab = Lab::new(story.id, &palette.id, Some(state), viewport)?;
                        let time_ms = story.time_ms(state);
                        let now = time_ms as f64 / 1000.;
                        let cx = lab.preview_context(now, Density::integer(scale)?);
                        let tree = lab.story_view(&cx);
                        let scene =
                            layout.build(&tree, cx.width, cx.height, &Interaction::default(), now);
                        let mut input = preset_interaction(&lab, &scene);
                        input.synchronize(&scene);
                        let scene = layout.build(&tree, cx.width, cx.height, &input, now);
                        let width = cx.width as u32;
                        let height = cx.height as u32;
                        let mut pixels = canvas.snapshot(width, height, scale, &scene.paint)?;
                        unpremultiply(&mut pixels);
                        let id = format!(
                            "{}-{}-{}-{}-{}x",
                            story.id,
                            palette.id,
                            state,
                            viewport.id(),
                            scale
                        );
                        let path = format!("images/{id}.png");
                        image::save_buffer_with_format(
                            output.join(&path),
                            &pixels,
                            width * scale,
                            height * scale,
                            image::ColorType::Rgba8,
                            image::ImageFormat::Png,
                        )?;
                        variants.push(json!({"id":id,"palette":palette.id,"state":state,"density":{"numerator":scale,"denominator":1},"viewport":{"id":viewport.id(),"width":width,"height":height},"image":{"path":path,"width":width*scale,"height":height*scale,"sha256":sha256(&output.join(&path))?},"time_ms":time_ms}));
                    }
                }
            }
        }
        stories.push(json!({"id":story.id,"family":story.family,"title":story.title,"description":story.description,"source":{"path":story.source.path,"symbol":story.source.symbol},"tokens":story.tokens,"recipe":story.recipe,"variants":variants}));
    }
    if sources(&root)? != inputs || git(&root, &["rev-parse", "HEAD"])? != source {
        return Err("Producing source changed during export; catalogue is incomplete".into());
    }
    committed_inputs(&root, &inputs)?;
    let manifest: Value = json!({"schema_version":1,"renderer":{"engine":"lucent-render","source_commit":source,"token_sha256":inputs["design/tokens.json"],"scope":"offscreen-native","inputs":inputs},"palettes":palettes().iter().map(|palette| json!({"id":palette.id,"name":palette.name})).collect::<Vec<_>>(),"stories":stories});
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

/// These are real engine state presets for snapshots, not copied hover colors.
/// The native viewer always keeps pointer state under runtime authority.
pub fn preset_interaction(lab: &Lab, scene: &Scene<Message>) -> Interaction<Message> {
    let mut input = Interaction::default();
    if lab.story().id == "buttons" {
        match lab.state_id() {
            "hover" => {
                if let Some(hit) = scene.hits.iter().find(|hit| hit.id == "sample-primary") {
                    input.motion(
                        scene,
                        hit.rect.x + hit.rect.w / 2.,
                        hit.rect.y + hit.rect.h / 2.,
                        0.,
                    );
                }
            }
            "focus" => {
                input.focus = Some("sample-primary".into());
                input.focus_visible = true;
            }
            _ => {}
        }
    }
    input
}

fn unpremultiply(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for value in &mut pixel[..3] {
                *value = ((u32::from(*value) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_design::component::lab as token;
    #[test]
    fn png_pixels_preserve_transparency_without_dark_premultiplied_edges() {
        let mut pixels = [32, 64, 0, 128, 9, 8, 7, 255, 0, 0, 0, 0];
        unpremultiply(&mut pixels);
        assert_eq!(pixels, [64, 128, 0, 128, 9, 8, 7, 255, 0, 0, 0, 0]);
    }
    #[test]
    fn all_declared_variants_have_unique_bounded_geometry_and_times() {
        let mut count = 0;
        for story in STORIES {
            for state in story.states {
                assert!(story.time_ms(state) <= 1000);
                for viewport in Viewport::ALL {
                    assert!(viewport.width() <= token::STAGE_WIDTH);
                    count += palettes().len() * 2;
                }
            }
        }
        assert_eq!(count, 480);
    }

    #[test]
    fn export_fingerprints_neutral_video_values_and_their_dependency_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let inputs = sources(&root).unwrap();
        for path in [
            "lucent/crates/lucent-video-frame/src/lib.rs",
            "lucent/crates/lucent-video-frame/Cargo.toml",
        ] {
            assert_eq!(
                inputs.get(path),
                Some(&sha256(&root.join(path)).unwrap()),
                "indirect public video values must invalidate captures"
            );
        }
    }

    #[test]
    fn export_requires_committed_producers_but_allows_unrelated_docs_work() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lucent-lab-source-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        git(&root, &["init", "--quiet"]).unwrap();
        fs::write(root.join("producer.rs"), "original").unwrap();
        git(&root, &["add", "producer.rs"]).unwrap();
        git(
            &root,
            &[
                "-c",
                "user.name=Lab fixture",
                "-c",
                "user.email=lab@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        )
        .unwrap();
        let mut inputs = BTreeMap::from([("producer.rs".into(), "fixture hash".into())]);
        fs::write(root.join("docs.md"), "work in progress").unwrap();
        assert!(committed_inputs(&root, &inputs).is_ok());
        fs::write(root.join("producer.rs"), "changed").unwrap();
        assert!(committed_inputs(&root, &inputs).is_err());
        git(&root, &["add", "producer.rs"]).unwrap();
        assert!(committed_inputs(&root, &inputs).is_err());
        git(&root, &["reset", "--hard", "--quiet", "HEAD"]).unwrap();
        fs::write(root.join("new.rs"), "untracked producer").unwrap();
        inputs.insert("new.rs".into(), "fixture hash".into());
        assert!(committed_inputs(&root, &inputs).is_err());
    }
}
