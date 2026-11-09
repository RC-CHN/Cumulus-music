//! Bounded Win32 transport; only validated paths reach the native player.
use super::{
    effective::{AccessShape, Connector, HELLO},
    identity::{Identity, local_path},
    security::{open_handle, wide, with_logon_security},
    stable::{Server, Stop},
    wire,
};
use crate::native_open::warm::{Reply, dispatch_windows, ready_windows};
use std::{
    io,
    os::windows::io::{AsRawHandle, IntoRawHandle},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, Runtime};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::NamedPipeClient,
    time::timeout,
};
use windows_sys::Win32::Foundation::{ERROR_NO_DATA, ERROR_PIPE_BUSY};

const IO_LIMIT: Duration = Duration::from_secs(2);
const ACK_LIMIT: Duration = Duration::from_secs(20);
const READY: u8 = 6;
const CONSUMED: u8 = 7;

pub(crate) struct Listener {
    pipe: Server,
    stop: Stop,
}

impl Listener {
    pub fn bind(address: &str, identity: &Identity) -> io::Result<Self> {
        let address = wide(address);
        let stop = Stop::new()?;
        // SAFETY: private derived name is NUL-terminated and the protected
        // security descriptor remains alive for CreateNamedPipeW.
        let pipe = with_logon_security(identity, |attributes| unsafe {
            Server::create(&address, attributes, stop.clone())
        })?;
        Ok(Self { pipe, stop })
    }
}

pub(crate) struct Relay {
    stopped: Arc<AtomicBool>,
    wake: Stop,
}
impl Relay {
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.wake.signal();
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn install<R: Runtime>(app: &AppHandle<R>, listener: Listener) -> io::Result<()> {
    let access = AccessShape::current().inspect_err(|_error| {
        trace_handoff!("owner.current_token_denied", _error.raw_os_error());
    })?;
    let Listener { mut pipe, stop } = listener;
    let stopped = Arc::new(AtomicBool::new(false));
    app.manage(Relay {
        stopped: stopped.clone(),
        wake: stop,
    });
    let app = app.clone();
    thread::Builder::new()
        .name("cumulus-file-open-relay".into())
        .spawn(move || {
            trace_handoff!("owner.relay_running");
            while !stopped.load(Ordering::Acquire) {
                match pipe.connect(Instant::now() + Duration::from_secs(60)) {
                    Ok(()) => trace_handoff!("owner.pipe_connected"),
                    Err(error)
                        if error.kind() == io::ErrorKind::TimedOut
                            || error.raw_os_error() == Some(ERROR_NO_DATA as i32) =>
                    {
                        continue;
                    }
                    Err(_error) => {
                        trace_handoff!("owner.pipe_connect_failed", _error.raw_os_error());
                        break;
                    }
                }
                let attestation = if stopped.load(Ordering::Acquire) {
                    Connector::Denied
                } else {
                    access.authenticate(&mut pipe, Instant::now() + IO_LIMIT, |_stage, _code| {
                        trace_handoff!(_stage, _code);
                    })
                };
                if attestation == Connector::RetireWorker {
                    break;
                }
                if attestation == Connector::Verified {
                    trace_handoff!("owner.sender_authenticated");
                    let Some(reply) = handle(&app, &mut pipe, &access, stopped.clone()) else {
                        break;
                    };
                    if pipe
                        .write_all(&receipt(reply as u8), Instant::now() + IO_LIMIT)
                        .inspect_err(|_error| {
                            trace_handoff!(
                                "owner.final_receipt_write_failed",
                                _error.raw_os_error()
                            );
                        })
                        .is_ok()
                    {
                        // DisconnectNamedPipe may discard a receipt still in the
                        // buffer. Allow its consumer to confirm or close, with a
                        // fixed deadline and never FlushFileBuffers.
                        let _ = pipe.read_exact(&mut [0u8; 1], Instant::now() + IO_LIMIT);
                    }
                }
                if let Err(_error) = pipe.disconnect() {
                    trace_handoff!("owner.pipe_disconnect_failed", _error.raw_os_error());
                    break;
                }
            }
        })
        .inspect_err(|_error| trace_handoff!("owner.relay_spawn_failed", _error.raw_os_error()))?;
    Ok(())
}

fn handle<R: Runtime>(
    app: &AppHandle<R>,
    pipe: &mut Server,
    access: &AccessShape,
    stopped: Arc<AtomicBool>,
) -> Option<Reply> {
    let Some(permit) = ready_windows(app) else {
        return Some(Reply::Unavailable);
    };
    if let Err(_error) = pipe.write_all(&receipt(READY), Instant::now() + IO_LIMIT) {
        trace_handoff!("owner.ready_write_failed", _error.raw_os_error());
        return Some(Reply::Unavailable);
    }
    trace_handoff!("owner.ready_sent");
    Some(match receive(pipe) {
        Ok(paths) if !stopped.load(Ordering::Acquire) => match access
            .verify_connector(pipe.as_raw_handle())
        {
            Connector::Verified if access.verify_process(pipe.as_raw_handle(), true).is_ok() => {
                trace_handoff!("owner.dispatch_entered");
                dispatch_windows(app, paths, stopped, permit)
            }
            Connector::RetireWorker => return None,
            Connector::Verified => {
                trace_handoff!("owner.sender_process_recheck_denied");
                Reply::Unavailable
            }
            Connector::Denied => {
                trace_handoff!("owner.sender_effective_token_recheck_denied");
                Reply::Unavailable
            }
        },
        Ok(_) => {
            trace_handoff!("owner.relay_stopped_before_dispatch");
            Reply::Unavailable
        }
        Err(_error) => {
            trace_handoff!("owner.frame_rejected", _error.raw_os_error());
            Reply::Invalid
        }
    })
}

fn receive(pipe: &mut Server) -> io::Result<Vec<PathBuf>> {
    let deadline = Instant::now() + IO_LIMIT;
    let mut header = [0; 8];
    pipe.read_exact(&mut header, deadline)?;
    let mut body = vec![0; wire::size(header)?];
    pipe.read_exact(&mut body, deadline)?;
    wire::decode(&body)?
        .into_iter()
        .map(|value| {
            let path = PathBuf::from(value);
            if !local_path(&path) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            Ok(path)
        })
        .collect()
}

fn receipt(code: u8) -> [u8; 5] {
    [
        wire::MAGIC[0],
        wire::MAGIC[1],
        wire::MAGIC[2],
        wire::MAGIC[3],
        code,
    ]
}

async fn read_code(pipe: &mut NamedPipeClient, limit: Duration) -> io::Result<u8> {
    let mut bytes = [0; 5];
    timeout(limit, pipe.read_exact(&mut bytes)).await??;
    if bytes[..4] != wire::MAGIC[..] {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(bytes[4])
}

async fn send(address: &str, access: &AccessShape, frame: &[u8]) -> Reply {
    let raw = match open_handle(address) {
        Ok(handle) => handle.into_raw_handle(),
        Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
            trace_handoff!("sender.pipe_busy", error.raw_os_error());
            return Reply::Busy;
        }
        Err(_error) => {
            trace_handoff!("sender.pipe_open_failed", _error.raw_os_error());
            return Reply::Unavailable;
        }
    };
    trace_handoff!("sender.pipe_opened");
    // SAFETY: unique fresh overlapped handle, transferred to Tokio on all paths.
    let mut pipe = match unsafe { NamedPipeClient::from_raw_handle(raw) } {
        Ok(pipe) => pipe,
        Err(_error) => {
            trace_handoff!("sender.pipe_registration_failed", _error.raw_os_error());
            return Reply::Unavailable;
        }
    };
    if let Err(_error) = access.verify_process(pipe.as_raw_handle(), false) {
        trace_handoff!("sender.owner_process_denied", _error.raw_os_error());
        return Reply::Unavailable;
    }
    if !matches!(
        timeout(IO_LIMIT, pipe.write_all(&[HELLO])).await,
        Ok(Ok(()))
    ) {
        trace_handoff!("sender.hello_write_failed");
        return Reply::Unavailable;
    }
    match read_code(&mut pipe, IO_LIMIT).await {
        Ok(READY) => trace_handoff!("sender.ready_received"),
        Ok(2) => {
            trace_handoff!("sender.owner_not_ready");
            return Reply::Unavailable;
        }
        Ok(_) => {
            trace_handoff!("sender.unexpected_ready_reply");
            return Reply::Unavailable;
        }
        Err(_error) => {
            trace_handoff!("sender.ready_read_failed", _error.raw_os_error());
            return Reply::Unavailable;
        }
    }
    if let Err(_error) = access.verify_process(pipe.as_raw_handle(), false) {
        trace_handoff!("sender.owner_process_recheck_denied", _error.raw_os_error());
        return Reply::Unavailable;
    }
    if !matches!(timeout(IO_LIMIT, pipe.write_all(frame)).await, Ok(Ok(()))) {
        trace_handoff!("sender.frame_write_unconfirmed");
        return Reply::Unknown;
    }
    trace_handoff!("sender.frame_written");
    let reply = match read_code(&mut pipe, ACK_LIMIT).await {
        Ok(0) => Reply::Accepted,
        Ok(2) => Reply::Unavailable,
        Ok(3) => Reply::Invalid,
        Ok(5) => Reply::Busy,
        _ => Reply::Unknown,
    };
    trace_handoff!(match reply {
        Reply::Accepted => "sender.accepted",
        Reply::Unavailable => "sender.dispatched_unavailable",
        Reply::Invalid => "sender.frame_invalid",
        Reply::Unknown => "sender.dispatch_unconfirmed",
        Reply::Busy => "sender.dispatched_busy",
    });
    // The result is already authenticated; this byte only lets the owner
    // release its single slot sooner. A lost result never causes retransmission.
    let _ = timeout(IO_LIMIT, pipe.write_all(&[CONSUMED])).await;
    reply
}

pub(super) fn forward(address: &str, identity: &Identity, paths: &[PathBuf]) -> Reply {
    let Some(paths) = paths
        .iter()
        .map(|path| path.to_str().filter(|_| local_path(path)))
        .collect::<Option<Vec<_>>>()
    else {
        return Reply::Invalid;
    };
    let Ok(frame) = wire::encode(&paths) else {
        return Reply::Invalid;
    };
    let Ok(access) = AccessShape::current() else {
        trace_handoff!("sender.current_token_denied");
        return Reply::Unavailable;
    };
    if !access.matches_identity(identity) {
        trace_handoff!("sender.current_token_changed");
        return Reply::Unavailable;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    else {
        trace_handoff!("sender.runtime_failed");
        return Reply::Unavailable;
    };
    runtime.block_on(send(address, &access, &frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn same_logon_uses_the_retained_listener_and_consumes_each_receipt_once() {
        let root = tempfile::tempdir().unwrap();
        let identity = Identity::current().unwrap();
        let address = identity.address(root.path()).unwrap();
        let Listener { mut pipe, .. } = Listener::bind(&address, &identity).unwrap();
        let expected = vec![
            PathBuf::from(r"C:\合成\歌 曲 01.mp3"),
            PathBuf::from(r"C:\合成\歌 曲 02.flac"),
        ];
        let server_expected = expected.clone();
        let (reset, next) = mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(move || {
                let access = AccessShape::current().unwrap();
                for outcome in [Reply::Accepted, Reply::Invalid] {
                    pipe.connect(Instant::now() + IO_LIMIT).unwrap();
                    assert_eq!(
                        access.authenticate(&mut pipe, Instant::now() + IO_LIMIT, |_, _| {}),
                        Connector::Verified
                    );
                    pipe.write_all(&receipt(READY), Instant::now() + IO_LIMIT)
                        .unwrap();
                    assert_eq!(receive(&mut pipe).unwrap(), server_expected);
                    assert_eq!(
                        access.verify_connector(pipe.as_raw_handle()),
                        Connector::Verified
                    );
                    pipe.write_all(&receipt(outcome as u8), Instant::now() + IO_LIMIT)
                        .unwrap();
                    let mut done = [0];
                    pipe.read_exact(&mut done, Instant::now() + IO_LIMIT)
                        .unwrap();
                    assert_eq!(done, [CONSUMED]);
                    pipe.disconnect().unwrap();
                    reset.send(()).unwrap();
                }
            });
            let identity = Identity::current().unwrap();
            for outcome in [Reply::Accepted, Reply::Invalid] {
                assert_eq!(forward(&address, &identity, &expected), outcome);
                next.recv_timeout(IO_LIMIT).unwrap();
            }
        });
    }

    #[test]
    fn waiting_for_ready_never_sends_paths_to_an_unready_player() {
        let root = tempfile::tempdir().unwrap();
        let identity = Identity::current().unwrap();
        let address = identity.address(root.path()).unwrap();
        let Listener { mut pipe, .. } = Listener::bind(&address, &identity).unwrap();
        thread::scope(|scope| {
            scope.spawn(move || {
                let access = AccessShape::current().unwrap();
                pipe.connect(Instant::now() + IO_LIMIT).unwrap();
                assert_eq!(
                    access.authenticate(&mut pipe, Instant::now() + IO_LIMIT, |_, _| {}),
                    Connector::Verified
                );
                pipe.write_all(
                    &receipt(Reply::Unavailable as u8),
                    Instant::now() + IO_LIMIT,
                )
                .unwrap();
                let mut byte = [0];
                assert!(
                    pipe.read_exact(&mut byte, Instant::now() + IO_LIMIT)
                        .is_err()
                );
                pipe.disconnect().unwrap();
            });
            let identity = Identity::current().unwrap();
            assert_eq!(
                forward(&address, &identity, &[PathBuf::from(r"C:\合成\歌.flac")]),
                Reply::Unavailable
            );
        });
    }
}
