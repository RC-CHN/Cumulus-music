//! Main-window transport for receipt-bound episode SongCard collection.
use super::voice_related::require_main;
use cumulus_core::client::{
    ClientError, MusicClient,
    dto::{
        AccountData, PlaylistCollectPage, PlaylistCollectSort, VoiceSongCollectCreation,
        VoiceSongCollectPlan, VoiceSongCollectResult, VoiceSongLikedDestination,
    },
};
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VoiceSongCollectCreationCommand {
    Create {
        name: String,
        private_playlist: bool,
    },
    Status {
        reconcile: bool,
    },
    AddCreated {},
    CancelPrepared {},
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceSongCollectCreationReply {
    Creation { creation: VoiceSongCollectCreation },
    Cancelled {},
}

#[tauri::command]
pub async fn player_voice_songs_collect_create_action<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongCollectTarget,
    command: VoiceSongCollectCreationCommand,
) -> Result<VoiceSongCollectCreationReply, ClientError> {
    use VoiceSongCollectCreationCommand as C;
    use VoiceSongCollectCreationReply as Reply;

    require_main(&window)?;
    let VoiceSongCollectTarget {
        epoch,
        play_id,
        card_receipt,
        selection_receipt,
    } = &target;
    for receipt in [card_receipt, selection_receipt] {
        if receipt.len() != 32 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(cumulus_core::Error::InvalidInput("invalid SongCard group receipt").into());
        }
    }
    let creation = match command {
        C::Create {
            name,
            private_playlist,
        } => {
            client
                .player_voice_songs_collect_create(
                    epoch,
                    play_id,
                    card_receipt,
                    selection_receipt,
                    &name,
                    private_playlist,
                )
                .await?
        }
        C::Status { reconcile } => {
            client
                .player_voice_songs_collect_creation_status(
                    epoch,
                    play_id,
                    card_receipt,
                    selection_receipt,
                    reconcile,
                )
                .await?
        }
        C::AddCreated {} => {
            client
                .player_voice_songs_collect_add_created(
                    epoch,
                    play_id,
                    card_receipt,
                    selection_receipt,
                )
                .await?
        }
        C::CancelPrepared {} => {
            client.player_voice_songs_collect_cancel_prepared(
                epoch,
                play_id,
                card_receipt,
                selection_receipt,
            )?;
            return Ok(Reply::Cancelled {});
        }
    };
    Ok(Reply::Creation { creation })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    #[test]
    fn creation_bridge_requires_explicit_privacy_and_rejects_raw_ids() {
        for command in [
            json!({"kind":"create","name":"新歌单"}),
            json!({"kind":"create","name":"新歌单","privatePlaylist":10}),
            json!({"kind":"create","name":"新歌单","privatePlaylist":true,"playlistId":"91"}),
            json!({"kind":"add_created","operationId":"replace"}),
            json!({"kind":"status"}),
            json!({"kind":"status","reconcile":false,"songIds":[301]}),
            json!({"kind":"cancel_prepared","selectionReceipt":"replace"}),
        ] {
            assert!(from_value::<VoiceSongCollectCreationCommand>(command).is_err());
        }
        let explicit = json!({"kind":"create","name":"新歌单","privatePlaylist":false});
        assert!(matches!(
            from_value(explicit),
            Ok(VoiceSongCollectCreationCommand::Create {
                private_playlist: false,
                ..
            })
        ));
        assert!(matches!(
            from_value(json!({"kind":"add_created"})),
            Ok(VoiceSongCollectCreationCommand::AddCreated {})
        ));
    }
}
