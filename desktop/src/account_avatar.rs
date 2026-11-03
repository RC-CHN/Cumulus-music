use cumulus_core::{
    Error,
    account::profile::avatar::AvatarPhase,
    client::{ClientError, MusicClient, dto::*},
};
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/// Host-only picker dependency for isolated WebView verification.
pub struct AccountAvatarPicker(Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>);
impl AccountAvatarPicker {
    pub fn with_picker(picker: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) -> Self {
        Self(Arc::new(picker))
    }
}

#[tauri::command]
pub async fn account_avatar_tasks(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<AvatarTaskView>>, ClientError> {
    client
        .account_avatar_tasks(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn account_avatar_image(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<AvatarImage>, ClientError> {
    client
        .account_avatar_image(&epoch, &id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn pick_account_avatar<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<AvatarTaskView>>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("avatar picker requires main window").into());
    }
    client.validate_account_avatar_epoch(&epoch)?;
    let selected = if let Some(picker) = window.try_state::<AccountAvatarPicker>() {
        (picker.0)()
    } else {
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择头像图片")
            .add_filter("图片", &["jpg", "jpeg", "png", "bmp", "gif"])
            .pick_file(move |path| {
                let _ = send.send(path);
            });
        receive
            .await
            .map_err(|_| Error::Cancelled)?
            .map(|path| {
                path.into_path()
                    .map_err(|_| Error::InvalidInput("unsupported file location"))
            })
            .transpose()?
    };
    client.validate_account_avatar_epoch(&epoch)?;
    let value = match selected {
        Some(path) => Some(client.select_account_avatar(&epoch, &path).await?.value),
        None => None,
    };
    Ok(AccountData { epoch, value })
}

#[tauri::command]
pub async fn account_avatar_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    command: AccountAvatarCommand,
) -> Result<AccountData<AvatarTaskView>, ClientError> {
    let result = client.account_avatar_command(&epoch, &id, command).await?;
    if matches!(
        result.value.phase,
        AvatarPhase::Confirmed | AvatarPhase::AcceptedCurrentState
    ) && let Ok(account) = client.account()
        && account.epoch == epoch
    {
        let _ = window.emit("cumulus:account-changed", account);
    }
    Ok(result)
}
