//! Cached device policy and actual native availability. No SQLite on GUI ticks.
use cumulus_core::client::{MusicClient, dto::SystemMediaPreferencesView};
use serde::Serialize;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{Emitter, Manager};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotInstalled,
    Starting,
    Disabled,
    Damaged,
    ReadFailed,
    Idle,
    Connecting,
    Active,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub revision: String,
    pub installed: bool,
    pub phase: Phase,
}
struct Snapshot {
    revision: u64,
    phase: Phase,
}
pub(super) struct Host {
    // Serializes startup/read/write through commit+cache update, so a late read
    // cannot re-enable a policy that a newer successful change disabled.
    pub operations: Mutex<()>,
    enabled: AtomicBool,
    fatal: AtomicBool,
    generation: AtomicU64,
    snapshot: Mutex<Snapshot>,
}
pub(super) fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    if app.try_state::<Host>().is_some() || app.try_state::<MusicClient>().is_none() {
        return false;
    }
    app.manage(Host {
        operations: Mutex::new(()),
        enabled: AtomicBool::new(false),
        fatal: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        snapshot: Mutex::new(Snapshot {
            revision: 0,
            phase: Phase::Starting,
        }),
    });
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let Ok(_operation) = host.operations.lock() else {
            return;
        };
        match app.state::<MusicClient>().system_media_preferences_native() {
            Ok(value) => apply(&app, &value),
            Err(_) => unavailable(&app),
        }
    });
    true
}
fn view(snapshot: &Snapshot) -> Status {
    Status {
        revision: snapshot.revision.to_string(),
        installed: true,
        phase: snapshot.phase,
    }
}
pub(super) fn status<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Status {
    app.try_state::<Host>()
        .and_then(|host| host.snapshot.lock().ok().map(|s| view(&s)))
        .unwrap_or(Status {
            revision: "0".into(),
            installed: false,
            phase: Phase::NotInstalled,
        })
}
fn update<R: tauri::Runtime>(app: &tauri::AppHandle<R>, snapshot: &mut Snapshot, phase: Phase) {
    if snapshot.phase != phase {
        snapshot.phase = phase;
        snapshot.revision = snapshot.revision.saturating_add(1);
        let value = view(snapshot);
        let app = app.clone();
        // Native listeners may synchronously read settings; never call them
        // while this thread holds the operation/snapshot mutexes.
        tauri::async_runtime::spawn(async move {
            let _ = app.emit_to("main", "cumulus:system-media", value);
        });
    }
}
pub(super) fn apply<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    value: &SystemMediaPreferencesView,
) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let enabled = value.enabled && !value.damaged;
    let Ok(mut snapshot) = host.snapshot.lock() else {
        return;
    };
    let previous = host.enabled.swap(enabled, Ordering::AcqRel);
    if previous != enabled {
        host.generation.fetch_add(1, Ordering::AcqRel);
    }
    let phase = if value.damaged {
        Phase::Damaged
    } else if !enabled {
        Phase::Disabled
    } else if host.fatal.load(Ordering::Acquire) {
        Phase::Unavailable
    } else if !previous {
        Phase::Idle
    } else {
        snapshot.phase
    };
    update(app, &mut snapshot, phase);
}
pub(super) fn unavailable<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut snapshot) = host.snapshot.lock() else {
        return;
    };
    host.enabled.store(false, Ordering::Release);
    host.generation.fetch_add(1, Ordering::AcqRel);
    update(app, &mut snapshot, Phase::ReadFailed);
}
pub(super) fn ticket<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<u64> {
    let host = app.try_state::<Host>()?;
    (host.enabled.load(Ordering::Acquire) && !host.fatal.load(Ordering::Acquire))
        .then(|| host.generation.load(Ordering::Acquire))
}
#[cfg(target_os = "windows")]
pub(super) fn failed_to_start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut snapshot) = host.snapshot.lock() else {
        return;
    };
    host.fatal.store(true, Ordering::Release);
    host.generation.fetch_add(1, Ordering::AcqRel);
    if host.enabled.load(Ordering::Acquire) {
        update(app, &mut snapshot, Phase::Unavailable);
    }
}
pub(super) fn valid<R: tauri::Runtime>(app: &tauri::AppHandle<R>, ticket: u64) -> bool {
    self::ticket(app) == Some(ticket)
}
pub(super) fn report<R: tauri::Runtime>(app: &tauri::AppHandle<R>, ticket: u64, phase: Phase) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut snapshot) = host.snapshot.lock() else {
        return;
    };
    if valid(app, ticket) {
        update(app, &mut snapshot, phase);
    }
}
