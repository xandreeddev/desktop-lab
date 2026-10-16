//! In-memory adapters: component tests do not inspect the host or execute commands.
use crate::adapters::DesktopAdapters;
use lucent_domain::*;
use std::sync::{Arc, Mutex};
#[derive(Default)]
pub struct Fake {
    pub calls: Mutex<Vec<&'static str>>,
}
impl ApplicationPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<Vec<Application>>), _: &dyn StopSignal) {
        emit(self.discover());
    }
    fn discover(&self) -> Result<Vec<Application>> {
        Ok(vec![])
    }
    fn launch(&self, _: &Application) -> Result<()> {
        self.calls.lock().unwrap().push("launch");
        Ok(())
    }
}
impl SettingsPort for Fake {
    fn load(&self) -> Result<DesktopSettings> {
        Ok(DesktopSettings::default())
    }
    fn save(&self, _: &DesktopSettings) -> Result<()> {
        self.calls.lock().unwrap().push("save");
        Ok(())
    }
}
impl ClockPort for Fake {
    fn now(&self) -> ClockSnapshot {
        ClockSnapshot {
            date: Date::new(2026, 1, 15).expect("valid calendar date"),
            hour: 10,
            minute: 24,
            unix_seconds: 1_768_472_640,
        }
    }
}
impl ThemePort for Fake {
    fn apply(&self, palette: &ThemePalette) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(ThemeMode::from_light(palette.light()).name());
        Ok(())
    }
}
impl SystemPort for Fake {
    fn sample(&self) -> Result<SystemSnapshot> {
        Ok(SystemSnapshot::default())
    }
}
impl AudioPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<AudioSnapshot>), _: &dyn StopSignal) {
        emit(Ok(AudioSnapshot::default()));
    }
    fn control(&self, _: AudioCommand) -> Result<()> {
        self.calls.lock().unwrap().push("audio");
        Ok(())
    }
}
impl MediaPort for Fake {
    fn snapshot(&self) -> Result<MediaSnapshot> {
        Ok(MediaSnapshot::default())
    }
    fn control(&self, _: MediaCommand) -> Result<()> {
        self.calls.lock().unwrap().push("media");
        Ok(())
    }
}
impl NetworkPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<NetworkSnapshot>), _: &dyn StopSignal) {
        emit(Err(DomainError::Unavailable("offline fixture".into())));
    }
    fn control(&self, _: NetworkCommand) -> Result<()> {
        self.calls.lock().unwrap().push("network");
        Ok(())
    }
}
impl WeatherPort for Fake {
    fn snapshot(&self) -> Result<WeatherSnapshot> {
        Err(DomainError::Unavailable("offline fixture".into()))
    }
}
impl WallpaperPort for Fake {
    fn list(&self) -> Vec<Wallpaper> {
        vec![]
    }
    fn current(&self) -> Result<Option<WallpaperAsset>> {
        Ok(None)
    }
    fn stage(&self, source: &Wallpaper) -> Result<WallpaperAsset> {
        Ok(WallpaperAsset {
            source: source.clone(),
            path: source.path.clone(),
        })
    }
    fn commit(&self, _: &WallpaperAsset) -> Result<()> {
        self.calls.lock().unwrap().push("wallpaper");
        Ok(())
    }
    fn discard(&self, _: &WallpaperAsset) -> Result<()> {
        Ok(())
    }
}
impl SessionPort for Fake {
    fn watch_lock(&self, emit: &mut dyn FnMut(Result<SessionLockState>), _: &dyn StopSignal) {
        emit(Ok(SessionLockState::Unlocked));
    }
    fn lock(&self) -> Result<()> {
        self.calls.lock().unwrap().push("lock");
        Ok(())
    }
}
pub fn adapters() -> DesktopAdapters {
    with(Arc::new(Fake::default()))
}
pub fn with(fake: Arc<Fake>) -> DesktopAdapters {
    DesktopAdapters {
        theme: fake.clone(),
        palette_generation: fake.clone(),
        catalog: fake.clone(),
        apps: fake.clone(),
        compositor: None,
        settings: fake.clone(),
        clock: fake.clone(),
        system: fake.clone(),
        audio: fake.clone(),
        network: fake.clone(),
        bluetooth: fake.clone(),
        power: fake.clone(),
        tray: fake.clone(),
        media: fake.clone(),
        weather: fake.clone(),
        wallpaper: fake.clone(),
        playback: fake.clone(),
        session: fake.clone(),
        notifications: fake.clone(),
        uri: fake,
        assets: Arc::new(crate::platform::NativeAssets),
    }
}
impl UriPort for Fake {
    fn open(&self, _: &ExternalUri) -> Result<()> {
        self.calls.lock().unwrap().push("uri");
        Ok(())
    }
}

impl PowerPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<PowerSnapshot>), _: &dyn StopSignal) {
        emit(Ok(PowerSnapshot::default()));
    }
    fn control(&self, _: PowerCommand) -> Result<PowerOperationId> {
        self.calls.lock().unwrap().push("power");
        PowerOperationId::new(1)
    }
}
impl NotificationPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<NotificationSnapshot>), _: &dyn StopSignal) {
        emit(Ok(NotificationSnapshot::default()));
    }
    fn dismiss(&self, _: NotificationTarget) -> Result<()> {
        self.calls.lock().unwrap().push("dismiss");
        Ok(())
    }
    fn invoke(&self, _: NotificationTarget, _: &str) -> Result<()> {
        self.calls.lock().unwrap().push("invoke");
        Ok(())
    }
    fn set_do_not_disturb(&self, _: bool) -> Result<()> {
        Ok(())
    }
    fn clear_history(&self) -> Result<()> {
        Ok(())
    }
    fn retry_history(&self) -> Result<()> {
        Ok(())
    }
}

impl AuthenticationPort for Fake {
    fn authenticate(&self, _: &str, _: &mut dyn AuthConversation) -> Result<()> {
        Err(DomainError::Failed("fixture only".into()))
    }
}

impl PaletteGenerationPort for Fake {
    fn seed(&self, _: &str) -> Result<Rgb> {
        Ok(Rgb::new(80, 120, 180))
    }
}
impl WallpaperCatalogPort for Fake {
    fn search(&self, _: WallpaperProvider, _: &str, _: u32) -> Result<WallpaperPage> {
        Ok(WallpaperPage {
            items: vec![],
            page: 1,
            has_more: false,
        })
    }
    fn preview(&self, _: &RemoteWallpaper) -> Result<String> {
        Err(DomainError::Unavailable("offline fixture".into()))
    }
    fn download(&self, _: &RemoteWallpaper) -> Result<Wallpaper> {
        Err(DomainError::Unavailable("offline fixture".into()))
    }
}

impl BluetoothPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<BluetoothSnapshot>), _: &dyn StopSignal) {
        emit(Ok(BluetoothSnapshot::default()));
    }
    fn control(&self, _: BluetoothCommand) -> Result<()> {
        self.calls.lock().unwrap().push("bluetooth");
        Ok(())
    }
    fn start_connection(&self, _: BluetoothObjectId) -> Result<BluetoothConnectionId> {
        Err(DomainError::Unavailable(
            "No fixture connection target".into(),
        ))
    }
    fn cancel_connection(&self, _: BluetoothConnectionId) -> Result<()> {
        Err(DomainError::Invalid("No fixture connection attempt".into()))
    }
    fn start_pairing(&self, _: BluetoothObjectId) -> Result<BluetoothPairingId> {
        Err(DomainError::Unavailable("No fixture pairing target".into()))
    }
    fn answer_pairing(&self, _: BluetoothPairingId, _: BluetoothPairingAnswer) -> Result<()> {
        Err(DomainError::Invalid("No fixture pairing prompt".into()))
    }
    fn cancel_pairing(&self, _: BluetoothPairingId) -> Result<()> {
        Err(DomainError::Invalid("No fixture pairing attempt".into()))
    }
    fn set_incoming_pairing(&self, _: bool) -> Result<()> {
        self.calls.lock().unwrap().push("bluetooth incoming");
        Ok(())
    }
}

impl TrayPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<TraySnapshot>), _: &dyn StopSignal) {
        emit(Ok(TraySnapshot::default()));
    }
    fn act(&self, _: TrayItemId, _: TrayAction) -> Result<()> {
        self.calls.lock().unwrap().push("tray");
        Ok(())
    }
    fn menu(&self, _: TrayItemId, _: i32) -> Result<TrayMenu> {
        Err(DomainError::Unavailable("No fixture menu".into()))
    }
    fn invoke(&self, _: TrayItemId, _: u32, _: i32, _: u32) -> Result<()> {
        self.calls.lock().unwrap().push("tray-menu");
        Ok(())
    }
}

impl WallpaperPlaybackPort for Fake {
    fn prepare(&self, source: &Wallpaper) -> Result<LiveWallpaperAsset> {
        Ok(LiveWallpaperAsset {
            source: source.clone(),
            path: "/fixture/media.mp4".into(),
            poster: Wallpaper {
                path: "/fixture/poster.png".into(),
                name: "Poster".into(),
            },
        })
    }
    fn play(
        &self,
        _: &LiveWallpaperAsset,
        _: PlaybackOptions,
        _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        _: Arc<dyn StopSignal>,
    ) {
    }
}
