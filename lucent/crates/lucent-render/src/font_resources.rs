//! Standalone, opt-in preparation probe; no GPU/compositor or client state.
//! Run this exact ignored case alone in separate before/after processes.
use crate::raster;
use std::{fs, time::Instant};

fn resident() -> (u64, u64) {
    let values = fs::read_to_string("/proc/self/smaps_rollup").expect("Linux smaps_rollup");
    let value = |name: &str| {
        values
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    (value("Rss:"), value("Pss:"))
}
#[test]
#[ignore = "isolated Linux font-resource probe; run one exact case with --nocapture"]
fn cold_and_warm_fallback_preparation_resources() {
    let case = std::env::var("LUCENT_FONT_RESOURCE_CASE").unwrap_or_else(|_| "cjk".into());
    let text = match case.as_str() {
        "latin" => "Lucent desktop ready",
        "arabic" => "السَّلَامُ عَلَيْكُمْ",
        "devanagari" => "नमस्ते दुनिया",
        "cjk" => "日本語 中文 壁紙",
        "hangul" => "한국어 데스크톱",
        _ => panic!("Use a bounded known font probe case"),
    };
    let initial = resident();
    let font = lucent_ui::text::Font::from_bytes(
        include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
        Default::default(),
    )
    .unwrap();
    let primary = resident();
    let started = Instant::now();
    let first = raster::text(&font, text, 24., 2., 8192).unwrap();
    let cold_ms = started.elapsed().as_secs_f64() * 1000.;
    let mask_bytes = first.0.len();
    let dimensions = (first.1, first.2);
    let coverage_sum: u64 = first.0.iter().map(|v| u64::from(*v)).sum();
    assert!(coverage_sum > 0);
    drop(first);
    let cold = resident();
    let started = Instant::now();
    for _ in 0..20 {
        let image = raster::text(&font, text, 24., 2., 8192).unwrap();
        assert_eq!((image.1, image.2), dimensions);
    }
    let warm_mean_ms = started.elapsed().as_secs_f64() * 1000. / 20.;
    let warm = resident();
    eprintln!(
        "{{\"case\":\"{case}\",\"density\":2,\"logical_size\":24,\"cold_ms\":{cold_ms},\"warm_mean_ms\":{warm_mean_ms},\"mask_bytes\":{mask_bytes},\"coverage_sum\":{coverage_sum},\"rss_kib_initial\":{},\"pss_kib_initial\":{},\"rss_kib_primary\":{},\"pss_kib_primary\":{},\"rss_kib_cold\":{},\"pss_kib_cold\":{},\"rss_kib_warm\":{},\"pss_kib_warm\":{}}}",
        initial.0, initial.1, primary.0, primary.1, cold.0, cold.1, warm.0, warm.1
    );
}
