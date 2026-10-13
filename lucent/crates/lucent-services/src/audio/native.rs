//! Audio handle and callback FFI ownership. All handles/callbacks are confined to one worker.
//! Pending owns its callback allocation, cancels before freeing it, and retains
//! the connection. No Rust closure ownership is delegated to the C library.
use super::{
    Notices,
    snapshot::{self, Data, Key, Kind, Objects, Record},
    unavailable,
};
use lucent_domain::*;
use pulse::*;
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_void},
    io::Read,
    os::{fd::AsRawFd, unix::net::UnixStream},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{self, NonNull},
    rc::Rc,
};

struct Callback<T> {
    value: T,
    failed: Cell<bool>,
}
impl<T> Callback<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            failed: Cell::new(false),
        }
    }
    fn call(&self, f: impl FnOnce(&T)) {
        if catch_unwind(AssertUnwindSafe(|| f(&self.value))).is_err() {
            self.failed.set(true);
        }
    }
    fn get(&self) -> Result<&T> {
        if self.failed.get() {
            Err(unavailable("Audio callback failed"))
        } else {
            Ok(&self.value)
        }
    }
}
struct Inner {
    context: NonNull<pa_context>,
    mainloop: NonNull<pa_mainloop>,
    io: NonNull<pa_io_event>,
    _reader: Box<Callback<UnixStream>>,
    notices: Box<Callback<RefCell<Notices>>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        // SAFETY: Inner is Rc-owned on one thread. Pending keeps it alive until
        // every operation is cancelled/unref'd. Callback boxes still live here.
        unsafe {
            pa_context_set_subscribe_callback(self.context.as_ptr(), None, ptr::null_mut());
            pa_context_disconnect(self.context.as_ptr());
            pa_context_unref(self.context.as_ptr());
            let api = pa_mainloop_get_api(self.mainloop.as_ptr());
            ((*api).io_free.unwrap())(self.io.as_ptr());
            pa_mainloop_free(self.mainloop.as_ptr());
        }
    }
}
#[derive(Clone)]
pub(super) struct Connection(Rc<Inner>);
pub(super) struct Pending<T> {
    operation: NonNull<pa_operation>,
    state: Box<Callback<T>>,
    _connection: Connection,
}
impl<T> Pending<T> {
    pub fn state(&self) -> Result<&T> {
        self.state.get()
    }
}
impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        // SAFETY: cancels future callback invocations before the box is dropped;
        // cancellation does not promise to undo a server-side operation.
        unsafe {
            pa_operation_cancel(self.operation.as_ptr());
            pa_operation_unref(self.operation.as_ptr());
        }
    }
}
impl Connection {
    pub fn new(server: Option<&str>, reader: UnixStream) -> Result<Self> {
        let server = server
            .map(CString::new)
            .transpose()
            .map_err(|_| unavailable("Invalid audio server"))?;
        let reader = Box::new(Callback::new(reader));
        let notices = Box::new(Callback::new(RefCell::new(Notices {
            dirty: true,
            ..Default::default()
        })));
        // SAFETY: C allocations checked for null, paired cleanup on each early
        // error. Callback boxes have stable addresses and outlive registrations.
        unsafe {
            let mainloop = NonNull::new(pa_mainloop_new())
                .ok_or_else(|| unavailable("Audio event loop allocation failed"))?;
            let api = pa_mainloop_get_api(mainloop.as_ptr());
            let context = match NonNull::new(pa_context_new(api, c"Lucent audio".as_ptr())) {
                Some(c) => c,
                None => {
                    pa_mainloop_free(mainloop.as_ptr());
                    return Err(unavailable("Audio context allocation failed"));
                }
            };
            let io = ((*api).io_new.unwrap())(
                api as *mut _,
                reader.value.as_raw_fd(),
                PA_IO_EVENT_INPUT,
                Some(wake),
                (&*reader as *const Callback<UnixStream>).cast_mut().cast(),
            );
            let Some(io) = NonNull::new(io) else {
                pa_context_unref(context.as_ptr());
                pa_mainloop_free(mainloop.as_ptr());
                return Err(unavailable("Audio wake registration failed"));
            };
            let connection = Self(Rc::new(Inner {
                context,
                mainloop,
                io,
                _reader: reader,
                notices,
            }));
            pa_context_set_subscribe_callback(
                context.as_ptr(),
                Some(notice),
                (&*connection.0.notices as *const Callback<RefCell<Notices>>)
                    .cast_mut()
                    .cast(),
            );
            if pa_context_connect(
                context.as_ptr(),
                server.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
                PA_CONTEXT_NOAUTOSPAWN,
                ptr::null(),
            ) < 0
            {
                return Err(connection.error("Audio connection failed"));
            }
            Ok(connection)
        }
    }
    fn error(&self, message: &str) -> DomainError {
        // The numeric backend error is sufficient and cannot expose device data.
        unavailable(format!("{message} (code {})", unsafe {
            pa_context_errno(self.0.context.as_ptr())
        }))
    }
    pub fn ready(&self) -> Result<bool> {
        match unsafe { pa_context_get_state(self.0.context.as_ptr()) } {
            PA_CONTEXT_READY => Ok(true),
            PA_CONTEXT_FAILED | PA_CONTEXT_TERMINATED => {
                Err(self.error("Audio service disconnected"))
            }
            _ => Ok(false),
        }
    }
    pub fn poll(&self) -> Result<()> {
        // Standard libpulse loop timeout is microseconds. Network and command
        // fd events wake immediately; this ceiling bounds cooperative shutdown.
        let m = self.0.mainloop.as_ptr();
        unsafe {
            if pa_mainloop_prepare(m, 250_000) < 0
                || pa_mainloop_poll(m) < 0
                || pa_mainloop_dispatch(m) < 0
            {
                return Err(self.error("Audio event dispatch failed"));
            }
        }
        self.0._reader.get()?;
        self.0.notices.get()?;
        Ok(())
    }
    pub fn notices(&self) -> Result<std::cell::RefMut<'_, Notices>> {
        Ok(self.0.notices.get()?.borrow_mut())
    }
    fn submit<T>(
        &self,
        state: T,
        start: impl FnOnce(*mut pa_context, *mut c_void) -> *mut pa_operation,
    ) -> Result<Pending<T>> {
        let state = Box::new(Callback::new(state));
        let operation = NonNull::new(start(
            self.0.context.as_ptr(),
            (&*state as *const Callback<T>).cast_mut().cast(),
        ))
        .ok_or_else(|| self.error("Audio operation could not start"))?;
        Ok(Pending {
            operation,
            state,
            _connection: self.clone(),
        })
    }
    pub fn subscribe(&self) -> Result<Pending<Cell<Option<bool>>>> {
        self.submit(Cell::new(None), |c, data| unsafe {
            pa_context_subscribe(
                c,
                PA_SUBSCRIPTION_MASK_SINK
                    | PA_SUBSCRIPTION_MASK_SOURCE
                    | PA_SUBSCRIPTION_MASK_SINK_INPUT
                    | PA_SUBSCRIPTION_MASK_SOURCE_OUTPUT
                    | PA_SUBSCRIPTION_MASK_SERVER,
                Some(success),
                data,
            )
        })
    }
    pub fn query(&self) -> Result<Batch> {
        let data = Rc::new(RefCell::new(Data {
            remaining: 6,
            ..Default::default()
        }));
        let operations = vec![
            self.submit(data.clone(), |c, d| unsafe {
                pa_context_get_sink_info_list(c, Some(sink), d)
            })?,
            self.submit(data.clone(), |c, d| unsafe {
                pa_context_get_source_info_list(c, Some(source), d)
            })?,
            self.submit(data.clone(), |c, d| unsafe {
                pa_context_get_sink_input_info_list(c, Some(playback), d)
            })?,
            self.submit(data.clone(), |c, d| unsafe {
                pa_context_get_source_output_info_list(c, Some(capture), d)
            })?,
            self.submit(data.clone(), |c, d| unsafe {
                pa_context_get_sink_info_by_name(
                    c,
                    c"@DEFAULT_SINK@".as_ptr(),
                    Some(default_sink),
                    d,
                )
            })?,
            self.submit(data.clone(), |c, d| unsafe {
                pa_context_get_source_info_by_name(
                    c,
                    c"@DEFAULT_SOURCE@".as_ptr(),
                    Some(default_source),
                    d,
                )
            })?,
        ];
        Ok(Batch { operations, data })
    }
    pub fn control(
        &self,
        objects: &Objects,
        command: AudioCommand,
    ) -> Result<Pending<Cell<Option<bool>>>> {
        use super::control::{Prepared, prepare};
        let command = prepare(objects, command)?;
        self.submit(Cell::new(None), |c, d| unsafe {
            match &command {
                Prepared::Volume(Key(Kind::Output, i), v) => {
                    pa_context_set_sink_volume_by_index(c, *i, v, Some(success), d)
                }
                Prepared::Volume(Key(Kind::Input, i), v) => {
                    pa_context_set_source_volume_by_index(c, *i, v, Some(success), d)
                }
                Prepared::Volume(Key(Kind::Playback, i), v) => {
                    pa_context_set_sink_input_volume(c, *i, v, Some(success), d)
                }
                Prepared::Volume(Key(Kind::Capture, i), v) => {
                    pa_context_set_source_output_volume(c, *i, v, Some(success), d)
                }
                Prepared::Mute(Key(Kind::Output, i), m) => {
                    pa_context_set_sink_mute_by_index(c, *i, i32::from(*m), Some(success), d)
                }
                Prepared::Mute(Key(Kind::Input, i), m) => {
                    pa_context_set_source_mute_by_index(c, *i, i32::from(*m), Some(success), d)
                }
                Prepared::Mute(Key(Kind::Playback, i), m) => {
                    pa_context_set_sink_input_mute(c, *i, i32::from(*m), Some(success), d)
                }
                Prepared::Mute(Key(Kind::Capture, i), m) => {
                    pa_context_set_source_output_mute(c, *i, i32::from(*m), Some(success), d)
                }
                Prepared::Default(AudioDirection::Playback, name) => {
                    pa_context_set_default_sink(c, name.as_ptr(), Some(success), d)
                }
                Prepared::Default(AudioDirection::Capture, name) => {
                    pa_context_set_default_source(c, name.as_ptr(), Some(success), d)
                }
                Prepared::Route(AudioDirection::Playback, stream, device) => {
                    pa_context_move_sink_input_by_index(c, *stream, *device, Some(success), d)
                }
                Prepared::Route(AudioDirection::Capture, stream, device) => {
                    pa_context_move_source_output_by_index(c, *stream, *device, Some(success), d)
                }
            }
        })
    }
}
pub(super) struct Batch {
    operations: Vec<Pending<Rc<RefCell<Data>>>>,
    data: Rc<RefCell<Data>>,
}
impl Batch {
    pub fn complete(&self) -> Result<bool> {
        for operation in &self.operations {
            operation.state()?;
        }
        Ok(self.data.borrow().remaining == 0)
    }
    pub fn take(&self) -> Result<Data> {
        let mut data = self.data.borrow_mut();
        if let Some(error) = data.error.take() {
            return Err(error);
        }
        Ok(std::mem::take(&mut *data))
    }
}
// SAFETY for callbacks below: userdata points to a stable Callback box owned by
// Inner/Pending. Only this thread dispatches libpulse, and each owner unregisters
// or cancels before freeing. Info/string pointers are borrowed only during the
// callback. Every callback catches unwinding before returning through C.
extern "C" fn success(_: *mut pa_context, ok: i32, userdata: *mut c_void) {
    let state = unsafe { &*userdata.cast::<Callback<Cell<Option<bool>>>>() };
    state.call(|v| v.set(Some(ok != 0)));
}
extern "C" fn wake(
    _: *const pa_mainloop_api,
    _: *mut pa_io_event,
    _: i32,
    _: pa_io_event_flags_t,
    userdata: *mut c_void,
) {
    let state = unsafe { &*userdata.cast::<Callback<UnixStream>>() };
    state.call(|mut stream| {
        let mut bytes = [0; 256];
        loop {
            match stream.read(&mut bytes) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
}
extern "C" fn notice(
    _: *mut pa_context,
    event: pa_subscription_event_type_t,
    index: u32,
    userdata: *mut c_void,
) {
    let state = unsafe { &*userdata.cast::<Callback<RefCell<Notices>>>() };
    state.call(|v| {
        let mut notices = v.borrow_mut();
        notices.dirty = true;
        let kind = match event & PA_SUBSCRIPTION_EVENT_FACILITY_MASK {
            PA_SUBSCRIPTION_EVENT_SINK => Some(Kind::Output),
            PA_SUBSCRIPTION_EVENT_SOURCE => Some(Kind::Input),
            PA_SUBSCRIPTION_EVENT_SINK_INPUT => Some(Kind::Playback),
            PA_SUBSCRIPTION_EVENT_SOURCE_OUTPUT => Some(Kind::Capture),
            _ => None,
        };
        if event & PA_SUBSCRIPTION_EVENT_TYPE_MASK == PA_SUBSCRIPTION_EVENT_REMOVE
            && let Some(kind) = kind
        {
            if notices.removed.len() < 4096 {
                notices.removed.insert(Key(kind, index));
            } else {
                notices.reset = true;
            }
        }
    });
}
unsafe fn text<'a>(s: *const std::ffi::c_char) -> Option<&'a str> {
    if s.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
}
unsafe fn name(s: *const std::ffi::c_char) -> String {
    // Retain at most one bounded key even for a malformed service response.
    unsafe { text(s) }
        .filter(|s| s.len() <= 1024)
        .unwrap_or("")
        .into()
}
macro_rules! list_callback {
    ($callback:ident, $ty:ty, $record:expr) => {
        extern "C" fn $callback(
            _: *mut pa_context,
            info: *const $ty,
            end: i32,
            userdata: *mut c_void,
        ) {
            let state = unsafe { &*userdata.cast::<Callback<Rc<RefCell<Data>>>>() };
            state.call(|v| {
                let mut data = v.borrow_mut();
                if end != 0 {
                    data.finish(end < 0);
                } else if let Some(info) = unsafe { info.as_ref() } {
                    data.receive(Some(($record)(info)));
                } else {
                    data.finish(true);
                }
            });
        }
    };
}
list_callback!(sink, pa_sink_info, |s: &pa_sink_info| Record {
    key: Key(Kind::Output, s.index),
    name: unsafe { name(s.name) },
    label: snapshot::label(unsafe { text(s.description).or(text(s.name)) }),
    application: String::new(),
    volumes: Some(s.volume),
    writable: true,
    muted: s.mute != 0,
    monitor: false,
    device: None,
});
list_callback!(source, pa_source_info, |s: &pa_source_info| Record {
    key: Key(Kind::Input, s.index),
    name: unsafe { name(s.name) },
    label: snapshot::label(unsafe { text(s.description).or(text(s.name)) }),
    application: String::new(),
    volumes: Some(s.volume),
    writable: true,
    muted: s.mute != 0,
    monitor: s.monitor_of_sink != PA_INVALID_INDEX,
    device: None,
});
unsafe fn application(properties: *mut pa_proplist) -> String {
    if properties.is_null() {
        return String::new();
    }
    snapshot::label(unsafe { text(pa_proplist_gets(properties, c"application.name".as_ptr())) })
}
list_callback!(playback, pa_sink_input_info, |s: &pa_sink_input_info| {
    Record {
        key: Key(Kind::Playback, s.index),
        name: String::new(),
        label: snapshot::label(unsafe { text(s.name) }),
        application: unsafe { application(s.proplist) },
        volumes: (s.has_volume != 0).then_some(s.volume),
        writable: s.volume_writable != 0,
        muted: s.mute != 0,
        monitor: false,
        device: Some(Key(Kind::Output, s.sink)),
    }
});
list_callback!(
    capture,
    pa_source_output_info,
    |s: &pa_source_output_info| Record {
        key: Key(Kind::Capture, s.index),
        name: String::new(),
        label: snapshot::label(unsafe { text(s.name) }),
        application: unsafe { application(s.proplist) },
        volumes: (s.has_volume != 0).then_some(s.volume),
        writable: s.volume_writable != 0,
        muted: s.mute != 0,
        monitor: false,
        device: Some(Key(Kind::Input, s.source)),
    }
);
macro_rules! default_callback {
    ($callback:ident, $ty:ty, $slot:expr, $kind:expr) => {
        extern "C" fn $callback(
            context: *mut pa_context,
            info: *const $ty,
            end: i32,
            userdata: *mut c_void,
        ) {
            let state = unsafe { &*userdata.cast::<Callback<Rc<RefCell<Data>>>>() };
            state.call(|v| {
                let mut data = v.borrow_mut();
                if end != 0 {
                    // A missing default is legitimate; other errors are not.
                    let missing = unsafe { pa_context_errno(context) } == PA_ERR_NOENTITY as i32;
                    data.finish(end < 0 && !missing);
                } else if let Some(info) = unsafe { info.as_ref() } {
                    data.defaults[$slot] = Some(Key($kind, info.index));
                } else {
                    data.finish(true);
                }
            });
        }
    };
}
default_callback!(default_sink, pa_sink_info, 0, Kind::Output);
default_callback!(default_source, pa_source_info, 1, Kind::Input);

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    #[ignore = "requires isolated PipeWire test servers"]
    fn audio_protocol_cancelled_queries_release_callback_storage() {
        let server = super::super::tests::Server::start();
        let (reader, _writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let connection = Connection::new(Some(&server.server), reader).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !connection.ready().unwrap() {
            assert!(Instant::now() < deadline);
            connection.poll().unwrap();
        }
        for _ in 0..64 {
            let batch = connection.query().unwrap();
            let weak = Rc::downgrade(&batch.data);
            assert!(weak.upgrade().is_some());
            drop(batch); // no terminal callback has run
            assert!(
                weak.upgrade().is_none(),
                "cancelled query retained callback data"
            );
        }
        // Dispatch replies for cancelled operations and complete another query.
        let batch = connection.query().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !batch.complete().unwrap() {
            assert!(Instant::now() < deadline);
            connection.poll().unwrap();
        }
        assert!(batch.take().is_ok());
    }
}
