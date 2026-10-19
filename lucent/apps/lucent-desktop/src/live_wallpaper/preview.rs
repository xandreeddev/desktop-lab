//! Transient, read-only playback for one selected library card and at most two
//! adjacent stills. One subscription serializes every decoder call and retains
//! retiring ownership until the adapter returns. No import, Apply or persistence.
use lucent_api::{Cancellation, ImageData, ImageStream, Subscription};
use lucent_domain::{
    DomainError, MediaFrame, Result, StopSignal, Wallpaper, WallpaperPlaybackPort,
    WallpaperPreviewOptions,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CACHE_ENTRIES: usize = 5;

#[derive(Clone)]
pub enum Message {
    Wake(u64),
    /// All serial decoder calls returned (or fully unwound).
    Ended(u64, Result<()>),
    /// Runtime admission failed, or its subscription body fully unwound.
    WorkerFailed(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Loading,
    Ready,
    Failed,
}
#[derive(Clone, PartialEq, Eq)]
struct Request {
    source: Wallpaper,
    options: WallpaperPreviewOptions,
    neighbors: Vec<Wallpaper>,
}
impl Request {
    fn contains(&self, path: &str) -> bool {
        self.source.path == path || self.neighbors.iter().any(|source| source.path == path)
    }
}
enum Delivery {
    Still(Result<Arc<ImageData>>),
    Frame(Arc<ImageData>),
}
#[derive(Default)]
struct Mailbox {
    // At most selected + two neighbors, and one pending wake for the whole batch.
    latest: BTreeMap<String, Delivery>,
    notified: bool,
}
impl Mailbox {
    fn take(&mut self) -> BTreeMap<String, Delivery> {
        self.notified = false;
        std::mem::take(&mut self.latest)
    }
}
struct Active {
    generation: u64,
    request: Request,
    cancel: Cancellation,
    mailbox: Arc<Mutex<Mailbox>>,
}
struct Cached {
    options: WallpaperPreviewOptions,
    image: Result<Arc<ImageData>>,
}
/// UI cancellation wakes immediately; bounded wait slices also observe runtime
/// shutdown. Slicing happens only inside active adapter waits, never an idle job.
struct Stop {
    ui: Cancellation,
    runtime: Cancellation,
}
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.ui.cancelled() || self.runtime.cancelled()
    }
    fn wait(&self, duration: Duration) {
        let started = Instant::now();
        while !self.cancelled() {
            let remaining = duration.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return;
            }
            self.ui.sleep(remaining.min(Duration::from_millis(20)));
        }
    }
}
pub struct Preview {
    adapter: Arc<dyn WallpaperPlaybackPort>,
    desired: Option<Request>,
    active: Option<Active>,
    next: u64,
    frame: Option<Arc<ImageData>>,
    cache: BTreeMap<String, Cached>,
    order: VecDeque<String>,
    error: String,
}
impl Preview {
    pub fn new(adapter: Arc<dyn WallpaperPlaybackPort>) -> Self {
        Self {
            adapter,
            desired: None,
            active: None,
            next: 0,
            frame: None,
            cache: BTreeMap::new(),
            order: VecDeque::new(),
            error: String::new(),
        }
    }
    /// Retain identical demand and failures. Adjacent sources are deduplicated
    /// by path and capped before capture; only the selected source animates.
    pub fn select(
        &mut self,
        source: Wallpaper,
        options: WallpaperPreviewOptions,
        neighbors: &[Wallpaper],
    ) -> bool {
        let mut seen = BTreeSet::from([source.path.clone()]);
        let neighbors = neighbors
            .iter()
            .filter(|source| seen.insert(source.path.clone()))
            .take(2)
            .cloned()
            .collect();
        let request = Request {
            source,
            options,
            neighbors,
        };
        if self.desired.as_ref() == Some(&request) {
            return false;
        }
        self.freeze_selected();
        self.desired = Some(request);
        self.retire();
        self.restore_selected();
        self.start();
        true
    }
    pub fn clear(&mut self) -> bool {
        let changed = self.desired.take().is_some()
            || self.frame.is_some()
            || !self.cache.is_empty()
            || !self.error.is_empty();
        self.retire();
        self.cache.clear();
        self.order.clear();
        changed
    }
    pub fn retry(&mut self) -> bool {
        let Some(request) = &self.desired else {
            return false;
        };
        let paths: Vec<_> = std::iter::once(&request.source)
            .chain(&request.neighbors)
            .filter(|source| {
                self.cache
                    .get(&source.path)
                    .is_some_and(|cached| cached.image.is_err())
            })
            .map(|source| source.path.clone())
            .collect();
        for path in paths {
            self.cache.remove(&path);
            self.order.retain(|entry| entry != &path);
        }
        self.retire();
        self.restore_selected();
        self.start();
        true
    }
    fn retire(&mut self) {
        self.frame = None;
        self.error.clear();
        if let Some(active) = &self.active {
            active.cancel.cancel();
            active
                .mailbox
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
        }
    }
    fn cached(
        &self,
        path: &str,
        options: WallpaperPreviewOptions,
    ) -> Option<&Result<Arc<ImageData>>> {
        self.cache
            .get(path)
            .filter(|cached| cached.options == options)
            .map(|cached| &cached.image)
    }
    fn insert(
        &mut self,
        path: String,
        options: WallpaperPreviewOptions,
        image: Result<Arc<ImageData>>,
    ) {
        self.order.retain(|entry| entry != &path);
        self.order.push_back(path.clone());
        self.cache.insert(path, Cached { options, image });
        while self.cache.len() > CACHE_ENTRIES {
            let index = self
                .order
                .iter()
                .position(|path| {
                    self.desired
                        .as_ref()
                        .is_none_or(|request| !request.contains(path))
                })
                .unwrap_or(0);
            if let Some(path) = self.order.remove(index) {
                self.cache.remove(&path);
            }
        }
    }
    fn freeze_selected(&mut self) {
        if let Some(request) = &self.desired
            && let Some(frame) = &self.frame
        {
            self.insert(
                request.source.path.clone(),
                request.options,
                Ok(Arc::new(frame.snapshot())),
            );
        }
    }
    fn restore_selected(&mut self) {
        if let Some(request) = &self.desired
            && let Some(cached) = self.cached(&request.source.path, request.options)
        {
            match cached {
                Ok(image) => self.frame = Some(image.clone()),
                Err(error) => self.error = error.to_string(),
            }
        }
    }
    fn start(&mut self) {
        if self.active.is_some() || !self.error.is_empty() {
            return;
        }
        let Some(request) = self.desired.clone() else {
            return;
        };
        let Some(generation) = self.next.checked_add(1) else {
            self.error = "Preview request limit reached; restart the desktop".into();
            return;
        };
        self.next = generation;
        self.active = Some(Active {
            generation,
            request,
            cancel: Cancellation::default(),
            mailbox: Arc::new(Mutex::new(Mailbox::default())),
        });
    }
    pub fn frame(&self) -> Option<&Arc<ImageData>> {
        self.frame.as_ref()
    }
    /// Read-only pixels for carousel cards; no decoder or scheduling in views.
    pub fn frame_for(&self, path: &str) -> Option<&Arc<ImageData>> {
        let request = self.desired.as_ref()?;
        if request.source.path == path {
            return self.frame();
        }
        self.cached(path, request.options)?.as_ref().ok()
    }
    pub fn error(&self) -> &str {
        &self.error
    }
    pub fn status(&self) -> Status {
        if self.desired.is_none() {
            Status::Idle
        } else if !self.error.is_empty() {
            Status::Failed
        } else if self.frame.is_some() {
            Status::Ready
        } else {
            Status::Loading
        }
    }
    /// Distinct prepared allocations only, including undelivered frames and the
    /// five-entry still cache. Excludes in-flight conversion/scenes/driver/GPU.
    pub fn retained_bytes(&self) -> usize {
        let queued = self
            .active
            .as_ref()
            .map(|active| {
                active
                    .mailbox
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .latest
                    .values()
                    .filter_map(|delivery| match delivery {
                        Delivery::Still(Ok(image)) | Delivery::Frame(image) => Some(image.clone()),
                        Delivery::Still(Err(_)) => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        crate::previews::usage(
            self.frame
                .iter()
                .chain(
                    self.cache
                        .values()
                        .filter_map(|cached| cached.image.as_ref().ok()),
                )
                .chain(queued.iter()),
        )
        .bytes
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({ "selected":self.desired.is_some(),"active":self.active.is_some(),
            "retiring":self.active.as_ref().is_some_and(|active|active.cancel.cancelled()),
            "loading":self.status()==Status::Loading,"failed":self.status()==Status::Failed,
            "frame_size":self.frame.as_ref().map(|image|(image.width(),image.height())),
            "target":self.desired.as_ref().map(|request|request.options.size()),
            "cached":self.cache.len(),"retained_bytes":self.retained_bytes() })
    }
    fn drain(&mut self, generation: u64) -> bool {
        let Some(active) = self
            .active
            .as_ref()
            .filter(|active| active.generation == generation)
        else {
            return false;
        };
        let latest = active
            .mailbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if active.cancel.cancelled() || self.desired.as_ref() != Some(&active.request) {
            return false;
        }
        let options = active.request.options;
        let selected = active.request.source.path.clone();
        let changed = !latest.is_empty();
        for (path, delivery) in latest {
            match delivery {
                Delivery::Frame(image) if path == selected => self.frame = Some(image),
                Delivery::Frame(_) => (),
                Delivery::Still(image) => {
                    if path == selected
                        && let Ok(image) = &image
                    {
                        self.frame = Some(image.clone());
                    }
                    self.insert(path, options, image);
                }
            }
        }
        changed
    }
    pub fn update(&mut self, message: Message) -> bool {
        match message {
            Message::Wake(generation) => self.drain(generation),
            Message::Ended(generation, result) => self.ended(generation, result),
            Message::WorkerFailed(generation) => self.ended(
                generation,
                Err(DomainError::Unavailable(
                    "Wallpaper preview worker failed".into(),
                )),
            ),
        }
    }
    fn ended(&mut self, generation: u64, result: Result<()>) -> bool {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.generation != generation)
        {
            return false;
        }
        self.drain(generation);
        let active = self.active.take().unwrap();
        let current = !active.cancel.cancelled() && self.desired.as_ref() == Some(&active.request);
        if current {
            let failure = match result {
                Err(error) => Some(error),
                Ok(()) if self.frame.is_none() => Some(DomainError::Unavailable(
                    "No preview frames were produced".into(),
                )),
                Ok(()) => None,
            };
            if let Some(error) = failure {
                self.error = error.to_string();
                self.insert(
                    active.request.source.path,
                    active.request.options,
                    Err(error),
                );
            }
        } else {
            self.restore_selected();
            self.start();
        }
        true
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let Some(active) = &self.active else {
            return vec![];
        };
        let generation = active.generation;
        let request = active.request.clone();
        let warm: Vec<_> = if request.neighbors.is_empty() {
            vec![]
        } else {
            std::iter::once(&request.source)
                .chain(&request.neighbors)
                .filter(|source| self.cached(&source.path, request.options).is_none())
                .cloned()
                .collect()
        };
        let own_cancel = active.cancel.clone();
        let mailbox = active.mailbox.clone();
        let adapter = self.adapter.clone();
        vec![Subscription::stream(
            format!("live-wallpaper-preview:{generation}"),
            move |out, runtime_cancel| {
                let stop = Arc::new(Stop {
                    ui: own_cancel,
                    runtime: runtime_cancel,
                });
                let stream = ImageStream::new();
                let publish = |path: &str, delivery: Delivery| {
                    let notify = {
                        let mut mailbox = mailbox.lock().unwrap_or_else(|p| p.into_inner());
                        if stop.cancelled() {
                            return;
                        }
                        mailbox.latest.insert(path.into(), delivery);
                        let notify = !mailbox.notified;
                        mailbox.notified = true;
                        notify
                    };
                    if notify {
                        out.send(Message::Wake(generation));
                    }
                };
                let prepare = |frame: MediaFrame| {
                    let (width, height) = frame.dimensions();
                    let (max_width, max_height) = request.options.size();
                    if width > max_width || height > max_height {
                        return Err(DomainError::Invalid(
                            "Preview frame exceeds the requested dimensions".into(),
                        ));
                    }
                    let (width, height, pixels, aspect) = frame.into_rgba_with_aspect();
                    stream
                        .frame_rgba_with_aspect(width, height, pixels, aspect)
                        .map(Arc::new)
                        .map_err(|error| DomainError::Failed(error.to_string()))
                };
                let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
                    for source in warm {
                        if stop.cancelled() {
                            return Ok(());
                        }
                        let mut received = None;
                        let decoded = catch_unwind(AssertUnwindSafe(|| {
                            adapter.preview(
                                &source,
                                request.options,
                                &mut |result| {
                                    if stop.cancelled() || received.is_some() {
                                        return false;
                                    }
                                    received = Some(
                                        result
                                            .and_then(prepare)
                                            .map(|image| Arc::new(image.snapshot())),
                                    );
                                    false // finite first-frame call, completely returned before the next
                                },
                                stop.clone(),
                            )
                        }));
                        if stop.cancelled() {
                            return Ok(());
                        }
                        let image = if decoded.is_err() {
                            Err(DomainError::Failed(
                                "Wallpaper preview decoder failed".into(),
                            ))
                        } else {
                            received.unwrap_or_else(|| {
                                Err(DomainError::Unavailable(
                                    "No preview frames were produced".into(),
                                ))
                            })
                        };
                        let selected_failure = if source.path == request.source.path {
                            image.as_ref().err().cloned()
                        } else {
                            None
                        };
                        publish(&source.path, Delivery::Still(image));
                        if let Some(error) = selected_failure {
                            return Err(error);
                        }
                    }
                    if stop.cancelled() {
                        return Ok(());
                    }
                    let mut failure = None;
                    let mut emitted = false;
                    adapter.preview(
                        &request.source,
                        request.options,
                        &mut |result| {
                            if stop.cancelled() || failure.is_some() {
                                return false;
                            }
                            match result.and_then(prepare) {
                                Ok(image) => {
                                    emitted = true;
                                    publish(&request.source.path, Delivery::Frame(image));
                                    !stop.cancelled()
                                }
                                Err(error) => {
                                    failure = Some(error);
                                    false
                                }
                            }
                        },
                        stop.clone(),
                    );
                    if let Some(error) = failure {
                        Err(error)
                    } else if !emitted && !stop.cancelled() {
                        Err(DomainError::Unavailable(
                            "No preview frames were produced".into(),
                        ))
                    } else {
                        Ok(())
                    }
                }));
                let result = result.unwrap_or_else(|_| {
                    Err(DomainError::Failed(
                        "Wallpaper preview decoder failed".into(),
                    ))
                });
                // Own cancellation keeps this identity alive so ACK remains
                // deliverable. Runtime shutdown may suppress it: the client is gone.
                out.send(Message::Ended(generation, result));
            },
        )]
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::Emitter;
    use lucent_domain::{LiveWallpaperAsset, MediaFrame, PlaybackOptions};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    };

    fn source(name: &str) -> Wallpaper {
        Wallpaper {
            path: format!("/owned-fixture/{name}.mp4"),
            name: name.into(),
        }
    }
    fn options() -> WallpaperPreviewOptions {
        WallpaperPreviewOptions::new(32, 24).unwrap()
    }
    fn frame(marker: u8, width: u32, height: u32) -> Result<MediaFrame> {
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[marker, 0, 0, 255]);
        }
        MediaFrame::rgba_with_aspect(width, height, pixels, (16, 9), Duration::ZERO)
    }
    enum Command {
        Frame(u8),
        Fail,
        Return,
    }
    struct Call {
        source: Wallpaper,
        stop: Arc<dyn StopSignal>,
        commands: Sender<Command>,
    }
    struct Controlled {
        calls: Sender<Call>,
        active: AtomicUsize,
        maximum: AtomicUsize,
    }
    struct DecoderGuard<'a>(&'a AtomicUsize);
    impl Drop for DecoderGuard<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl WallpaperPlaybackPort for Controlled {
        fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
            panic!("preview must never import/apply a source")
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            _: PlaybackOptions,
            _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            panic!("preview must never use applied-background playback")
        }
        fn preview(
            &self,
            source: &Wallpaper,
            _: WallpaperPreviewOptions,
            emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            stop: Arc<dyn StopSignal>,
        ) {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            let _guard = DecoderGuard(&self.active);
            let (commands, incoming) = mpsc::channel();
            self.calls
                .send(Call {
                    source: source.clone(),
                    stop,
                    commands,
                })
                .unwrap();
            // Deliberately delay returning after cancellation. This models an
            // adapter still unwinding native work and proves ACK, not selection,
            // controls admission of its successor. No daemon/files are touched.
            while let Ok(command) = incoming.recv_timeout(Duration::from_secs(2)) {
                match command {
                    Command::Frame(marker) if emit(frame(marker, 1, 1)) => (),
                    Command::Fail => {
                        emit(Err(DomainError::Unavailable(
                            "owned neighbor failure".into(),
                        )));
                        break;
                    }
                    Command::Frame(_) | Command::Return => break,
                }
            }
        }
    }
    fn run(preview: &Preview) -> (Receiver<Message>, std::thread::JoinHandle<()>) {
        let subscription = preview.subscriptions().pop().unwrap();
        let (send, receive) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            (subscription.run)(
                Emitter::new(move |message| {
                    let _ = send.send(message);
                }),
                Cancellation::default(),
            );
        });
        (receive, handle)
    }
    fn receive(messages: &Receiver<Message>) -> Message {
        messages
            .recv_timeout(Duration::from_secs(2))
            .expect("owned preview did not respond")
    }
    fn call(calls: &Receiver<Call>) -> Call {
        calls
            .recv_timeout(Duration::from_secs(2))
            .expect("owned decoder did not start")
    }

    #[test]
    fn latest_selection_waits_for_decoder_ack_including_close_and_reopen() {
        let (send_calls, calls) = mpsc::channel();
        let adapter = Arc::new(Controlled {
            calls: send_calls,
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        });
        let mut preview = Preview::new(adapter.clone());
        assert_eq!(preview.status(), Status::Idle);
        assert!(preview.subscriptions().is_empty());
        preview.select(source("A"), options(), &[]);
        let a_identity = preview.subscriptions()[0].id.clone();
        let a_generation = preview.active.as_ref().unwrap().generation;
        let (a_messages, a_thread) = run(&preview);
        let a = call(&calls);
        a.commands.send(Command::Frame(10)).unwrap();
        assert!(preview.update(receive(&a_messages)));
        assert_eq!(preview.frame().unwrap().pixels()[0], 10);

        preview.select(source("B"), options(), &[]);
        preview.select(source("C"), options(), &[]);
        assert!(a.stop.cancelled());
        assert!(preview.frame().is_none());
        assert_eq!(preview.status(), Status::Loading);
        assert_eq!(preview.subscriptions()[0].id, a_identity);
        assert!(calls.try_recv().is_err());
        assert_eq!(adapter.active.load(Ordering::SeqCst), 1);
        a.commands.send(Command::Return).unwrap();
        let ended = receive(&a_messages);
        assert!(matches!(ended, Message::Ended(_, Ok(()))));
        assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
        preview.update(ended);
        a_thread.join().unwrap();
        let (c_messages, c_thread) = run(&preview);
        let c = call(&calls);
        assert_eq!(c.source, source("C"), "intermediate B must not decode");

        let c_identity = preview.subscriptions()[0].id.clone();
        preview.clear();
        assert_eq!(preview.status(), Status::Idle);
        assert_eq!(preview.retained_bytes(), 0);
        assert!(c.stop.cancelled());
        preview.select(source("D"), options(), &[]);
        assert_eq!(preview.subscriptions()[0].id, c_identity);
        assert!(calls.try_recv().is_err());
        c.commands.send(Command::Frame(99)).unwrap();
        preview.update(receive(&c_messages));
        c_thread.join().unwrap();
        assert!(
            preview.frame().is_none(),
            "cancelled C cannot flash after reopening"
        );
        let (d_messages, d_thread) = run(&preview);
        let d = call(&calls);
        assert_eq!(d.source, source("D"));
        assert!(!preview.update(Message::Wake(a_generation)));
        assert!(!preview.update(Message::Ended(a_generation, Ok(()))));
        d.commands.send(Command::Frame(42)).unwrap();
        preview.update(receive(&d_messages));
        assert_eq!(preview.frame().unwrap().pixels()[0], 42);
        assert_eq!(preview.status(), Status::Ready);
        assert_eq!(preview.frame().unwrap().aspect_size(), (16, 9));
        preview.clear();
        assert!(
            !preview.subscriptions().is_empty(),
            "closed body still needs its ACK"
        );
        d.commands.send(Command::Return).unwrap();
        preview.update(receive(&d_messages));
        d_thread.join().unwrap();
        assert!(preview.subscriptions().is_empty());
        assert_eq!(preview.retained_bytes(), 0);
        assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
        assert_eq!(adapter.maximum.load(Ordering::SeqCst), 1);
    }

    #[derive(Clone, Copy)]
    enum Fixture {
        Burst,
        NeighborPanic,
        Failure,
        Oversized,
        Empty,
        Panic,
    }
    impl WallpaperPlaybackPort for Fixture {
        fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
            panic!("preview must not import")
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            _: PlaybackOptions,
            _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            panic!("preview must not apply")
        }
        fn preview(
            &self,
            source: &Wallpaper,
            _: WallpaperPreviewOptions,
            emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            match self {
                Self::NeighborPanic if source.name == "bad" => {
                    panic!("private neighbor fixture payload")
                }
                Self::Burst | Self::NeighborPanic => {
                    for marker in 0..1000 {
                        if !emit(frame(marker as u8, 1, 1)) {
                            break;
                        }
                    }
                }
                Self::Failure => {
                    assert!(!emit(Err(DomainError::Unavailable(
                        "fixture unavailable".into()
                    ))));
                }
                Self::Oversized => {
                    assert!(!emit(frame(7, 33, 1)));
                }
                Self::Empty => (),
                Self::Panic => panic!("private fixture payload must not reach UI"),
            }
        }
    }
    fn completed(fixture: Fixture) -> (Preview, Vec<Message>) {
        let mut preview = Preview::new(Arc::new(fixture));
        preview.select(source("finite"), options(), &[]);
        let (send, receive) = mpsc::channel();
        (preview.subscriptions().pop().unwrap().run)(
            Emitter::new(move |message| {
                send.send(message).unwrap();
            }),
            Cancellation::default(),
        );
        (preview, receive.try_iter().collect())
    }

    #[test]
    fn burst_retains_one_latest_frame_one_wake_and_one_gpu_stream_identity() {
        let (mut preview, mut messages) = completed(Fixture::Burst);
        assert_eq!(
            messages.len(),
            2,
            "pixels never accumulate in runtime events"
        );
        assert!(matches!(messages[0], Message::Wake(_)));
        assert!(matches!(messages[1], Message::Ended(_, Ok(()))));
        assert_eq!(preview.retained_bytes(), 4);
        // Terminal delivery also drains the latest frame if Wake has not yet
        // been dispatched. Its late duplicate must leave the frame untouched.
        preview.update(messages.pop().unwrap());
        assert_eq!(preview.frame().unwrap().pixels()[0], 999_u16 as u8);
        assert_eq!(preview.frame().unwrap().stream_revision().unwrap().1, 1000);
        assert_eq!(preview.retained_bytes(), 4);
        assert!(!preview.update(messages.pop().unwrap()));
        assert_eq!(preview.status(), Status::Ready);
        assert!(preview.subscriptions().is_empty());
        assert!(!preview.select(source("finite"), options(), &[]));
        assert!(
            preview.subscriptions().is_empty(),
            "a view declaration must not replay finite work"
        );
    }

    #[test]
    fn failures_bounds_empty_decode_and_panics_end_without_automatic_retry() {
        for fixture in [
            Fixture::Failure,
            Fixture::Oversized,
            Fixture::Empty,
            Fixture::Panic,
        ] {
            let (mut preview, messages) = completed(fixture);
            assert_eq!(messages.len(), 1);
            assert!(matches!(messages[0], Message::Ended(_, _)));
            for message in messages {
                preview.update(message);
            }
            assert_eq!(preview.status(), Status::Failed);
            assert!(!preview.error().is_empty());
            assert!(!preview.error().contains("private fixture payload"));
            assert!(preview.frame().is_none());
            assert_eq!(preview.retained_bytes(), 0);
            assert!(preview.subscriptions().is_empty());
            assert!(!preview.select(source("finite"), options(), &[]));
            assert!(preview.subscriptions().is_empty());
            assert!(preview.retry());
            assert_eq!(preview.status(), Status::Loading);
            let generation = preview.active.as_ref().unwrap().generation;
            assert!(!preview.update(Message::WorkerFailed(generation - 1)));
            assert!(preview.update(Message::WorkerFailed(generation)));
            assert_eq!(preview.status(), Status::Failed);
            assert!(preview.subscriptions().is_empty());
            preview.clear();
            assert_eq!(preview.status(), Status::Idle);
            assert!(!preview.retry());
        }
    }

    #[test]
    fn extent_changes_retire_instead_of_retargeting_old_decoders() {
        let mut preview = Preview::new(Arc::new(Fixture::Empty));
        preview.select(source("same"), options(), &[]);
        let old = preview.active.as_ref().unwrap().generation;
        preview.select(
            source("same"),
            WallpaperPreviewOptions::new(64, 48).unwrap(),
            &[],
        );
        assert_eq!(preview.active.as_ref().unwrap().generation, old);
        assert!(preview.active.as_ref().unwrap().cancel.cancelled());
        preview.update(Message::Ended(old, Ok(())));
        let active = preview.active.as_ref().unwrap();
        assert!(active.generation > old);
        assert_eq!(active.request.options.size(), (64, 48));
        preview.next = u64::MAX;
        preview.clear();
        let retiring = preview.active.as_ref().unwrap().generation;
        preview.select(source("new"), options(), &[]);
        preview.update(Message::Ended(retiring, Ok(())));
        assert_eq!(preview.status(), Status::Failed);
        assert!(preview.subscriptions().is_empty());
        assert!(preview.error().contains("limit"));
    }

    #[test]
    fn both_ui_and_runtime_cancellation_interrupt_adapter_waits() {
        for runtime in [false, true] {
            let ui = Cancellation::default();
            let native = Cancellation::default();
            let stop = Arc::new(Stop {
                ui: ui.clone(),
                runtime: native.clone(),
            });
            let (started, ready) = mpsc::channel();
            let (finished, done) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                started.send(()).unwrap();
                stop.wait(Duration::from_secs(30));
                finished.send(stop.cancelled()).unwrap();
            });
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            if runtime {
                native.cancel();
            } else {
                ui.cancel();
            }
            assert!(done.recv_timeout(Duration::from_secs(2)).unwrap());
            worker.join().unwrap();
        }
    }

    #[test]
    fn cancelled_before_admission_acknowledges_without_entering_adapter() {
        let mut preview = Preview::new(Arc::new(Fixture::Panic));
        preview.select(source("not-started"), options(), &[]);
        preview.clear();
        let (send, receive) = mpsc::channel();
        (preview.subscriptions().pop().unwrap().run)(
            Emitter::new(move |message| {
                send.send(message).unwrap();
            }),
            Cancellation::default(),
        );
        let messages: Vec<_> = receive.try_iter().collect();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::Ended(_, Ok(()))));
        for message in messages {
            preview.update(message);
        }
        assert_eq!(preview.status(), Status::Idle);
        assert!(preview.subscriptions().is_empty());
        assert_eq!(preview.inspect()["retained_bytes"], 0);
    }

    #[test]
    fn selected_first_neighbors_are_serial_bounded_and_failures_do_not_stop_animation() {
        let (send_calls, calls) = mpsc::channel();
        let adapter = Arc::new(Controlled {
            calls: send_calls,
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        });
        let mut preview = Preview::new(adapter.clone());
        let neighbors = [
            source("A"),
            source("B"),
            source("B"),
            source("C"),
            source("never"),
        ];
        preview.select(source("A"), options(), &neighbors);
        assert_eq!(preview.desired.as_ref().unwrap().neighbors.len(), 2);
        let (messages, worker) = run(&preview);
        let first = call(&calls);
        assert_eq!(first.source, source("A"));
        first.commands.send(Command::Frame(1)).unwrap();
        let b = call(&calls);
        preview.update(receive(&messages));
        assert!(preview.frame_for(&source("A").path).is_some());
        assert!(
            preview.frame().unwrap().stream_revision().is_none(),
            "warmup pixels are frozen stills"
        );
        assert_eq!(b.source, source("B"));
        b.commands.send(Command::Frame(2)).unwrap();
        let c = call(&calls);
        preview.update(receive(&messages));
        assert_eq!(preview.frame_for(&source("B").path).unwrap().pixels()[0], 2);
        assert_eq!(c.source, source("C"));
        c.commands.send(Command::Fail).unwrap();
        let selected = call(&calls);
        preview.update(receive(&messages));
        assert_eq!(selected.source, source("A"));
        assert!(
            preview.error().is_empty(),
            "neighbor failure must not become selected failure"
        );
        assert!(preview.frame_for(&source("C").path).is_none());
        selected.commands.send(Command::Frame(4)).unwrap();
        preview.update(receive(&messages));
        assert_eq!(preview.frame().unwrap().pixels()[0], 4);
        assert!(preview.frame().unwrap().stream_revision().is_some());
        assert_eq!(preview.status(), Status::Ready);
        assert_eq!(
            preview.retained_bytes(),
            12,
            "selected still + neighbor still + current frame"
        );

        assert!(preview.retry());
        assert!(selected.stop.cancelled());
        selected.commands.send(Command::Return).unwrap();
        preview.update(receive(&messages));
        worker.join().unwrap();
        let (retry_messages, retry_worker) = run(&preview);
        let retry_neighbor = call(&calls);
        assert_eq!(
            retry_neighbor.source,
            source("C"),
            "cached A/B are reused; explicit Retry warms failed C"
        );
        preview.clear();
        assert_eq!(preview.retained_bytes(), 0);
        retry_neighbor.commands.send(Command::Return).unwrap();
        preview.update(receive(&retry_messages));
        retry_worker.join().unwrap();
        assert_eq!(preview.status(), Status::Idle);
        assert!(preview.subscriptions().is_empty());
        assert_eq!(adapter.maximum.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn still_cache_reuses_frozen_pixels_caps_entries_and_closes_to_zero() {
        let mut preview = Preview::new(Arc::new(Fixture::Burst));
        for index in 0..8 {
            preview.select(source(&format!("clip{index}")), options(), &[]);
            let (send, receive) = mpsc::channel();
            (preview.subscriptions().pop().unwrap().run)(
                Emitter::new(move |message| {
                    send.send(message).unwrap();
                }),
                Cancellation::default(),
            );
            for message in receive.try_iter() {
                preview.update(message);
            }
            assert_eq!(preview.status(), Status::Ready);
            assert!(preview.cache.len() <= CACHE_ENTRIES);
            assert!(preview.retained_bytes() <= (CACHE_ENTRIES + 1) * 4);
        }
        assert_eq!(preview.cache.len(), CACHE_ENTRIES);
        assert!(preview.frame_for(&source("clip0").path).is_none());
        assert!(
            preview.cache.values().all(|cached| cached
                .image
                .as_ref()
                .unwrap()
                .stream_revision()
                .is_none())
        );
        let cached = preview.frame_for(&source("clip6").path).unwrap().clone();
        preview.select(source("clip6"), options(), &[]);
        assert!(
            Arc::ptr_eq(preview.frame().unwrap(), &cached),
            "selection reuses existing immutable pixels before decoding"
        );
        assert!(
            preview.retained_bytes() <= CACHE_ENTRIES * 4,
            "cache and selected view share allocations"
        );
        preview.clear();
        assert_eq!(preview.retained_bytes(), 0);
        let generation = preview.active.as_ref().unwrap().generation;
        preview.update(Message::Ended(generation, Ok(())));
        assert!(preview.subscriptions().is_empty());
        assert!(preview.cache.is_empty());
    }

    #[test]
    fn neighbor_panic_is_contained_after_unwind_and_selected_playback_continues() {
        let mut preview = Preview::new(Arc::new(Fixture::NeighborPanic));
        preview.select(source("hero"), options(), &[source("bad"), source("good")]);
        let (send, receive) = mpsc::channel();
        (preview.subscriptions().pop().unwrap().run)(
            Emitter::new(move |message| {
                send.send(message).unwrap();
            }),
            Cancellation::default(),
        );
        for message in receive.try_iter() {
            preview.update(message);
        }
        assert_eq!(preview.status(), Status::Ready);
        assert!(preview.error().is_empty());
        assert!(preview.frame_for(&source("bad").path).is_none());
        assert!(preview.frame_for(&source("good").path).is_some());
        assert_eq!(preview.frame().unwrap().pixels()[0], 999_u16 as u8);
        assert!(preview.subscriptions().is_empty());
    }
}
