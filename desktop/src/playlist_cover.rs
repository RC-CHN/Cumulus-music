use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn playlist_cover_tasks(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<CoverTaskView>>, ClientError> {
    client
        .playlist_cover_tasks(&epoch)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn playlist_cover_image(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<CoverImage>, ClientError> {
    client
        .playlist_cover_image(&epoch, &id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn playlist_cover_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    command: CoverCommand,
) -> Result<AccountData<CoverOutcome>, ClientError> {
    client
        .playlist_cover_command(&epoch, &id, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn pick_playlist_cover<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    playlist_id: String,
    revision: String,
    replace_id: Option<String>,
) -> Result<AccountData<Option<CoverTaskView>>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("cover picker requires main window").into());
    }
    client
        .validate_playlist_epoch(&epoch)
        .map_err(ClientError::from)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("选择歌单封面")
        .add_filter("图片", &["jpg", "jpeg", "png", "bmp", "gif"])
        .pick_file(move |path| {
            let _ = send.send(path);
        });
    let selected = receive
        .await
        .map_err(|_| ClientError::from(Error::Cancelled))?;
    client
        .validate_playlist_epoch(&epoch)
        .map_err(ClientError::from)?;
    let Some(selected) = selected else {
        return Ok(AccountData { epoch, value: None });
    };
    let path = selected
        .into_path()
        .map_err(|_| ClientError::from(Error::InvalidInput("unsupported file location")))?;
    let result = if let Some(id) = replace_id {
        client
            .replace_playlist_cover(&epoch, &playlist_id, &revision, &id, &path)
            .await
    } else {
        client
            .select_playlist_cover(&epoch, &playlist_id, &revision, &path)
            .await
    }
    .map_err(ClientError::from)?;
    Ok(AccountData {
        epoch: result.epoch,
        value: Some(result.value),
    })
}
