//! Hyprland's Lua API is kept at this boundary. Requests contain literal values.
use super::*;
use crate::compositor::Hyprland;
use serde_json::Value;
use std::os::unix::fs::MetadataExt;
#[derive(Clone)]
pub(super) struct Native {
    pub hypr: Hyprland,
    socket: std::path::PathBuf,
    identity: (u64, u64),
}
impl Native {
    pub fn from_env() -> Result<Self> {
        let hypr = Hyprland::from_env()?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| unavailable("No runtime directory"))?;
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| unavailable("No compositor instance"))?;
        let socket = std::path::PathBuf::from(runtime)
            .join("hypr")
            .join(signature)
            .join(".socket.sock");
        let m = std::fs::symlink_metadata(&socket)
            .map_err(|_| unavailable("Compositor socket is unavailable"))?;
        Ok(Self {
            hypr,
            socket,
            identity: (m.dev(), m.ino()),
        })
    }
    pub fn identity(&self) -> (u64, u64) {
        self.identity
    }
    pub fn check(&self) -> Result<()> {
        let m = std::fs::symlink_metadata(&self.socket)
            .map_err(|_| unavailable("Compositor instance retired"))?;
        if (m.dev(), m.ino()) != self.identity {
            return Err(unavailable("Compositor instance changed"));
        }
        Ok(())
    }
    pub fn outputs(&self) -> Result<Vec<DisplayDevice>> {
        self.check()?;
        let json = self.hypr.request("j/monitors all")?;
        let value: Value =
            serde_json::from_str(&json).map_err(|_| failed("Invalid display observation"))?;
        decode(&value)
    }
    pub fn apply(&self, configs: &[DisplayConfiguration]) -> Result<()> {
        self.check()?;
        let actual = self.outputs()?;
        let changed = configs
            .iter()
            .filter(|c| {
                !actual
                    .iter()
                    .any(|o| matches(std::slice::from_ref(*c), std::slice::from_ref(o)))
            })
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Ok(());
        }
        // Enable first: disabling the old primary before its replacement would
        // transiently remove the last screen, even for a valid final layout.
        let rules = changed
            .iter()
            .filter(|c| c.enabled)
            .chain(changed.iter().filter(|c| !c.enabled))
            .map(|c| lua(c))
            .collect::<Vec<_>>()
            .join("; ");
        // One bounded IPC exchange for the whole layout. A 32-output draft must
        // not multiply the native request timeout by 32 during confirmation.
        let answer = self.hypr.request(&format!("eval {rules}"))?;
        if answer.trim() != "ok" {
            return Err(failed("Hyprland rejected the display configuration"));
        }
        Ok(())
    }
    pub fn validate_configuration(&self, source: &str) -> Result<()> {
        self.check()?;
        // Omarchy's supported user module is required after packaged defaults.
        // Compile the full proposed file without executing any of its rules.
        let answer = self.hypr.request(&format!(
            "eval assert(package.loaded[\"hypr.monitors\"] ~= nil); assert(load({}))",
            literal(source)
        ))?;
        if answer.trim() != "ok" {
            return Err(failed(
                "The user monitor module is not loaded or its proposed Lua configuration is invalid",
            ));
        }
        Ok(())
    }
    pub fn event_socket(&self) -> Result<std::os::unix::net::UnixStream> {
        let stream = crate::compositor::wire::connect(&self.socket.with_file_name(".socket2.sock"))
            .map_err(|_| unavailable("Display observation disconnected"))?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(250)))
            .map_err(|_| unavailable("Display event timeout is unavailable"))?;
        Ok(stream)
    }
}
fn required_u32(v: &Value, key: &str) -> Result<u32> {
    v[key]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| failed("Display dimensions are unavailable"))
}
pub(super) fn decode(value: &Value) -> Result<Vec<DisplayDevice>> {
    let array = value
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 32)
        .ok_or_else(|| unavailable("No bounded display observation is available"))?;
    let mut result = vec![];
    let mut names = std::collections::BTreeSet::new();
    for v in array {
        let output = MonitorId::new(
            v["name"]
                .as_str()
                .ok_or_else(|| failed("Display identity is unavailable"))?,
        )?;
        if !names.insert(output.clone()) {
            return Err(failed("Ambiguous display identity"));
        }
        let modes = v["availableModes"]
            .as_array()
            .into_iter()
            .flatten()
            .take(256)
            .filter_map(|v| v.as_str().and_then(parse_mode))
            .fold(Vec::new(), |mut modes, m| {
                if !modes.contains(&m) {
                    modes.push(m)
                }
                modes
            });
        let enabled = v["disabled"].as_bool() != Some(true);
        let observed_mode = (|| {
            DisplayMode::new(
                required_u32(v, "width")?,
                required_u32(v, "height")?,
                (v["refreshRate"]
                    .as_f64()
                    .filter(|f| f.is_finite() && *f >= 1.)
                    .ok_or_else(|| failed("Display refresh rate is unavailable"))?
                    * 1000.)
                    .round() as u32,
            )
        })();
        let current_mode = match observed_mode {
            Ok(m) => m,
            Err(error) => {
                if !enabled {
                    modes.first().copied().ok_or(error)?
                } else {
                    return Err(error);
                }
            }
        };
        let scale = v["scale"]
            .as_f64()
            .filter(|f| f.is_finite() && *f > 0.)
            .unwrap_or(1.);
        let position = |key: &str| {
            v[key]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .unwrap_or(0)
        };
        let transform = v["transform"]
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(0);
        if transform > 7 {
            return Err(failed("Unsupported display rotation"));
        }
        let mut modes = modes;
        if !modes.contains(&current_mode) {
            modes.insert(0, current_mode);
        }
        result.push(DisplayDevice {
            current: DisplayConfiguration {
                output,
                enabled,
                mode: current_mode,
                scale: DisplayScale::new((scale * 1000.).round() as u32)?,
                position: DisplayPosition::new(position("x"), position("y"))?,
                transform,
            },
            modes,
            description: v["description"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(256)
                .filter(|c| !c.is_control())
                .collect(),
            // The monitor JSON cannot reconstruct every HDR/VRR policy. Do
            // not turn an edited geometry into an implicit advanced-policy reset.
            editable: matches!(v["mirrorOf"].as_str(), None | Some("") | Some("none"))
                && matches!(
                    v["colorManagementPreset"].as_str(),
                    None | Some("srgb") | Some("default")
                )
                && v["vrr"].as_bool() != Some(true)
                && !v["currentFormat"].as_str().is_some_and(|format| {
                    format.contains("2101010") || format.contains("16161616")
                }),
        });
    }
    result.sort_by(|a, b| a.current.output.cmp(&b.current.output));
    Ok(result)
}
pub(super) fn parse_mode(value: &str) -> Option<DisplayMode> {
    let (dims, hz) = value.trim_end_matches("Hz").split_once('@')?;
    let (w, h) = dims.split_once('x')?;
    let hz = hz.parse::<f64>().ok()?;
    if !hz.is_finite() {
        return None;
    }
    DisplayMode::new(
        w.parse().ok()?,
        h.parse().ok()?,
        (hz * 1000.).round() as u32,
    )
    .ok()
}
fn literal(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii_control() => out.push_str(&format!("\\{:03}", u32::from(c))),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
pub(super) fn lua(c: &DisplayConfiguration) -> String {
    if !c.enabled {
        return format!(
            "hl.monitor({{ output = {}, disabled = true }})",
            literal(c.output.as_str())
        );
    }
    format!(
        "hl.monitor({{ output = {}, disabled = false, mode = \"{}x{}@{:.3}\", position = \"{}x{}\", scale = {:.3}, transform = {} }})",
        literal(c.output.as_str()),
        c.mode.width(),
        c.mode.height(),
        f64::from(c.mode.millihertz()) / 1000.,
        c.position.x(),
        c.position.y(),
        c.scale.value(),
        c.transform
    )
}
pub(super) fn matches(expected: &[DisplayConfiguration], outputs: &[DisplayDevice]) -> bool {
    expected.len() == outputs.len()
        && expected.iter().all(|e| {
            outputs.iter().any(|o| {
                let a = &o.current;
                e.output == a.output
                    && e.enabled == a.enabled
                    && (!e.enabled
                        || (e.mode.width() == a.mode.width()
                            && e.mode.height() == a.mode.height()
                            && e.mode.millihertz().abs_diff(a.mode.millihertz()) <= 20
                            && e.scale == a.scale
                            && e.position == a.position
                            && e.transform == a.transform))
            })
        })
}
