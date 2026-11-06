pub mod intelligence;
pub mod power;
pub mod quality;
pub mod reports;
pub mod vinyl_similar;
pub mod voice_related;
pub mod voice_song_download;
use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
    lyrics::LyricPreferences,
    playback::controller::OrdinarySkipNotice,
};
use serde::Serialize;
use std::{collections::HashMap, sync::Mutex};
use tauri::{Manager, ipc::Channel};

struct Running {
    id: String,
    epoch: u64,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}
#[derive(Default)]
pub struct PlayerStreams(Mutex<HashMap<String, Running>>);
impl PlayerStreams {
    fn reserve(&self, window: &str, id: &str, epoch: u64) -> cumulus_core::Result<()> {
        let mut values = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if values
            .get(window)
            .is_some_and(|current| current.epoch > epoch)
        {
            return Err(Error::StaleResponse);
        }
        if let Some(Running {
            task: Some(old), ..
        }) = values.insert(
            window.into(),
            Running {
                id: id.into(),
                epoch,
                task: None,
            },
        ) {
            old.abort();
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn ready_for_native_open(&self, epoch: &str) -> bool {
        self.0.lock().is_ok_and(|values| {
            values
                .get("main")
                .is_some_and(|current| current.task.is_some() && current.epoch.to_string() == epoch)
        })
    }

    pub fn stop(&self, window: &str, id: Option<&str>) {
        if let Ok(mut values) = self.0.lock()
            && values
                .get(window)
                .is_some_and(|current| id.is_none_or(|id| id == current.id))
            && let Some(Running {
                task: Some(task), ..
            }) = values.remove(window)
        {
            task.abort();
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayerUpdate {
    State {
        value: Box<PlayerView>,
        #[serde(rename = "nativeOpenError", skip_serializing_if = "Option::is_none")]
        native_open_error: Option<ClientError>,
    },
    Error {
        error: ClientError,
    },
    OrdinaryCopyrightSkip {
        #[serde(flatten)]
        value: OrdinarySkipNotice,
    },
}

#[tauri::command]
pub async fn watch_player<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    streams: tauri::State<'_, PlayerStreams>,
    epoch: String,
    stream_id: String,
    on_update: Channel<PlayerUpdate>,
) -> Result<(), ClientError> {
    if stream_id.is_empty() || stream_id.len() > 64 {
        return Err(Error::InvalidInput("invalid stream id").into());
    }
    client
        .validate_player_epoch(&epoch)
        .map_err(ClientError::from)?;
    let generation = epoch
        .parse()
        .map_err(|_| ClientError::from(Error::InvalidInput("invalid epoch")))?;
    streams
        .reserve(window.label(), &stream_id, generation)
        .map_err(ClientError::from)?;
    let native_open_error = if window.label() == "main"
        && let Some(pending) = window.try_state::<crate::native_open::Pending>()
        && let Some(paths) = pending.take()
    {
        client
            .play_native_open_files(&epoch, paths)
            .await
            .err()
            .map(crate::native_open::public_error)
    } else {
        None
    };
    let mut source = client
        .subscribe_playback(&epoch)
        .await
        .map_err(ClientError::from)?;
    let mut skip_notices = if window.label() == "main" {
        Some(
            client
                .subscribe_ordinary_skip(&epoch)
                .map_err(ClientError::from)?,
        )
    } else {
        None
    };
    let mut values = streams
        .0
        .lock()
        .map_err(|_| ClientError::from(Error::LockPoisoned))?;
    let Some(current) = values.get_mut(window.label()) else {
        return Err(Error::Cancelled.into());
    };
    if current.id != stream_id {
        return Err(Error::Cancelled.into());
    }
    on_update
        .send(PlayerUpdate::State {
            value: Box::new(source.current().map_err(ClientError::from)?),
            native_open_error,
        })
        .map_err(|_| ClientError::from(Error::Cancelled))?;
    let app = window.app_handle().clone();
    current.task = Some(tauri::async_runtime::spawn(async move {
        loop {
            let event = tokio::select! {
                next = source.changed() => match next {
                    Ok(value) => PlayerUpdate::State {
                        value: Box::new(value),
                        native_open_error: None,
                    },
                    Err(error) => {
                        let _ = on_update.send(PlayerUpdate::Error { error: error.into() });
                        break;
                    }
                },
                notice = async {
                    match &mut skip_notices {
                        Some(stream) => stream.next().await,
                        None => std::future::pending().await,
                    }
                } => match notice {
                    Ok(value) => {
                        if app.state::<MusicClient>().validate_player_epoch(&value.epoch).is_err() {
                            continue;
                        }
                        PlayerUpdate::OrdinaryCopyrightSkip { value }
                    }
                    Err(_) => {
                        // Stop this optional arm; normal account/player
                        // invalidation remains owned by the state stream.
                        skip_notices = None;
                        continue;
                    }
                },
            };
            if on_update.send(event).is_err() {
                break;
            }
        }
    }));
    Ok(())
}
#[tauri::command]
pub fn unwatch_player<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    streams: tauri::State<'_, PlayerStreams>,
    stream_id: String,
) {
    streams.stop(window.label(), Some(&stream_id));
}
#[tauri::command]
pub async fn player_snapshot(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<PlayerView, ClientError> {
    client.player_snapshot(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn queue_more_tip<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: QueueMoreTipAction,
) -> Result<Option<String>, ClientError> {
    if window.label() != "main" {
        return Err(
            Error::InvalidInput("queue recommendation cue requires the main window").into(),
        );
    }
    if matches!(&action, QueueMoreTipAction::Begin { .. })
        && (!matches!(window.is_focused(), Ok(true))
            || !matches!(window.is_visible(), Ok(true))
            || !matches!(window.is_minimized(), Ok(false)))
    {
        return Ok(None);
    }
    client
        .queue_more_tip(&epoch, action)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_action<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: PlayerAction,
) -> Result<PlayerView, ClientError> {
    if matches!(
        &action,
        PlayerAction::StepMiniBar { .. }
            | PlayerAction::StepEndedMiniBar { .. }
            | PlayerAction::SongSpeed { .. }
    ) && window.label() != "main"
    {
        return Err(Error::InvalidInput("bottom bar action requires the main window").into());
    }
    client
        .player_action(&epoch, action)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn play_selection(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    selection: PlaySelection,
) -> Result<PlayerView, ClientError> {
    client
        .play_selection(&epoch, selection)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn prepare_play_selection<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    expected_queue_revision: String,
    expected_play_id: Option<String>,
) -> Result<String, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("prepared playback requires the main window").into());
    }
    client
        .prepare_play_selection(
            &epoch,
            &expected_queue_revision,
            expected_play_id.as_deref(),
        )
        .map_err(Into::into)
}
#[tauri::command]
pub async fn commit_prepared_selection<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    ticket: String,
    selection: PlaySelection,
) -> Result<PlayerView, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("prepared playback requires the main window").into());
    }
    client
        .commit_prepared_selection(&epoch, &ticket, selection)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<QueueView, ClientError> {
    client.player_queue(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_collection(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
) -> Result<QueueCollectionView, ClientError> {
    client
        .player_queue_collection(&epoch, &revision)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_collect_rows<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_ids: Vec<String>,
) -> Result<QueueCollectionView, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue song collection requires the main window").into());
    }
    client
        .player_queue_collect_rows(&epoch, &revision, &entry_ids)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_like<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
    liked: bool,
) -> Result<AccountData<LikeResult>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue song like requires the main window").into());
    }
    client
        .player_queue_like(&epoch, &revision, &entry_id, liked)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_catalog_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
) -> Result<QueueCatalogMenu, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue song menu requires the main window").into());
    }
    client
        .player_queue_catalog_menu(&epoch, &revision, &entry_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_chorus(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<Option<f64>, ClientError> {
    client
        .player_chorus(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_comment_count(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<Option<u64>, ClientError> {
    client
        .player_comment_count(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_comments_available<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<bool, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_comments_available(&epoch)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_comments<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    offset: usize,
) -> Result<AccountData<cumulus_core::client::VinylCommentPageView>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_comments(&epoch, &play_id, offset)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_comment_like_operations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<AccountData<Vec<CommentOperationView>>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_comment_like_operations(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_comment_like<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    id: String,
    action: cumulus_core::client::VinylCommentLikeAction,
) -> Result<AccountData<CommentOperationView>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_comment_like(&epoch, &play_id, &id, action)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_comment_report<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    id: String,
    command: CommentReportCommand,
) -> Result<AccountData<CommentReportView>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_comment_report(&epoch, &play_id, &id, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_prepare_write<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    command: CommentWriteCommand,
) -> Result<AccountData<CommentWriteView>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_prepare_write(&epoch, &play_id, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_write_operations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<AccountData<Vec<CommentWriteView>>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_write_operations(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_vinyl_write_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    id: String,
    command: CommentWriteAction,
) -> Result<AccountData<CommentWriteView>, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    client
        .player_vinyl_write_command(&epoch, &play_id, &id, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_lyrics(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    cached_only: Option<bool>,
) -> Result<LyricsView, ClientError> {
    if cached_only == Some(true) {
        return client
            .player_cached_lyrics(&epoch, &play_id)
            .await
            .map_err(Into::into);
    }
    client
        .player_lyrics(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn lyric_preferences(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    preferences: LyricPreferences,
) -> Result<LyricsView, ClientError> {
    client
        .lyric_preferences(&epoch, &play_id, preferences)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn lyric_offset(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    gain: f64,
) -> Result<LyricsView, ClientError> {
    client
        .lyric_offset(&epoch, &play_id, gain)
        .await
        .map_err(Into::into)
}

pub fn close<R: tauri::Runtime>(window: &tauri::Window<R>) {
    if let Some(streams) = window.try_state::<PlayerStreams>() {
        streams.stop(window.label(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_account_subscription_and_late_unwatch_cannot_remove_the_new_stream() {
        let streams = PlayerStreams::default();
        streams.reserve("main", "old", 1).unwrap();
        streams.reserve("main", "new", 2).unwrap();
        assert!(matches!(
            streams.reserve("main", "late-old", 1),
            Err(Error::StaleResponse)
        ));
        streams.stop("main", Some("old"));
        assert_eq!(streams.0.lock().unwrap().get("main").unwrap().id, "new");
        streams.stop("main", Some("new"));
        assert!(streams.0.lock().unwrap().is_empty());
    }
}
