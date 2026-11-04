use super::{Close, Completion, Registration, WaitError, Wake, Watch};
use std::{
    cell::UnsafeCell,
    mem,
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, GetLastError,
        HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    NetworkManagement::IpHelper::{CancelIPChangeNotify, NotifyAddrChange},
    System::{
        IO::{GetOverlappedResult, OVERLAPPED},
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
    },
};

static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeError {
    Active,
    Event(u32),
    Reset(u32),
    Register(u32),
    Wait(u32),
    Completion(u32),
}

struct Permit;

impl Permit {
    fn acquire() -> Result<Self, NativeError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| NativeError::Active)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

struct Native {
    event: HANDLE,
    borrowed_file: HANDLE,
    overlap: Box<UnsafeCell<OVERLAPPED>>,
    _permit: Permit,
}

impl Native {
    fn open() -> Result<Self, NativeError> {
        let permit = Permit::acquire()?;
        // SAFETY: unnamed manual-reset event with default security.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            // SAFETY: immediately following the failed event creation on this thread.
            return Err(NativeError::Event(unsafe { GetLastError() }));
        }
        Ok(Self {
            event,
            borrowed_file: null_mut(),
            overlap: Box::new(UnsafeCell::new(OVERLAPPED {
                hEvent: event,
                ..Default::default()
            })),
            _permit: permit,
        })
    }
}

impl Registration for Native {
    type Error = NativeError;

    fn arm(&mut self) -> Result<(), NativeError> {
        // SAFETY: the driver only arms after a known terminal completion.
        if unsafe { ResetEvent(self.event) } == 0 {
            return Err(NativeError::Reset(unsafe { GetLastError() }));
        }
        let overlapped = self.overlap.get();
        // SAFETY: the previous request has terminated. The stable heap allocation
        // and event remain live until terminal completion or process teardown.
        unsafe {
            overlapped.write(OVERLAPPED {
                hEvent: self.event,
                ..Default::default()
            })
        };
        self.borrowed_file = null_mut();
        // SAFETY: both out parameters are live. The OS may mutate only the
        // OVERLAPPED after this call; the driver's wait synchronizes its use.
        let status = unsafe { NotifyAddrChange(&mut self.borrowed_file, overlapped) };
        if status == ERROR_IO_PENDING {
            Ok(())
        } else {
            Err(NativeError::Register(status))
        }
    }

    fn wait(&mut self, milliseconds: u32) -> Result<Completion, NativeError> {
        // SAFETY: event remains open while the request is pending.
        match unsafe { WaitForSingleObject(self.event, milliseconds) } {
            WAIT_TIMEOUT => Ok(Completion::Idle),
            WAIT_OBJECT_0 => self.completion(),
            _ => Err(NativeError::Wait(unsafe { GetLastError() })),
        }
    }

    fn request_cancel(&mut self) -> bool {
        // SAFETY: this is the outstanding request's exact OVERLAPPED pointer.
        // Return alone is not used as proof that its storage can be reclaimed.
        unsafe { CancelIPChangeNotify(self.overlap.get()) != 0 }
    }

    fn completion(&mut self) -> Result<Completion, NativeError> {
        let mut bytes = 0;
        // SAFETY: the API permits use of the borrowed file with its live
        // OVERLAPPED. This call is nonblocking even when the event did not
        // signal. No returned address/record data is read.
        if unsafe { GetOverlappedResult(self.borrowed_file, self.overlap.get(), &mut bytes, 0) }
            != 0
        {
            return Ok(Completion::Changed);
        }
        // SAFETY: immediately after the failed call on this thread.
        let error = unsafe { GetLastError() };
        match error {
            ERROR_OPERATION_ABORTED => Ok(Completion::Cancelled),
            ERROR_IO_INCOMPLETE => Ok(Completion::Idle),
            _ => Err(NativeError::Completion(error)),
        }
    }

    fn retain(self) {
        // Windows documents process-exit cleanup. Retain at most one event,
        // allocation and lease on any uncertain cancellation; never reuse it.
        mem::forget(self);
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: Watch drops only unarmed or acknowledged terminal requests.
        // The output file handle belongs to IP Helper and is never closed here.
        unsafe { CloseHandle(self.event) };
    }
}

/// One explicit current-process IPv4 table watcher; no threads, network
/// requests or native callbacks are started automatically.
pub struct AddressWatcher(Watch<Native>);

impl AddressWatcher {
    pub fn start() -> Result<Self, NativeError> {
        Watch::start(Native::open()?).map(Self)
    }

    /// Wait at most 500 ms. No state or connectivity authority is inferred.
    pub fn wait(&mut self, timeout: Duration) -> Result<Wake<NativeError>, WaitError<NativeError>> {
        self.0.wait(timeout)
    }

    /// Retry an arm explicitly, after a failed rearm or cancellation.
    pub fn rearm(&mut self) -> Result<(), NativeError> {
        self.0.rearm()
    }

    /// At most one 250 ms wait; returns whether terminal cleanup was observed.
    pub fn close(mut self) -> Close {
        self.0.close()
    }
}
