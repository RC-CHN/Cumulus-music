use super::*;
use cumulus_core::{
    client::{
        ClientError,
        desktop_lyrics::DesktopLyricView,
        dto::{DesktopLyricPreferencesCommand, DesktopLyricPreferencesView},
        mini::{MiniAction, MiniView},
    },
    preferences::desktop_lyrics::{Edit, rgba},
};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    generation: String,
    ticket: String,
    media: MiniView,
    preferences: DesktopLyricPreferencesView,
    colors: Vec<[u8; 4]>,
    alignments: [String; 2],
    locked: bool,
    state: state::ContentState,
    lyrics: Option<DesktopLyricView>,
    error: Option<&'static str>,
}
#[tauri::command]
pub async fn desktop_lyric_read<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
) -> std::result::Result<View, ClientError> {
    let client = app.state::<MusicClient>();
    let snapshot = client.mini_snapshot()?;
    let preferences = client
        .desktop_lyric_preferences(&snapshot.view.epoch)?
        .value;
    let Some(current) = preferences.current.as_ref() else {
        gui(&app, move |app| {
            dismiss(&app, Some(&generation));
            Ok(())
        })
        .await?;
        return Err(Error::State("desktop lyric settings damaged").into());
    };
    let appearance = current.appearance.clone();
    let (ticket, locked, error) = {
        let host = app.state::<Host>();
        let mut ledger = host.lock()?;
        let ticket = ledger.observe(&generation, snapshot.clone())?;
        (ticket, ledger.get(&generation)?.locked, ledger.error)
    };
    let (state, lyrics) = content::read(&app, &generation, &snapshot, &appearance).await?;
    let colors = appearance.palette.colors();
    let colors = colors
        .text
        .iter()
        .chain(colors.outline.iter())
        .map(|v| rgba(v))
        .collect::<Result<Vec<_>>>()?;
    Ok(View {
        generation,
        ticket,
        media: snapshot.view,
        preferences,
        colors,
        alignments: appearance.alignments().map(str::to_owned),
        locked,
        state,
        lyrics,
        error,
    })
}
#[tauri::command]
pub async fn desktop_lyric_ready<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
) -> std::result::Result<(), ClientError> {
    gui(&app, move |app| {
        let label = {
            let host = app.state::<Host>();
            let ledger = host.lock()?;
            let w = ledger.get(&generation)?;
            if w.ready {
                return Err(Error::StaleResponse);
            }
            w.label.clone()
        };
        app.get_webview_window(&label)
            .ok_or(Error::StaleResponse)?
            .show()
            .map_err(|_| Error::State("desktop lyric show failed"))?;
        let host = app.state::<Host>();
        let mut ledger = host.lock()?;
        let w = ledger.get_mut(&generation)?;
        w.ready = true;
        w.loaded = true;
        Ok(())
    })
    .await
    .map_err(Into::into)
}
pub(super) async fn close<R: Runtime>(app: &tauri::AppHandle<R>, generation: &str) -> Result<()> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    host.lock()?.get(generation)?;
    // Position reporting is unavailable on some Wayland compositors. A failed
    // optional position read must not trap a user in the lyric window.
    let worker = app.clone();
    let current = generation.to_owned();
    let _ = tauri::async_runtime::spawn_blocking(move || geometry::save(&worker, &current)).await;
    let client = app.state::<MusicClient>();
    let epoch = client.account()?.epoch;
    let saved = client.desktop_lyric_preferences(&epoch)?.value.current;
    host.lock()?.get(generation)?;
    if let Some(saved) = saved {
        client.desktop_lyric_preferences_command(
            &epoch,
            DesktopLyricPreferencesCommand::Change {
                expected_revision: saved.revision,
                edit: Box::new(Edit::Enabled { value: false }),
            },
        )?;
    }
    let generation = generation.to_owned();
    gui(app, move |app| {
        dismiss(&app, Some(&generation));
        Ok(())
    })
    .await
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Toggle,
    Previous,
    Next,
    OffsetBack,
    OffsetForward,
    Retry,
    Lock,
    Close,
    Details,
    Settings,
}
#[tauri::command]
pub async fn desktop_lyric_action<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    action: Action,
) -> std::result::Result<(), ClientError> {
    let snapshot = app.state::<Host>().lock()?.captured(&generation, &ticket)?;
    if !snapshot.same_controls(&app.state::<MusicClient>().mini_snapshot()?) {
        return Err(Error::StaleResponse.into());
    }
    if matches!(action, Action::Close) {
        return close(&app, &generation).await.map_err(Into::into);
    }
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    match action {
        Action::Toggle | Action::Previous | Action::Next => {
            let action = match action { Action::Previous => MiniAction::Previous, Action::Next => MiniAction::Next, _ => MiniAction::Toggle };
            app.state::<MusicClient>().mini_action(&snapshot, action).await?;
        }
        Action::OffsetBack | Action::OffsetForward | Action::Retry => {
            let cache = host.lock()?.get(&generation)?.content.clone();
            let mut content = cache.try_lock().map_err(|_| Error::MutationBusy)?;
            if matches!(action, Action::Retry) {
                content.key = None; content.document = None; content.state = state::ContentState::Loading;
            } else {
                let document = content.document.as_mut().ok_or(Error::MutationBusy)?;
                let gain = if matches!(action, Action::OffsetBack) { -1.0 } else { 1.0 };
                app.state::<MusicClient>().desktop_lyric_offset(document, gain).await?;
            }
        }
        Action::Lock => gui(&app, move |app| {
            if !snapshot.same_controls(&app.state::<MusicClient>().mini_snapshot()?) { return Err(Error::StaleResponse); }
            let label = app.state::<Host>().lock()?.get(&generation)?.label.clone();
            if !crate::lifecycle::view(&app).is_ok_and(|v| v.tray_available) {
                let main = app.get_webview_window("main").ok_or(Error::State("lyric unlock surface unavailable"))?;
                main.show().and_then(|_| main.unminimize()).map_err(|_| Error::State("lyric unlock surface unavailable"))?;
            }
            app.get_webview_window(&label).ok_or(Error::StaleResponse)?.set_ignore_cursor_events(true)
                .map_err(|_| Error::State("desktop lyric lock failed"))?;
            app.state::<Host>().lock()?.get_mut(&generation)?.locked = true;
            Ok(())
        }).await?,
        Action::Details | Action::Settings => gui(&app, move |app| {
            if !snapshot.same_controls(&app.state::<MusicClient>().mini_snapshot()?) { return Err(Error::StaleResponse); }
            app.state::<Host>().lock()?.get(&generation)?;
            crate::lifecycle::show(&app);
            app.emit_to("main", "cumulus:desktop-lyric-open", serde_json::json!({"epoch": snapshot.view.epoch, "playId": snapshot.view.selection.audio_play_id, "settings": matches!(action, Action::Settings)}))
                .map_err(|_| Error::State("main lyric surface unavailable"))?;
            Ok(())
        }).await?,
        Action::Close => unreachable!(),
    }
    Ok(())
}
