use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::{MusicClient, dto::AccountData},
};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Default)]
pub(crate) struct Host(Mutex<Option<tauri::async_runtime::JoinHandle<()>>>);

pub(super) fn observe<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    client: &MusicClient,
) -> Result<()> {
    let host = app.state::<Host>();
    let mut task = host.0.lock().map_err(|_| Error::LockPoisoned)?;
    if task.is_some() {
        return Ok(());
    }
    let mut changes = client.subscribe_history_sync();
    let handle = app.clone();
    *task = Some(tauri::async_runtime::spawn(async move {
        loop {
            let observed = *changes.borrow_and_update();
            if let Some((epoch, value)) = observed {
                let epoch = epoch.to_string();
                if handle
                    .state::<MusicClient>()
                    .account()
                    .is_ok_and(|current| {
                        current.epoch == epoch && current.status == SessionStatus::Authenticated
                    })
                {
                    let _ = handle.emit_to(
                        "main",
                        "cumulus:history-sync-changed",
                        AccountData { epoch, value },
                    );
                }
            }
            if changes.changed().await.is_err() {
                break;
            }
        }
    }));
    Ok(())
}

pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>()
        && let Ok(mut task) = host.0.lock()
        && let Some(task) = task.take()
    {
        task.abort();
    }
}
