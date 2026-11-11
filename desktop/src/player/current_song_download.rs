//! Main-window bridge for a verified, current playback download choice.
use super::{
    voice_related::require_main,
    voice_song_download::purchased::{
        PurchasedOptions, PurchasedQuality, Receipt, explicit_continuation,
    },
};
use cumulus_core::client::{
    ClientError, MusicClient,
    dto::{
        CurrentSongDownloadCommand as NativeCommand, CurrentSongDownloadReply,
        CurrentSongDownloadTarget, PlaylistCollectSort,
    },
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum CurrentSongCommand {
    Prepare {},
    Select {
        offer: Receipt,
        quality: PurchasedQuality,
    },
    Cancel {
        receipt: Receipt,
    },
    Destinations {
        selection: Receipt,
        sort: PlaylistCollectSort,
        page: u32,
    },
    ChoosePlaylist {
        selection: Receipt,
        destination: Receipt,
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

impl From<CurrentSongCommand> for NativeCommand {
    fn from(value: CurrentSongCommand) -> Self {
        match value {
            CurrentSongCommand::Prepare {} => Self::Prepare,
            CurrentSongCommand::Select { offer, quality } => Self::Select {
                offer: offer.into(),
                quality: quality.into(),
            },
            CurrentSongCommand::Cancel { receipt } => Self::Cancel {
                receipt: receipt.into(),
            },
            CurrentSongCommand::Destinations {
                selection,
                sort,
                page,
            } => Self::Destinations {
                selection: selection.into(),
                sort,
                page,
            },
            CurrentSongCommand::ChoosePlaylist {
                selection,
                destination,
            } => Self::ChoosePlaylist {
                selection: selection.into(),
                destination: destination.into(),
            },
            CurrentSongCommand::PickerStatus {
                selection,
                reconcile,
            } => Self::PickerStatus {
                selection: selection.into(),
                reconcile,
            },
            CurrentSongCommand::CreatePlaylist {
                selection,
                name,
                private,
            } => Self::CreatePlaylist {
                selection: selection.into(),
                name,
                private,
            },
            CurrentSongCommand::CreationStatus {
                selection,
                reconcile,
            } => Self::CreationStatus {
                selection: selection.into(),
                reconcile,
            },
            CurrentSongCommand::AddCreatedPlaylist { selection } => Self::AddCreatedPlaylist {
                selection: selection.into(),
            },
            CurrentSongCommand::Start {
                selection,
                continuation,
                options,
            } => Self::Start {
                selection: selection.into(),
                continuation: continuation.map(Into::into),
                options: options.into(),
            },
        }
    }
}

#[tauri::command]
pub(crate) async fn player_current_song_download<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: CurrentSongDownloadTarget,
    command: CurrentSongCommand,
) -> Result<CurrentSongDownloadReply, ClientError> {
    require_main(&window)?;
    client
        .player_current_song_download(&epoch, &target, command.into())
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cumulus_core::membership::Quality;
    use serde_json::{Value, json};

    const OFFER: &str = "0123456789abcdef0123456789abcdef";
    const SELECTION: &str = "a1a2a3a4a5a6a7a8a9a0aaabacadaeaf";
    const DESTINATION: &str = "00000000000000000000000000000001";

    fn parse(value: Value) -> NativeCommand {
        serde_json::from_value::<CurrentSongCommand>(value)
            .unwrap()
            .into()
    }

    #[test]
    fn renderer_can_choose_only_a_native_offer_and_destination_receipt() {
        let target = json!({"playId":"play-1","entryId":"entry-2"});
        let decoded: CurrentSongDownloadTarget = serde_json::from_value(target.clone()).unwrap();
        assert_eq!(decoded.entry_id, "entry-2");
        let mut forged = target;
        forged["songId"] = json!("123");
        assert!(serde_json::from_value::<CurrentSongDownloadTarget>(forged).is_err());
        assert!(
            matches!(parse(json!({"kind":"select","offer":OFFER,"quality":"hires"})),
            NativeCommand::Select { offer, quality: Quality::HiRes } if offer == OFFER)
        );
        assert!(matches!(
            parse(json!({"kind":"choose_playlist","selection":SELECTION,"destination":DESTINATION})),
            NativeCommand::ChoosePlaylist { selection, destination }
                if selection == SELECTION && destination == DESTINATION
        ));
        for forbidden in [
            json!({"kind":"prepare","payed":6}),
            json!({"kind":"select","offer":OFFER,"quality":"dolby"}),
            json!({"kind":"choose_playlist","selection":SELECTION,"playlistId":"81"}),
            json!({"kind":"choose_playlist","selection":SELECTION,"destination":"81"}),
        ] {
            assert!(serde_json::from_value::<CurrentSongCommand>(forbidden).is_err());
        }
    }

    #[test]
    fn final_admission_requires_an_explicit_continuation_and_scoped_options() {
        let options = json!({"quality":"lossless","tags":true,"cover":false,
            "lyrics":true,"container":"audio"});
        let mut request = json!({"kind":"start","selection":SELECTION,"options":options});
        assert!(serde_json::from_value::<CurrentSongCommand>(request.clone()).is_err());
        request["continuation"] = Value::Null;
        assert!(matches!(parse(request.clone()),
            NativeCommand::Start { selection, continuation: None, options }
                if selection == SELECTION && options.quality == Quality::Lossless
                    && options.tags && options.lyrics && !options.cover));
        request["options"]["url"] = json!("https://example.invalid/should-not-enter");
        assert!(serde_json::from_value::<CurrentSongCommand>(request).is_err());
    }
}
