//! Keep Windows MTA resources alive until native media handles are destroyed.
//! A libmpv worker can create WASAPI COM objects and exit before mp_destroy
//! releases them on the caller's thread. A process MTA usage lease bridges that
//! lifetime without changing a GUI thread's existing STA apartment.
use crate::{Error, Result};
use std::{ffi::c_void, ptr::NonNull};

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoIncrementMTAUsage(cookie: *mut *mut c_void) -> i32;
    fn CoDecrementMTAUsage(cookie: *mut c_void) -> i32;
}

pub(super) struct Mta(NonNull<c_void>);

// SAFETY: Microsoft explicitly permits increment/decrement on different
// threads when the same cookie is used. This is not a CoInitializeEx guard.
unsafe impl Send for Mta {}

impl Mta {
    pub(super) fn acquire() -> Result<Self> {
        let mut cookie = std::ptr::null_mut();
        // SAFETY: writable out-pointer; no caller COM interface is accessed.
        let status = unsafe { CoIncrementMTAUsage(&mut cookie) };
        if status < 0 {
            return Err(Error::Playback {
                operation: "COM MTA initialization",
                code: status,
            });
        }
        NonNull::new(cookie).map(Self).ok_or(Error::Playback {
            operation: "COM MTA initialization",
            code: -1,
        })
    }
}

impl Drop for Mta {
    fn drop(&mut self) {
        // SAFETY: exactly one successful acquisition, unique cookie ownership;
        // Native releases all media handles before this field is dropped.
        unsafe {
            CoDecrementMTAUsage(self.0.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mta_usage_can_end_on_another_thread_and_be_reacquired() {
        for _ in 0..3 {
            let lease = Mta::acquire().unwrap();
            std::thread::spawn(move || drop(lease)).join().unwrap();
        }
    }
}
