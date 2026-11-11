//! Native clipboard and the two authorized public current-song share channels.
use super::{Lease, PlayerStreams};
use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{
            AccountData, HistoryRowShareChoice, HistoryRowShareOutcome, HistoryRowSharePreparation,
            PlayerSongShareTarget,
        },
    },
    playback::engine::Cancellation,
};
use serde::Serialize;
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PlayerSongShareChannel {
    Copied,
    CopyFailed,
    WechatQr { modules: Vec<Vec<bool>> },
    Unavailable,
    AlbumRequired,
}

fn require_main<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("player sharing requires the main window").into())
    }
}

fn original_subscription(lease: Lease) -> Cancellation {
    Cancellation::new(move || lease.ready())
}

#[tauri::command]
pub(crate) async fn player_song_share_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    streams: tauri::State<'_, PlayerStreams>,
    epoch: String,
    target: PlayerSongShareTarget,
) -> Result<AccountData<HistoryRowSharePreparation>, ClientError> {
    require_main(&window)?;
    let lease = streams.ready_lease(&epoch).ok_or(Error::StaleResponse)?;
    let reply = client
        .player_song_share_prepare_with_guard(
            &epoch,
            target.clone(),
            original_subscription(lease.clone()),
        )
        .await?;
    if !lease.ready() {
        if let HistoryRowSharePreparation::Ready { receipt } = &reply.value {
            let _ = client.player_song_share_cancel(&epoch, target, receipt);
        }
        return Err(Error::StaleResponse.into());
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_epoch_subscription_replacement_revokes_the_original_share_authority() {
        let streams = PlayerStreams::default();
        let original = streams.reserve("main", "old-share-stream", 42).unwrap();
        assert!(original.install());
        let authority = original_subscription(original);
        assert!(authority.check().is_ok());
        let replacement = streams.reserve("main", "new-share-stream", 42).unwrap();
        assert!(replacement.install());
        assert!(original_subscription(replacement).check().is_ok());
        assert!(authority.is_cancelled());
    }
}

#[tauri::command]
pub(crate) async fn player_song_share_choose<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    streams: tauri::State<'_, PlayerStreams>,
    epoch: String,
    target: PlayerSongShareTarget,
    offer: String,
    choice: HistoryRowShareChoice,
) -> Result<AccountData<PlayerSongShareChannel>, ClientError> {
    require_main(&window)?;
    let lease = streams.ready_lease(&epoch).ok_or(Error::StaleResponse)?;
    let app = window.app_handle().clone();
    let reply = client
        .player_song_share_choose_with(
            &epoch,
            target,
            &offer,
            choice,
            move |value| -> Result<_, ClientError> {
                if !lease.ready() {
                    return Err(Error::StaleResponse.into());
                }
                Ok(match value {
                    HistoryRowShareOutcome::Copy { url } => {
                        if app.clipboard().write_text(url).is_ok() {
                            PlayerSongShareChannel::Copied
                        } else {
                            PlayerSongShareChannel::CopyFailed
                        }
                    }
                    HistoryRowShareOutcome::WechatQr { modules } => {
                        PlayerSongShareChannel::WechatQr { modules }
                    }
                    HistoryRowShareOutcome::Unavailable => PlayerSongShareChannel::Unavailable,
                    HistoryRowShareOutcome::AlbumRequired => PlayerSongShareChannel::AlbumRequired,
                })
            },
        )
        .await?;
    Ok(AccountData {
        epoch: reply.epoch,
        value: reply.value?,
    })
}

#[tauri::command]
pub(crate) async fn player_song_share_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PlayerSongShareTarget,
    offer: String,
) -> Result<(), ClientError> {
    require_main(&window)?;
    client
        .player_song_share_cancel(&epoch, target, &offer)
        .map_err(Into::into)
}
