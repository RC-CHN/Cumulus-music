//! libmpv read-only stream callback ABI (mpv/stream_cb.h, client API 1.108+).
//! The native handle owns the registration until mpv_terminate_destroy returns.
use super::Handle;
use crate::{
    Error, Result,
    playback::engine::{Cancellation, PinnedReader},
};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io::SeekFrom,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

type Open = unsafe extern "C" fn(*mut c_void, *mut c_char, *mut Info) -> c_int;
pub(super) type Register = unsafe extern "C" fn(Handle, *const c_char, *mut c_void, Open) -> c_int;
type ReadFn = unsafe extern "C" fn(*mut c_void, *mut c_char, u64) -> i64;
type SeekFn = unsafe extern "C" fn(*mut c_void, i64) -> i64;
type SizeFn = unsafe extern "C" fn(*mut c_void) -> i64;
type CloseFn = unsafe extern "C" fn(*mut c_void);
type CancelFn = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
pub(super) struct Info {
    cookie: *mut c_void,
    read: Option<ReadFn>,
    seek: Option<SeekFn>,
    size: Option<SizeFn>,
    close: Option<CloseFn>,
    cancel: Option<CancelFn>,
}

pub(super) const SCHEME: &CStr = c"cumuluspinned";
const MAX_READ: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct Registry(Mutex<Option<Pending>>);

struct Pending {
    uri: CString,
    stream: Stream,
}

struct Stream {
    input: Mutex<Input>,
    size: i64,
    cancelled: AtomicBool,
    authority: Cancellation,
}

struct Input {
    reader: Box<dyn PinnedReader>,
    scratch: Box<[u8]>,
}

impl Stream {
    fn valid(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire) && !self.authority.is_cancelled()
    }
}

impl Registry {
    pub(super) fn stage(
        &self,
        mut reader: Box<dyn PinnedReader>,
        authority: Cancellation,
    ) -> Result<CString> {
        let size = i64::try_from(reader.seek(SeekFrom::End(0))?)
            .map_err(|_| Error::InvalidInput("pinned audio is too large"))?;
        if size == 0 {
            return Err(Error::InvalidInput("pinned audio is empty"));
        }
        reader.rewind()?;
        authority.check()?;
        let uri = CString::new(format!("cumuluspinned://{}", uuid::Uuid::new_v4().simple()))
            .expect("generated URI");
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        *slot = Some(Pending {
            uri: uri.clone(),
            stream: Stream {
                input: Mutex::new(Input {
                    reader,
                    scratch: vec![0; MAX_READ].into_boxed_slice(),
                }),
                size,
                cancelled: AtomicBool::new(false),
                authority,
            },
        });
        Ok(uri)
    }

    pub(super) fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

fn checked<T>(failure: T, work: impl FnOnce() -> Option<T>) -> T {
    catch_unwind(AssertUnwindSafe(work))
        .ok()
        .flatten()
        .unwrap_or(failure)
}

pub(super) unsafe extern "C" fn open(
    user: *mut c_void,
    uri: *mut c_char,
    info: *mut Info,
) -> c_int {
    checked(-1, || {
        if user.is_null() || uri.is_null() || info.is_null() {
            return None;
        }
        // SAFETY: libmpv passes the registered registry and a NUL-terminated
        // URI; both remain valid for the callback. Info is writable for it.
        let (registry, uri) = unsafe { (&*user.cast::<Registry>(), CStr::from_ptr(uri)) };
        let mut slot = registry.0.lock().ok()?;
        if !slot
            .as_ref()
            .is_some_and(|item| item.uri.as_c_str() == uri && item.stream.valid())
        {
            return None;
        }
        let stream = slot.take()?.stream;
        let cookie = Box::into_raw(Box::new(stream)).cast::<c_void>();
        // SAFETY: supplied by libmpv for this call; ownership of cookie is
        // transferred until close, after it finishes concurrent callbacks.
        unsafe {
            info.write(Info {
                cookie,
                read: Some(read),
                seek: Some(seek),
                size: Some(size),
                close: Some(close),
                cancel: Some(cancel),
            });
        }
        Some(0)
    })
}

unsafe extern "C" fn read(cookie: *mut c_void, dst: *mut c_char, len: u64) -> i64 {
    checked(-1, || {
        if cookie.is_null() || (len > 0 && dst.is_null()) {
            return None;
        }
        // SAFETY: the cookie and destination were provided by libmpv and
        // close is not concurrent with read. The returned slice is bounded.
        let stream = unsafe { &*cookie.cast::<Stream>() };
        if !stream.valid() {
            return None;
        }
        if len == 0 {
            return Some(0);
        }
        let mut input = stream.input.lock().ok()?;
        let Input { reader, scratch } = &mut *input;
        let position = reader.stream_position().ok()?;
        let remaining = u64::try_from(stream.size).ok()?.saturating_sub(position);
        let bound = len.min(remaining).min(MAX_READ as u64) as usize;
        let mut count = None;
        for _ in 0..4 {
            match reader.read(&mut scratch[..bound]) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                value => {
                    count = Some(value.ok()?);
                    break;
                }
            }
        }
        let count = count?;
        if !stream.valid() {
            return None;
        }
        // SAFETY: libmpv provided len writable bytes. Copy from initialized
        // scratch into the raw buffer without making a Rust reference to it.
        unsafe { std::ptr::copy_nonoverlapping(scratch.as_ptr(), dst.cast::<u8>(), count) };
        i64::try_from(count).ok()
    })
}

unsafe extern "C" fn seek(cookie: *mut c_void, pos: i64) -> i64 {
    checked(-1, || {
        if cookie.is_null() {
            return None;
        }
        // SAFETY: libmpv serializes close after callbacks and owns the cookie.
        let stream = unsafe { &*cookie.cast::<Stream>() };
        if !stream.valid() || !(0..=stream.size).contains(&pos) {
            return None;
        }
        i64::try_from(
            stream
                .input
                .lock()
                .ok()?
                .reader
                .seek(SeekFrom::Start(pos as u64))
                .ok()?,
        )
        .ok()
    })
}

unsafe extern "C" fn size(cookie: *mut c_void) -> i64 {
    checked(-1, || {
        if cookie.is_null() {
            return None;
        }
        // SAFETY: libmpv retains the cookie for the duration of the call.
        let stream = unsafe { &*cookie.cast::<Stream>() };
        stream.valid().then_some(stream.size)
    })
}

unsafe extern "C" fn cancel(cookie: *mut c_void) {
    if !cookie.is_null() {
        // SAFETY: libmpv allows cancel to race read/seek but not close.
        unsafe { &*cookie.cast::<Stream>() }
            .cancelled
            .store(true, Ordering::Release);
    }
}

unsafe extern "C" fn close(cookie: *mut c_void) {
    if !cookie.is_null() {
        // SAFETY: the stream was allocated in open and libmpv calls close
        // once, after it is done using the cookie.
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(cookie.cast::<Stream>()))
        }));
    }
}
