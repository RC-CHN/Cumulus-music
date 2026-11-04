//! Linux x86_64 native recognition capture, private lab only.
//! No renderer entry, host-default discovery, microphone fallback or network.

pub mod selection;
use selection::{Candidate, Device, OutputOptions, Topology};

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    fs,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt},
        },
    },
    path::{Path, PathBuf},
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, ProbeError>;

#[derive(Debug)]
pub enum ProbeError {
    Cancelled,
    Rejected(&'static str),
    TimedOut,
}

use ProbeError::Rejected;

const RATE: u32 = 48_000;
const CONTEXT_READY: c_int = 4;
const STREAM_READY: c_int = 2;
const OPERATION_RUNNING: c_int = 0;
const OPERATION_DONE: c_int = 1;
const NO_AUTOSPAWN: u32 = 0x0001;
const START_CORKED: u32 = 0x0001;
const DONT_MOVE: u32 = 0x0200;
const TOPOLOGY_INTERVAL: Duration = Duration::from_millis(250);
const INVALID_INDEX: u32 = u32::MAX;
const MAX_FRAGMENT: usize = RATE as usize * 8;

#[derive(Clone, Copy)]
pub enum CaptureExtent {
    FourSeconds,
    SevenSeconds,
}

impl CaptureExtent {
    fn seconds(self) -> u64 {
        match self {
            Self::FourSeconds => 4,
            Self::SevenSeconds => 7,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Format {
    I16,
    F32,
}

impl Format {
    fn spec(self) -> SampleSpec {
        SampleSpec {
            format: match self {
                Self::I16 => 3, // PA_SAMPLE_S16LE
                Self::F32 => 5, // PA_SAMPLE_FLOAT32LE
            },
            rate: RATE,
            channels: 2,
        }
    }

    fn frame_bytes(self) -> usize {
        match self {
            Self::I16 => 4,
            Self::F32 => 8,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct ChannelMap {
    channels: u8,
    map: [c_int; 32],
}

#[repr(C)]
struct ChannelVolume {
    channels: u8,
    volumes: [u32; 32],
}

// PulseAudio v17.0 introspect.h: pa_sink_info and pa_source_info share
// this prefix through monitor_source / monitor_of_sink, respectively.
#[repr(C)]
struct EndpointInfo {
    name: *const c_char,
    index: u32,
    description: *const c_char,
    spec: SampleSpec,
    map: ChannelMap,
    owner_module: u32,
    volume: ChannelVolume,
    mute: c_int,
    related_index: u32,
    related_name: *const c_char,
}

#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

#[repr(C)]
struct SockAddrUn {
    family: u16,
    path: [u8; 108],
}

#[repr(C)]
struct UCred {
    pid: i32,
    uid: u32,
    gid: u32,
}

#[link(name = "libpulse.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn pa_mainloop_new() -> *mut c_void;
    fn pa_mainloop_free(mainloop: *mut c_void);
    fn pa_mainloop_get_api(mainloop: *mut c_void) -> *mut c_void;
    fn pa_mainloop_iterate(mainloop: *mut c_void, block: c_int, retval: *mut c_int) -> c_int;
    fn pa_context_new(api: *mut c_void, name: *const c_char) -> *mut c_void;
    fn pa_context_connect(
        ctx: *mut c_void,
        server: *const c_char,
        flags: u32,
        spawn: *const c_void,
    ) -> c_int;
    fn pa_context_get_state(ctx: *const c_void) -> c_int;
    fn pa_context_disconnect(ctx: *mut c_void);
    fn pa_context_unref(ctx: *mut c_void);
    fn pa_context_get_sink_info_list(
        ctx: *mut c_void,
        cb: unsafe extern "C" fn(*mut c_void, *const EndpointInfo, c_int, *mut c_void),
        data: *mut c_void,
    ) -> *mut c_void;
    fn pa_context_get_source_info_list(
        ctx: *mut c_void,
        cb: unsafe extern "C" fn(*mut c_void, *const EndpointInfo, c_int, *mut c_void),
        data: *mut c_void,
    ) -> *mut c_void;
    fn pa_operation_get_state(op: *const c_void) -> c_int;
    fn pa_operation_cancel(op: *mut c_void);
    fn pa_operation_unref(op: *mut c_void);
    fn pa_stream_new(
        ctx: *mut c_void,
        name: *const c_char,
        spec: *const SampleSpec,
        map: *const ChannelMap,
    ) -> *mut c_void;
    fn pa_stream_connect_record(
        stream: *mut c_void,
        source: *const c_char,
        attr: *const BufferAttr,
        flags: u32,
    ) -> c_int;
    fn pa_stream_cork(
        stream: *mut c_void,
        cork: c_int,
        cb: unsafe extern "C" fn(*mut c_void, c_int, *mut c_void),
        data: *mut c_void,
    ) -> *mut c_void;
    fn pa_stream_is_corked(stream: *const c_void) -> c_int;
    fn pa_stream_get_state(stream: *const c_void) -> c_int;
    fn pa_stream_get_device_index(stream: *const c_void) -> u32;
    fn pa_stream_get_device_name(stream: *const c_void) -> *const c_char;
    fn pa_stream_get_sample_spec(stream: *const c_void) -> *const SampleSpec;
    fn pa_stream_peek(stream: *mut c_void, data: *mut *const c_void, bytes: *mut usize) -> c_int;
    fn pa_stream_drop(stream: *mut c_void) -> c_int;
    fn pa_stream_disconnect(stream: *mut c_void) -> c_int;
    fn pa_stream_unref(stream: *mut c_void);
}

unsafe extern "C" {
    fn geteuid() -> u32;
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn connect(fd: c_int, address: *const c_void, len: u32) -> c_int;
    fn getsockopt(fd: c_int, level: c_int, opt: c_int, value: *mut c_void, len: *mut u32) -> c_int;
}

struct Mainloop(NonNull<c_void>);
struct Context(NonNull<c_void>, Mainloop);
struct Stream<'a>(NonNull<c_void>, &'a Context);
struct Operation<'a>(NonNull<c_void>, &'a Context);

impl Drop for Mainloop {
    fn drop(&mut self) {
        // SAFETY: the mainloop owns its handle and every context is already gone.
        unsafe { pa_mainloop_free(self.0.as_ptr()) };
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: context outlives all streams, on the mainloop's thread.
        unsafe {
            pa_context_disconnect(self.0.as_ptr());
            pa_context_unref(self.0.as_ptr());
        }
    }
}
impl Drop for Stream<'_> {
    fn drop(&mut self) {
        // SAFETY: the context is still live; the stream is uniquely owned.
        unsafe {
            pa_stream_disconnect(self.0.as_ptr());
            pa_stream_unref(self.0.as_ptr());
        }
    }
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        // SAFETY: the operation owns a live ref; cancel before callback data expires.
        unsafe {
            if pa_operation_get_state(self.0.as_ptr()) == OPERATION_RUNNING {
                pa_operation_cancel(self.0.as_ptr());
            }
            pa_operation_unref(self.0.as_ptr());
        }
    }
}

impl Operation<'_> {
    fn wait(&self, deadline: &Deadline<'_>, failure: &'static str) -> Result<()> {
        loop {
            deadline.drive(&self.1.1)?;
            // SAFETY: the operation still owns the live native reference.
            match unsafe { pa_operation_get_state(self.0.as_ptr()) } {
                OPERATION_RUNNING => thread::sleep(Duration::from_millis(4)),
                OPERATION_DONE => return Ok(()),
                _ => return Err(Rejected(failure)),
            }
        }
    }
}

pub struct PrivateServer {
    server: CString,
    socket: PathBuf,
    peer: PeerIdentity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PeerIdentity {
    pid: i32,
    uid: u32,
    device: u64,
    inode: u64,
}

impl PrivateServer {
    pub fn new(runtime: &Path, pid: i32) -> Result<Self> {
        let outer = std::env::var_os("CUMULUS_MONITOR_OUTER_NET")
            .ok_or(Rejected("private network namespace required"))?;
        let active = fs::read_link("/proc/self/ns/net")
            .map_err(|_| Rejected("network namespace unavailable"))?;
        if active.as_os_str() == outer || Path::new("/dev/snd").exists() || !loopback_only()? {
            return Err(Rejected("private network and device namespace required"));
        }
        if pid <= 1 || !runtime.is_absolute() {
            return Err(Rejected("invalid private monitor configuration"));
        }
        let root =
            fs::canonicalize(runtime).map_err(|_| Rejected("private runtime unavailable"))?;
        let private_dir = root.parent() == Some(Path::new("/tmp"))
            && root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("cumulus-recognition-")
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                });
        let metadata =
            fs::metadata(&root).map_err(|_| Rejected("private runtime metadata unavailable"))?;
        // SAFETY: process effective UID, no arguments or pointers.
        let uid = unsafe { geteuid() };
        if !private_dir
            || !metadata.is_dir()
            || metadata.uid() != uid
            || metadata.mode() & 0o077 != 0
        {
            return Err(Rejected("runtime is not a private fixture directory"));
        }
        let socket = root.join("pulse/native");
        let peer = inspect_peer(&socket, pid, uid)?;
        let mut server = b"unix:".to_vec();
        server.extend_from_slice(socket.as_os_str().as_bytes());
        Ok(Self {
            server: CString::new(server).map_err(|_| Rejected("invalid Pulse socket path"))?,
            socket,
            peer,
        })
    }

    fn verify(&self) -> Result<()> {
        if Path::new("/dev/snd").exists() || !loopback_only()? {
            return Err(Rejected("private network and device namespace required"));
        }
        if inspect_peer(&self.socket, self.peer.pid, self.peer.uid)? != self.peer {
            return Err(Rejected("private Pulse server identity changed"));
        }
        Ok(())
    }

    pub fn discover(self, stop: &AtomicBool) -> Result<OutputOptions> {
        let deadline = Deadline::startup(stop);
        self.verify()?;
        let context = Context::open(&self, &deadline)?;
        self.verify()?;
        let topology = context.topology(&deadline)?;
        OutputOptions::new(self, context, topology)
    }
}

fn loopback_only() -> Result<bool> {
    let interfaces = fs::read_to_string("/proc/net/dev")
        .map_err(|_| Rejected("network interfaces unavailable"))?;
    let mut names = interfaces
        .lines()
        .skip(2)
        .filter_map(|row| row.split_once(':').map(|(name, _)| name.trim()));
    Ok(names.next() == Some("lo") && names.next().is_none())
}

fn inspect_peer(socket: &Path, pid: i32, uid: u32) -> Result<PeerIdentity> {
    let meta =
        fs::symlink_metadata(socket).map_err(|_| Rejected("private Pulse socket unavailable"))?;
    if !meta.file_type().is_socket()
        || meta.uid() != uid
        || fs::canonicalize(socket).ok().as_deref() != Some(socket)
    {
        return Err(Rejected("invalid private Pulse socket"));
    }
    // A separate nonblocking connection attests the explicit Unix pathname
    // around the initial libpulse handshake; the selected context never reconnects.
    let peer = peer_socket(socket)?;
    let mut cred = UCred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = size_of::<UCred>() as u32;
    // SAFETY: SO_PEERCRED fills a correctly sized Linux ucred.
    let rc = unsafe {
        getsockopt(
            peer.as_raw_fd(),
            1,
            17,
            (&mut cred as *mut UCred).cast(),
            &mut len,
        )
    };
    if rc != 0 || len as usize != size_of::<UCred>() || cred.uid != uid || cred.pid != pid {
        return Err(Rejected("private Pulse server identity mismatch"));
    }
    let after =
        fs::symlink_metadata(socket).map_err(|_| Rejected("private Pulse socket unavailable"))?;
    if (meta.dev(), meta.ino()) != (after.dev(), after.ino()) {
        return Err(Rejected("private Pulse server identity changed"));
    }
    Ok(PeerIdentity {
        pid,
        uid,
        device: meta.dev(),
        inode: meta.ino(),
    })
}

fn peer_socket(path: &Path) -> Result<OwnedFd> {
    let bytes = path.as_os_str().as_bytes();
    let mut address = SockAddrUn {
        family: 1,
        path: [0; 108],
    };
    if bytes.len() >= address.path.len() || bytes.contains(&0) {
        return Err(Rejected("invalid private Pulse socket length"));
    }
    address.path[..bytes.len()].copy_from_slice(bytes);
    // SAFETY: Linux AF_UNIX / SOCK_STREAM|SOCK_NONBLOCK|SOCK_CLOEXEC.
    let fd = unsafe { socket(1, 1 | 0o4000 | 0o2000000, 0) };
    if fd < 0 {
        return Err(Rejected("private Pulse peer probe unavailable"));
    }
    // SAFETY: socket returned a new owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: address is a valid, NUL-terminated sockaddr_un. The descriptor is
    // nonblocking; a full backlog or incomplete connect is rejected.
    if unsafe {
        connect(
            fd.as_raw_fd(),
            (&address as *const SockAddrUn).cast(),
            (2 + bytes.len() + 1) as u32,
        )
    } != 0
    {
        return Err(Rejected("private Pulse socket refused connection"));
    }
    Ok(fd)
}

struct Lookup {
    rows: Vec<Device>,
    finished: bool,
    failed: bool,
}

const MAX_ENDPOINTS: usize = 256;

unsafe extern "C" fn endpoint_info(
    _: *mut c_void,
    info: *const EndpointInfo,
    end: c_int,
    data: *mut c_void,
) {
    // SAFETY: the lookup lives through the single-threaded mainloop operation.
    let lookup = unsafe { &mut *data.cast::<Lookup>() };
    if end != 0 {
        lookup.finished = true;
        lookup.failed |= end < 0;
        return;
    }
    // SAFETY: libpulse exposes a live introspection prefix during the callback.
    let Some(info) = (unsafe { info.as_ref() }) else {
        lookup.failed = true;
        return;
    };
    if lookup.rows.len() == MAX_ENDPOINTS || info.index == INVALID_INDEX {
        lookup.failed = true;
        return;
    }
    let Some(name) = copy_name(info.name) else {
        lookup.failed = true;
        return;
    };
    let relation = match (info.related_index, copy_name(info.related_name)) {
        (INVALID_INDEX, None) => None,
        (index, Some(name)) if index != INVALID_INDEX => Some((index, name)),
        _ => {
            lookup.failed = true;
            return;
        }
    };
    lookup.rows.push(Device {
        index: info.index,
        name,
        relation,
    });
}

fn copy_name(pointer: *const c_char) -> Option<CString> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: libpulse owns a NUL-terminated string for this callback.
    let value = unsafe { CStr::from_ptr(pointer) };
    (value.to_bytes().len() <= 255).then(|| value.to_owned())
}

fn same_name(ptr: *const c_char, expected: &CStr) -> bool {
    // SAFETY: all callers provide a libpulse-owned, NUL-terminated name or NULL,
    // read only while the callback or stream owning the name is live.
    !ptr.is_null() && unsafe { CStr::from_ptr(ptr) } == expected
}

struct Deadline<'a> {
    stop: &'a AtomicBool,
    end: Instant,
}
impl<'a> Deadline<'a> {
    fn startup(stop: &'a AtomicBool) -> Self {
        Self {
            stop,
            end: Instant::now() + Duration::from_secs(5),
        }
    }

    fn check(&self) -> Result<()> {
        if self.stop.load(Ordering::Acquire) {
            return Err(ProbeError::Cancelled);
        }
        if Instant::now() >= self.end {
            return Err(ProbeError::TimedOut);
        }
        Ok(())
    }

    fn drive(&self, mainloop: &Mainloop) -> Result<()> {
        self.check()?;
        // SAFETY: nonblocking iteration on the owning thread.
        if unsafe { pa_mainloop_iterate(mainloop.0.as_ptr(), 0, ptr::null_mut()) } < 0 {
            return Err(Rejected("private Pulse mainloop stopped"));
        }
        Ok(())
    }
}

impl Context {
    fn open(endpoint: &PrivateServer, deadline: &Deadline<'_>) -> Result<Self> {
        // SAFETY: the newly created mainloop belongs to this context and thread.
        let mainloop = Mainloop(
            NonNull::new(unsafe { pa_mainloop_new() })
                .ok_or(Rejected("private Pulse mainloop allocation failed"))?,
        );
        // SAFETY: mainloop is live; C copies the app name.
        let api = unsafe { pa_mainloop_get_api(mainloop.0.as_ptr()) };
        let raw = NonNull::new(unsafe {
            pa_context_new(api, c"Cumulus recognition monitor proof".as_ptr())
        })
        .ok_or(Rejected("private Pulse context allocation failed"))?;
        let this = Self(raw, mainloop);
        // SAFETY: server is a validated explicit pathname; no autospawn or spawn hooks.
        if unsafe {
            pa_context_connect(
                raw.as_ptr(),
                endpoint.server.as_ptr(),
                NO_AUTOSPAWN,
                ptr::null(),
            )
        } < 0
        {
            return Err(Rejected("private Pulse server connection failed"));
        }
        loop {
            deadline.drive(&this.1)?;
            match unsafe { pa_context_get_state(raw.as_ptr()) } {
                CONTEXT_READY => return Ok(this),
                0..=3 => thread::sleep(Duration::from_millis(4)),
                _ => return Err(Rejected("private Pulse server did not become ready")),
            }
        }
    }

    fn topology(&self, deadline: &Deadline<'_>) -> Result<Topology> {
        Topology::new(
            self.list_devices(true, deadline)?,
            self.list_devices(false, deadline)?,
        )
    }

    fn list_devices(&self, sinks: bool, deadline: &Deadline<'_>) -> Result<Vec<Device>> {
        let mut lookup = Lookup {
            rows: Vec::new(),
            finished: false,
            failed: false,
        };
        let function = if sinks {
            pa_context_get_sink_info_list
        } else {
            pa_context_get_source_info_list
        };
        // SAFETY: lookup remains alive until after Operation drops; the callback
        // only executes during drive on the owner thread.
        let operation = NonNull::new(unsafe {
            function(
                self.0.as_ptr(),
                endpoint_info,
                (&mut lookup as *mut Lookup).cast(),
            )
        })
        .map(|raw| Operation(raw, self))
        .ok_or(Rejected("private output enumeration unavailable"))?;
        operation.wait(deadline, "private output enumeration stopped")?;
        if !lookup.finished || lookup.failed {
            return Err(Rejected("private output enumeration incomplete"));
        }
        Ok(lookup.rows)
    }
}

impl<'a> Stream<'a> {
    fn open(
        context: &'a Context,
        selected: &Candidate,
        format: Format,
        deadline: &Deadline<'_>,
    ) -> Result<Self> {
        let spec = format.spec();
        // SAFETY: sample spec is valid and Pulse copies it.
        let raw = NonNull::new(unsafe {
            pa_stream_new(
                context.0.as_ptr(),
                c"Cumulus private output capture".as_ptr(),
                &spec,
                ptr::null(),
            )
        })
        .ok_or(Rejected("private monitor stream allocation failed"))?;
        let this = Self(raw, context);
        let attr = BufferAttr {
            maxlength: (RATE as usize * format.frame_bytes() / 2) as u32,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: (RATE as usize / 50 * format.frame_bytes()) as u32,
        };
        // SAFETY: source is explicit and does not move; remain paused until
        // its actual route and full reciprocal topology have been rechecked.
        if unsafe {
            pa_stream_connect_record(
                raw.as_ptr(),
                selected.source.as_ptr(),
                &attr,
                DONT_MOVE | START_CORKED,
            )
        } < 0
        {
            return Err(Rejected("private monitor stream connection failed"));
        }
        loop {
            deadline.drive(&this.1.1)?;
            match unsafe { pa_stream_get_state(raw.as_ptr()) } {
                STREAM_READY => return Ok(this),
                0..=1 => thread::sleep(Duration::from_millis(4)),
                _ => return Err(Rejected("private monitor stream did not become ready")),
            }
        }
    }

    fn start(&self, deadline: &Deadline<'_>) -> Result<()> {
        deadline.check()?;
        // SAFETY: the stream has been initialized on this thread.
        if unsafe { pa_stream_is_corked(self.0.as_ptr()) } != 1 {
            return Err(Rejected("private monitor did not start paused"));
        }
        let mut accepted = false;
        // SAFETY: the operation and callback cannot outlive accepted.
        let operation = NonNull::new(unsafe {
            pa_stream_cork(
                self.0.as_ptr(),
                0,
                uncorked,
                (&mut accepted as *mut bool).cast(),
            )
        })
        .map(|raw| Operation(raw, self.1))
        .ok_or(Rejected("private monitor could not start"))?;
        operation.wait(deadline, "private monitor could not start")?;
        if !accepted {
            return Err(Rejected("private monitor start was rejected"));
        }
        Ok(())
    }

    fn verify(&self, selected: &Candidate, spec: SampleSpec) -> Result<()> {
        // SAFETY: all returned pointers belong to the still-live stream. They are
        // checked and discarded before the next mainloop iteration.
        let valid = unsafe {
            pa_stream_get_state(self.0.as_ptr()) == STREAM_READY
                && pa_stream_get_device_index(self.0.as_ptr()) == selected.source_index
                && same_name(pa_stream_get_device_name(self.0.as_ptr()), &selected.source)
                && pa_stream_get_sample_spec(self.0.as_ptr()).as_ref() == Some(&spec)
        };
        if !valid {
            return Err(Rejected("private monitor binding or format changed"));
        }
        Ok(())
    }
}

unsafe extern "C" fn uncorked(_: *mut c_void, success: c_int, data: *mut c_void) {
    // SAFETY: Stream::start holds the callback slot until the operation drops.
    unsafe { *data.cast::<bool>() = success == 1 };
}

pub struct Fragment<'a> {
    pub bytes: Option<&'a [u8]>,
    pub frames: usize,
}

/// Consumed by capture; not serializable, cloneable or constructible by a caller.
pub struct ConfirmedCapture {
    server: PrivateServer,
    context: Context,
    selected: Candidate,
    expires: Instant,
}

pub fn capture(
    permit: ConfirmedCapture,
    format: Format,
    extent: CaptureExtent,
    stop: &AtomicBool,
    ready: impl FnOnce(),
    mut consume: impl FnMut(Fragment<'_>) -> Result<()>,
) -> Result<usize> {
    let ConfirmedCapture {
        server,
        context,
        selected,
        expires,
    } = permit;
    let startup = Deadline::startup(stop);
    startup.check()?;
    if Instant::now() >= expires {
        return Err(Rejected("private capture confirmation expired"));
    }
    server.verify()?;
    let observed = context.topology(&startup)?;
    if !observed.contains(&selected) {
        return Err(Rejected("confirmed private output changed"));
    }
    let stream = Stream::open(&context, &selected, format, &startup)?;
    stream.verify(&selected, format.spec())?;
    if !context.topology(&startup)?.contains(&selected) {
        return Err(Rejected("confirmed private output changed"));
    }
    if Instant::now() >= expires {
        return Err(Rejected("private capture confirmation expired"));
    }
    stream.verify(&selected, format.spec())?;
    stream.start(&startup)?;
    stream.verify(&selected, format.spec())?;
    startup.check()?;
    ready();
    let deadline = Deadline {
        stop,
        end: Instant::now() + Duration::from_secs(extent.seconds() + 3),
    };
    let frame_bytes = format.frame_bytes();
    let mut frames = 0;
    let max_frames = RATE as usize * extent.seconds() as usize;
    let mut next_topology_check = Instant::now() + TOPOLOGY_INTERVAL;
    while frames < max_frames {
        deadline.drive(&context.1)?;
        if Instant::now() >= next_topology_check {
            let route_deadline = Deadline {
                stop,
                end: deadline.end.min(Instant::now() + TOPOLOGY_INTERVAL),
            };
            if !context.topology(&route_deadline)?.contains(&selected) {
                return Err(Rejected("private monitor binding or format changed"));
            }
            next_topology_check = Instant::now() + TOPOLOGY_INTERVAL;
        }
        stream.verify(&selected, format.spec())?;
        let mut data = ptr::null();
        let mut size = 0;
        // SAFETY: live record stream and writable output slots.
        if unsafe { pa_stream_peek(stream.0.as_ptr(), &mut data, &mut size) } < 0 {
            return Err(Rejected("private monitor packet unavailable"));
        }
        if size == 0 {
            thread::sleep(Duration::from_millis(4));
            continue;
        }
        let result = (|| {
            if size > MAX_FRAGMENT || !size.is_multiple_of(frame_bytes) {
                return Err(Rejected("private monitor fragment exceeds its bound"));
            }
            deadline.check()?;
            let count = (size / frame_bytes).min(max_frames - frames);
            // SAFETY: Pulse keeps this packet valid until drop. Null data with
            // nonzero length is a documented hole, represented separately.
            let bytes = (!data.is_null())
                .then(|| unsafe { slice::from_raw_parts(data.cast::<u8>(), count * frame_bytes) });
            consume(Fragment {
                bytes,
                frames: count,
            })?;
            frames += count;
            Ok(())
        })();
        // SAFETY: every nonempty peek is dropped exactly once, even on a hole/error.
        let dropped = unsafe { pa_stream_drop(stream.0.as_ptr()) };
        result?;
        if dropped < 0 {
            return Err(Rejected("private monitor packet release failed"));
        }
    }
    Ok(frames)
}
