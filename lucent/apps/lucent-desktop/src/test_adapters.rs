//! In-memory adapters: component tests do not inspect the host or execute commands.
use crate::adapters::DesktopAdapters;
use lucent_domain::*;
use std::sync::{Arc, Mutex};
#[derive(Default)]
pub struct Fake {
    pub calls: Mutex<Vec<&'static str>>,
}
impl ApplicationPort for Fake {
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
            date: Date {
                year: 2026,
                month: 1,
                day: 15,
                weekday: 3,
            },
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
impl WeatherPort for Fake {
    fn snapshot(&self) -> Result<WeatherSnapshot> {
        Err(DomainError::Unavailable("offline fixture".into()))
    }
}
impl WallpaperPort for Fake {
    fn list(&self) -> Vec<Wallpaper> {
        vec![]
    }
    fn current(&self) -> String {
        String::new()
    }
    fn apply(&self, _: &str) -> Result<()> {
        self.calls.lock().unwrap().push("wallpaper");
        Ok(())
    }
}
impl SessionPort for Fake {
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
        media: fake.clone(),
        weather: fake.clone(),
        wallpaper: fake.clone(),
        session: fake.clone(),
        notifications: fake,
        assets: Arc::new(crate::platform::NativeAssets),
    }
}

impl NotificationPort for Fake {
    fn watch(&self, emit: &mut dyn FnMut(Result<NotificationSnapshot>), _: &dyn StopSignal) {
        emit(Ok(NotificationSnapshot::default()));
    }
    fn dismiss(&self, _: u32) -> Result<()> {
        self.calls.lock().unwrap().push("dismiss");
        Ok(())
    }
    fn invoke(&self, _: u32, _: &str) -> Result<()> {
        self.calls.lock().unwrap().push("invoke");
        Ok(())
    }
    fn set_do_not_disturb(&self, _: bool) -> Result<()> {
        Ok(())
    }
    fn clear_history(&self) -> Result<()> {
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
