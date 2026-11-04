//! Dispatch only into the owner application's active, ready player generation.
use super::wire::{READY, Reply, receive, same_user, send_code};
use cumulus_core::{Error, client::MusicClient, playback::engine::Cancellation};
use std::{
    io,
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Manager, Runtime};

const TICK: Duration = Duration::from_millis(20);

pub(crate) struct Relay(Arc<AtomicBool>);
impl Relay {
    pub(crate) fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) fn install<R: Runtime>(app: &AppHandle<R>, listener: UnixListener) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let stopped = Arc::new(AtomicBool::new(false));
    app.manage(Relay(stopped.clone()));
    let app = app.clone();
    thread::Builder::new()
        .name("cumulus-file-open-relay".into())
        .spawn(move || {
            let busy = Arc::new(AtomicBool::new(false));
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if same_user(&stream).is_err() {
                            continue;
                        }
                        if busy.swap(true, Ordering::AcqRel) {
                            let _ = send_code(&mut stream, Reply::Busy as u8);
                            continue;
                        }
                        let app = app.clone();
                        let work = Busy(busy.clone());
                        let stopped = stopped.clone();
                        let _ = thread::Builder::new()
                            .name("cumulus-file-open-request".into())
                            .spawn(move || {
                                let _work = work;
                                if send_code(&mut stream, READY).is_err() {
                                    return;
                                }
                                let reply = match receive(&mut stream) {
                                    Ok(paths) if !stopped.load(Ordering::Acquire) => {
                                        dispatch(&app, paths, stopped.clone())
                                    }
                                    Ok(_) => Reply::Unavailable,
                                    Err(_) => Reply::Invalid,
                                };
                                let _ = send_code(&mut stream, reply as u8);
                            });
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(TICK),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        })?;
    Ok(())
}

struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    paths: Vec<PathBuf>,
    stopped: Arc<AtomicBool>,
) -> Reply {
    let Some(client) = app.try_state::<MusicClient>() else {
        return Reply::Unavailable;
    };
    let Ok(account) = client.account() else {
        return Reply::Unavailable;
    };
    if app.get_webview_window("main").is_none()
        || !app
            .state::<crate::player::PlayerStreams>()
            .ready_for_native_open(&account.epoch)
    {
        return Reply::Unavailable;
    }
    let alive = stopped.clone();
    let authority = Cancellation::new(move || !alive.load(Ordering::Acquire));
    match tauri::async_runtime::block_on(client.play_native_open_files_warm(
        &account.epoch,
        paths,
        authority,
    )) {
        Ok(_) => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if !stopped.load(Ordering::Acquire) {
                    crate::lifecycle::show(&handle);
                }
            });
            Reply::Accepted
        }
        Err(error) => match error.root() {
            Error::StaleResponse | Error::Cancelled | Error::QueueChanged => Reply::Unavailable,
            Error::InvalidInput(_) => Reply::Invalid,
            _ => Reply::Unknown,
        },
    }
}
