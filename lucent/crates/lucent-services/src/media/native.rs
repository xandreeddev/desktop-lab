use super::{
    snapshot::{self, Properties, Raw},
    *,
};
use async_executor::{Executor, Task};
use futures_lite::{StreamExt, future};
use std::{collections::BTreeMap, future::Future};
use zbus::{
    Connection, MatchRule, Message, MessageStream, message::Type, zvariant::OwnedObjectPath,
};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
pub(super) const PATH: &str = "/org/mpris/MediaPlayer2";
pub(super) const ROOT: &str = "org.mpris.MediaPlayer2";
pub(super) const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const MAX_PLAYERS: usize = 16;
const MESSAGE_BYTES: usize = 128 * 1024;

pub(super) fn unavailable(message: &str) -> DomainError {
    DomainError::Unavailable(message.into())
}
fn stale() -> DomainError {
    DomainError::Invalid("Media target changed; choose a current player or track".into())
}
fn bus_error(error: zbus::Error) -> DomainError {
    match error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.AccessDenied" => {
                DomainError::Failed("Media player denied the action".into())
            }
            "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.NotSupported" => {
                DomainError::Invalid("Media player does not support this action".into())
            }
            "org.freedesktop.DBus.Error.UnknownObject" => stale(),
            "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.Disconnected"
            | "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Timeout" => {
                unavailable("Media player or transport is unavailable")
            }
            _ => DomainError::Failed("Media player rejected the action".into()),
        },
        _ => unavailable("Media transport is unavailable"),
    }
}
async fn bounded<T>(operation: impl Future<Output = Result<T>>, deadline: Instant) -> Result<T> {
    if Instant::now() >= deadline {
        return Err(unavailable("Media operation deadline expired"));
    }
    future::or(operation, async {
        async_io::Timer::at(deadline).await;
        Err(unavailable(
            "Media operation timed out; its outcome may be uncertain",
        ))
    })
    .await
}
fn body<T>(message: Message) -> Result<T>
where
    T: for<'a> zbus::zvariant::DynamicDeserialize<'a>,
{
    if message.body().len() > MESSAGE_BYTES {
        return Err(unavailable("Media message exceeds the observation limit"));
    }
    message
        .body()
        .deserialize()
        .map_err(|_| snapshot::invalid())
}
async fn properties(connection: &Connection, owner: &str, interface: &str) -> Result<Properties> {
    body(
        connection
            .call_method(
                Some(owner),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(interface,),
            )
            .await
            .map_err(bus_error)?,
    )
}
async fn read(connection: &Connection, owner: &str) -> Result<Raw> {
    let root = properties(connection, owner, ROOT).await?;
    let player = properties(connection, owner, PLAYER).await?;
    Ok(Raw {
        root,
        player,
        sampled_at: Instant::now(),
    })
}
fn target(command: &MediaCommand) -> MediaPlayerId {
    match command {
        MediaCommand::Play(id)
        | MediaCommand::Pause(id)
        | MediaCommand::Next(id)
        | MediaCommand::Previous(id)
        | MediaCommand::Stop(id)
        | MediaCommand::Raise(id) => *id,
        MediaCommand::SetPosition { player, .. } => *player,
    }
}
/// Re-read current native capabilities immediately before sending a method.
/// Unique-owner addressing prevents a new application incarnation receiving it;
/// SetPosition's native track argument additionally protects a track-change race.
async fn control(
    connection: &Connection,
    owner: &str,
    command: &MediaCommand,
    track: Option<&OwnedObjectPath>,
    deadline: Instant,
) -> Result<()> {
    use snapshot::field;
    if matches!(command, MediaCommand::Raise(_)) {
        let properties = properties(connection, owner, ROOT).await?;
        if !field::<bool>(&properties, "CanRaise").unwrap_or(false) {
            return Err(stale());
        }
    } else {
        let properties = properties(connection, owner, PLAYER).await?;
        if !field::<bool>(&properties, "CanControl").unwrap_or(false) {
            return Err(stale());
        }
        let capability = match command {
            MediaCommand::Play(_) => Some("CanPlay"),
            MediaCommand::Pause(_) => Some("CanPause"),
            MediaCommand::Next(_) => Some("CanGoNext"),
            MediaCommand::Previous(_) => Some("CanGoPrevious"),
            MediaCommand::SetPosition { .. } => Some("CanSeek"),
            _ => None,
        };
        if capability
            .is_some_and(|capability| !field::<bool>(&properties, capability).unwrap_or(false))
        {
            return Err(stale());
        }
        if let MediaCommand::SetPosition { position, .. } = command {
            let metadata = snapshot::metadata(&properties);
            if snapshot::track_path(&metadata).as_ref() != track || track.is_none() {
                return Err(stale());
            }
            if field::<i64>(&metadata, "mpris:length")
                .and_then(|value| u64::try_from(value).ok())
                .is_some_and(|length| position.micros() > length)
            {
                return Err(stale());
            }
        }
    }
    // Revalidation and the deadline timer may become ready together. Check
    // again before dispatch so polling this future first cannot start a new
    // native action after the request's admission deadline has expired.
    if Instant::now() >= deadline {
        return Err(unavailable(
            "Media request deadline expired before dispatch",
        ));
    }
    let (interface, method) = match command {
        MediaCommand::Play(_) => (PLAYER, "Play"),
        MediaCommand::Pause(_) => (PLAYER, "Pause"),
        MediaCommand::Next(_) => (PLAYER, "Next"),
        MediaCommand::Previous(_) => (PLAYER, "Previous"),
        MediaCommand::Stop(_) => (PLAYER, "Stop"),
        MediaCommand::Raise(_) => (ROOT, "Raise"),
        MediaCommand::SetPosition { position, .. } => {
            connection
                .call_method(
                    Some(owner),
                    PATH,
                    Some(PLAYER),
                    "SetPosition",
                    &(track.ok_or_else(stale)?, position.micros() as i64),
                )
                .await
                .map_err(bus_error)?;
            return Ok(());
        }
    };
    connection
        .call_method(Some(owner), PATH, Some(interface), method, &())
        .await
        .map_err(bus_error)?;
    Ok(())
}
struct Entry {
    owner: String,
    data: MediaPlayer,
    track: Option<OwnedObjectPath>,
    generation: u64,
    refresh: Option<Task<()>>,
    dirty: bool,
    authoritative: bool,
    retry_at: Option<Instant>,
    retries: u8,
}
impl Entry {
    fn invalidate(&mut self) -> Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| unavailable("Media observation generation exhausted"))?;
        // Finish one accepted query before starting the latest requested
        // refresh. Dropping/restarting it on every signal would leave already
        // dispatched GetAll calls running in the peer during a signal burst.
        self.dirty = true;
        self.authoritative = false;
        self.retry_at = None;
        Ok(())
    }
    fn retire_track(&mut self) {
        self.track = None;
        if let MediaPlayerState::Ready(playback) = &mut self.data.state {
            playback.track = None;
            playback.position = None;
        }
    }
}
enum Completion {
    Read {
        id: MediaPlayerId,
        generation: u64,
        result: Result<Raw>,
    },
    Control {
        revision: u64,
        result: Result<()>,
    },
}
struct Active {
    id: MediaPlayerId,
    revision: u64,
    request: Request,
    _task: Task<()>,
}
impl Drop for Active {
    fn drop(&mut self) {
        // If it already received a reply, a second try_send either fills an
        // unobserved slot or fails; it never blocks or authorizes a retry.
        let _ = self.request.reply.try_send(Err(stale()));
    }
}
fn retire_seek(active: &mut Option<Active>, player: MediaPlayerId) {
    if active.as_ref().is_some_and(|active| {
        active.id == player && matches!(active.request.command, MediaCommand::SetPosition { .. })
    }) {
        // Dropping Task cancels a GetAll/method still awaiting its reply. This
        // cannot undo an accepted method, but must prevent A -> B -> A native
        // path reuse from restoring a retired domain track's authority.
        *active = None;
    }
}
fn catalog(entries: &BTreeMap<String, Entry>) -> MediaSnapshot {
    MediaSnapshot {
        players: entries.values().map(|entry| entry.data.clone()).collect(),
    }
}
fn refresh(
    executor: &Executor<'static>,
    connection: &Connection,
    done: &async_channel::Sender<Completion>,
    entries: &mut BTreeMap<String, Entry>,
    timeout: Duration,
) {
    for entry in entries
        .values_mut()
        .filter(|entry| entry.dirty && entry.refresh.is_none())
    {
        entry.dirty = false;
        let owner = entry.owner.clone();
        let id = entry.data.id;
        let generation = entry.generation;
        let connection = connection.clone();
        let done = done.clone();
        let deadline = Instant::now() + timeout;
        entry.refresh = Some(executor.spawn(async move {
            let result = bounded(read(&connection, &owner), deadline).await;
            let _ = done
                .send(Completion::Read {
                    id,
                    generation,
                    result,
                })
                .await;
        }));
    }
}
fn add(
    adapter: &Mpris,
    entries: &mut BTreeMap<String, Entry>,
    name: String,
    owner: String,
) -> Result<()> {
    if owner.is_empty() || !name.starts_with(PREFIX) {
        return Ok(());
    }
    if entries.get(&name).is_some_and(|entry| entry.owner == owner) {
        return Ok(());
    }
    if !entries.contains_key(&name) && entries.len() >= MAX_PLAYERS {
        return Err(unavailable(
            "Media player catalog exceeds the observation limit",
        ));
    }
    let id = MediaPlayerId::new(adapter.identity()?)?;
    entries.insert(
        name,
        Entry {
            owner,
            data: MediaPlayer {
                id,
                name: "Media player".into(),
                desktop_entry: None,
                state: MediaPlayerState::Unavailable("Reading media player state".into()),
            },
            track: None,
            generation: 0,
            refresh: None,
            dirty: true,
            authoritative: false,
            retry_at: None,
            retries: 0,
        },
    );
    Ok(())
}
pub(super) fn current_owner_transition(known: Option<&str>, old: &str, new: &str) -> bool {
    old != new && known.is_none_or(|owner| owner == old)
}
struct Retire(Connection);
impl Drop for Retire {
    fn drop(&mut self) {
        let _ = future::block_on(self.0.clone().close());
    }
}
async fn signals(
    connection: &Connection,
    interface: &str,
    member: &str,
    limit: usize,
) -> Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(PATH)
        .map_err(bus_error)?
        .interface(interface)
        .map_err(bus_error)?
        .member(member)
        .map_err(bus_error)?
        .build();
    MessageStream::for_match_rule(rule, connection, Some(limit))
        .await
        .map_err(bus_error)
}
pub(super) async fn observe(
    adapter: &Mpris,
    emit: &mut dyn FnMut(Result<MediaSnapshot>),
    stop: &dyn StopSignal,
) -> Result<()> {
    let discovery_deadline = Instant::now() + adapter.timeout;
    let builder = match adapter.address.as_deref() {
        Some(address) => zbus::connection::Builder::address(address),
        None => zbus::connection::Builder::session(),
    }
    .map_err(bus_error)?;
    let connection = bounded(
        async { builder.build().await.map_err(bus_error) },
        discovery_deadline,
    )
    .await?;
    let _retire = Retire(connection.clone());
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .map_err(bus_error)?
        .interface("org.freedesktop.DBus")
        .map_err(bus_error)?
        .member("NameOwnerChanged")
        .map_err(bus_error)?
        .arg0ns("org.mpris.MediaPlayer2")
        .map_err(bus_error)?
        .build();
    // Register all event sources before discovery/native property reads.
    let mut owners = bounded(
        async {
            MessageStream::for_match_rule(rule, &connection, Some(64))
                .await
                .map_err(bus_error)
        },
        discovery_deadline,
    )
    .await?;
    let mut properties = bounded(
        signals(
            &connection,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            128,
        ),
        discovery_deadline,
    )
    .await?;
    let mut seeked = bounded(
        signals(&connection, PLAYER, "Seeked", 32),
        discovery_deadline,
    )
    .await?;
    let mut entries = BTreeMap::new();
    let mut names: Vec<String> = bounded(
        async {
            body(
                connection
                    .call_method(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        Some("org.freedesktop.DBus"),
                        "ListNames",
                        &(),
                    )
                    .await
                    .map_err(bus_error)?,
            )
        },
        discovery_deadline,
    )
    .await?;
    names.retain(|name| name.starts_with(PREFIX));
    names.sort();
    if names.len() > MAX_PLAYERS {
        return Err(unavailable(
            "Media player catalog exceeds the observation limit",
        ));
    }
    for name in names {
        let owner: Option<String> = bounded(
            async {
                let reply = match connection
                    .call_method(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        Some("org.freedesktop.DBus"),
                        "GetNameOwner",
                        &(name.as_str(),),
                    )
                    .await
                {
                    Ok(reply) => reply,
                    Err(zbus::Error::MethodError(error, _, _))
                        if error.as_str() == "org.freedesktop.DBus.Error.NameHasNoOwner" =>
                    {
                        return Ok(None);
                    }
                    Err(error) => return Err(bus_error(error)),
                };
                body(reply).map(Some)
            },
            discovery_deadline,
        )
        .await?;
        // A player may disappear while discovering it. Its queued owner event
        // reconciles that race; unrelated discovery failures are explicit.
        if let Some(owner) = owner {
            add(adapter, &mut entries, name, owner)?;
        }
    }
    let (send, requests) = async_channel::bounded(REQUEST_LIMIT);
    *adapter
        .online
        .lock()
        .map_err(|_| unavailable("Media admission failed"))? = Some(send);
    let (done, completed) = async_channel::bounded(32);
    let executor = Executor::new();
    let mut active: Option<Active> = None;
    let mut command_revision = 0u64;
    let mut previous = None;
    enum Wake {
        Owner(Option<zbus::Result<Message>>),
        Properties(Option<zbus::Result<Message>>),
        Seeked(Option<zbus::Result<Message>>),
        Completed(std::result::Result<Completion, async_channel::RecvError>),
        Request(std::result::Result<Request, async_channel::RecvError>),
        Cancel,
    }
    let result = executor
        .run(async {
            while !stop.cancelled() {
                refresh(&executor, &connection, &done, &mut entries, adapter.timeout);
                let snapshot = catalog(&entries);
                if previous.as_ref() != Some(&snapshot) {
                    emit(Ok(snapshot.clone()));
                    previous = Some(snapshot);
                }
                let accept = active.is_none();
                let wake = future::or(
                    async { Wake::Owner(owners.next().await) },
                    future::or(
                        async { Wake::Completed(completed.recv().await) },
                        future::or(
                            async { Wake::Properties(properties.next().await) },
                            future::or(
                                async { Wake::Seeked(seeked.next().await) },
                                future::or(
                                    async {
                                        if accept {
                                            Wake::Request(requests.recv().await)
                                        } else {
                                            future::pending().await
                                        }
                                    },
                                    async {
                                        async_io::Timer::after(Duration::from_millis(100)).await;
                                        Wake::Cancel
                                    },
                                ),
                            ),
                        ),
                    ),
                )
                .await;
                match wake {
                    Wake::Cancel => {
                        if active
                            .as_ref()
                            .is_some_and(|active| active.request.cancelled.load(Ordering::Acquire))
                        {
                            active = None;
                        }
                        // Failed reads get at most three bounded recovery attempts;
                        // ordinary ready players are never periodically queried.
                        for entry in entries.values_mut() {
                            if entry.retry_at.is_some_and(|at| at <= Instant::now()) {
                                entry.invalidate()?;
                            }
                        }
                    }
                    Wake::Owner(Some(message)) => {
                        let (name, old, new): (String, String, String) =
                            body(message.map_err(bus_error)?)?;
                        if !name.starts_with(PREFIX) {
                            continue;
                        }
                        // Discovery may already have resolved a newer owner
                        // than historical events queued before GetNameOwner.
                        // Only advance from the incarnation currently known.
                        if !current_owner_transition(
                            entries.get(&name).map(|entry| entry.owner.as_str()),
                            &old,
                            &new,
                        ) {
                            continue;
                        }
                        let retired = entries
                            .get(&name)
                            .filter(|entry| entry.owner == old && old != new)
                            .map(|entry| entry.data.id);
                        if let Some(retired) = retired {
                            entries.remove(&name); // drops the in-flight property future
                            if active.as_ref().is_some_and(|active| active.id == retired) {
                                active = None; // replies immediately without waiting on the old player
                            }
                        }
                        if !new.is_empty() {
                            add(adapter, &mut entries, name, new)?;
                        }
                    }
                    Wake::Properties(Some(message)) => {
                        let message = message.map_err(bus_error)?;
                        let header = message.header();
                        let Some(sender) = header.sender().map(|sender| sender.as_str().to_owned())
                        else {
                            continue;
                        };
                        if !entries.values().any(|entry| entry.owner == sender) {
                            continue;
                        }
                        let parsed: Result<(String, Properties, Vec<String>)> = body(message);
                        let (interface, changed, invalidated) = match parsed {
                            Ok(value) if value.1.len() <= 64 && value.2.len() <= 64 => value,
                            _ => {
                                for entry in
                                    entries.values_mut().filter(|entry| entry.owner == sender)
                                {
                                    retire_seek(&mut active, entry.data.id);
                                    entry.retire_track();
                                    entry.invalidate()?;
                                    entry.data.state = MediaPlayerState::Unavailable(
                                        "Media player returned an invalid signal".into(),
                                    );
                                }
                                continue;
                            }
                        };
                        if interface != ROOT && interface != PLAYER {
                            continue;
                        }
                        for entry in entries.values_mut().filter(|entry| entry.owner == sender) {
                            if interface == PLAYER
                                && (changed.contains_key("Metadata")
                                    || invalidated.iter().any(|p| p == "Metadata"))
                            {
                                let native = snapshot::track_path(&snapshot::metadata(&changed));
                                if native.as_ref() != entry.track.as_ref()
                                    || invalidated.iter().any(|p| p == "Metadata")
                                {
                                    retire_seek(&mut active, entry.data.id);
                                    entry.retire_track();
                                }
                            }
                            entry.invalidate()?;
                            entry.retries = 0;
                        }
                    }
                    Wake::Seeked(Some(message)) => {
                        let message = message.map_err(bus_error)?;
                        let header = message.header();
                        let Some(sender) = header.sender().map(|sender| sender.as_str().to_owned())
                        else {
                            continue;
                        };
                        if !entries.values().any(|entry| entry.owner == sender) {
                            continue;
                        }
                        let position = body::<(i64,)>(message)
                            .ok()
                            .and_then(|(position,)| u64::try_from(position).ok())
                            .and_then(|position| MediaTime::new(position).ok());
                        for entry in entries.values_mut().filter(|entry| entry.owner == sender) {
                            if let Some(position) = position
                                && let MediaPlayerState::Ready(playback) = &mut entry.data.state
                                && let Some(sample) = &mut playback.position
                            {
                                sample.position = position;
                                sample.sampled_at = Instant::now();
                            }
                            // Query once after a discontinuity. Regular position
                            // advances are projected by consumers, never polled here.
                            entry.invalidate()?;
                            entry.retries = 0;
                        }
                    }
                    Wake::Completed(Ok(Completion::Read {
                        id,
                        generation,
                        result,
                    })) => {
                        let Some(entry) = entries.values_mut().find(|entry| entry.data.id == id)
                        else {
                            continue;
                        };
                        entry.refresh = None;
                        if entry.generation != generation {
                            continue; // dirty/latest demand starts after this accepted query
                        }
                        let previous_track = match &entry.data.state {
                            MediaPlayerState::Ready(playback) => {
                                playback.track.as_ref().map(|track| track.id)
                            }
                            MediaPlayerState::Unavailable(_) => None,
                        };
                        match result.and_then(|raw| {
                            snapshot::decode(adapter, id, raw, entry.track.as_ref(), previous_track)
                        }) {
                            Ok((data, track)) => {
                                entry.data = data;
                                entry.track = track;
                                entry.authoritative = true;
                                entry.retry_at = None;
                                entry.retries = 0;
                            }
                            Err(error) => {
                                retire_seek(&mut active, entry.data.id);
                                entry.retire_track();
                                entry.data.state = MediaPlayerState::Unavailable(error.to_string());
                                entry.authoritative = false;
                                if entry.retries < 3 {
                                    entry.retries += 1;
                                    entry.retry_at = Some(Instant::now() + Duration::from_secs(2));
                                }
                            }
                        }
                    }
                    Wake::Completed(Ok(Completion::Control { revision, result })) => {
                        if active
                            .as_ref()
                            .is_some_and(|active| active.revision == revision)
                        {
                            let active = active.take().unwrap();
                            let _ = active.request.reply.try_send(result);
                            if let Some(entry) = entries
                                .values_mut()
                                .find(|entry| entry.data.id == active.id)
                            {
                                entry.invalidate()?;
                            }
                        }
                    }
                    Wake::Request(Ok(request)) => {
                        if request.cancelled.load(Ordering::Acquire)
                            || request.deadline <= Instant::now()
                        {
                            let _ = request
                                .reply
                                .try_send(Err(unavailable("Media request deadline expired")));
                            continue;
                        }
                        let id = target(&request.command);
                        let Some(entry) = entries
                            .values()
                            .find(|entry| entry.data.id == id && entry.authoritative)
                        else {
                            let _ = request.reply.try_send(Err(stale()));
                            continue;
                        };
                        if let Err(error) = lucent_usecases::media::validate_media_command(
                            &catalog(&entries),
                            &request.command,
                        ) {
                            let _ = request.reply.try_send(Err(error));
                            continue;
                        }
                        command_revision = command_revision
                            .checked_add(1)
                            .ok_or_else(|| unavailable("Media request identities exhausted"))?;
                        let revision = command_revision;
                        let owner = entry.owner.clone();
                        let track = entry.track.clone();
                        let connection = connection.clone();
                        let done = done.clone();
                        let command = request.command;
                        let deadline = request.deadline;
                        let task = executor.spawn(async move {
                            let result = bounded(
                                control(&connection, &owner, &command, track.as_ref(), deadline),
                                deadline,
                            )
                            .await;
                            let _ = done.send(Completion::Control { revision, result }).await;
                        });
                        active = Some(Active {
                            id,
                            revision,
                            request,
                            _task: task,
                        });
                    }
                    Wake::Owner(None)
                    | Wake::Properties(None)
                    | Wake::Seeked(None)
                    | Wake::Completed(Err(_))
                    | Wake::Request(Err(_)) => return Err(unavailable("Media observation ended")),
                }
            }
            Ok(())
        })
        .await;
    adapter.offline();
    drop(active);
    drop(entries);
    result
}
