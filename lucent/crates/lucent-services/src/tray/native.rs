use super::{watcher::*, *};
use futures_lite::{
    future,
    stream::{self, Stream, StreamExt},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    future::Future,
    pin::Pin,
    sync::Arc,
};
use zbus::{
    Connection, MatchRule, MessageStream,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};
const ITEM_INTERFACES: [&str; 2] = [
    "org.kde.StatusNotifierItem",
    "org.freedesktop.StatusNotifierItem",
];
const MENU: &str = "com.canonical.dbusmenu";
pub(super) type Properties = HashMap<String, OwnedValue>;
type Layout = (i32, Properties, Vec<OwnedValue>);
type Events = Pin<Box<dyn Stream<Item = zbus::Result<zbus::Message>> + Send>>;
pub(super) fn unavailable(text: &str) -> DomainError {
    DomainError::Unavailable(text.into())
}
fn invalid() -> DomainError {
    DomainError::Invalid("Tray returned invalid data".into())
}
pub(super) fn bus_error(error: zbus::Error) -> DomainError {
    match error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.UnknownMethod" => {
                DomainError::Invalid("Tray application does not support this action".into())
            }
            "org.freedesktop.DBus.Error.UnknownObject" => {
                DomainError::Invalid("Tray target retired".into())
            }
            "org.freedesktop.DBus.Error.AccessDenied" => {
                DomainError::Failed("Tray application denied this action".into())
            }
            "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Timeout"
            | "org.freedesktop.DBus.Error.Disconnected"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.ServiceUnknown" => {
                unavailable("Tray application or transport is unavailable")
            }
            _ => DomainError::Failed("Tray application rejected this action".into()),
        },
        _ => unavailable("Tray transport is unavailable"),
    }
}
pub(super) async fn bounded<T>(
    operation: impl Future<Output = Result<T>>,
    deadline: Instant,
) -> Result<T> {
    if Instant::now() >= deadline {
        return Err(unavailable("Tray operation deadline expired"));
    }
    future::or(operation, async {
        async_io::Timer::at(deadline).await;
        Err(unavailable("Tray operation timed out"))
    })
    .await
}
fn field<T: TryFrom<OwnedValue>>(properties: &Properties, name: &str) -> Option<T> {
    T::try_from(properties.get(name)?.try_clone().ok()?).ok()
}
fn text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}
fn plain(value: &str) -> String {
    let mut tag = false;
    text(value, 2048)
        .chars()
        .filter(|c| {
            if *c == '<' {
                tag = true;
                false
            } else if *c == '>' {
                tag = false;
                false
            } else {
                !tag
            }
        })
        .collect()
}
pub(super) fn icon(properties: &Properties, prefix: &str) -> TrayIcon {
    let name = text(
        &field::<String>(properties, &format!("{prefix}Name")).unwrap_or_default(),
        256,
    );
    let candidates = field::<Vec<(i32, i32, Vec<u8>)>>(properties, &format!("{prefix}Pixmap"))
        .unwrap_or_default();
    // Largest accepted variant provides 2x density; cap retained content, not wire allocations.
    let pixmap = candidates
        .into_iter()
        .filter(|(w, h, b)| {
            *w > 0 && *h > 0 && *w <= 256 && *h <= 256 && b.len() == *w as usize * *h as usize * 4
        })
        .max_by_key(|(w, h, _)| w * h)
        .and_then(|(w, h, mut data)| {
            for pixel in data.as_chunks_mut::<4>().0 {
                pixel.rotate_left(1);
            }
            TrayPixmap::new(w as u32, h as u32, data).ok()
        });
    TrayIcon { name, pixmap }
}
struct Item {
    target: Target,
    interface: &'static str,
    data: TrayItem,
    menu_path: Option<String>,
    menu: Option<TrayMenu>,
}
async fn properties(
    connection: &Connection,
    target: &Target,
    interface: &str,
) -> Result<Properties> {
    connection
        .call_method(
            Some(target.owner.as_str()),
            target.path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(interface,),
        )
        .await
        .map_err(bus_error)?
        .body()
        .deserialize()
        .map_err(|_| invalid())
}
async fn read(connection: &Connection, target: Target, id: TrayItemId) -> Result<Item> {
    let mut result = None;
    for interface in ITEM_INTERFACES {
        if let Ok(properties) = properties(connection, &target, interface).await
            && properties.contains_key("Status")
        {
            result = Some((interface, properties));
            break;
        }
    }
    let (interface, p) = result.ok_or_else(|| unavailable("Tray item properties unavailable"))?;
    let status = match field::<String>(&p, "Status").as_deref() {
        Some("Passive") => TrayStatus::Passive,
        Some("Active") => TrayStatus::Active,
        Some("NeedsAttention") => TrayStatus::NeedsAttention,
        _ => return Err(invalid()),
    };
    let tooltip = field::<(String, Vec<(i32, i32, Vec<u8>)>, String, String)>(&p, "ToolTip")
        .map(|(_, _, title, body)| format!("{} {}", text(&title, 256), plain(&body)))
        .unwrap_or_default();
    let menu_path = field::<OwnedObjectPath>(&p, "Menu")
        .filter(|p| p.as_str() != "/")
        .map(|p| p.to_string());
    Ok(Item {
        target,
        interface,
        menu: None,
        data: TrayItem {
            id,
            application: text(&field::<String>(&p, "Id").unwrap_or_default(), 128),
            title: text(&field::<String>(&p, "Title").unwrap_or_default(), 256),
            tooltip,
            status,
            icon: icon(&p, "Icon"),
            attention_icon: icon(&p, "AttentionIcon"),
            overlay_icon: icon(&p, "OverlayIcon"),
            item_is_menu: field(&p, "ItemIsMenu").unwrap_or(false),
            has_menu: menu_path.is_some(),
            menu_generation: 0,
        },
        menu_path,
    })
}
fn menu_node(
    raw: Layout,
    depth: usize,
    count: &mut usize,
    ids: &mut BTreeSet<i32>,
) -> Result<TrayMenuEntry> {
    *count += 1;
    if depth > 8 || *count > 256 || !ids.insert(raw.0) {
        return Err(invalid());
    }
    let mut children = Vec::new();
    for child in raw.2 {
        let raw: Layout = child.try_into().map_err(|_| invalid())?;
        children.push(menu_node(raw, depth + 1, count, ids)?);
    }
    let checked = field::<i32>(&raw.1, "toggle-state").unwrap_or(0) == 1;
    Ok(TrayMenuEntry {
        id: raw.0,
        label: text(&field::<String>(&raw.1, "label").unwrap_or_default(), 256),
        enabled: field(&raw.1, "enabled").unwrap_or(true),
        visible: field(&raw.1, "visible").unwrap_or(true),
        separator: field::<String>(&raw.1, "type").as_deref() == Some("separator"),
        submenu: field::<String>(&raw.1, "children-display").as_deref() == Some("submenu"),
        check: match field::<String>(&raw.1, "toggle-type").as_deref() {
            Some("checkmark") => TrayCheck::Check(checked),
            Some("radio") => TrayCheck::Radio(checked),
            _ => TrayCheck::None,
        },
        children,
    })
}
async fn call(connection: &Connection, item: &Item, action: TrayAction) -> Result<()> {
    let result = match action {
        TrayAction::Activate { x, y } => {
            connection
                .call_method(
                    Some(item.target.owner.as_str()),
                    item.target.path.as_str(),
                    Some(item.interface),
                    "Activate",
                    &(x, y),
                )
                .await
        }
        TrayAction::SecondaryActivate { x, y } => {
            connection
                .call_method(
                    Some(item.target.owner.as_str()),
                    item.target.path.as_str(),
                    Some(item.interface),
                    "SecondaryActivate",
                    &(x, y),
                )
                .await
        }
        TrayAction::ContextMenu { x, y } => {
            connection
                .call_method(
                    Some(item.target.owner.as_str()),
                    item.target.path.as_str(),
                    Some(item.interface),
                    "ContextMenu",
                    &(x, y),
                )
                .await
        }
        TrayAction::Scroll { delta, horizontal } => {
            connection
                .call_method(
                    Some(item.target.owner.as_str()),
                    item.target.path.as_str(),
                    Some(item.interface),
                    "Scroll",
                    &(delta, if horizontal { "horizontal" } else { "vertical" }),
                )
                .await
        }
    };
    result
        .map_err(bus_error)?
        .body()
        .deserialize::<()>()
        .map_err(|_| invalid())
}
async fn load_menu(connection: &Connection, item: &Item, parent: i32) -> Result<TrayMenu> {
    let path = item
        .menu_path
        .as_deref()
        .ok_or_else(|| unavailable("This application has no native menu"))?;
    let _: bool = connection
        .call_method(
            Some(item.target.owner.as_str()),
            path,
            Some(MENU),
            "AboutToShow",
            &(parent,),
        )
        .await
        .map_err(bus_error)?
        .body()
        .deserialize()
        .map_err(|_| invalid())?;
    let (revision, layout): (u32, Layout) = connection
        .call_method(
            Some(item.target.owner.as_str()),
            path,
            Some(MENU),
            "GetLayout",
            &(
                0i32,
                8i32,
                vec![
                    "label",
                    "enabled",
                    "visible",
                    "type",
                    "toggle-type",
                    "toggle-state",
                    "children-display",
                ],
            ),
        )
        .await
        .map_err(bus_error)?
        .body()
        .deserialize()
        .map_err(|_| invalid())?;
    if layout.0 != 0 {
        return Err(invalid());
    }
    Ok(TrayMenu {
        item: item.data.id,
        revision,
        root: menu_node(layout, 0, &mut 0, &mut BTreeSet::new())?,
    })
}
async fn command(
    connection: &Connection,
    items: &mut BTreeMap<Target, Item>,
    command: Command,
) -> Result<Reply> {
    let id = match command {
        Command::Act(id, _) | Command::Menu(id, _) | Command::Invoke(id, _, _, _) => id,
    };
    let item = items
        .values_mut()
        .find(|i| i.data.id == id)
        .ok_or_else(|| DomainError::Invalid("Tray item retired".into()))?;
    match command {
        Command::Act(_, action) => {
            call(connection, item, action).await?;
            Ok(Reply::Done)
        }
        Command::Menu(_, parent) => {
            if parent != 0
                && !item.menu.as_ref().is_some_and(|menu| {
                    lucent_usecases::tray::menu_entry(&menu.root, parent).is_some()
                })
            {
                return Err(DomainError::Invalid("Tray submenu retired".into()));
            }
            let menu = load_menu(connection, item, parent).await?;
            item.menu = Some(menu.clone());
            Ok(Reply::Menu(menu))
        }
        Command::Invoke(_, revision, id, timestamp) => {
            // Fresh layout closes the queued-signal race; never execute an old row.
            let menu = load_menu(connection, item, 0).await?;
            lucent_usecases::tray::validate_menu_action(&menu, revision, id)?;
            let path = item.menu_path.as_deref().ok_or_else(invalid)?;
            connection
                .call_method(
                    Some(item.target.owner.as_str()),
                    path,
                    Some(MENU),
                    "Event",
                    &(id, "clicked", Value::from(0i32), timestamp),
                )
                .await
                .map_err(bus_error)?
                .body()
                .deserialize::<()>()
                .map_err(|_| invalid())?;
            item.menu = Some(menu);
            Ok(Reply::Done)
        }
    }
}
async fn events(
    connection: &Connection,
    items: &BTreeMap<Target, Item>,
    registered: &[Target],
) -> Result<Events> {
    let mut events: Events = Box::pin(stream::pending());
    for target in registered {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(target.owner.as_str())
            .map_err(bus_error)?
            .path(target.path.as_str())
            .map_err(bus_error)?
            .build();
        let input = MessageStream::for_match_rule(rule, connection, Some(16))
            .await
            .map_err(bus_error)?;
        events = Box::pin(events.or(input));
        if let Some(path) = items.get(target).and_then(|item| item.menu_path.as_deref()) {
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(target.owner.as_str())
                .map_err(bus_error)?
                .path(path)
                .map_err(bus_error)?
                .interface(MENU)
                .map_err(bus_error)?
                .build();
            let input = MessageStream::for_match_rule(rule, connection, Some(8))
                .await
                .map_err(bus_error)?;
            events = Box::pin(events.or(input));
        }
    }
    Ok(events)
}
struct Retire(Connection);
impl Drop for Retire {
    fn drop(&mut self) {
        let _ = future::block_on(self.0.clone().close());
    }
}
struct Online(Arc<Shared>);
impl Drop for Online {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .online = false;
    }
}
pub(super) async fn observe(
    adapter: &StatusNotifier,
    emit: &mut dyn FnMut(Result<TraySnapshot>),
    stop: &dyn StopSignal,
) -> Result<()> {
    let (wake_send, wake_receive) = async_channel::bounded(1);
    let shared = Arc::new(Shared {
        state: Mutex::new(Registry::default()),
        wake: wake_send,
    });
    let builder = match adapter.address.as_deref() {
        Some(address) => zbus::connection::Builder::address(address).map_err(bus_error)?,
        None => zbus::connection::Builder::session().map_err(bus_error)?,
    };
    let builder = builder
        .serve_at(PATH, Kde(shared.clone()))
        .map_err(bus_error)?
        .serve_at(PATH, Freedesktop(shared.clone()))
        .map_err(bus_error)?;
    let connection = bounded(
        async { builder.build().await.map_err(bus_error) },
        Instant::now() + DEADLINE,
    )
    .await?;
    let _retire = Retire(connection.clone());
    let owners_rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .map_err(bus_error)?
        .interface("org.freedesktop.DBus")
        .map_err(bus_error)?
        .member("NameOwnerChanged")
        .map_err(bus_error)?
        .build();
    let mut owners = MessageStream::for_match_rule(owners_rule, &connection, Some(128))
        .await
        .map_err(bus_error)?;
    let host = format!("org.kde.StatusNotifierHost.Lucent{}", std::process::id());
    connection
        .request_name(host.as_str())
        .await
        .map_err(bus_error)?;
    {
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.hosts.insert(
            host,
            connection.unique_name().ok_or_else(invalid)?.to_string(),
        );
        state.online = true;
    }
    let _online = Online(shared.clone());
    for name in INTERFACES {
        // Never replace/queue behind an existing watcher.
        use zbus::fdo::{RequestNameFlags, RequestNameReply};
        let reply = connection
            .request_name_with_flags(name, RequestNameFlags::DoNotQueue.into())
            .await
            .map_err(bus_error)?;
        if reply != RequestNameReply::PrimaryOwner {
            return Err(unavailable("Another tray watcher is already running"));
        }
    }
    let (send, receive) = async_channel::bounded::<Request>(32);
    *adapter
        .online
        .lock()
        .map_err(|_| unavailable("Tray admission failed"))? = Some(send);
    let mut items = BTreeMap::<Target, Item>::new();
    let mut signals: Events = Box::pin(stream::pending());
    let mut previous = None;
    let mut dirty = true;
    enum Wake {
        Registry,
        Owner(Option<zbus::Result<zbus::Message>>),
        Signal(Option<zbus::Result<zbus::Message>>),
        Request(std::result::Result<Request, async_channel::RecvError>),
        Tick,
    }
    while !stop.cancelled() {
        let publish = dirty;
        if dirty {
            let targets: Vec<_> = shared
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .items
                .iter()
                .cloned()
                .collect();
            items.retain(|target, _| targets.contains(target));
            // Subscribe before reading properties so changes during the read stay queued.
            let before_read = events(&connection, &items, &targets).await?;
            let refresh_deadline = Instant::now() + DEADLINE;
            for (index, target) in targets.iter().enumerate() {
                // A slow app gets only its fair share, preserving time for later items.
                let remaining = refresh_deadline.saturating_duration_since(Instant::now());
                let item_deadline = Instant::now() + remaining / ((targets.len() - index) as u32);
                let id = match items.get(target) {
                    Some(i) => i.data.id,
                    None => adapter.identity()?,
                };
                match bounded(read(&connection, target.clone(), id), item_deadline).await {
                    Ok(mut item) => {
                        item.data.menu_generation = items
                            .get(target)
                            .map(|i| i.data.menu_generation)
                            .unwrap_or(0);
                        items.insert(target.clone(), item);
                    }
                    Err(_) => {
                        items.remove(target);
                    }
                }
            }
            // Menu paths may have changed while refreshing.
            signals = Box::pin(before_read.or(events(&connection, &items, &targets).await?));
            dirty = false;
        }
        if publish {
            let snapshot = TraySnapshot {
                items: items.values().map(|i| i.data.clone()).collect(),
            };
            if previous.as_ref() != Some(&snapshot) {
                emit(Ok(snapshot.clone()));
                previous = Some(snapshot);
            }
        }
        let wake = future::or(
            async {
                let _ = wake_receive.recv().await;
                Wake::Registry
            },
            future::or(
                async { Wake::Owner(owners.next().await) },
                future::or(
                    async { Wake::Signal(signals.next().await) },
                    future::or(async { Wake::Request(receive.recv().await) }, async {
                        async_io::Timer::after(Duration::from_millis(100)).await;
                        Wake::Tick
                    }),
                ),
            ),
        )
        .await;
        match wake {
            Wake::Tick => (),
            Wake::Registry => dirty = true,
            Wake::Owner(Some(message)) => {
                let message = message.map_err(bus_error)?;
                let (name, old, new): (String, String, String) =
                    message.body().deserialize().map_err(|_| invalid())?;
                if name.starts_with(':') && !old.is_empty() && new.is_empty() {
                    let removed = {
                        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                        let removed: Vec<_> = state
                            .items
                            .iter()
                            .filter(|t| t.owner == old)
                            .cloned()
                            .collect();
                        state.items.retain(|t| t.owner != old);
                        state.hosts.retain(|_, o| o != &old);
                        removed
                    };
                    for target in removed {
                        items.remove(&target);
                        for interface in INTERFACES {
                            connection
                                .emit_signal(
                                    None::<&str>,
                                    PATH,
                                    interface,
                                    "StatusNotifierItemUnregistered",
                                    &(target.registration(),),
                                )
                                .await
                                .map_err(bus_error)?;
                        }
                        dirty = true;
                    }
                }
            }
            Wake::Owner(None) | Wake::Signal(None) => {
                return Err(unavailable("Tray signal stream ended"));
            }
            Wake::Signal(Some(message)) => {
                let message = message.map_err(bus_error)?;
                let header = message.header();
                for item in items.values_mut() {
                    if header
                        .sender()
                        .is_some_and(|o| o.as_str() == item.target.owner)
                        && header
                            .path()
                            .is_some_and(|p| item.menu_path.as_deref() == Some(p.as_str()))
                    {
                        item.menu = None;
                        item.data.menu_generation = item.data.menu_generation.wrapping_add(1);
                    }
                }
                dirty = true;
            }
            Wake::Request(Ok(request)) => {
                if request.cancelled.load(Ordering::Acquire) || Instant::now() >= request.deadline {
                    continue;
                }
                // Registration presence is checked again before touching the app.
                items.retain(|t, _| {
                    shared
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .items
                        .contains(t)
                });
                let result = bounded(
                    command(&connection, &mut items, request.command),
                    request.deadline,
                )
                .await;
                let uncertain = matches!(result, Err(DomainError::Unavailable(_)));
                let delivered = request.reply.try_send(result).is_ok();
                if uncertain || !delivered {
                    return Err(unavailable("Tray outcome uncertain; connection retired"));
                }
            }
            Wake::Request(Err(_)) => return Err(unavailable("Tray request stream ended")),
        }
    }
    connection.close().await.map_err(bus_error)
}
