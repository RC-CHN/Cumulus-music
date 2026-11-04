use cumulus_core::client::{ClientError, MusicClient, dto::*};

#[tauri::command]
pub async fn community_new_song_notice(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    mode: CommunityNoticeMode,
) -> Result<AccountData<CommunityNewSongNotice>, ClientError> {
    client
        .community_new_song_notice(&epoch, mode)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_feed(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: CommunityFeedAction,
) -> Result<AccountData<CommunityFeedPage>, ClientError> {
    client
        .community_feed(&epoch, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_detail(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    detail: String,
) -> Result<AccountData<Option<CommunityDetail>>, ClientError> {
    client
        .community_detail(&epoch, &detail)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_comments(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    detail: String,
    offset: usize,
) -> Result<AccountData<CommunityCommentPage>, ClientError> {
    client
        .community_comments(&epoch, &detail, offset)
        .await
        .map_err(Into::into)
}
