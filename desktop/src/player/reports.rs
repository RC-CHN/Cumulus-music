use super::*;
#[tauri::command]
pub async fn lyric_report_open(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<LyricReportView, ClientError> {
    client
        .lyric_report_open(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn lyric_report_read(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<LyricReportView, ClientError> {
    client.lyric_report_read(&epoch, &id).map_err(Into::into)
}
#[tauri::command]
pub async fn lyric_report_types(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    revision: String,
) -> Result<LyricReportView, ClientError> {
    client
        .lyric_report_types(&epoch, &id, &revision)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn lyric_report_submit(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    revision: String,
    key: String,
    content: String,
) -> Result<LyricReportView, ClientError> {
    client
        .lyric_report_submit(&epoch, &id, &revision, &key, &content)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn lyric_report_abandon(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    revision: String,
) -> Result<LyricReportView, ClientError> {
    client
        .lyric_report_abandon(&epoch, &id, &revision)
        .map_err(Into::into)
}
