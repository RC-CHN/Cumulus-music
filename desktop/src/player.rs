pub mod intelligence;
pub mod lyric_bounty;
mod native_authority;
#[cfg(any(windows, test))]
mod native_quit;
pub mod power;
pub mod quality;
pub mod reports;
pub mod vinyl_similar;
pub mod voice_related;
pub mod voice_song_collect;
pub mod voice_song_download;
pub(crate) mod voice_song_group_download;
use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
    lyrics::LyricPreferences,
    playback::controller::OrdinarySkipNotice,
};
use native_authority::{Lease, RevokeOnDrop};
#[cfg(windows)]
pub(crate) use native_quit::Recovery as NativeQuitRecovery;
#[cfg(windows)]
use native_quit::{Gate as NativeQuitGate, Permit as NativeQuitPermit};
use serde::Serialize;
use std::{collections::HashMap, sync::Mutex};
use tauri::{Manager, ipc::Channel};

struct Running {
    id: String,
    epoch: u64,
    lease: Lease,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.lease.revoke();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(windows)]
#[derive(Clone)]
pub(crate) struct NativeOpenPermit {
    pub(crate) epoch: String,
    lease: Lease,
    quit: NativeQuitPermit,
}

#[cfg(windows)]
impl NativeOpenPermit {
    pub(crate) fn ready(&self) -> bool {
        self.lease.ready() && self.quit.active()
    }
}

#[derive(Default)]
pub struct PlayerStreams {
    streams: Mutex<HashMap<String, Running>>,
    #[cfg(windows)]
    native_quit: NativeQuitGate,
}
impl PlayerStreams {
    fn reserve(&self, window: &str, id: &str, epoch: u64) -> cumulus_core::Result<Lease> {
        let mut values = self.streams.lock().map_err(|_| Error::LockPoisoned)?;
        if values
            .get(window)
            .is_some_and(|current| current.epoch > epoch)
        {
            return Err(Error::StaleResponse);
        }
        let lease = Lease::default();
        values.insert(
            window.into(),
            Running {
                id: id.into(),
                epoch,
                lease: lease.clone(),
                task: None,
            },
        );
        Ok(lease)
    }

    #[cfg(any(target_os = "linux", windows))]
    fn ready_lease(&self, epoch: &str) -> Option<Lease> {
        let values = self.streams.lock().ok()?;
        let current = values.get("main")?;
        (current.epoch.to_string() == epoch
            && current.lease.ready()
            && current
                .task
                .as_ref()
                .is_some_and(|task| !task.inner().is_finished()))
        .then(|| current.lease.clone())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn ready_for_native_open(&self, epoch: &str) -> bool {
        self.ready_lease(epoch).is_some()
    }

    #[cfg(windows)]
    pub(crate) fn native_open_permit(&self, epoch: String) -> Option<NativeOpenPermit> {
        Some(NativeOpenPermit {
            lease: self.ready_lease(&epoch)?,
            quit: self.native_quit.permit()?,
            epoch,
        })
    }

    #[cfg(windows)]
    pub(crate) fn close_native_open_for_quit(&self) -> Option<NativeQuitRecovery> {
        self.native_quit.close()
    }

    #[cfg(windows)]
    fn take_pending_native_open(
        &self,
        lease: &Lease,
        epoch: u64,
        pending: &crate::native_open::Pending,
    ) -> Option<(Vec<std::path::PathBuf>, NativeQuitPermit)> {
        let values = self.streams.lock().ok()?;
        let current = values.get("main")?;
        if !current.lease.same(lease) || !lease.active() {
            return None;
        }
        let quit = self.native_quit.permit()?;
        Some((pending.take_for_epoch(epoch)?, quit))
    }

    pub fn stop(&self, window: &str, id: Option<&str>) {
        if let Ok(mut values) = self.streams.lock()
            && values
                .get(window)
                .is_some_and(|current| id.is_none_or(|id| id == current.id))
        {
            values.remove(window);
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
    let lease = streams
        .reserve(window.label(), &stream_id, generation)
        .map_err(ClientError::from)?;
    let mut registration = RevokeOnDrop::new(&lease);
    #[cfg(not(windows))]
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
    #[cfg(windows)]
    let native_open_error = if window.label() == "main"
        && let Some(pending) = window.try_state::<crate::native_open::Pending>()
        && let Some((paths, quit)) = streams.take_pending_native_open(&lease, generation, &pending)
    {
        let opening = lease.clone();
        let authority = cumulus_core::playback::engine::Cancellation::new(move || {
            opening.active() && quit.active()
        });
        client
            .play_windows_native_open_files(&epoch, paths, authority)
            .await
            .err()
            .map(crate::native_open::public_error)
    } else {
        None
    };
    // Account access happens outside the stream map lock. The exact lease
    // is checked again before publishing this first state.
    let initial = source.current().map_err(ClientError::from)?;
    let mut values = streams
        .streams
        .lock()
        .map_err(|_| ClientError::from(Error::LockPoisoned))?;
    let Some(current) = values.get_mut(window.label()) else {
        return Err(Error::Cancelled.into());
    };
    if !current.lease.same(&lease) {
        return Err(Error::Cancelled.into());
    }
    on_update
        .send(PlayerUpdate::State {
            value: Box::new(initial),
            native_open_error,
        })
        .map_err(|_| ClientError::from(Error::Cancelled))?;
    let app = window.app_handle().clone();
    let task_revocation = RevokeOnDrop::new(&lease);
    let task = tauri::async_runtime::spawn(async move {
        let _task_revocation = task_revocation;
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
    });
    if !lease.install() {
        task.abort();
        return Err(Error::Cancelled.into());
    }
    current.task = Some(task);
    registration.disarm();
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
        && (!matches!(crate::queue_tip_window_foreground(&window), Ok(true))
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
    if matches!(
        &action,
        PlayerAction::QueueVoice {
            program: Some(_),
            ..
        } | PlayerAction::QueueRecentProgram { .. }
            | PlayerAction::QueuePurchasedProgram { .. }
            | PlayerAction::QueueLikedPageProgram { .. }
    ) && window.label() != "main"
    {
        return Err(Error::InvalidInput("podcast program queue requires the main window").into());
    }
    if matches!(
        &action,
        PlayerAction::QueueOfflinePlaylist { .. } | PlayerAction::QueueSortedPlaylist { .. }
    ) && window.label() != "main"
    {
        return Err(Error::InvalidInput("cached playlist queue requires the main window").into());
    }
    if matches!(
        &action,
        PlayerAction::SearchGroupNextPlay { .. } | PlayerAction::SearchLyricGroupPlayback { .. }
    ) && window.label() != "main"
    {
        return Err(Error::InvalidInput("search selection requires the main window").into());
    }
    #[cfg(windows)]
    if matches!(&action, PlayerAction::Startup)
        && window.label() == "main"
        && window
            .try_state::<crate::native_open::Pending>()
            .is_some_and(|pending| pending.blocks_startup(&epoch))
    {
        // An explicit process launch is one-shot. If a rebind superseded the
        // opening stream, the replacement cannot autoplay the restored queue.
        return client.player_snapshot(&epoch).await.map_err(Into::into);
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
pub async fn player_queue_open_catalog_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
    receipt: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue catalog directory requires the main window").into());
    }
    let reveal = client
        .player_queue_catalog_file_reveal(&epoch, &revision, &entry_id, &receipt)
        .await?;
    let app = window.app_handle().clone();
    crate::file_manager::reveal_file_checked(reveal.path().to_owned(), move || {
        let client = app.state::<MusicClient>();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                reveal.check_current(&client),
            )
            .await
            .map_err(|_| Error::State("queue catalog file validation timed out"))?
        })
    })
    .await
    .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_voice_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
) -> Result<QueueVoiceMenu, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue voice menu requires the main window").into());
    }
    client
        .player_queue_voice_menu(
            &epoch,
            &revision,
            &entry_id,
            crate::network_diagnostics::observed_default_route(&window),
        )
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_open_voice_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue voice directory requires the main window").into());
    }
    let reveal = client
        .player_queue_voice_file_reveal(&epoch, &revision, &entry_id)
        .await?;
    let app = window.app_handle().clone();
    crate::file_manager::reveal_file_checked(reveal.path().to_owned(), move || {
        let client = app.state::<MusicClient>();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                reveal.check_current(&client),
            )
            .await
            .map_err(|_| Error::State("queue voice file reveal validation timed out"))?
        })
    })
    .await
    .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_open_local_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("queue local directory requires the main window").into());
    }
    let reveal = client
        .player_queue_local_file_reveal(&epoch, &revision, &entry_id)
        .await?;
    let app = window.app_handle().clone();
    crate::file_manager::reveal_file_checked(reveal.path().to_owned(), move || {
        let client = app.state::<MusicClient>();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                reveal.check_current(&client),
            )
            .await
            .map_err(|_| Error::State("queue local file validation timed out"))?
        })
    })
    .await
    .map_err(Into::into)
}
#[tauri::command]
pub async fn player_queue_open_download_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    entry_id: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(
            Error::InvalidInput("queue download directory requires the main window").into(),
        );
    }
    let reveal = client
        .player_queue_download_file_reveal(&epoch, &revision, &entry_id)
        .await?;
    let app = window.app_handle().clone();
    crate::file_manager::reveal_file_checked(reveal.path().to_owned(), move || {
        let client = app.state::<MusicClient>();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                reveal.check_current(&client),
            )
            .await
            .map_err(|_| Error::State("queue download file validation timed out"))?
        })
    })
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
pub async fn lyric_contribution_open<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    launch_lyric_contribution(client.lyric_contribution_url(&epoch, &play_id).await?).await
}

#[tauri::command]
pub async fn lyric_toolbar_contribution_open<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    action: LyricToolbarAction,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("main window required").into());
    }
    let url = client
        .lyric_toolbar_contribution_url(&epoch, &play_id, action)
        .await?;
    launch_lyric_contribution(url).await
}

async fn launch_lyric_contribution(url: String) -> Result<(), ClientError> {
    tauri::async_runtime::spawn_blocking(move || crate::browser::launch(&url))
        .await
        .map_err(|_| Error::State("browser worker stopped"))?
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
    #[cfg(target_os = "linux")]
    #[test]
    fn warm_file_open_requires_a_live_main_subscription() {
        let streams = PlayerStreams::default();
        let lease = streams.reserve("main", "player", 42).unwrap();
        assert!(!streams.ready_for_native_open("42"));
        let (finish, waiting) = tokio::sync::oneshot::channel::<()>();
        let task = tauri::async_runtime::spawn(async move {
            let _ = waiting.await;
        });
        streams
            .streams
            .lock()
            .unwrap()
            .get_mut("main")
            .unwrap()
            .task = Some(task);
        assert!(lease.install());
        assert!(streams.ready_for_native_open("42"));
        assert!(!streams.ready_for_native_open("43"));
        finish.send(()).unwrap();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while streams.ready_for_native_open("42") {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        });
        // The terminated handle is still present until the WebView unbinds.
        assert!(
            streams
                .streams
                .lock()
                .unwrap()
                .get("main")
                .unwrap()
                .task
                .is_some()
        );
    }

    #[test]
    fn same_epoch_rebind_never_resurrects_a_captured_permit() {
        let streams = PlayerStreams::default();
        let first = streams.reserve("main", "same-id", 42).unwrap();
        assert!(first.install());
        let next = streams.reserve("main", "same-id", 42).unwrap();
        assert!(!first.ready());
        assert!(!first.install());
        assert!(!first.same(&next));
        let guard = RevokeOnDrop::new(&next);
        let unpolled = async move { drop(guard) };
        drop(unpolled);
        assert!(!next.install());
    }

    #[cfg(windows)]
    #[test]
    fn pending_open_is_bound_once_and_suppresses_startup_after_a_same_epoch_rebind() {
        let streams = PlayerStreams::default();
        let pending = crate::native_open::Pending::from_args(["--play".into()].into_iter());
        let old = streams.reserve("main", "first", 42).unwrap();
        let (paths, quit) = streams
            .take_pending_native_open(&old, 42, &pending)
            .unwrap();
        assert!(paths.is_empty());
        assert!(quit.active());
        let replacement = streams.reserve("main", "replacement", 42).unwrap();
        assert!(!old.active());
        assert!(
            streams
                .take_pending_native_open(&replacement, 42, &pending)
                .is_none()
        );
        assert!(pending.blocks_startup("42"));
        assert!(!pending.blocks_startup("43"));
    }

    #[cfg(windows)]
    #[test]
    fn confirmed_quit_cancels_a_consumed_cold_selection_without_enabling_old_startup() {
        let streams = PlayerStreams::default();
        let pending = crate::native_open::Pending::from_args(["--play".into()].into_iter());
        let lease = streams.reserve("main", "first", 42).unwrap();
        let first_quit = streams.close_native_open_for_quit().unwrap();
        assert!(
            streams
                .take_pending_native_open(&lease, 42, &pending)
                .is_none()
        );
        assert!(pending.blocks_startup("42"));
        assert!(first_quit.reopen());
        let (_, permit) = streams
            .take_pending_native_open(&lease, 42, &pending)
            .unwrap();
        let second_quit = streams.close_native_open_for_quit().unwrap();
        assert!(second_quit.reopen());
        assert!(!permit.active());
        assert!(
            streams
                .take_pending_native_open(&lease, 42, &pending)
                .is_none()
        );
        assert!(pending.blocks_startup("42"));
        assert!(!pending.blocks_startup("43"));
    }

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
        assert_eq!(
            streams.streams.lock().unwrap().get("main").unwrap().id,
            "new"
        );
        streams.stop("main", Some("new"));
        assert!(streams.streams.lock().unwrap().is_empty());
    }
}
