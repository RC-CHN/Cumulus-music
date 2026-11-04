use cumulus_core::client::{
    ClientError, MusicClient,
    dto::{AccountData, ProfileRelationAccess, ProfileRelationKind, ProfileRelationPage},
};

#[tauri::command]
pub async fn profile_relation_access(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
) -> Result<AccountData<ProfileRelationAccess>, ClientError> {
    client
        .profile_relation_access(&epoch, &uid)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn profile_relation_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
    kind: ProfileRelationKind,
    continuation: Option<String>,
) -> Result<AccountData<ProfileRelationPage>, ClientError> {
    client
        .profile_relation_page(&epoch, &uid, kind, continuation.as_deref())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn profile_play_record(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: Option<String>,
    period: cumulus_core::client::dto::ProfilePlayRecordPeriod,
) -> Result<AccountData<cumulus_core::client::dto::ProfilePlayRecordView>, ClientError> {
    client
        .profile_play_record(&epoch, uid.as_deref(), period)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn remove_profile_play_record(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    period: cumulus_core::client::dto::ProfilePlayRecordPeriod,
    song_id: String,
    revision: String,
    action: cumulus_core::client::dto::ProfilePlayRecordRemovalAction,
) -> Result<AccountData<cumulus_core::client::dto::ProfilePlayRecordRemovalOutcome>, ClientError> {
    client
        .remove_profile_play_record(&epoch, period, &song_id, &revision, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn profile_follow(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
    command: cumulus_core::client::dto::ProfileFollowCommand,
) -> Result<AccountData<cumulus_core::client::dto::ProfileFollowView>, ClientError> {
    client
        .profile_follow(&epoch, &uid, command)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn profile_blacklist_list(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<cumulus_core::client::dto::ProfileBlacklistList>, ClientError> {
    client
        .profile_blacklist_list(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn profile_blacklist(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
    command: cumulus_core::client::dto::ProfileBlacklistCommand,
) -> Result<AccountData<cumulus_core::client::dto::ProfileBlacklistResult>, ClientError> {
    client
        .profile_blacklist(&epoch, &uid, command)
        .await
        .map_err(Into::into)
}
