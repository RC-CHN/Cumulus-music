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
