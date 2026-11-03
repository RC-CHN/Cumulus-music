use cumulus_core::client::{ClientError, MusicClient, dto::*};
use tauri::{Emitter, Manager};

#[tauri::command]
pub fn login_presentation(
    client: tauri::State<'_, MusicClient>,
) -> Result<LoginPresentation, ClientError> {
    client.login_presentation().map_err(Into::into)
}

#[tauri::command]
pub fn select_login_country(
    client: tauri::State<'_, MusicClient>,
    alpha: String,
) -> Result<(), ClientError> {
    client.select_login_country(&alpha).map_err(Into::into)
}

#[tauri::command]
pub async fn open_login_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    document: LoginDocument,
) -> Result<(), ClientError> {
    let browser = app
        .try_state::<crate::LoginDocumentBrowser>()
        .map(|value| value.inner().clone())
        .unwrap_or_default();
    crate::browser::open_login_document(browser, document)
        .await
        .map_err(Into::into)
}

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
pub async fn cancel_login<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<(), ClientError> {
    let result = client
        .cancel_login_and_cleanup(&operation_id)
        .await
        .map_err(Into::into);
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
pub async fn quick_login_eligibility(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<QuickLoginEligibilityView>, ClientError> {
    client
        .quick_login_eligibility(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn begin_server_quick_login(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    saved_id: String,
    consent: bool,
) -> Result<AccountData<ServerQuickLoginOperationView>, ClientError> {
    client
        .begin_server_quick_login(&epoch, &saved_id, consent)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn server_quick_login_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    command: ServerQuickLoginCommand,
) -> Result<AccountData<ServerQuickLoginOperationView>, ClientError> {
    let result = client
        .server_quick_login_command(&epoch, &id, command)
        .await?;
    if result.value.state.phase
        == cumulus_core::account::profiles::ServerQuickLoginPhase::Authenticated
    {
        publish(&window, &client);
    }
    Ok(result)
}
#[tauri::command]
pub async fn begin_active_server_switch(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    saved_id: String,
    consent: bool,
) -> Result<AccountData<ServerQuickLoginOperationView>, ClientError> {
    client
        .begin_active_server_switch(&epoch, &saved_id, consent)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn active_server_switch_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    command: ServerQuickLoginCommand,
) -> Result<AccountData<ServerQuickLoginOperationView>, ClientError> {
    let started_current = client.account().is_ok_and(|account| account.epoch == epoch);
    let result = client
        .active_server_switch_command(&epoch, &id, command)
        .await
        .map_err(Into::into);
    // A local cleanup error after adoption does not roll back the new
    // account. Publish the actual epoch even in that case.
    if started_current && client.account().is_ok_and(|account| account.epoch != epoch) {
        publish(&window, &client);
    }
    result
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
