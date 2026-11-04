use cumulus_core::client::{ClientError, MusicClient, dto::*};

#[tauri::command]
pub async fn community_note_report(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    detail: String,
    command: CommunityReportCommand,
) -> Result<AccountData<CommunityReportView>, ClientError> {
    client
        .community_note_report(&epoch, &detail, command)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_note_delete(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    detail: String,
    action: CommunityDeleteAction,
) -> Result<AccountData<CommunityDeleteReply>, ClientError> {
    client
        .community_note_delete(&epoch, &detail, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_composer_search(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: ComposerKind,
    keyword: String,
    offset: usize,
) -> Result<AccountData<ComposerSearchPage>, ClientError> {
    client
        .community_composer_search(&epoch, kind, &keyword, offset)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_composer_draft<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    action: ComposerDraftAction,
) -> Result<AccountData<Option<ComposerDraft>>, ClientError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<MusicClient>()
            .community_composer_draft(&epoch, action)
    })
    .await
    .map_err(|_| ClientError::from(cumulus_core::Error::State("community draft worker stopped")))?
    .map_err(Into::into)
}

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

#[tauri::command]
pub async fn profile_notes(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
    action: CommunityFeedAction,
) -> Result<AccountData<CommunityFeedPage>, ClientError> {
    client
        .profile_notes(&epoch, &uid, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_composer_publish(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: ComposerPublishAction,
) -> Result<AccountData<ComposerPublication>, ClientError> {
    client
        .community_composer_publish(&epoch, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_topic(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    name: String,
) -> Result<AccountData<Option<cumulus_core::client::dto::CommunityTopic>>, ClientError> {
    client
        .community_topic(&epoch, &name)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_topic_slice(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    topic: String,
    action: cumulus_core::client::dto::CommunityTopicAction,
) -> Result<AccountData<cumulus_core::client::dto::CommunityTopicSlice>, ClientError> {
    client
        .community_topic_slice(&epoch, &topic, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn community_topic_notice(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    topic: String,
) -> Result<AccountData<bool>, ClientError> {
    client
        .community_topic_notice(&epoch, &topic)
        .await
        .map_err(Into::into)
}
