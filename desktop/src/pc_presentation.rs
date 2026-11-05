//! Native ownership for M147 startup/account/periodic display experiments.
use cumulus_core::{
    Error, Result,
    client::{ClientError, MusicClient, dto::*, pc_experiments::PcPresentationRuntimeLease},
};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Default)]
pub(crate) struct Host {
    runtime: Mutex<Option<PcPresentationRuntimeLease>>,
    observer: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// Explicit native setup only. A renderer snapshot never activates a runtime,
/// forces a remote experiment refresh, or supplies a cohort.
pub fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
    let host = app
        .try_state::<Host>()
        .ok_or(Error::State("PC presentation host is unavailable"))?;
    let mut runtime = host.runtime.lock().map_err(|_| Error::LockPoisoned)?;
    if runtime.is_some() {
        return Ok(());
    }
    let client = app.state::<MusicClient>();
    observe(app, &client)?;
    let lease = tauri::async_runtime::block_on(async { client.start_pc_presentation_runtime() })?;
    *runtime = Some(lease);
    Ok(())
}

fn observe<R: tauri::Runtime>(app: &tauri::AppHandle<R>, client: &MusicClient) -> Result<()> {
    let host = app
        .try_state::<Host>()
        .ok_or(Error::State("PC presentation host is unavailable"))?;
    let mut task = host.observer.lock().map_err(|_| Error::LockPoisoned)?;
    if task.is_some() {
        return Ok(());
    }
    let mut changes = client.subscribe_pc_presentation()?;
    let handle = app.clone();
    let observer = tauri::async_runtime::spawn(async move {
        loop {
            let value = changes.borrow_and_update().clone();
            let _ = handle.emit_to("main", "cumulus:pc-presentation", value);
            if changes.changed().await.is_err() {
                break;
            }
        }
    });
    *task = Some(observer);
    Ok(())
}

pub fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>() {
        if let Ok(mut runtime) = host.runtime.lock() {
            drop(runtime.take());
        }
        if let Ok(mut observer) = host.observer.lock()
            && let Some(task) = observer.take()
        {
            task.abort();
        }
    }
}

fn main_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> std::result::Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("PC presentation is main-window only").into())
    }
}

#[tauri::command]
pub fn pc_presentation<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> std::result::Result<PcPresentationView, ClientError> {
    main_window(&window)?;
    observe(window.app_handle(), &client).map_err(ClientError::from)?;
    let view = client.pc_presentation().map_err(ClientError::from)?;
    if view.epoch != epoch {
        return Err(Error::StaleResponse.into());
    }
    Ok(view)
}

#[tauri::command]
pub fn search_presentation<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    revision: String,
) -> std::result::Result<AccountData<SearchPresentationPatch>, ClientError> {
    main_window(&window)?;
    client
        .search_presentation(&epoch, &receipt, &revision)
        .map_err(Into::into)
}
