//! Each selection starts at most one bounded fetch. Polling/clock reads use the
//! shared versioned document; changing or closing a window aborts its old fetch.
use super::*;
use cumulus_core::client::{
    desktop_lyrics::DesktopLyricView, mini::MiniSnapshot, native_media::NativeMediaKind,
};
use cumulus_core::preferences::desktop_lyrics::Appearance;
use state::ContentState;

pub(super) async fn read<R: Runtime>(
    app: &tauri::AppHandle<R>,
    generation: &str,
    media: &MiniSnapshot,
    appearance: &Appearance,
) -> Result<(ContentState, Option<DesktopLyricView>)> {
    let cache = app.state::<Host>().lock()?.get(generation)?.content.clone();
    // Serialize local projections without replacing valid lyrics with a false
    // loading frame. Network fetches deliberately run outside this lock.
    let mut content = cache.lock().await;
    let key = media
        .view
        .item
        .as_ref()
        .filter(|i| i.kind == NativeMediaKind::Music)
        .and(media.view.selection.audio_play_id.as_ref())
        .map(|id| (media.view.epoch.clone(), id.clone()));
    if content.key != key {
        content.attempt = content
            .attempt
            .checked_add(1)
            .ok_or(Error::State("lyric fetch lifetime exhausted"))?;
        content.key = key.clone();
        content.document = None;
        content.state = if key.is_some() {
            ContentState::Loading
        } else {
            ContentState::Idle
        };
        let host = app.state::<Host>();
        if let Some(fetch) = host.lock()?.get_mut(generation)?.fetch.take() {
            fetch.abort();
        }
        if let Some((epoch, play_id)) = key.clone() {
            let worker = app.clone();
            let generation = generation.to_owned();
            let completed = cache.clone();
            let attempt = content.attempt;
            let task_generation = generation.clone();
            let task = tauri::async_runtime::spawn(async move {
                let client = worker.state::<MusicClient>();
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(12),
                    client.desktop_lyric_document(&epoch, &play_id),
                )
                .await
                .unwrap_or(Err(Error::State("desktop lyric request timed out")));
                if worker
                    .state::<Host>()
                    .lock()
                    .is_ok_and(|s| s.get(&task_generation).is_ok())
                {
                    let mut content = completed.lock().await;
                    if content.attempt == attempt && content.key == Some((epoch, play_id)) {
                        match result {
                            Ok(document) => {
                                content.document = Some(document);
                                content.state = ContentState::Ready;
                            }
                            Err(_) => {
                                content.document = None;
                                content.state = ContentState::Failed;
                            }
                        }
                    }
                }
            });
            // Keep an abort handle without retaining or waiting for the task.
            let abort = task.inner().abort_handle();
            match host.lock()?.get_mut(&generation) {
                Ok(window) => window.fetch = Some(abort),
                Err(error) => {
                    abort.abort();
                    return Err(error);
                }
            }
        }
    }
    let mut frame = None;
    if let Some(document) = content.document.as_mut() {
        let client = app.state::<MusicClient>();
        match client.refresh_desktop_lyric_document(document).await {
            Ok(_) | Err(Error::MutationBusy) => {
                match client.desktop_lyric_frame(document, appearance) {
                    Ok(value) => {
                        content.state = if value.info.fetch_failed {
                            ContentState::Failed
                        } else {
                            ContentState::Ready
                        };
                        frame = Some(value);
                    }
                    Err(Error::StaleResponse) => {
                        content.document = None;
                        content.key = None;
                        content.state = ContentState::Loading;
                    }
                    Err(_) => {
                        content.document = None;
                        content.state = ContentState::Failed;
                    }
                }
            }
            Err(Error::StaleResponse) => {
                content.document = None;
                content.key = None;
                content.state = ContentState::Loading;
            }
            Err(_) => {
                content.document = None;
                content.state = ContentState::Failed;
            }
        }
    }
    app.state::<Host>().lock()?.get(generation)?;
    if !media.same_controls(&app.state::<MusicClient>().mini_snapshot()?) {
        return Err(Error::StaleResponse);
    }
    Ok((content.state, frame))
}
