//! Native registrations live on one worker, never on the GUI or SQLite thread.
//! No persisted preference is an acknowledgement from the window system.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;
use cumulus_core::preferences::shortcuts::{
    Hotkey, Registration, RegistrationStatus, Scope, Ticket,
};
#[cfg(target_os = "linux")]
use linux::Backend;
use std::{collections::BTreeMap, sync::mpsc, time::Duration};
#[cfg(target_os = "windows")]
use windows::Backend;

#[derive(Clone, Copy, Debug)]
pub(super) enum Failure {
    Conflict,
    Unsupported,
    Unavailable,
    Fatal,
}
impl Failure {
    fn status(self) -> RegistrationStatus {
        match self {
            Self::Conflict => RegistrationStatus::Conflict { code: 1 },
            Self::Unsupported => RegistrationStatus::Unavailable { code: 2 },
            Self::Unavailable | Self::Fatal => RegistrationStatus::Unavailable { code: 3 },
        }
    }
}
pub(super) enum Command {
    Replace(Ticket, Vec<Registration>),
    Stop,
}
pub(super) enum Event {
    Registered(Ticket, Vec<(Hotkey, RegistrationStatus)>),
    Pressed(Ticket, Hotkey),
}
pub(super) fn start(callback: impl Fn(Event) + Send + 'static) -> mpsc::Sender<Command> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || run(receiver, callback));
    sender
}
fn run(receiver: mpsc::Receiver<Command>, callback: impl Fn(Event)) {
    let mut backend: Option<Backend> = None;
    let mut registered = BTreeMap::new();
    // Never reuse Windows WM_HOTKEY ids: queued events from a retired map must
    // not acquire the meaning of a replacement binding. Exhaustion fails closed.
    let mut next_id = 1_u32;
    let mut current: Option<(Ticket, Vec<Registration>)> = None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(8)) {
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Replace(ticket, rows)) => {
                registered.clear();
                let cleared = backend.as_mut().is_none_or(|b| b.clear().is_ok());
                backend.take();
                if !cleared {
                    report_unavailable(&callback, &ticket, &rows);
                    // Windows releases thread-owned registrations on exit.
                    // Do not retry a new registration over an uncertain cleanup.
                    break;
                }
                let mut statuses = vec![];
                let mut fatal = false;
                if !rows.is_empty() {
                    backend = Backend::open().ok();
                }
                for row in &rows {
                    debug_assert_eq!(row.scope, Scope::Global);
                    let result = match backend.as_mut() {
                        Some(b) if next_id <= 0xbfff && !fatal => b.register(next_id, &row.chord),
                        _ => Err(Failure::Unavailable),
                    };
                    let status = match result {
                        Ok(()) => {
                            registered.insert(next_id, row.hotkey);
                            RegistrationStatus::Active
                        }
                        Err(error) => {
                            fatal |= matches!(error, Failure::Fatal);
                            error.status()
                        }
                    };
                    statuses.push((row.hotkey, status));
                    next_id = next_id.saturating_add(1);
                }
                current = Some((ticket.clone(), rows.clone()));
                if fatal {
                    registered.clear();
                    backend.take();
                    report_unavailable(&callback, &ticket, &rows);
                } else {
                    callback(Event::Registered(ticket, statuses));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
        if let Some(b) = backend.as_mut() {
            match b.poll() {
                Ok(ids) => {
                    if let Some((ticket, _)) = &current {
                        for id in ids {
                            if let Some(&key) = registered.get(&id) {
                                callback(Event::Pressed(ticket.clone(), key));
                            }
                        }
                    }
                }
                Err(_) => {
                    registered.clear();
                    backend.take();
                    if let Some((ticket, rows)) = &current {
                        report_unavailable(&callback, ticket, rows);
                    }
                }
            }
        }
    }
}
fn report_unavailable(callback: &impl Fn(Event), ticket: &Ticket, rows: &[Registration]) {
    callback(Event::Registered(
        ticket.clone(),
        rows.iter()
            .map(|r| (r.hotkey, Failure::Unavailable.status()))
            .collect(),
    ));
}
