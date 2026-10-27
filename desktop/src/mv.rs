use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
#[tauri::command]
pub async fn mv_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: MvQuery,
    refresh: bool,
) -> Result<MvPage, ClientError> {
    client
        .mv_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn mv_detail(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<MvView>, ClientError> {
    client.mv_detail(&epoch, &id).await.map_err(Into::into)
}
#[tauri::command]
pub async fn mv_collect(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    desired: bool,
) -> Result<AccountData<MvMutationView>, ClientError> {
    client
        .mv_collect(&epoch, &id, desired)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn mv_reconcile(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<MvMutationView>, ClientError> {
    client.mv_reconcile(&epoch, &id).await.map_err(Into::into)
}
#[tauri::command]
pub async fn video_snapshot(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<VideoView, ClientError> {
    client.video_snapshot(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn mv_like(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    desired: bool,
) -> Result<AccountData<MvLikeView>, ClientError> {
    client
        .mv_like(&epoch, &id, desired)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn mv_like_reconcile(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<MvLikeView>, ClientError> {
    client
        .mv_like_reconcile(&epoch, &id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn mv_recommendations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    refresh: bool,
) -> Result<AccountData<MvRecommendationsView>, ClientError> {
    client
        .mv_recommendations(&epoch, &id, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn video_action(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: VideoAction,
) -> Result<VideoView, ClientError> {
    client
        .video_action(&epoch, action)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn video_frame(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    after: String,
    width: u32,
    height: u32,
) -> Result<tauri::ipc::Response, ClientError> {
    let after = after
        .parse::<u64>()
        .map_err(|_| ClientError::from(Error::InvalidInput("invalid frame sequence")))?;
    let Some(frame) = client
        .video_frame(&epoch, &play_id, after, width, height)
        .map_err(ClientError::from)?
    else {
        return Ok(tauri::ipc::Response::new(Vec::<u8>::new()));
    };
    // Little-endian width:u32, height:u32, sequence:u64, followed by RGBA8.
    // Raw response avoids JSON arrays/base64 expansion for every video frame.
    let mut bytes = Vec::with_capacity(16 + frame.rgba.len());
    bytes.extend_from_slice(&frame.width.to_le_bytes());
    bytes.extend_from_slice(&frame.height.to_le_bytes());
    bytes.extend_from_slice(&frame.sequence.to_le_bytes());
    bytes.extend_from_slice(&frame.rgba);
    Ok(tauri::ipc::Response::new(bytes))
}
