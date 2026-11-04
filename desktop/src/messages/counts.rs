//! One native poller; the main renderer only reports the message sheet's
//! visibility and consumes count-only snapshots.
use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::{
        ClientError, MusicClient,
        messages::{MessageCounts, PcMessageConnectivity, PcMessageConnectivityView},
    },
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Manager, ipc::Channel};

#[derive(Default)]
pub struct Host(Mutex<Slot>);

#[derive(Default)]
struct Slot {
    poller: Option<PcMessageConnectivity>,
    stream: Option<Stream>,
    window_visible: bool,
    closed: bool,
}

struct Stream {
    id: String,
    sequence: u32,
    requested: Option<(u64, bool)>,
    applied: Option<(u64, bool)>,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountView {
    epoch: String,
    counts: Option<MessageCounts>,
    refreshing: bool,
    failed: bool,
}

fn view(value: &PcMessageConnectivityView) -> CountView {
    CountView {
        epoch: value.epoch.clone(),
        counts: value.counts.clone(),
        refreshing: value.count_read_in_flight,
        failed: value.last_count_read_failed,
    }
}

pub fn start_message_count_polling<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
    let host = app.state::<Host>();
    let mut state = host.0.lock().map_err(|_| Error::LockPoisoned)?;
    if state.closed {
        return Err(Error::Cancelled);
    }
    if state.poller.is_none() {
        let client = app.state::<MusicClient>();
        let poller =
            tauri::async_runtime::block_on(async { client.start_pc_message_count_polling() })?;
        state.poller = Some(poller);
    }
    Ok(())
}

pub fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>()
        && let Ok(mut state) = host.0.lock()
    {
        state.closed = true;
        if let Some(stream) = state.stream.take() {
            stream.task.abort();
        }
        drop(state.poller.take());
    }
}

fn window_visible<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    matches!(window.is_visible(), Ok(true)) && matches!(window.is_minimized(), Ok(false))
}

pub fn window_visibility<R: tauri::Runtime>(app: &tauri::AppHandle<R>, known_hidden: bool) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    if host.0.lock().map_or(true, |state| state.stream.is_none()) {
        return;
    }
    let visible = !known_hidden
        && app
            .get_webview_window("main")
            .as_ref()
            .is_some_and(window_visible);
    if let Ok(mut state) = host.0.lock() {
        state.window_visible = visible;
        let _ = apply_visibility(&mut state);
    }
}

fn apply_visibility(state: &mut Slot) -> Result<()> {
    let Some((stream, poller)) = state.stream.as_mut().zip(state.poller.as_ref()) else {
        return Ok(());
    };
    if let Some((generation, requested)) = stream.requested {
        let target = (generation, requested && state.window_visible);
        if stream.applied != Some(target) {
            poller.try_drawer_visible(target.0, target.1)?;
            stream.applied = Some(target);
        }
    }
    Ok(())
}

fn detach(state: &mut Slot) -> Result<()> {
    if let Some(stream) = state.stream.as_ref()
        && let Some((generation, true)) = stream.applied
        && let Some(poller) = state.poller.as_ref()
    {
        poller.try_drawer_visible(generation, false)?;
    }
    if let Some(stream) = state.stream.take() {
        stream.task.abort();
    }
    Ok(())
}

fn detach_page<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: Option<&str>) {
    if let Some(host) = app.try_state::<Host>()
        && let Ok(mut state) = host.0.lock()
        && state
            .stream
            .as_ref()
            .is_some_and(|stream| id.is_none_or(|id| stream.id == id))
    {
        let _ = detach(&mut state);
    }
}

pub fn lifecycle<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("message-count-window")
        .on_page_load(|window, payload| {
            if window.label() == "main" && payload.event() == tauri::webview::PageLoadEvent::Started
            {
                detach_page(window.app_handle(), None);
            }
        })
        .build()
}

fn stream_id_valid(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

#[tauri::command]
pub fn watch_message_counts<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    host: tauri::State<'_, Host>,
    stream_id: String,
    on_update: Channel<CountView>,
) -> std::result::Result<(), ClientError> {
    if window.label() != "main" || !stream_id_valid(&stream_id) {
        return Err(Error::InvalidInput("invalid message count stream").into());
    }
    let visible = window_visible(&window);
    let mut state = host.0.lock().map_err(|_| Error::LockPoisoned)?;
    let mut source = state
        .poller
        .as_ref()
        .ok_or(Error::State("message count host unavailable"))?
        .subscribe();
    on_update
        .send(view(&source.borrow_and_update()))
        .map_err(|_| Error::Cancelled)?;
    detach(&mut state)?;
    state.window_visible = visible;
    let app = window.app_handle().clone();
    let identity = stream_id.clone();
    let task = tauri::async_runtime::spawn(async move {
        while source.changed().await.is_ok() {
            let next = view(&source.borrow_and_update());
            if on_update.send(next).is_err() {
                break;
            }
        }
        detach_page(&app, Some(&identity));
    });
    state.stream = Some(Stream {
        id: stream_id,
        sequence: 0,
        requested: None,
        applied: None,
        task,
    });
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CountAction {
    Visibility {
        epoch: String,
        sequence: u32,
        visible: bool,
    },
    Refresh {
        epoch: String,
    },
    Detach,
}

fn generation(client: &MusicClient, epoch: &str) -> Result<u64> {
    let active = client.account()?;
    if active.epoch != epoch || active.status != SessionStatus::Authenticated {
        return Err(Error::StaleResponse);
    }
    epoch
        .parse()
        .map_err(|_| Error::InvalidInput("invalid message count epoch"))
}

#[tauri::command]
pub fn message_count_action<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    host: tauri::State<'_, Host>,
    client: tauri::State<'_, MusicClient>,
    stream_id: String,
    action: CountAction,
) -> std::result::Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main message count stream required").into());
    }
    let visible = window_visible(&window);
    let mut state = host.0.lock().map_err(|_| Error::LockPoisoned)?;
    let stream = state
        .stream
        .as_mut()
        .filter(|stream| stream.id == stream_id)
        .ok_or(Error::StaleResponse)?;
    match action {
        CountAction::Detach => detach(&mut state)?,
        CountAction::Refresh { epoch } => {
            let owner = generation(&client, &epoch)?;
            state
                .poller
                .as_ref()
                .ok_or(Error::Cancelled)?
                .try_refresh_counts(owner)?;
        }
        CountAction::Visibility {
            epoch,
            sequence,
            visible: requested,
        } => {
            let owner = generation(&client, &epoch)?;
            if sequence <= stream.sequence {
                return Err(Error::StaleResponse.into());
            }
            stream.sequence = sequence;
            stream.requested = Some((owner, requested));
            state.window_visible = visible;
            apply_visibility(&mut state)?;
        }
    }
    Ok(())
}
