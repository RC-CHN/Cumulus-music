//! mpv v0.40 include/mpv/render.h, software render API. All raw pointers live
//! here. The renderer never calls normal mpv APIs or waits for their worker.
use super::super::{
    VideoFrame, VideoFrames,
    frames::{Surface, size},
};
use super::{Native, check, failed};
use crate::{Error, Result};
use std::{
    ffi::{c_int, c_void},
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[repr(C)]
struct Param {
    kind: c_int,
    data: *mut c_void,
}
type Create = unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut Param) -> c_int;
type Update = unsafe extern "C" fn(*mut c_void) -> u64;
type Render = unsafe extern "C" fn(*mut c_void, *mut Param) -> c_int;
type Free = unsafe extern "C" fn(*mut c_void);
#[repr(C)]
struct FrameInfo {
    flags: u64,
    target_time: i64,
}
type Info = unsafe extern "C" fn(*mut c_void, Param) -> c_int;
struct Context {
    handle: NonNull<c_void>,
    update: Update,
    render: Render,
    free: Free,
    info: Info,
}
// SAFETY: API >=1.108 allows moving a software context between threads.
// Exactly one render worker owns it, with no concurrent render API calls.
// Native joins that worker before destroying the core or unloading the library.
unsafe impl Send for Context {}
#[repr(C, align(64))]
#[derive(Clone)]
struct Block([u8; 64]);
pub(super) struct Worker {
    shared: Arc<Surface>,
    thread: Option<JoinHandle<()>>,
}
impl Context {
    fn create(native: &Native) -> Result<Self> {
        // SAFETY: symbols are the official C signatures; the trusted library
        // stays owned by Native until the worker and context have been dropped.
        unsafe {
            let create = *native
                ._library
                .get::<Create>(b"mpv_render_context_create\0")
                .map_err(|_| failed("video ABI", -1))?;
            let update = *native
                ._library
                .get::<Update>(b"mpv_render_context_update\0")
                .map_err(|_| failed("video ABI", -1))?;
            let render = *native
                ._library
                .get::<Render>(b"mpv_render_context_render\0")
                .map_err(|_| failed("video ABI", -1))?;
            let free = *native
                ._library
                .get::<Free>(b"mpv_render_context_free\0")
                .map_err(|_| failed("video ABI", -1))?;
            let info = *native
                ._library
                .get::<Info>(b"mpv_render_context_get_info\0")
                .map_err(|_| failed("video ABI", -1))?;
            let mut params = [
                Param {
                    kind: 1,
                    data: c"sw".as_ptr().cast_mut().cast(),
                },
                Param {
                    kind: 0,
                    data: std::ptr::null_mut(),
                },
            ];
            let mut context = std::ptr::null_mut();
            check(
                "video context",
                create(&mut context, native.handle.as_ptr(), params.as_mut_ptr()),
            )?;
            Ok(Self {
                handle: NonNull::new(context).ok_or(failed("video context", -1))?,
                update,
                render,
                free,
                info,
            })
        }
    }
    fn run(&mut self, shared: &Surface) -> Result<()> {
        let mut previous_size = 0;
        let mut memory: Vec<Block> = vec![];
        let mut sequence = 0u64;
        let mut decoded = false;
        while !shared.stop.load(Ordering::Acquire) {
            let packed = shared.size.load(Ordering::Acquire);
            let width = (packed >> 32) as u32;
            let height = packed as u32;
            // SAFETY: unique context; no callback or normal client API is used
            // on this thread. Polling is permitted without advanced control.
            let updated = unsafe { (self.update)(self.handle.as_ptr()) };
            if updated & 1 != 0 || packed != previous_size {
                let mut info = FrameInfo {
                    flags: 0,
                    target_time: 0,
                };
                // SAFETY: repr(C) matches mpv_render_frame_info; get_info takes
                // the parameter by value and writes only this live struct.
                unsafe {
                    check(
                        "video frame info",
                        (self.info)(
                            self.handle.as_ptr(),
                            Param {
                                kind: 11,
                                data: (&mut info as *mut FrameInfo).cast(),
                            },
                        ),
                    )?;
                }
                // Initial surface clearing/redraws are not decoded video.
                // After a real frame, paused resize may redraw that same frame.
                decoded |= info.flags & 1 != 0 && info.flags & 2 == 0;
                let stride = (width as usize * 4).div_ceil(64) * 64;
                let bytes = stride * height as usize;
                memory.resize(bytes / 64, Block([0; 64]));
                let mut dimensions = [width as c_int, height as c_int];
                let mut stride_arg = stride;
                let mut params = [
                    Param {
                        kind: 17,
                        data: dimensions.as_mut_ptr().cast(),
                    },
                    Param {
                        kind: 18,
                        data: c"rgb0".as_ptr().cast_mut().cast(),
                    },
                    Param {
                        kind: 19,
                        data: (&mut stride_arg as *mut usize).cast(),
                    },
                    Param {
                        kind: 20,
                        data: memory.as_mut_ptr().cast(),
                    },
                    Param {
                        kind: 0,
                        data: std::ptr::null_mut(),
                    },
                ];
                // SAFETY: aligned, initialized buffer contains stride*height
                // writable bytes, including padding. All parameter lifetimes
                // cover the synchronous call. Default target-time blocking
                // preserves mpv's audio/video synchronization on this worker.
                unsafe {
                    check(
                        "video render",
                        (self.render)(self.handle.as_ptr(), params.as_mut_ptr()),
                    )?;
                }
                if shared.stop.load(Ordering::Acquire) {
                    break;
                }
                previous_size = packed;
                if !decoded {
                    continue;
                }
                // SAFETY: repr(C,align(64)) Block is exactly 64 bytes; its Vec
                // is contiguous and every byte was initialized before FFI.
                let bytes =
                    unsafe { std::slice::from_raw_parts(memory.as_ptr().cast::<u8>(), bytes) };
                let row_bytes = width as usize * 4;
                let mut rgba = vec![0; row_bytes * height as usize];
                for (source, target) in bytes
                    .chunks_exact(stride)
                    .zip(rgba.chunks_exact_mut(row_bytes))
                {
                    target.copy_from_slice(&source[..row_bytes]);
                    for alpha in target.iter_mut().skip(3).step_by(4) {
                        *alpha = 255;
                    }
                }
                sequence = sequence
                    .checked_add(1)
                    .ok_or(Error::State("video frame sequence exhausted"))?;
                *shared.latest.lock().map_err(|_| Error::LockPoisoned)? =
                    Some(Arc::new(VideoFrame {
                        sequence,
                        width,
                        height,
                        rgba: rgba.into(),
                    }));
            } else {
                thread::park_timeout(Duration::from_millis(4));
            }
        }
        Ok(())
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: this is the unique live render context; Native's core and
        // library still exist. Free completes before the parent thread joins.
        unsafe {
            (self.free)(self.handle.as_ptr());
        }
    }
}
impl Worker {
    pub fn start(native: &Native, width: u32, height: u32) -> Result<(Self, VideoFrames)> {
        let packed = size(width, height)?;
        let mut context = Context::create(native)?;
        let shared = Arc::new(Surface {
            size: AtomicU64::new(packed),
            stop: AtomicBool::new(false),
            latest: Mutex::new(None),
            error: Mutex::new(None),
        });
        let surface = shared.clone();
        let thread = thread::Builder::new()
            .name("cumulus-video-render".into())
            .spawn(move || {
                if let Err(e) = context.run(&surface)
                    && let Ok(mut error) = surface.error.lock()
                {
                    *error = Some(Arc::new(e));
                }
                surface.stop.store(true, Ordering::Release);
                if let Ok(mut frame) = surface.latest.lock() {
                    *frame = None;
                }
                // Context is destroyed on this worker; no other mpv API is called.
            })?;
        let frames = VideoFrames {
            surface: shared.clone(),
            worker: thread.thread().clone(),
        };
        Ok((
            Self {
                shared,
                thread: Some(thread),
            },
            frames,
        ))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}
