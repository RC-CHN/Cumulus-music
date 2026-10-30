//! The only unsafe libmpv boundary. Signatures/layouts checked against
//! mpv v0.40.0 include/mpv/client.h (API 2.5), compatible with 1.108+/2.x.
//! Every borrowed event is copied before another mpv_wait_event call.
use super::{EndReason, EngineEvent, MpvRuntime};
mod devices;
mod render;
use crate::{Error, Result};
use std::{
    ffi::{CStr, CString, OsStr, c_char, c_int, c_ulong, c_void},
    ptr::NonNull,
};

#[repr(C)]
struct Event {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct Start {
    entry_id: i64,
}
#[repr(C)]
struct End {
    reason: c_int,
    error: c_int,
    entry_id: i64,
    insert_id: i64,
    insert_count: c_int,
}

type Handle = *mut c_void;
type Create = unsafe extern "C" fn() -> Handle;
type Initialize = unsafe extern "C" fn(Handle) -> c_int;
type OptionString = unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int;
type Command = unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int;
type Property = unsafe extern "C" fn(Handle, *const c_char) -> *mut c_char;
type PropertyAsync = unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int;
type Free = unsafe extern "C" fn(*mut c_void);
type Destroy = unsafe extern "C" fn(Handle);
type Wait = unsafe extern "C" fn(Handle, f64) -> *const Event;
type Version = unsafe extern "C" fn() -> c_ulong;

pub(super) enum Received {
    None,
    Reply {
        sequence: u64,
        error: i32,
    },
    Devices {
        sequence: u64,
        result: Result<Vec<super::OutputDevice>>,
    },
    Event(EngineEvent),
    Ignored,
}

pub(super) struct Native {
    // Native::drop destroys media handles before the runtime/MTA lease ends.
    _runtime: MpvRuntime,
    handle: NonNull<c_void>,
    initialize: Initialize,
    option: OptionString,
    command: Command,
    property: Property,
    property_async: PropertyAsync,
    free: Free,
    destroy: Destroy,
    wait: Wait,
    version: u64,
    render: Option<render::Worker>,
}
// SAFETY: libmpv documents a thread-safe client API. Native is exclusively
// accessed through &mut self; no event pointer or library Symbol escapes it.
// Moving the handle between workers is permitted; sharing Native is not.
unsafe impl Send for Native {}

fn failed(operation: &'static str, code: i32) -> Error {
    Error::Playback { operation, code }
}
fn check(operation: &'static str, code: i32) -> Result<()> {
    if code < 0 {
        Err(failed(operation, code))
    } else {
        Ok(())
    }
}

impl Native {
    pub fn open(path: &OsStr) -> Result<Self> {
        // SAFETY: native host selects a trusted system/bundled libmpv. The
        // resolved symbols use documented C signatures and stay library-owned.
        unsafe {
            let library = MpvRuntime::open(path)?;
            let version = *library
                .get::<Version>(b"mpv_client_api_version\0")
                .map_err(|_| failed("ABI", -1))?;
            // C unsigned long is 32 bits on Windows and 64 on LP64 Unix.
            #[allow(clippy::unnecessary_cast)]
            let version = version() as u64;
            let major = version >> 16;
            let minor = version & 65535;
            if !((major == 1 && minor >= 108) || major == 2) {
                return Err(failed("ABI", -1));
            }
            let create = *library
                .get::<Create>(b"mpv_create\0")
                .map_err(|_| failed("ABI", -1))?;
            let initialize = *library
                .get::<Initialize>(b"mpv_initialize\0")
                .map_err(|_| failed("ABI", -1))?;
            let option = *library
                .get::<OptionString>(b"mpv_set_option_string\0")
                .map_err(|_| failed("ABI", -1))?;
            let command = *library
                .get::<Command>(b"mpv_command_async\0")
                .map_err(|_| failed("ABI", -1))?;
            let property = *library
                .get::<Property>(b"mpv_get_property_string\0")
                .map_err(|_| failed("ABI", -1))?;
            let property_async = *library
                .get::<PropertyAsync>(b"mpv_get_property_async\0")
                .map_err(|_| failed("ABI", -1))?;
            let free = *library
                .get::<Free>(b"mpv_free\0")
                .map_err(|_| failed("ABI", -1))?;
            let destroy = *library
                .get::<Destroy>(b"mpv_terminate_destroy\0")
                .map_err(|_| failed("ABI", -1))?;
            let wait = *library
                .get::<Wait>(b"mpv_wait_event\0")
                .map_err(|_| failed("ABI", -1))?;
            let handle = NonNull::new(create()).ok_or(failed("allocation", -1))?;
            Ok(Self {
                _runtime: library,
                handle,
                initialize,
                option,
                command,
                property,
                property_async,
                free,
                destroy,
                wait,
                version,
                render: None,
            })
        }
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn start_video(&mut self, width: u32, height: u32) -> Result<super::VideoFrames> {
        if self.render.is_some() {
            return Err(Error::State("video renderer already exists"));
        }
        let (worker, frames) = render::Worker::start(self, width, height)?;
        self.render = Some(worker);
        Ok(frames)
    }
    pub fn option(&mut self, key: &CStr, value: &CStr) -> Result<()> {
        // SAFETY: handle and NUL-terminated arguments are live for the call.
        check("option", unsafe {
            (self.option)(self.handle.as_ptr(), key.as_ptr(), value.as_ptr())
        })
    }
    pub fn initialize(&mut self) -> Result<()> {
        // SAFETY: this method is used once, after setting options.
        check("initialization", unsafe {
            (self.initialize)(self.handle.as_ptr())
        })
    }
    pub fn command(&mut self, sequence: u64, args: &[CString]) -> Result<()> {
        let mut pointers: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
        pointers.push(std::ptr::null());
        // SAFETY: mpv copies the NULL-terminated argument vector before return.
        check("command", unsafe {
            (self.command)(self.handle.as_ptr(), sequence, pointers.as_ptr())
        })
    }
    pub fn property(&mut self, name: &CStr) -> Option<String> {
        // SAFETY: property strings are allocated by mpv and freed exactly once
        // through mpv_free; lossy conversion never leaks the allocation.
        unsafe {
            let p = NonNull::new((self.property)(self.handle.as_ptr(), name.as_ptr()))?;
            let value = CStr::from_ptr(p.as_ptr()).to_string_lossy().into_owned();
            (self.free)(p.as_ptr().cast());
            Some(value)
        }
    }
    pub fn request_devices(&mut self, sequence: u64) -> Result<()> {
        // SAFETY: live handle; mpv copies the static property name and replies
        // asynchronously. MPV_FORMAT_NODE is 6 in the pinned client header.
        check("audio device query", unsafe {
            (self.property_async)(
                self.handle.as_ptr(),
                sequence,
                c"audio-device-list".as_ptr(),
                6,
            )
        })
    }
    pub fn event(&mut self, timeout: f64) -> Result<Received> {
        // SAFETY: mpv owns this event until the next wait call. We copy only
        // layouts documented for its tag, retain no raw pointers and never
        // read unknown event payloads (including log text/authorized URLs).
        unsafe {
            let event = (self.wait)(self.handle.as_ptr(), timeout)
                .as_ref()
                .ok_or(failed("event", -1))?;
            Ok(match event.event_id {
                0 => Received::None,
                1 => Received::Event(EngineEvent::Shutdown),
                3 => Received::Devices {
                    sequence: event.reply_userdata,
                    // The only asynchronous property request this adapter
                    // issues is audio-device-list. Copy it before the next
                    // wait; event-owned nodes must NOT be freed by the caller.
                    result: if event.error < 0 {
                        Err(failed("audio device query", event.error))
                    } else {
                        devices::copy(event.data)
                    },
                },
                5 => Received::Reply {
                    sequence: event.reply_userdata,
                    error: event.error,
                },
                6 => {
                    let start = event
                        .data
                        .cast::<Start>()
                        .as_ref()
                        .ok_or(failed("start event", -1))?;
                    Received::Event(EngineEvent::Started {
                        entry_id: start.entry_id,
                    })
                }
                7 => {
                    let end = event
                        .data
                        .cast::<End>()
                        .as_ref()
                        .ok_or(failed("end event", -1))?;
                    Received::Event(EngineEvent::Ended {
                        entry_id: end.entry_id,
                        reason: EndReason::from(end.reason),
                        error: end.error,
                    })
                }
                8 => Received::Event(EngineEvent::Loaded),
                24 => return Err(failed("event queue overflow", -1)),
                _ => Received::Ignored,
            })
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        // Render context must be freed (on its dedicated thread) before core
        // termination and before unloading any function pointer's library.
        drop(self.render.take());
        // SAFETY: unique live handle; destruction precedes the Library field.
        unsafe {
            (self.destroy)(self.handle.as_ptr());
        }
    }
}
