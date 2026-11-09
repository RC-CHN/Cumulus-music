//! Dispatch only into the owner application's active, ready player generation.
use super::super::warm::{Reply, dispatch};
use super::wire::{READY, receive, same_user, send_code};
use std::{
    io,
    os::unix::net::UnixListener,
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
                                        dispatch(&app, paths, stopped.clone(), None)
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
