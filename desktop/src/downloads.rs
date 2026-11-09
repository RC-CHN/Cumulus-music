use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
use serde::Deserialize;
use tauri::ipc::{InvokeBody, Request};
use tauri_plugin_dialog::DialogExt;

fn require_collect_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("download collection requires main window").into())
    }
}

#[tauri::command]
pub fn download_collect_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    task_id: String,
) -> Result<AccountData<DownloadCollectPlan>, ClientError> {
    require_collect_window(&window)?;
    client
        .download_collect_prepare(&epoch, &task_id)
        .map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DownloadBatchCollectInput {
    epoch: String,
    task_ids: Vec<String>,
}

#[tauri::command]
pub async fn download_batch_collect_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<AccountData<DownloadBatchCollectPlan>, ClientError> {
    require_collect_window(&window)?;
    let invalid = || Error::InvalidInput("invalid download collection selection");
    let input = match request.body() {
        InvokeBody::Json(body) if body.is_object() => {
            DownloadBatchCollectInput::deserialize(body).map_err(|_| invalid())?
        }
        _ => return Err(invalid().into()),
    };
    client
        .download_batch_collect_prepare(&input.epoch, input.task_ids)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn download_collect_destinations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    sort: PlaylistCollectSort,
    page: u32,
) -> Result<AccountData<PlaylistCollectPage>, ClientError> {
    require_collect_window(&window)?;
    client
        .download_collect_destinations(&epoch, &receipt, sort, page)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn download_collect_existing<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    playlist_id: String,
) -> Result<AccountData<PlaylistMutationView>, ClientError> {
    require_collect_window(&window)?;
    client
        .download_collect_existing(&epoch, &receipt, &playlist_id)
        .await
        .map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadCollectionCreationInput {
    epoch: String,
    receipt: String,
    command: DownloadCollectCreationCommand,
}

#[tauri::command]
pub async fn download_collect_creation<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<AccountData<DownloadCollectCreation>, ClientError> {
    require_collect_window(&window)?;
    let invalid = || Error::InvalidInput("invalid scoped download collection creation");
    let input = match request.body() {
        InvokeBody::Json(body) if body.is_object() => {
            DownloadCollectionCreationInput::deserialize(body).map_err(|_| invalid())?
        }
        _ => return Err(invalid().into()),
    };
    if input.receipt.len() != 32 || !input.receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid().into());
    }
    client
        .download_collect_creation(&input.epoch, &input.receipt, input.command)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn download_collect_liked_destination<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
) -> Result<AccountData<Option<DownloadLikedDestination>>, ClientError> {
    require_collect_window(&window)?;
    client
        .download_collect_liked_destination(&epoch, &receipt)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn download_collect_liked<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    destination_receipt: String,
    reconcile: bool,
) -> Result<AccountData<PlaylistMutationView>, ClientError> {
    require_collect_window(&window)?;
    client
        .download_collect_liked(&epoch, &receipt, &destination_receipt, reconcile)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn download_collect_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
) -> Result<(), ClientError> {
    require_collect_window(&window)?;
    client
        .download_collect_cancel(&epoch, &receipt)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn downloads(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: DownloadQuery,
) -> Result<DownloadsView, ClientError> {
    client.downloads(&epoch, query).await.map_err(Into::into)
}
#[tauri::command]
pub async fn download_comment_count<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    task_id: String,
) -> Result<AccountData<Option<u64>>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("download comment menu requires main window").into());
    }
    client
        .download_comment_count(
            &epoch,
            &task_id,
            crate::network_diagnostics::observed_default_route(&window),
        )
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn download_song_share_access<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    task_id: String,
) -> Result<AccountData<DownloadShareAccess>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("download song sharing requires main window").into());
    }
    client
        .download_song_share_access(&epoch, &task_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn download_quality_preferences<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<DownloadQualityPreferences, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("download quality settings require main window").into());
    }
    if refresh {
        client.refresh_download_quality_preferences(&epoch).await
    } else {
        client.download_quality_preferences(&epoch)
    }
    .map_err(Into::into)
}
#[tauri::command]
pub async fn song_download_state(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<SongDownloadState, ClientError> {
    client
        .song_download_state(&epoch, &id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn voice_download_state<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<SongDownloadState, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("voice download state requires main window").into());
    }
    client
        .voice_download_state(&epoch, &id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn download_clear_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<DownloadClearPlan, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("download clearing requires main window").into());
    }
    client
        .download_clear_prepare(&epoch)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn download_clear_discard<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("download clearing requires main window").into());
    }
    client
        .download_clear_discard(&epoch, &receipt)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn download_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: DownloadCommand,
) -> Result<DownloadResult, ClientError> {
    if matches!(command, DownloadCommand::Preferences { .. }) && window.label() != "main" {
        return Err(Error::InvalidInput("download quality settings require main window").into());
    }
    if matches!(command, DownloadCommand::ConfirmClear { .. }) && window.label() != "main" {
        return Err(Error::InvalidInput("download clearing requires main window").into());
    }
    client
        .download_command(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn play_downloads(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    task_ids: Vec<String>,
    index: usize,
) -> Result<PlayerView, ClientError> {
    client
        .play_downloads(&epoch, task_ids, index)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn pick_download_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<Option<String>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("directory picker requires main window").into());
    }
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    let mut picker = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("选择下载目录")
        .set_can_create_directories(true);
    if let Ok(path) = client.download_directory_path(&epoch, None).await {
        picker = picker.set_directory(path);
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    picker.pick_folder(move |path| {
        let _ = send.send(path);
    });
    let path = receive
        .await
        .map_err(|_| ClientError::from(Error::Cancelled))?;
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    path.map(|p| {
        p.into_path()
            .map_err(|_| ClientError::from(Error::InvalidInput("unsupported directory location")))?
            .into_os_string()
            .into_string()
            .map_err(|_| ClientError::from(Error::InvalidInput("directory must be UTF-8")))
    })
    .transpose()
}
#[tauri::command]
pub async fn open_download_directory(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    task_id: Option<String>,
) -> Result<(), ClientError> {
    let path = client
        .download_directory_path(&epoch, task_id)
        .await
        .map_err(ClientError::from)?;
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}
