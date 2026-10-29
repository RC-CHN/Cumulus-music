use super::*;
use cumulus_core::client::{
    ClientError,
    dto::{MiniPreferencesCommand, MiniPreferencesView},
    mini::{MiniAction, MiniQueuePage, MiniView},
};
use serde::Serialize;
use tauri::Emitter;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    generation: String,
    ticket: String,
    media: MiniView,
    preferences: MiniPreferencesView,
}
#[tauri::command]
pub async fn mini_enter<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> std::result::Result<(), ClientError> {
    enter(&app).await.map_err(Into::into)
}
#[tauri::command]
pub async fn mini_read<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
) -> std::result::Result<View, ClientError> {
    tauri::async_runtime::spawn_blocking(move || -> Result<View> {
        app.state::<Host>().lock()?.get(&generation)?;
        let client = app.state::<MusicClient>();
        let snapshot = client.mini_snapshot()?;
        let preferences = client.mini_preferences(&snapshot.view.epoch)?.value;
        let media = snapshot.view.clone();
        let ticket = app.state::<Host>().lock()?.observe(&generation, snapshot)?;
        Ok(View {
            generation,
            ticket,
            media,
            preferences,
        })
    })
    .await
    .map_err(|_| Error::State("mini reader unavailable"))?
    .map_err(Into::into)
}
#[tauri::command]
pub async fn mini_ready<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
) -> std::result::Result<(), ClientError> {
    gui(&app, move |app| {
        let label = app.state::<Host>().lock()?.get(&generation)?.label.clone();
        let lifecycle = crate::lifecycle::view(&app)
            .map_err(|_| Error::State("window lifecycle unavailable"))?;
        if lifecycle.busy || lifecycle.prompt.is_some() {
            dismiss(&app, Some(&generation), true);
            return Err(Error::MutationBusy);
        }
        let mini = app.get_webview_window(&label).ok_or(Error::StaleResponse)?;
        let main = app.get_webview_window("main").ok_or(Error::StaleResponse)?;
        {
            let host = app.state::<Host>();
            let ledger = host.lock()?;
            let pending = ledger.get(&generation)?;
            if pending.open_epoch != app.state::<MusicClient>().account()?.epoch {
                return Err(Error::StaleResponse);
            }
            if pending.phase != Phase::Opening || pending.snapshot.is_none() {
                return Err(Error::StaleResponse);
            }
        }
        let result = mini
            .show()
            .and_then(|_| mini.set_focus())
            .and_then(|_| main.hide());
        if result.is_err() {
            dismiss(&app, Some(&generation), true);
            return Err(Error::State("mini window handover failed"));
        }
        app.state::<Host>().lock()?.ready(&generation)?;
        Ok(())
    })
    .await
    .map_err(Into::into)
}
#[tauri::command]
pub async fn mini_return<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
) -> std::result::Result<(), ClientError> {
    gui(&app, move |app| {
        app.state::<Host>().lock()?.get(&generation)?;
        if !dismiss(&app, Some(&generation), true) {
            return Err(Error::State("main window unavailable"));
        }
        crate::lifecycle::show(&app);
        Ok(())
    })
    .await
    .map_err(Into::into)
}
#[tauri::command]
pub async fn mini_action<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    action: MiniAction,
) -> std::result::Result<(), ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let snapshot = host.lock()?.captured(&generation, &ticket)?;
    let client = app.state::<MusicClient>();
    let like = if matches!(action, MiniAction::Like | MiniAction::RefreshLike) {
        client.native_like_selection()?
    } else {
        None
    };
    let result = client.mini_action(&snapshot, action).await;
    if let Some(like) = like {
        let _ = app.emit_to(
            "main",
            "cumulus:native-like-changed",
            serde_json::json!({"epoch":like.epoch(),"resource":like.resource}),
        );
    }
    result.map_err(Into::into)
}
#[tauri::command]
pub async fn mini_queue<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    offset: usize,
) -> std::result::Result<MiniQueuePage, ClientError> {
    let snapshot = app.state::<Host>().lock()?.captured(&generation, &ticket)?;
    app.state::<MusicClient>()
        .mini_queue(&snapshot, offset, 30)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn mini_panel<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    expanded: bool,
) -> std::result::Result<(), ClientError> {
    gui(&app, move |app| {
        let label = app.state::<Host>().lock()?.get(&generation)?.label.clone();
        app.get_webview_window(&label)
            .ok_or(Error::StaleResponse)?
            .set_size(tauri::LogicalSize::new(
                420.0,
                if expanded { 516.0 } else { 206.0 },
            ))
            .map_err(|_| Error::State("mini resize failed"))
    })
    .await
    .map_err(Into::into)
}
#[tauri::command]
pub async fn mini_preferences_command<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    command: MiniPreferencesCommand,
) -> std::result::Result<(), ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let snapshot = host.lock()?.captured(&generation, &ticket)?;
    let epoch = snapshot.view.epoch;
    let worker = app.clone();
    let value = tauri::async_runtime::spawn_blocking(move || {
        worker
            .state::<MusicClient>()
            .mini_preferences_command(&epoch, command)
    })
    .await
    .map_err(|_| Error::State("mini settings unavailable"))??;
    gui(&app, move |app| {
        let label = app.state::<Host>().lock()?.get(&generation)?.label.clone();
        app.get_webview_window(&label)
            .ok_or(Error::StaleResponse)?
            .set_always_on_top(value.value.always_on_top)
            .map_err(|_| Error::State("mini pin failed"))
    })
    .await
    .map_err(Into::into)
}
