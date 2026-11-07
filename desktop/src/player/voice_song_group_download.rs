//! Main-window transport for a prepared ordinary SongCard download group.
use super::voice_related::require_main;
use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{DownloadResult, VoiceSongGroupDownloadPlan},
    },
};
use serde::{Deserialize, de::DeserializeOwned};
use tauri::ipc::{InvokeBody, Request};

#[derive(Deserialize)]
#[serde(try_from = "String")]
struct Receipt(String);

impl TryFrom<String> for Receipt {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(value))
        } else {
            Err("invalid native SongCard receipt")
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    epoch: String,
    play_id: String,
    card_receipt: Receipt,
    anchor_index: usize,
    indices: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepared {
    epoch: String,
    play_id: String,
    card_receipt: Receipt,
    selection_receipt: Receipt,
}

fn decode<T: DeserializeOwned>(body: &InvokeBody) -> Result<T, ClientError> {
    let invalid = || Error::InvalidInput("invalid SongCard group download request").into();
    // Ordinary Tauri argument extraction ignores unknown top-level keys.
    // These commands accept exactly one flat, typed request as a whole.
    match body {
        InvokeBody::Json(value) if value.is_object() => {
            T::deserialize(value).map_err(|_| invalid())
        }
        _ => Err(invalid()),
    }
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<VoiceSongGroupDownloadPlan, ClientError> {
    require_main(&window)?;
    let input: Prepare = decode(request.body())?;
    client
        .player_voice_songs_download_prepare(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            input.anchor_index,
            &input.indices,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<(), ClientError> {
    require_main(&window)?;
    let input: Prepared = decode(request.body())?;
    client
        .player_voice_songs_download_cancel(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            &input.selection_receipt.0,
        )
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_start<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<DownloadResult, ClientError> {
    require_main(&window)?;
    let input: Prepared = decode(request.body())?;
    client
        .player_voice_songs_download_start(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            &input.selection_receipt.0,
        )
        .await
        .map_err(Into::into)
}
