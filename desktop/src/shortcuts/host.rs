use super::{dispatch, local, native};
use cumulus_core::{
    client::{MusicClient, dto::ShortcutSettingsView, native_media::NativeShortcutInvocation},
    preferences::shortcuts::{Registration, RegistrationLedger, RegistrationStatus, Scope, Ticket},
};
use serde::Serialize;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tauri::{Emitter, Manager};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotInstalled,
    Starting,
    Ready,
    Recording,
    Damaged,
    ReadFailed,
    Unavailable,
    Stopped,
}
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    #[serde(flatten)]
    pub registration: Registration,
    pub status: RegistrationStatus,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub revision: String,
    pub settings_revision: Option<String>,
    pub ticket: Option<String>,
    pub recording: Option<String>,
    pub phase: Phase,
    pub rows: Vec<Row>,
}
struct Inner {
    revision: u64,
    settings_revision: Option<String>,
    phase: Phase,
    ledger: Option<RegistrationLedger>,
    local: Option<local::Listener>,
    local_serial: u64,
    recording: Option<String>,
}
pub(super) struct Host {
    pub operations: Mutex<()>,
    inner: Mutex<Inner>,
    sender: Mutex<Option<mpsc::Sender<native::Command>>>,
    pub busy: AtomicBool,
    pub stopped: AtomicBool,
}
impl Inner {
    fn view(&self) -> Status {
        Status {
            revision: self.revision.to_string(),
            settings_revision: self.settings_revision.clone(),
            ticket: self.ledger.as_ref().map(|l| l.ticket().id()),
            recording: self.recording.clone(),
            phase: self.phase,
            rows: self
                .ledger
                .as_ref()
                .map(|l| {
                    l.rows()
                        .map(|(r, s)| Row {
                            registration: r.clone(),
                            status: s,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}
fn publish<R: tauri::Runtime>(app: &tauri::AppHandle<R>, inner: &mut Inner) {
    inner.revision = inner.revision.saturating_add(1);
    let value = inner.view();
    let app = app.clone();
    // Never synchronously call native listeners under state/operation locks.
    tauri::async_runtime::spawn(async move {
        let _ = app.emit_to("main", "cumulus:shortcuts", value);
    });
}
pub fn status<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Status {
    app.try_state::<Host>()
        .and_then(|h| h.inner.lock().ok().map(|i| i.view()))
        .unwrap_or(Status {
            revision: "0".into(),
            settings_revision: None,
            ticket: None,
            recording: None,
            phase: Phase::NotInstalled,
            rows: vec![],
        })
}
/// Install after MusicClient exists. SQLite and OS grabs run off the GUI thread.
pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if app.try_state::<Host>().is_some() || app.try_state::<MusicClient>().is_none() {
        return;
    }
    app.manage(Host {
        operations: Mutex::new(()),
        inner: Mutex::new(Inner {
            revision: 0,
            settings_revision: None,
            phase: Phase::Starting,
            ledger: None,
            local: None,
            local_serial: 0,
            recording: None,
        }),
        sender: Mutex::new(None),
        busy: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
    });
    let events = app.clone();
    let sender = native::start(move |event| match event {
        native::Event::Pressed(ticket, key) => dispatch::pressed(&events, ticket, key),
        native::Event::Registered(ticket, rows) => {
            let Some(host) = events.try_state::<Host>() else {
                return;
            };
            if host.stopped.load(Ordering::Acquire) {
                return;
            }
            let Ok(mut inner) = host.inner.lock() else {
                return;
            };
            let Some(ledger) = inner.ledger.as_mut() else {
                return;
            };
            if ledger.ticket() != ticket {
                return;
            }
            for (key, status) in rows {
                ledger.acknowledge(&ticket, key, Scope::Global, status);
            }
            inner.phase = if inner.recording.is_some() {
                Phase::Recording
            } else {
                Phase::Ready
            };
            publish(&events, &mut inner);
        }
    });
    *app.state::<Host>()
        .sender
        .lock()
        .expect("new shortcut host") = Some(sender);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let Ok(_operation) = host.operations.lock() else {
            return;
        };
        match app.state::<MusicClient>().shortcut_settings_native() {
            Ok(value) => apply(&app, &value),
            Err(_) => unavailable(&app),
        }
    });
}
fn send(host: &Host, command: native::Command) -> bool {
    host.sender
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.send(command).is_ok()))
        .unwrap_or(false)
}
pub(super) fn apply<R: tauri::Runtime>(app: &tauri::AppHandle<R>, value: &ShortcutSettingsView) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    if host.stopped.load(Ordering::Acquire) {
        return;
    }
    let Some(settings) = value.current.as_ref().filter(|_| !value.damaged) else {
        retire(app, Phase::Damaged);
        return;
    };
    let Ok(mut ledger) = RegistrationLedger::new(settings) else {
        retire(app, Phase::Damaged);
        return;
    };
    let ticket = ledger.ticket();
    let rows: Vec<_> = ledger.rows().map(|(r, _)| r.clone()).collect();
    let recording = host
        .inner
        .lock()
        .ok()
        .is_some_and(|i| i.recording.is_some());
    let mut native_rows = vec![];
    for row in rows {
        if recording {
            ledger.acknowledge(
                &ticket,
                row.hotkey,
                row.scope,
                RegistrationStatus::Unavailable { code: 5 },
            );
        } else if !NativeShortcutInvocation::supported(row.hotkey) {
            ledger.acknowledge(
                &ticket,
                row.hotkey,
                row.scope,
                RegistrationStatus::Unavailable { code: 1 },
            );
        } else if row.scope == Scope::Local {
            // Local WebView dispatch must acknowledge its own listener. A saved
            // row alone is never evidence that the frontend installed it.
            ledger.acknowledge(
                &ticket,
                row.hotkey,
                row.scope,
                RegistrationStatus::Unavailable { code: 4 },
            );
        } else {
            native_rows.push(row);
        }
    }
    {
        let Ok(mut inner) = host.inner.lock() else {
            return;
        };
        if host.stopped.load(Ordering::Acquire) {
            return;
        }
        inner.ledger = Some(ledger);
        inner.local = None;
        inner.settings_revision = Some(settings.revision.clone());
        inner.phase = Phase::Starting;
        publish(app, &mut inner);
    }
    // Retire callbacks before sending replacement to the OS worker.
    if !send(&host, native::Command::Replace(ticket, native_rows)) {
        retire(app, Phase::Unavailable);
    }
}
fn retire<R: tauri::Runtime>(app: &tauri::AppHandle<R>, phase: Phase) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    if let Ok(mut inner) = host.inner.lock() {
        if phase != Phase::Stopped && host.stopped.load(Ordering::Acquire) {
            return;
        }
        inner.ledger = None;
        inner.local = None;
        inner.recording = None;
        inner.settings_revision = None;
        inner.phase = phase;
        publish(app, &mut inner);
    }
    // An empty plan invalidates and releases all existing global grabs.
    if let Ok(ledger) = RegistrationLedger::new(&Default::default()) {
        send(&host, native::Command::Replace(ledger.ticket(), vec![]));
    }
}
pub(super) fn unavailable<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    retire(app, Phase::ReadFailed);
}
pub(super) fn active(
    host: &Host,
    ticket: &Ticket,
    key: cumulus_core::preferences::shortcuts::Hotkey,
) -> bool {
    !host.stopped.load(Ordering::Acquire)
        && host.inner.lock().ok().is_some_and(|i| {
            i.ledger
                .as_ref()
                .is_some_and(|l| l.active(ticket, key, Scope::Global))
        })
}
pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>() {
        host.stopped.store(true, Ordering::Release);
        retire(app, Phase::Stopped);
        if let Ok(mut sender) = host.sender.lock()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(native::Command::Stop);
        }
    }
}

pub(super) fn attach<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Host,
    epoch: String,
    ticket: String,
) -> cumulus_core::Result<local::Lease> {
    use cumulus_core::Error;
    let mut inner = host
        .inner
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    if inner.recording.is_some()
        || host.stopped.load(Ordering::Acquire)
        || inner
            .ledger
            .as_ref()
            .is_none_or(|l| l.ticket().id() != ticket)
    {
        return Err(Error::StaleResponse);
    }
    inner.local_serial = inner
        .local_serial
        .checked_add(1)
        .ok_or(Error::State("shortcut listener exhausted"))?;
    let id = inner.local_serial.to_string();
    inner.local = Some(local::Listener {
        id: id.clone(),
        epoch,
    });
    let ledger = inner.ledger.as_mut().expect("checked ledger");
    let token = ledger.ticket();
    let keys: Vec<_> = ledger
        .rows()
        .filter(|(r, _)| r.scope == Scope::Local && NativeShortcutInvocation::supported(r.hotkey))
        .map(|(r, _)| r.hotkey)
        .collect();
    for key in keys {
        ledger.acknowledge(&token, key, Scope::Local, RegistrationStatus::Active);
    }
    publish(app, &mut inner);
    Ok(local::Lease { id, ticket })
}
pub(super) fn detach<R: tauri::Runtime>(app: &tauri::AppHandle<R>, lease: Option<&str>) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut inner) = host.inner.lock() else {
        return;
    };
    if inner
        .local
        .as_ref()
        .is_none_or(|l| lease.is_some_and(|id| id != l.id))
    {
        return;
    }
    inner.local = None;
    if let Some(ledger) = inner.ledger.as_mut() {
        let ticket = ledger.ticket();
        let keys: Vec<_> = ledger
            .rows()
            .filter(|(r, s)| r.scope == Scope::Local && *s == RegistrationStatus::Active)
            .map(|(r, _)| r.hotkey)
            .collect();
        for key in keys {
            ledger.acknowledge(
                &ticket,
                key,
                Scope::Local,
                RegistrationStatus::Unavailable { code: 4 },
            );
        }
    }
    publish(app, &mut inner);
}
pub(super) fn local_key(
    host: &Host,
    epoch: &str,
    lease: &str,
    ticket: &str,
    chord: &cumulus_core::preferences::shortcuts::Chord,
) -> cumulus_core::Result<cumulus_core::preferences::shortcuts::Hotkey> {
    use cumulus_core::Error;
    let inner = host
        .inner
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    if host.stopped.load(Ordering::Acquire)
        || inner
            .local
            .as_ref()
            .is_none_or(|l| l.id != lease || l.epoch != epoch)
    {
        return Err(Error::StaleResponse);
    }
    let ledger = inner
        .ledger
        .as_ref()
        .filter(|l| l.ticket().id() == ticket)
        .ok_or(Error::StaleResponse)?;
    ledger
        .rows()
        .find(|(r, s)| {
            r.scope == Scope::Local && r.chord == *chord && *s == RegistrationStatus::Active
        })
        .map(|(r, _)| r.hotkey)
        .ok_or(Error::StaleResponse)
}

pub(super) fn record(host: &Host) -> cumulus_core::Result<String> {
    use cumulus_core::Error;
    let mut inner = host
        .inner
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    if host.stopped.load(Ordering::Acquire) {
        return Err(Error::StaleResponse);
    }
    if inner.recording.is_some() {
        return Err(Error::MutationBusy);
    }
    inner.local_serial = inner
        .local_serial
        .checked_add(1)
        .ok_or(Error::State("shortcut listener exhausted"))?;
    let id = inner.local_serial.to_string();
    inner.recording = Some(id.clone());
    Ok(id)
}
pub(super) fn finish_recording(host: &Host, id: Option<&str>) -> bool {
    let Ok(mut inner) = host.inner.lock() else {
        return false;
    };
    if inner
        .recording
        .as_ref()
        .is_none_or(|r| id.is_some_and(|id| id != r))
    {
        return false;
    }
    inner.recording = None;
    true
}
