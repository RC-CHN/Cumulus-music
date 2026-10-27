use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
use tauri::Manager;
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
pub async fn local_command(
    client: tauri::State<'_, MusicClient>,
    command: LocalCommand,
) -> Result<(), ClientError> {
    client.local_command(command).await.map_err(Into::into)
}
#[tauri::command]
pub async fn pick_local_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
) -> Result<Option<String>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("directory picker requires main window").into());
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    let mut picker = window
        .dialog()
        .file()
        .set_parent(&window)
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
            p.into_path()
                .map_err(|_| {
                    ClientError::from(Error::InvalidInput("unsupported directory location"))
                })?
                .into_os_string()
                .into_string()
                .map_err(|_| ClientError::from(Error::InvalidInput("directory must be UTF-8")))
        })
        .transpose()
}
