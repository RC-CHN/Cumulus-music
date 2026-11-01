use super::*;
use crate::desktop_lyrics::mini_menu::{Action, Snapshot, View as LyricView};
use cumulus_core::client::ClientError;
use serde::Serialize;
use tauri::Emitter;

#[tauri::command]
pub async fn mini_lyric_report<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
) -> std::result::Result<(), ClientError> {
    let captured = app.state::<Host>().lock()?.captured(&generation, &ticket)?;
    let report = app
        .state::<MusicClient>()
        .mini_lyric_report_open(&captured)
        .await?;
    gui(&app, move |app| {
        if !captured.same_controls(&app.state::<MusicClient>().mini_snapshot()?) {
            return Err(Error::StaleResponse);
        }
        app.state::<Host>().lock()?.get(&generation)?;
        if crate::lifecycle::view(&app)
            .map_err(|_| Error::State("window lifecycle unavailable"))?
            .busy
        {
            return Err(Error::MutationBusy);
        }
        if !dismiss(&app, Some(&generation), true) {
            return Err(Error::State("main window unavailable"));
        }
        app.emit_to(
            "main",
            "cumulus:mini-return",
            serde_json::json!({"epoch":report.epoch,"fm":false}),
        )
        .map_err(|_| Error::State("main window handoff unavailable"))?;
        app.emit_to(
            "main",
            "cumulus:lyric-report-open",
            serde_json::json!({"epoch":report.epoch,"id":report.id}),
        )
        .map_err(|_| Error::State("lyric report handoff unavailable"))?;
        Ok(())
    })
    .await
    .map_err(Into::into)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    ticket: String,
    #[serde(flatten)]
    lyrics: LyricView,
}
#[tauri::command]
pub async fn mini_lyric_menu_read<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
) -> std::result::Result<View, ClientError> {
    tauri::async_runtime::spawn_blocking(move || -> Result<View> {
        let host = app.state::<Host>();
        let (shown, serial) = {
            let mut ledger = host.lock()?;
            let shown = ledger.captured(&generation, &ticket)?;
            let window = ledger.get_mut(&generation)?;
            window.lyric_menu_serial = window
                .lyric_menu_serial
                .checked_add(1)
                .ok_or(Error::State("mini menu exhausted"))?;
            (shown, window.lyric_menu_serial)
        };
        let snapshot = Snapshot::capture(&app, &shown.view.epoch)?;
        let lyrics = snapshot.view();
        let mut ledger = host.lock()?;
        let window = ledger.get_mut(&generation)?;
        if window.ticket.to_string() != ticket || window.lyric_menu_serial != serial {
            return Err(Error::StaleResponse);
        }
        if let Some((ticket, previous)) = &window.lyric_menu
            && previous.same(&snapshot)
        {
            return Ok(View {
                ticket: ticket.clone(),
                lyrics,
            });
        }
        let ticket = serial.to_string();
        window.lyric_menu = Some((ticket.clone(), snapshot));
        Ok(View { ticket, lyrics })
    })
    .await
    .map_err(|_| Error::State("mini menu unavailable"))?
    .map_err(Into::into)
}
#[tauri::command]
pub async fn mini_lyric_menu_action<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    menu_ticket: String,
    action: Action,
) -> std::result::Result<(), ClientError> {
    let host = app.state::<Host>();
    let shown = {
        let mut ledger = host.lock()?;
        let window = ledger.get_mut(&generation)?;
        if window.phase != Phase::Active
            || window
                .lyric_menu
                .as_ref()
                .is_none_or(|(ticket, _)| ticket != &menu_ticket)
        {
            return Err(Error::StaleResponse.into());
        }
        let snapshot = window.lyric_menu.take().ok_or(Error::StaleResponse)?.1;
        window.lyric_menu_serial = window
            .lyric_menu_serial
            .checked_add(1)
            .ok_or(Error::State("mini menu exhausted"))?;
        snapshot
    };
    shown.apply(&app, action).await.map_err(Into::into)
}
#[tauri::command]
pub async fn mini_exit<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
) -> std::result::Result<(), ClientError> {
    gui(&app, move |app| {
        if app.state::<Host>().lock()?.get(&generation)?.phase != Phase::Active {
            return Err(Error::StaleResponse);
        }
        let life = crate::lifecycle::view(&app)
            .map_err(|_| Error::State("window lifecycle unavailable"))?;
        if life.busy || life.prompt.is_some() {
            return Err(Error::MutationBusy);
        }
        if !dismiss(&app, Some(&generation), true) {
            return Err(Error::State("main window unavailable"));
        }
        crate::lifecycle::request_exit(&app);
        Ok(())
    })
    .await
    .map_err(Into::into)
}
