use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
use tauri_plugin_dialog::DialogExt;
#[tauri::command]
pub async fn cloud_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: CloudQuery,
    refresh: bool,
) -> Result<CloudPageView, ClientError> {
    client
        .cloud_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn cloud_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: CloudCommand,
) -> Result<CloudResult, ClientError> {
    client
        .cloud_command(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn pick_cloud_files<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: String,
) -> Result<Vec<String>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("file picker requires main window").into());
    }
    client
        .validate_cloud_epoch(&epoch)
        .map_err(ClientError::from)?;
    let (title, extensions): (&str, &[&str]) = match kind.as_str() {
        "folder" => ("选择上传到音乐云盘的文件夹", &[]),
        "audio" => (
            "选择上传到音乐云盘的文件",
            &[
                "mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aiff", "ape", "wma",
            ],
        ),
        "cover" => ("选择封面", &["jpg", "jpeg", "png"]),
        "lyric" => ("选择歌词", &["lrc"]),
        "cue" => ("选择CUE", &["cue"]),
        _ => return Err(Error::InvalidInput("unknown cloud file selection").into()),
    };
    let mut picker = window.dialog().file().set_parent(&window).set_title(title);
    if !extensions.is_empty() {
        picker = picker.add_filter(title, extensions);
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    if kind == "folder" {
        picker.pick_folder(move |path| {
            let _ = send.send(path.map(|p| vec![p]));
        });
    } else if kind == "audio" {
        picker.pick_files(move |paths| {
            let _ = send.send(paths);
        });
    } else {
        picker.pick_file(move |path| {
            let _ = send.send(path.map(|p| vec![p]));
        });
    }
    let paths = receive
        .await
        .map_err(|_| ClientError::from(Error::Cancelled))?
        .unwrap_or_default();
    client
        .validate_cloud_epoch(&epoch)
        .map_err(ClientError::from)?;
    let paths = paths
        .into_iter()
        .map(|p| {
            p.into_path()
                .map_err(|_| ClientError::from(Error::InvalidInput("unsupported file location")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if matches!(kind.as_str(), "audio" | "folder") {
        return client
            .select_cloud_sources(&epoch, paths)
            .await
            .map_err(Into::into);
    }
    paths
        .into_iter()
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| ClientError::from(Error::InvalidInput("path must be UTF8")))
        })
        .collect()
}
