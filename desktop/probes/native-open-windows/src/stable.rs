//! Event-driven Win32 server: one retained handle and one operation at a time.
//! This is an isolated native lab adapter, not a desktop runtime.
use crate::wire;
use std::{
    cell::UnsafeCell,
    fmt, io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
    ptr,
    sync::Arc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED,
        HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile,
        WriteFile,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject},
    },
};

const DRAIN_MS: u32 = 2_000;

#[derive(Clone)]
pub(crate) struct Stop(Arc<OwnedHandle>);
impl Stop {
    pub(crate) fn new() -> io::Result<Self> {
        event().map(|handle| Self(Arc::new(handle)))
    }
    pub(crate) fn wait(&self, deadline: Instant) -> io::Result<()> {
        // SAFETY: a live event and bounded non-alertable wait.
        match unsafe { WaitForSingleObject(self.0.as_raw_handle(), wait_ms(deadline)) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err(io::ErrorKind::TimedOut.into()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub(crate) fn signal(&self) -> io::Result<()> {
        // SAFETY: the event remains live through this call.
        if unsafe { SetEvent(self.0.as_raw_handle()) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

fn event() -> io::Result<OwnedHandle> {
    // SAFETY: unnamed manual-reset event, non-inheritable, initially unset.
    let raw = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: unique ownership of the returned event handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum End {
    StoppedBeforeIo,
    PendingStopped,
    PendingDeadline,
    CompletionUnresolved,
}
impl fmt::Display for End {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StoppedBeforeIo => "native pipe stopped before I/O",
            Self::PendingStopped => "native pipe canceled and drained",
            Self::PendingDeadline => "native pipe deadline expired and drained",
            Self::CompletionUnresolved => "native pipe completion unresolved; listener disabled",
        })
    }
}
impl std::error::Error for End {}

impl End {
    fn error(self) -> io::Error {
        let kind = match self {
            Self::StoppedBeforeIo | Self::PendingStopped => io::ErrorKind::Interrupted,
            Self::PendingDeadline => io::ErrorKind::TimedOut,
            Self::CompletionUnresolved => io::ErrorKind::Other,
        };
        io::Error::new(kind, self)
    }

    pub(crate) fn is(&self, error: &io::Error) -> bool {
        error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<Self>())
            == Some(self)
    }
}

/// Heap-stable storage retained until the kernel confirms completion. The
/// kernel can mutate OVERLAPPED until then; Rust never reads it directly.
struct Operation {
    event: OwnedHandle,
    overlapped: Box<UnsafeCell<OVERLAPPED>>,
    bytes: UnsafeCell<Vec<u8>>,
}
impl Operation {
    fn new(bytes: Vec<u8>) -> io::Result<Self> {
        let event = event()?;
        let overlapped = Box::new(UnsafeCell::new(OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..OVERLAPPED::default()
        }));
        Ok(Self {
            event,
            overlapped,
            bytes: UnsafeCell::new(bytes),
        })
    }

    fn result(&self, pipe: HANDLE) -> io::Result<usize> {
        let mut bytes = 0;
        // SAFETY: same live pipe and OVERLAPPED used for submission, valid
        // output pointer, bWait false. Only this function observes completion.
        if unsafe { GetOverlappedResult(pipe, self.overlapped.get(), &mut bytes, 0) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(bytes as usize)
        }
    }
}

enum Kind {
    Connect,
    Read,
    Write,
}

pub(crate) struct Server {
    pipe: OwnedHandle,
    stop: Stop,
    connected: bool,
    disabled: bool,
    // Only used by the isolated lab's deterministic cancellation controls.
    pending_notice: Option<Stop>,
}

impl AsRawHandle for Server {
    fn as_raw_handle(&self) -> RawHandle {
        self.pipe.as_raw_handle()
    }
}

impl Server {
    /// # Safety
    /// Address must be a validated, NUL-terminated private local pipe name.
    /// Security descriptor and its storage must remain valid for the call.
    pub(crate) unsafe fn create(
        address: &[u16],
        security: &SECURITY_ATTRIBUTES,
        stop: Stop,
    ) -> io::Result<Self> {
        // SAFETY: the caller provides the live address and security descriptor.
        let pipe = unsafe {
            CreateNamedPipeW(
                address.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64,
                (wire::MAX + 8) as u32,
                0,
                security,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            // SAFETY: unique ownership of the freshly created pipe handle.
            pipe: unsafe { OwnedHandle::from_raw_handle(pipe) },
            stop,
            connected: false,
            disabled: false,
            pending_notice: None,
        })
    }

    pub(crate) fn notify_next_pending(&mut self, notice: Stop) {
        self.pending_notice = Some(notice);
    }

    pub(crate) fn connect(&mut self, deadline: Instant) -> io::Result<()> {
        if self.connected {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "pipe client still active",
            ));
        }
        self.run(Kind::Connect, Vec::new(), deadline)?;
        self.connected = true;
        Ok(())
    }

    pub(crate) fn disconnect(&mut self) -> io::Result<()> {
        self.require_connected()?;
        let result = disconnect(self.as_raw_handle());
        if result.is_ok() {
            self.connected = false;
        } else {
            // A handle in an unknown connection state must not accept again.
            self.disabled = true;
        }
        result
    }

    fn require_connected(&self) -> io::Result<()> {
        if self.disabled {
            Err(End::CompletionUnresolved.error())
        } else if !self.connected {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "no active pipe client",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn read_exact(&mut self, mut out: &mut [u8], deadline: Instant) -> io::Result<()> {
        self.require_connected()?;
        check_length(out.len())?;
        while !out.is_empty() {
            let (bytes, count) = self.run(Kind::Read, vec![0; out.len()], deadline)?;
            if count == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            out[..count].copy_from_slice(&bytes[..count]);
            out = &mut out[count..];
        }
        Ok(())
    }

    pub(crate) fn write_all(&mut self, mut input: &[u8], deadline: Instant) -> io::Result<()> {
        self.require_connected()?;
        check_length(input.len())?;
        while !input.is_empty() {
            let (_, count) = self.run(Kind::Write, input.to_vec(), deadline)?;
            if count == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            input = &input[count..];
        }
        Ok(())
    }

    fn run(
        &mut self,
        kind: Kind,
        bytes: Vec<u8>,
        deadline: Instant,
    ) -> io::Result<(Vec<u8>, usize)> {
        if self.disabled {
            return Err(End::CompletionUnresolved.error());
        }
        // SAFETY: live manual-reset event, zero timeout.
        match unsafe { WaitForSingleObject(self.stop.0.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => return Err(End::StoppedBeforeIo.error()),
            WAIT_TIMEOUT => {}
            _ => return Err(io::Error::last_os_error()),
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut operation = Operation::new(bytes)?;
        let pipe = self.as_raw_handle();
        let overlapped = operation.overlapped.get();
        let buffer = operation.bytes.get_mut();
        let len = buffer.len() as u32;
        // SAFETY: handle is overlapped, unique operation and owned buffer remain
        // alive without Rust access until known completion; Connect has no buffer.
        let started = unsafe {
            match kind {
                Kind::Connect => ConnectNamedPipe(pipe, overlapped),
                Kind::Read => ReadFile(pipe, buffer.as_mut_ptr(), len, ptr::null_mut(), overlapped),
                Kind::Write => WriteFile(pipe, buffer.as_ptr(), len, ptr::null_mut(), overlapped),
            }
        };
        let count = if started != 0 {
            if matches!(kind, Kind::Connect) {
                0
            } else {
                operation.result(pipe)?
            }
        } else {
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|code| code as u32) {
                Some(ERROR_PIPE_CONNECTED) if matches!(kind, Kind::Connect) => 0,
                Some(ERROR_IO_PENDING) => {
                    if let Some(notice) = self.pending_notice.take() {
                        // The operation must still be drained if signaling a
                        // test event fails; the observer will report failure.
                        let _ = notice.signal();
                    }
                    return self.complete(operation, kind, deadline);
                }
                _ => return Err(error),
            }
        };
        completed(operation, count)
    }

    fn complete(
        &mut self,
        operation: Operation,
        kind: Kind,
        deadline: Instant,
    ) -> io::Result<(Vec<u8>, usize)> {
        let pipe = self.as_raw_handle();
        let events = [self.stop.0.as_raw_handle(), operation.event.as_raw_handle()];
        // SAFETY: two live event handles, bounded non-alertable wait. Stop
        // comes first so simultaneous readiness closes rather than dispatches.
        let wait = unsafe {
            WaitForMultipleObjects(events.len() as u32, events.as_ptr(), 0, wait_ms(deadline))
        };
        let failure = if wait == WAIT_OBJECT_0 + 1 {
            match operation.result(pipe) {
                Ok(count) => {
                    return completed(
                        operation,
                        if matches!(kind, Kind::Connect) {
                            0
                        } else {
                            count
                        },
                    );
                }
                Err(error) if error.raw_os_error() != Some(ERROR_IO_INCOMPLETE as i32) => {
                    return Err(error);
                }
                Err(error) => error,
            }
        } else {
            match wait {
                WAIT_OBJECT_0 => End::PendingStopped.error(),
                WAIT_TIMEOUT => End::PendingDeadline.error(),
                WAIT_FAILED => io::Error::last_os_error(),
                _ => io::Error::other("unexpected native pipe wait outcome"),
            }
        };
        // SAFETY: cancel only the exact pending operation, then drain its
        // completion. ERROR_NOT_FOUND means it may have completed concurrently.
        unsafe { CancelIoEx(pipe, operation.overlapped.get()) };
        // SAFETY: live manual-reset event belonging exclusively to this operation.
        if unsafe { WaitForSingleObject(operation.event.as_raw_handle(), DRAIN_MS) }
            == WAIT_OBJECT_0
        {
            match operation.result(pipe) {
                Ok(_) => {
                    // A client may have connected just before cancellation.
                    // Close it rather than letting a later accept adopt it.
                    if matches!(kind, Kind::Connect)
                        && let Err(error) = disconnect(pipe)
                        && error.raw_os_error() != Some(ERROR_PIPE_NOT_CONNECTED as i32)
                    {
                        self.disabled = true;
                        return Err(error);
                    }
                    return Err(failure);
                }
                Err(error) if error.raw_os_error() != Some(ERROR_IO_INCOMPLETE as i32) => {
                    return Err(failure);
                }
                Err(_) => {}
            }
        }
        // Never free storage that the OS may still reference. Disable and
        // retire this handle; at most one bounded allocation can be retained
        // per failed listener. No loop may restart this poisoned owner.
        self.disabled = true;
        mem::forget(operation);
        Err(End::CompletionUnresolved.error())
    }
}

fn disconnect(pipe: HANDLE) -> io::Result<()> {
    // SAFETY: a live pipe whose outstanding operation, if any, has completed.
    if unsafe { DisconnectNamedPipe(pipe) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn completed(operation: Operation, count: usize) -> io::Result<(Vec<u8>, usize)> {
    let bytes = operation.bytes.into_inner();
    if count > bytes.len() {
        return Err(io::Error::other(
            "native pipe transferred beyond the buffer",
        ));
    }
    Ok((bytes, count))
}

fn check_length(length: usize) -> io::Result<()> {
    if length > wire::MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native pipe buffer exceeds limit",
        ));
    }
    Ok(())
}

fn wait_ms(deadline: Instant) -> u32 {
    let duration = deadline.saturating_duration_since(Instant::now());
    if duration == Duration::ZERO {
        return 0;
    }
    (duration.as_millis() + 1).min(u128::from(u32::MAX - 1)) as u32
}
