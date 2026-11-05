//! libmpv read-only stream callback ABI (mpv/stream_cb.h, client API 1.108+).
//! The native handle owns the registration until mpv_terminate_destroy returns.
use super::Handle;
use crate::{
    Error, Result,
    playback::engine::{Cancellation, PinnedReader, StereoPcmControl, StereoPcmReader},
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
    size: Option<i64>,
    cancelled: AtomicBool,
    authority: Cancellation,
    interrupt: Option<StereoPcmControl>,
}

struct Input {
    reader: Reader,
    scratch: Box<[u8]>,
}
enum Reader {
    Pinned(Box<dyn PinnedReader>),
    Pcm(Box<StereoPcmReader>),
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
                    reader: Reader::Pinned(reader),
                    scratch: vec![0; MAX_READ].into_boxed_slice(),
                }),
                size: Some(size),
                cancelled: AtomicBool::new(false),
                authority,
                interrupt: None,
            },
        });
        Ok(uri)
    }

    /// True nonseekable/unknown-size PCM. No finite length or WAV header is
    /// invented for a graph whose live controls can change its EOF tail.
    pub(super) fn stage_pcm(
        &self,
        reader: StereoPcmReader,
        authority: Cancellation,
    ) -> Result<CString> {
        authority.check()?;
        let control = reader.control();
        let uri = CString::new(format!("cumuluspinned://{}", uuid::Uuid::new_v4().simple()))
            .expect("generated URI");
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        *slot = Some(Pending {
            uri: uri.clone(),
            stream: Stream {
                input: Mutex::new(Input {
                    reader: Reader::Pcm(Box::new(reader)),
                    scratch: vec![0; MAX_READ].into_boxed_slice(),
                }),
                size: None,
                cancelled: AtomicBool::new(false),
                authority,
                interrupt: Some(control),
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
        let finite = stream.size.is_some();
        let cookie = Box::into_raw(Box::new(stream)).cast::<c_void>();
        // SAFETY: supplied by libmpv for this call; ownership of cookie is
        // transferred until close, after it finishes concurrent callbacks.
        unsafe {
            info.write(Info {
                cookie,
                read: Some(read),
                seek: finite.then_some(seek),
                size: finite.then_some(size),
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
        let remaining = match reader {
            Reader::Pinned(reader) => {
                let position = reader.stream_position().ok()?;
                u64::try_from(stream.size?).ok()?.saturating_sub(position)
            }
            Reader::Pcm(_) => u64::MAX,
        };
        let bound = len.min(remaining).min(MAX_READ as u64) as usize;
        let mut count = None;
        for _ in 0..4 {
            let value = match reader {
                Reader::Pinned(reader) => reader.read(&mut scratch[..bound]),
                Reader::Pcm(reader) => std::io::Read::read(reader, &mut scratch[..bound]),
            };
            match value {
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
        let size = stream.size?;
        if !stream.valid() || !(0..=size).contains(&pos) {
            return None;
        }
        let mut input = stream.input.lock().ok()?;
        let Reader::Pinned(reader) = &mut input.reader else {
            return None;
        };
        i64::try_from(reader.seek(SeekFrom::Start(pos as u64)).ok()?).ok()
    })
}

unsafe extern "C" fn size(cookie: *mut c_void) -> i64 {
    checked(-1, || {
        if cookie.is_null() {
            return None;
        }
        // SAFETY: libmpv retains the cookie for the duration of the call.
        let stream = unsafe { &*cookie.cast::<Stream>() };
        stream.valid().then_some(stream.size).flatten()
    })
}

unsafe extern "C" fn cancel(cookie: *mut c_void) {
    if !cookie.is_null() {
        // SAFETY: libmpv allows cancel to race read/seek but not close.
        let stream = unsafe { &*cookie.cast::<Stream>() };
        stream.cancelled.store(true, Ordering::Release);
        if let Some(interrupt) = &stream.interrupt {
            interrupt.cancel();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::engine::stereo_pcm::channel;
    use std::{
        mem::MaybeUninit,
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    #[test]
    fn unknown_pcm_callback_has_no_size_or_seek_and_cancel_interrupts_pending_read() {
        let (reader, control, shared) =
            channel(44100, 0, 5000, Cancellation::default(), None).unwrap();
        let registry = Arc::new(Registry::default());
        let uri = registry.stage_pcm(reader, control.authority()).unwrap();
        let mut info = MaybeUninit::<Info>::uninit();
        // SAFETY: valid live registry/URI and writable ABI-sized destination;
        // this test closes the returned cookie only after its read thread ends.
        assert_eq!(
            unsafe {
                open(
                    Arc::as_ptr(&registry).cast_mut().cast(),
                    uri.as_ptr().cast_mut(),
                    info.as_mut_ptr(),
                )
            },
            0
        );
        let info = unsafe { info.assume_init() };
        assert!(info.seek.is_none() && info.size.is_none());
        shared.publish(&[0.25, -0.5], 1, false).unwrap();
        let mut data = [0u8; 8];
        assert_eq!(
            unsafe { info.read.unwrap()(info.cookie, data.as_mut_ptr().cast(), 8) },
            8
        );
        assert_eq!(
            data,
            [0.25f32, -0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
                .as_slice()
        );
        let cookie = info.cookie as usize;
        let (entered, running) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            entered.send(()).unwrap();
            let mut output = [0x5a; 8];
            // SAFETY: open cookie remains owned by this test; close runs only
            // after join, while cancel is explicitly allowed concurrently.
            unsafe { read(cookie as *mut c_void, output.as_mut_ptr().cast(), 8) }
        });
        running.recv_timeout(Duration::from_secs(2)).unwrap();
        unsafe { info.cancel.unwrap()(info.cookie) };
        assert_eq!(worker.join().unwrap(), -1, "pending is not EOF");
        unsafe { info.close.unwrap()(info.cookie) };
        assert!(control.snapshot().unwrap().reader_closed);
    }
}
