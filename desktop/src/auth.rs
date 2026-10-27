use cumulus_core::client::{ClientError, MusicClient, dto::*};
use tauri::{Emitter, Manager};

pub(super) fn publish<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, client: &MusicClient) {
    if let Ok(account) = client.account() {
        let _ = window.emit("cumulus:account-changed", account);
    }
    crate::renewal::wake(window.app_handle());
}

#[tauri::command]
pub async fn begin_login(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    mode: LoginMode,
    saved_id: Option<String>,
) -> Result<LoginView, ClientError> {
    client
        .begin_login(&epoch, mode, saved_id.as_deref())
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn login_snapshot(
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<LoginView, ClientError> {
    client.login_snapshot(&operation_id).map_err(Into::into)
}
#[tauri::command]
pub async fn login_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
    command: LoginCommand,
) -> Result<LoginView, ClientError> {
    let result = client
        .login_command(&operation_id, command)
        .await
        .map_err(Into::into);
    publish(&window, &client);
    result
}
#[tauri::command]
pub fn cancel_login<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<(), ClientError> {
    let result = client.cancel_login(&operation_id).map_err(Into::into);
    publish(&window, &client);
    result
}
#[tauri::command]
pub fn saved_accounts(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<SavedAccountView>>, ClientError> {
    client.saved_accounts(&epoch).map_err(Into::into)
}
#[tauri::command]
pub async fn remove_saved_account<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<(), ClientError> {
    let result = client
        .remove_saved_account(&epoch, &id)
        .await
        .map_err(Into::into);
    publish(&window, &client);
    result
}
#[tauri::command]
pub async fn logout_account<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    remote: bool,
) -> Result<LogoutView, ClientError> {
    let result = client
        .logout_account(&epoch, remote)
        .await
        .map_err(Into::into);
    publish(&window, &client);
    result
}
