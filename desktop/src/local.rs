use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
use serde::Deserialize;
use std::path::PathBuf;
use tauri::{
    Manager,
    ipc::{InvokeBody, Request},
};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn local_match_view(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    ids: Vec<String>,
) -> Result<AccountData<LocalMatchView>, ClientError> {
    client
        .local_match_view(&epoch, ids)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn local_match_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: LocalMatchCommand,
) -> Result<(), ClientError> {
    client
        .local_match_command(&epoch, command)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn local_page(
    client: tauri::State<'_, MusicClient>,
    query: LocalQuery,
) -> Result<LocalPageView, ClientError> {
    client.local_page(query).await.map_err(Into::into)
}
#[tauri::command]
pub async fn local_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    command: LocalCommand,
) -> Result<(), ClientError> {
    if matches!(
        command,
        LocalCommand::HideTrack { .. }
            | LocalCommand::HideGroup { .. }
            | LocalCommand::HideTracks { .. }
    ) && window.label() != "main"
    {
        return Err(Error::InvalidInput("local list removal requires main window").into());
    }
    client.local_command(command).await.map_err(Into::into)
}
#[tauri::command]
pub async fn open_local_track_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    id: String,
    revision: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("local directory action requires main window").into());
    }
    let path = client.local_track_directory(id, revision).await?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn reveal_local_folder<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    query: LocalQuery,
    revision: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("local folder reveal requires main window").into());
    }
    let path = client.local_folder_directory(query, revision).await?;
    crate::file_manager::reveal_directory(path)
        .await
        .map_err(Into::into)
}

fn require_directory_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("local directory selection requires main window").into());
    }
    Ok(())
}

#[tauri::command]
pub async fn local_directory_selection<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
) -> Result<DirectorySelection, ClientError> {
    require_directory_window(&window)?;
    client.local_directory_selection().await.map_err(Into::into)
}

#[tauri::command]
pub async fn stage_local_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    revision: String,
) -> Result<Option<DirectorySelection>, ClientError> {
    let Some(path) = pick_directory(&window).await? else {
        return Ok(None);
    };
    client
        .stage_local_directory(path, revision)
        .await
        .map(Some)
        .map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectorySelectionInput {
    revision: String,
    choices: Vec<DirectoryChoice>,
}

#[tauri::command]
pub async fn apply_local_directories<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<DirectorySelection, ClientError> {
    require_directory_window(&window)?;
    let invalid = || Error::InvalidInput("invalid local directory selection");
    let input = match request.body() {
        InvokeBody::Json(body) if body.is_object() => {
            DirectorySelectionInput::deserialize(body).map_err(|_| invalid())?
        }
        _ => return Err(invalid().into()),
    };
    client
        .apply_local_directories(input.revision, input.choices)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn pick_local_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
) -> Result<Option<String>, ClientError> {
    pick_directory(&window)
        .await?
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| ClientError::from(Error::InvalidInput("directory must be UTF-8")))
        })
        .transpose()
}

async fn pick_directory<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<Option<PathBuf>, ClientError> {
    require_directory_window(window)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let mut picker = window
        .dialog()
        .file()
        .set_parent(window)
        .set_title("选择本地音乐目录");
    let directory = window
        .app_handle()
        .path()
        .audio_dir()
        .ok()
        .filter(|p| p.is_dir())
        .or_else(|| {
            window
                .app_handle()
                .path()
                .home_dir()
                .ok()
                .filter(|p| p.is_dir())
        });
    if let Some(path) = directory {
        picker = picker.set_directory(path);
    }
    picker.pick_folder(move |path| {
        let _ = send.send(path);
    });
    receive
        .await
        .map_err(|_| ClientError::from(Error::Cancelled))?
        .map(|p| {
            p.into_path().map_err(|_| {
                ClientError::from(Error::InvalidInput("unsupported directory location"))
            })
        })
        .transpose()
}

fn require_collection_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("local collection requires main window").into());
    }
    Ok(())
}

#[tauri::command]
pub async fn local_collect_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    selection: LocalCollectSelection,
) -> Result<AccountData<LocalCollectPlan>, ClientError> {
    require_collection_window(&window)?;
    client
        .local_collect_prepare(&epoch, selection)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn local_collect_destinations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    sort: PlaylistCollectSort,
    page: u32,
) -> Result<AccountData<PlaylistCollectPage>, ClientError> {
    require_collection_window(&window)?;
    client
        .local_collect_destinations(&epoch, &receipt, sort, page)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn local_collect_existing<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    target: String,
    reconcile: bool,
) -> Result<AccountData<LocalCollectResult>, ClientError> {
    require_collection_window(&window)?;
    client
        .local_collect_existing(&epoch, &receipt, &target, reconcile)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn local_collect_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
) -> Result<(), ClientError> {
    require_collection_window(&window)?;
    client
        .local_collect_cancel(&epoch, &receipt)
        .map_err(Into::into)
}
