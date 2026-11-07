//! Main-window transport for receipt-bound episode SongCard collection.
use super::voice_related::require_main;
use cumulus_core::client::{
    ClientError, MusicClient,
    dto::{
        AccountData, PlaylistCollectPage, PlaylistCollectSort, VoiceSongCollectPlan,
        VoiceSongCollectResult, VoiceSongLikedDestination,
    },
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceSongCollectCard {
    epoch: String,
    play_id: String,
    card_receipt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceSongCollectTarget {
    epoch: String,
    play_id: String,
    card_receipt: String,
    selection_receipt: String,
}

#[tauri::command]
pub async fn player_voice_songs_collect_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectCard,
    indices: Vec<usize>,
) -> Result<VoiceSongCollectPlan, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_collect_prepare(
            &target.epoch,
            &target.play_id,
            &target.card_receipt,
            &indices,
        )
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_collect_destinations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectTarget,
    sort: PlaylistCollectSort,
    page: u32,
) -> Result<AccountData<PlaylistCollectPage>, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_collect_destinations(
            &target.epoch,
            &target.play_id,
            &target.card_receipt,
            &target.selection_receipt,
            sort,
            page,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_collect_existing<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectTarget,
    playlist_id: String,
) -> Result<VoiceSongCollectResult, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_collect_existing(
            &target.epoch,
            &target.play_id,
            &target.card_receipt,
            &target.selection_receipt,
            &playlist_id,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_collect_liked_destination<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectTarget,
) -> Result<AccountData<Option<VoiceSongLikedDestination>>, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_collect_liked_destination(
            &target.epoch,
            &target.play_id,
            &target.card_receipt,
            &target.selection_receipt,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_collect_liked<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectTarget,
    destination_receipt: String,
) -> Result<VoiceSongCollectResult, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_collect_liked(
            &target.epoch,
            &target.play_id,
            &target.card_receipt,
            &target.selection_receipt,
            &destination_receipt,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_collect_status<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectTarget,
    reconcile: bool,
) -> Result<VoiceSongCollectResult, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_collect_status(
            &target.epoch,
            &target.play_id,
            &target.card_receipt,
            &target.selection_receipt,
            reconcile,
        )
        .await
        .map_err(Into::into)
}
