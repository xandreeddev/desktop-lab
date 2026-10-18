//! Client policy: nine bounded ordered lanes, matching the runtime contract.
//! Short pairing answers and media admission share controls. Native media method
//! waiting stays in its observer, independent of the audio volume lane.
pub const ASSETS: &str = "assets";
pub const APPEARANCE: &str = "appearance";
pub const SETTINGS: &str = "settings";
pub const AUDIO: &str = "audio";
pub const NETWORK: &str = "network";
pub const BLUETOOTH: &str = "bluetooth";
pub const CONTROLS: &str = "controls";
pub const SESSION: &str = "session";
pub const TRAY: &str = "tray";
