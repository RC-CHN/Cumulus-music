use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
    lyrics::LyricPreferences,
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
    State { value: Box<PlayerView> },
    Error { error: ClientError },
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
    let mut source = client
        .subscribe_playback(&epoch)
        .await
        .map_err(ClientError::from)?;
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
        })
        .map_err(|_| ClientError::from(Error::Cancelled))?;
    current.task = Some(tauri::async_runtime::spawn(async move {
        loop {
            let event = match source.changed().await {
                Ok(value) => PlayerUpdate::State {
                    value: Box::new(value),
                },
                Err(error) => {
                    let _ = on_update.send(PlayerUpdate::Error {
                        error: error.into(),
                    });
                    break;
                }
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
pub async fn player_action(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: PlayerAction,
) -> Result<PlayerView, ClientError> {
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
pub async fn player_queue(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<QueueView, ClientError> {
    client.player_queue(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn player_lyrics(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<LyricsView, ClientError> {
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
