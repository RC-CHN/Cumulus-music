use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
use tauri_plugin_dialog::DialogExt;
#[tauri::command]
pub async fn downloads(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: DownloadQuery,
) -> Result<DownloadsView, ClientError> {
    client.downloads(&epoch, query).await.map_err(Into::into)
}
#[tauri::command]
pub async fn download_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: DownloadCommand,
) -> Result<DownloadResult, ClientError> {
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
    tauri::async_runtime::spawn_blocking(move || -> cumulus_core::Result<()> {
        #[cfg(target_os = "linux")]
        let mut command = std::process::Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut c = std::process::Command::new("/usr/bin/open");
            c.arg("--");
            c
        };
        #[cfg(windows)]
        let mut command = std::process::Command::new("explorer.exe");
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        return Err(Error::State("file manager not supported on this platform"));
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            // Direct argv, no shell interpolation or arbitrary URLs/programs.
            let status = command
                .arg(path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                return Err(Error::State("file manager failed"));
            }
            Ok(())
        }
    })
    .await
    .map_err(|_| ClientError::from(Error::State("file manager worker stopped")))?
    .map_err(Into::into)
}
