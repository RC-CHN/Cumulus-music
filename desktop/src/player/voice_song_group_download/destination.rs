//! Destination commands use only the native group and picker receipts.
use super::{Plan, Prepare, Prepared, Receipt, decode, require_main};
use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{
            AccountData, DownloadResult, PlaylistCollectPage, PlaylistCollectSort,
            VoiceSongGroupPlaylistCreation, VoiceSongGroupPlaylistResult,
            VoiceSongLikedDestination,
        },
    },
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Request;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    target: Prepared,
    command: Command,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Command {
    Page {
        sort: PlaylistCollectSort,
        page: u32,
    },
    LikedDestination {},
    Choose {
        playlist_id: String,
    },
    ChooseLiked {
        destination_receipt: Receipt,
    },
    Status {
        reconcile: bool,
    },
    Create {
        name: String,
        private_playlist: bool,
    },
    CreationStatus {
        reconcile: bool,
    },
    AddCreated {},
    Start {
        continuation: Receipt,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Page {
        page: AccountData<PlaylistCollectPage>,
    },
    LikedDestination {
        destination: AccountData<Option<VoiceSongLikedDestination>>,
    },
    Playlist {
        playlist: VoiceSongGroupPlaylistResult,
    },
    Creation {
        creation: VoiceSongGroupPlaylistCreation,
    },
    Started {
        result: DownloadResult,
    },
    DirectoryRequired {},
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_prepare_with_destination<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<Plan, ClientError> {
    require_main(&window)?;
    let input: Prepare = decode(request.body())?;
    let plan = client
        .player_voice_songs_download_prepare_with_destination(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            input.anchor_index,
            &input.indices,
        )
        .await?;
    Plan::project(plan, true).map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_destination<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<Reply, ClientError> {
    require_main(&window)?;
    let Input { target, command } = decode(request.body())?;
    let epoch = target.epoch.as_str();
    let play = target.play_id.as_str();
    let card = target.card_receipt.0.as_str();
    let group = target.selection_receipt.0.as_str();
    Ok(match command {
        Command::Page { sort, page } => Reply::Page {
            page: client
                .player_voice_songs_download_destinations(epoch, play, card, group, sort, page)
                .await?,
        },
        Command::LikedDestination {} => Reply::LikedDestination {
            destination: client
                .player_voice_songs_download_liked_destination(epoch, play, card, group)
                .await?,
        },
        Command::Choose { playlist_id } => Reply::Playlist {
            playlist: client
                .player_voice_songs_download_choose_playlist(epoch, play, card, group, &playlist_id)
                .await?,
        },
        Command::ChooseLiked {
            destination_receipt,
        } => Reply::Playlist {
            playlist: client
                .player_voice_songs_download_choose_liked(
                    epoch,
                    play,
                    card,
                    group,
                    &destination_receipt.0,
                )
                .await?,
        },
        Command::Status { reconcile } => Reply::Playlist {
            playlist: client
                .player_voice_songs_download_playlist_status(epoch, play, card, group, reconcile)
                .await?,
        },
        Command::Create {
            name,
            private_playlist,
        } => Reply::Creation {
            creation: client
                .player_voice_songs_download_create_playlist(
                    epoch,
                    play,
                    card,
                    group,
                    &name,
                    private_playlist,
                )
                .await?,
        },
        Command::CreationStatus { reconcile } => Reply::Creation {
            creation: client
                .player_voice_songs_download_creation_status(epoch, play, card, group, reconcile)
                .await?,
        },
        Command::AddCreated {} => Reply::Creation {
            creation: client
                .player_voice_songs_download_add_created_playlist(epoch, play, card, group)
                .await?,
        },
        Command::Start { continuation } => match client
            .player_voice_songs_download_start_from_playlist(
                epoch,
                play,
                card,
                group,
                &continuation.0,
            )
            .await
        {
            Ok(result) => Reply::Started { result },
            Err(Error::State(
                "download directory is not configured" | "download directory is no longer usable",
            )) => Reply::DirectoryRequired {},
            Err(error) => return Err(error.into()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_exact_group_destination_receipts_and_intents_decode() {
        let receipt = "0123456789abcdef0123456789abcdef";
        let target =
            json!({"epoch":"1","playId":"play","cardReceipt":receipt,"selectionReceipt":receipt});
        let exact = |command| json!({"target":target,"command":command});
        for command in [
            json!({"kind":"page","sort":"recently","page":0}),
            json!({"kind":"choose","playlistId":"81"}),
            json!({"kind":"choose_liked","destinationReceipt":receipt}),
            json!({"kind":"status","reconcile":true}),
            json!({"kind":"create","name":"test","privatePlaylist":false}),
            json!({"kind":"start","continuation":receipt}),
        ] {
            assert!(serde_json::from_value::<Input>(exact(command)).is_ok());
        }
        for command in [
            json!({"kind":"choose","playlistId":"81","ids":["307","301"]}),
            json!({"kind":"choose_liked","playlistId":"81"}),
            json!({"kind":"choose_liked","destinationReceipt":"81"}),
            json!({"kind":"status","reconcile":true,"write":true}),
            json!({"kind":"start","continuation":receipt,"quality":"lossless"}),
            json!({"kind":"start","continuation":receipt,"path":"test"}),
            json!({"kind":"start","continuation":"81"}),
        ] {
            assert!(serde_json::from_value::<Input>(exact(command)).is_err());
        }
        let mut forged = exact(json!({"kind":"liked_destination"}));
        forged["target"]["songIds"] = json!(["301"]);
        assert!(serde_json::from_value::<Input>(forged).is_err());
    }
}
