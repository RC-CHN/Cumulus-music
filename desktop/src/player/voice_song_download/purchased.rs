//! Bounded renderer projection of the native purchased SongCard command.
use super::{VoiceSongDownloadTarget, require_main};
use cumulus_core::{
    client::{
        ClientError, MusicClient,
        dto::{
            VoiceSongPurchasedDownloadCommand as NativeCommand, VoiceSongPurchasedDownloadResult,
        },
    },
    download::service::{DownloadOptions, OutputContainer},
    membership::Quality,
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Receipt(String);

impl TryFrom<String> for Receipt {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(value))
        } else {
            Err("invalid purchased SongCard receipt")
        }
    }
}

fn explicit_continuation<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Receipt>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(try_from = "Quality", into = "Quality")]
pub(crate) struct PurchasedQuality(Quality);

impl TryFrom<Quality> for PurchasedQuality {
    type Error = &'static str;

    fn try_from(value: Quality) -> Result<Self, Self::Error> {
        match value {
            Quality::Vivid
            | Quality::Sky
            | Quality::JyMaster
            | Quality::JyEffect
            | Quality::HiRes
            | Quality::Lossless
            | Quality::ExHigh
            | Quality::Standard => Ok(Self(value)),
            Quality::Dolby | Quality::Higher | Quality::Aac64 => {
                Err("quality cannot be selected for a purchased SongCard download")
            }
        }
    }
}

impl From<PurchasedQuality> for Quality {
    fn from(value: PurchasedQuality) -> Self {
        value.0
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PurchasedOptions {
    quality: PurchasedQuality,
    tags: bool,
    cover: bool,
    lyrics: bool,
    container: OutputContainer,
}

impl From<PurchasedOptions> for DownloadOptions {
    fn from(value: PurchasedOptions) -> Self {
        Self {
            quality: value.quality.0,
            tags: value.tags,
            cover: value.cover,
            lyrics: value.lyrics,
            container: value.container,
        }
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum PurchasedCommand {
    Prepare {},
    Select {
        quality: PurchasedQuality,
    },
    Cancel {
        selection: Receipt,
    },
    ChoosePlaylist {
        selection: Receipt,
        playlist_id: String,
    },
    PickerStatus {
        selection: Receipt,
        reconcile: bool,
    },
    CreatePlaylist {
        selection: Receipt,
        name: String,
        private: bool,
    },
    CreationStatus {
        selection: Receipt,
        reconcile: bool,
    },
    AddCreatedPlaylist {
        selection: Receipt,
    },
    Start {
        selection: Receipt,
        #[serde(deserialize_with = "explicit_continuation")]
        continuation: Option<Receipt>,
        options: PurchasedOptions,
    },
}

impl From<PurchasedCommand> for NativeCommand {
    fn from(value: PurchasedCommand) -> Self {
        match value {
            PurchasedCommand::Prepare {} => Self::Prepare,
            PurchasedCommand::Select { quality } => Self::Select { quality: quality.0 },
            PurchasedCommand::Cancel { selection } => Self::Cancel {
                selection: selection.0,
            },
            PurchasedCommand::ChoosePlaylist {
                selection,
                playlist_id,
            } => Self::ChoosePlaylist {
                selection: selection.0,
                playlist_id,
            },
            PurchasedCommand::PickerStatus {
                selection,
                reconcile,
            } => Self::PickerStatus {
                selection: selection.0,
                reconcile,
            },
            PurchasedCommand::CreatePlaylist {
                selection,
                name,
                private,
            } => Self::CreatePlaylist {
                selection: selection.0,
                name,
                private,
            },
            PurchasedCommand::CreationStatus {
                selection,
                reconcile,
            } => Self::CreationStatus {
                selection: selection.0,
                reconcile,
            },
            PurchasedCommand::AddCreatedPlaylist { selection } => Self::AddCreatedPlaylist {
                selection: selection.0,
            },
            PurchasedCommand::Start {
                selection,
                continuation,
                options,
            } => Self::Start {
                selection: selection.0,
                continuation: continuation.map(|receipt| receipt.0),
                options: options.into(),
            },
        }
    }
}

#[tauri::command]
pub(crate) async fn open_voice_song_purchased_quality_guide<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    target: VoiceSongDownloadTarget,
) -> Result<(), ClientError> {
    use cumulus_core::Error;
    use tauri::Manager;
    require_main(&window)?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let url = app
            .state::<MusicClient>()
            .voice_song_purchased_quality_guide(
                &target.epoch,
                &target.play_id,
                &target.receipt,
                target.index,
            )?;
        crate::browser::launch(url)
    })
    .await
    .map_err(|_| ClientError::from(Error::State("browser worker stopped")))?
    .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_song_purchased_download<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    command: PurchasedCommand,
) -> Result<VoiceSongPurchasedDownloadResult, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_purchased_download(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            command.into(),
        )
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{NativeCommand, PurchasedCommand, Quality};
    use serde_json::{Value, json};

    const CHOICE: &str = "123456789012345678901234567890ab";
    const PERMIT: &str = "abcdabcdabcdabcdabcdabcdabcdabcd";

    fn native(input: Value) -> NativeCommand {
        serde_json::from_value::<PurchasedCommand>(input)
            .unwrap()
            .into()
    }

    #[test]
    fn purchased_commands_preserve_exact_destination_and_download_options() {
        assert!(matches!(
            native(json!({"kind":"prepare"})),
            NativeCommand::Prepare
        ));
        assert!(matches!(
            native(json!({"kind":"select","quality":"lossless"})),
            NativeCommand::Select {
                quality: Quality::Lossless
            }
        ));
        assert!(matches!(
            native(json!({"kind":"choose_playlist","selection":CHOICE,"playlistId":"81"})),
            NativeCommand::ChoosePlaylist { selection, playlist_id } if selection == CHOICE && playlist_id == "81"
        ));
        assert!(matches!(
            native(json!({"kind":"create_playlist","selection":CHOICE,"name":"合成","private":false})),
            NativeCommand::CreatePlaylist { selection, name, private:false } if selection == CHOICE && name == "合成"
        ));
        assert!(matches!(
            native(json!({"kind":"creation_status","selection":CHOICE,"reconcile":true})),
            NativeCommand::CreationStatus { selection, reconcile: true } if selection == CHOICE
        ));
        for permit in [None, Some(PERMIT)] {
            assert!(matches!(
                native(json!({"kind":"start","selection":CHOICE,"continuation":permit,
                    "options":{"quality":"hires","tags":true,"cover":false,"lyrics":true,"container":"audio"}})),
                NativeCommand::Start { selection, continuation, options }
                    if selection == CHOICE && continuation.as_deref() == permit
                        && options.quality == Quality::HiRes && options.tags && !options.cover && options.lyrics
            ));
        }
    }

    #[test]
    fn purchased_ipc_refuses_raw_rights_and_requires_explicit_native_receipt_inputs() {
        let options = json!({"quality":"standard","tags":true,"cover":true,"lyrics":false,"container":"audio"});
        let mut extra_options = options.clone();
        extra_options["songId"] = json!("301");
        for invalid in [
            json!({"kind":"checkout","selection":CHOICE}),
            json!({"kind":"prepare","payed":2}),
            json!({"kind":"select","quality":"dolby"}),
            json!({"kind":"select","quality":"higher"}),
            json!({"kind":"select","quality":"64aac"}),
            json!({"kind":"select","quality":"standard","trackId":"301"}),
            json!({"kind":"choose_playlist","selection":CHOICE,"playlist_id":"81"}),
            json!({"kind":"picker_status","selection":"unsigned-song-id","reconcile":true}),
            json!({"kind":"create_playlist","selection":CHOICE,"name":"合成"}),
            json!({"kind":"start","selection":CHOICE,"options":options}),
            json!({"kind":"start","selection":CHOICE,"continuation":"operation-id","options":options}),
            json!({"kind":"start","selection":CHOICE,"continuation":null,"options":extra_options}),
        ] {
            assert!(serde_json::from_value::<PurchasedCommand>(invalid).is_err());
        }
    }
}
