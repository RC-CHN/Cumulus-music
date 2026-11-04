use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
#[tauri::command]
pub async fn member_welfare_available(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<bool>, ClientError> {
    client
        .member_welfare_available(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn member_welfare_list(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<MemberWelfareView>>, ClientError> {
    client.member_welfare_list(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub fn member_portal_qr() -> Result<cumulus_core::membership::portal::MemberPortalQr, ClientError> {
    cumulus_core::membership::portal::member_portal_qr().map_err(Into::into)
}

#[tauri::command]
pub fn member_card_offer_qr(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
) -> Result<AccountData<cumulus_core::membership::portal::MemberPortalQr>, ClientError> {
    client
        .member_card_offer_qr(&epoch, &revision)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn member_card_offer(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<MemberCardOffer>, ClientError> {
    client
        .member_card_offer(&epoch, refresh)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn member_rights(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<MemberRightsView>, ClientError> {
    client
        .member_rights(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn member_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: MemberQuery,
    refresh: bool,
) -> Result<AccountData<MemberPageView>, ClientError> {
    client
        .member_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn member_sign_card(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<MemberSignCard>, ClientError> {
    client.member_sign_card(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn member_sign_detail(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    sign_time: u64,
) -> Result<AccountData<MemberSignDetail>, ClientError> {
    client
        .member_sign_detail(&epoch, sign_time)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn member_sign_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: MemberSignCommand,
) -> Result<AccountData<MemberSignOutcome>, ClientError> {
    client
        .member_sign_command(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn open_member_link<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    revision: String,
    key: String,
) -> Result<(), ClientError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || -> cumulus_core::Result<()> {
        let client = app.state::<MusicClient>();
        let url = client.member_external_url(&epoch, &revision, &key)?;
        crate::browser::launch(&url)
    })
    .await
    .map_err(|_| ClientError::from(Error::State("browser worker stopped")))?
    .map_err(Into::into)
}

#[tauri::command]
pub async fn open_member_card_offer<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    revision: String,
) -> Result<(), ClientError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let client = app.state::<MusicClient>();
        crate::browser::launch(client.member_card_external_url(&epoch, &revision)?)
    })
    .await
    .map_err(|_| ClientError::from(Error::State("browser worker stopped")))?
    .map_err(Into::into)
}
