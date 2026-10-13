//! Current tray identities and application-owned actions, without transport/UI types.
use crate::{DomainError, Result, StopSignal};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TrayItemId(u64);
impl TrayItemId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid("Tray identity must be nonzero".into()));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
/// Validated, unpremultiplied RGBA bytes. This is image content, not a GPU resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayPixmap {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}
impl TrayPixmap {
    pub const MAX_EDGE: u32 = 256;
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        if width == 0
            || height == 0
            || width > Self::MAX_EDGE
            || height > Self::MAX_EDGE
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err(DomainError::Invalid(
                "Invalid tray image dimensions or bytes".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            rgba: rgba.into(),
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayIcon {
    pub name: String,
    pub pixmap: Option<TrayPixmap>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    Passive,
    Active,
    NeedsAttention,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub id: TrayItemId,
    pub application: String,
    pub title: String,
    pub tooltip: String,
    pub status: TrayStatus,
    pub icon: TrayIcon,
    pub attention_icon: TrayIcon,
    pub overlay_icon: TrayIcon,
    pub item_is_menu: bool,
    pub has_menu: bool,
    /// Changes after an observed menu signal; old client menus must be discarded.
    pub menu_generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCheck {
    None,
    Check(bool),
    Radio(bool),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub separator: bool,
    pub check: TrayCheck,
    pub submenu: bool,
    pub children: Vec<TrayMenuEntry>,
}
impl TrayMenuEntry {
    pub fn is_submenu(&self) -> bool {
        self.submenu || !self.children.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenu {
    pub item: TrayItemId,
    pub revision: u32,
    pub root: TrayMenuEntry,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraySnapshot {
    pub items: Vec<TrayItem>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Activate { x: i32, y: i32 },
    SecondaryActivate { x: i32, y: i32 },
    ContextMenu { x: i32, y: i32 },
    Scroll { delta: i32, horizontal: bool },
}
pub trait TrayPort: Send + Sync {
    fn watch(&self, emit: &mut dyn FnMut(Result<TraySnapshot>), stop: &dyn StopSignal);
    /// A native reply acknowledges the application method, not a window appearing.
    fn act(&self, item: TrayItemId, action: TrayAction) -> Result<()>;
    /// Refresh after AboutToShow; menu IDs are valid only for this item/revision.
    fn menu(&self, item: TrayItemId, parent: i32) -> Result<TrayMenu>;
    fn invoke(&self, item: TrayItemId, revision: u32, entry: i32, timestamp: u32) -> Result<()>;
}
