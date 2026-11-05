mod segment;
use super::*;
use crate::{
    download::{completed::CompletedDownloads, offline::OfflineRecord},
    library::{LocalResource, native_open::OpenedFile},
    lyrics::{
        LyricDemand, LyricPreferences, LyricRequest, LyricSnapshot, LyricTarget,
        files::{LocalLyrics, NativeOpenLyrics},
    },
    playback::QueueEntry,
    protocol::api::ApiFuture,
};
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq)]
struct ForgottenLyricSelection {
    record: OfflineRecord,
    entry: QueueEntry,
    play_id: String,
}

#[derive(Clone, PartialEq)]
struct DownloadLyricSource {
    task_id: String,
    song_id: String,
    cloud: bool,
    audio: PathBuf,
    forgotten: Option<ForgottenLyricSelection>,
}

struct DownloadLyricFiles {
    session: Arc<crate::account::AccountSession>,
    token: SessionToken,
    source: DownloadLyricSource,
}

impl LocalLyrics for DownloadLyricFiles {
    fn read<'a>(&'a self, audio: &'a Path) -> ApiFuture<'a, Option<String>> {
        Box::pin(async move {
            if audio != self.source.audio {
                return Ok(None);
            }
            let session = self.session.clone();
            let token = self.token.clone();
            let source = self.source.clone();
            tokio::task::spawn_blocking(move || {
                session.check_generation(&token)?;
                let result =
                    if let Some(forgotten) = &source.forgotten {
                        require_current_entry(&session, &token, &forgotten.entry)?;
                        let result = CompletedDownloads::new(session.clone())?
                            .read_forgotten_lyrics(&token, &forgotten.record, source.cloud);
                        require_current_entry(&session, &token, &forgotten.entry)?;
                        result
                    } else {
                        crate::download::service::DownloadService::new(session.clone())?
                            .read_published_lyrics(
                                &token,
                                &source.task_id,
                                &source.song_id,
                                source.cloud,
                                &source.audio,
                            )
                    };
                session.check_generation(&token)?;
                // A missing, busy, unowned or unreadable optional sidecar is
                // equivalent to no fallback, not a remote lyric failure.
                match result {
                    Err(Error::StaleResponse) => Err(Error::StaleResponse),
                    other => Ok(other.unwrap_or_default()),
                }
            })
            .await
            .map_err(|_| Error::State("download lyric worker stopped"))?
        })
    }
}

fn require_current_entry(
    session: &crate::account::AccountSession,
    token: &SessionToken,
    expected: &QueueEntry,
) -> Result<()> {
    let store = session.persistent_store()?;
    session.with_token(token, |owner| {
        let value = store
            .get(owner, "playback", "queue")?
            .ok_or(Error::StaleResponse)?;
        let queue: crate::playback::QueueSnapshot = serde_json::from_value(value)?;
        if queue.current.as_ref() != Some(&expected.entry_id)
            || !queue.items.iter().any(|entry| entry == expected)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    })
}

pub(super) struct ActiveLyrics {
    token: SessionToken,
    download: Option<DownloadLyricSource>,
    native: Option<NativeLyricSelection>,
    service: Arc<LyricsService>,
}

#[derive(PartialEq, Eq)]
struct NativeLyricSelection {
    file: OpenedFile,
    play_id: String,
}

pub(super) struct LyricContext {
    pub token: SessionToken,
    pub service: Arc<LyricsService>,
    pub target: LyricTarget,
    pub downloaded_audio: Option<PathBuf>,
    forgotten_entry: Option<QueueEntry>,
}

fn auxiliary(text: Option<&str>) -> Vec<crate::lyrics::LyricLine> {
    let text = text.unwrap_or_default();
    let yrc = crate::lyrics::parse_yrc(text).lyric_lines;
    if yrc.is_empty() {
        crate::lyrics::parse_lrc(text).lyric_lines
    } else {
        yrc
    }
}

impl MusicClient {
    pub(super) async fn lyric_context(&self, epoch: &str, play_id: &str) -> Result<LyricContext> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let view = controller.snapshot();
        if view.account != token || view.play_id.as_deref() != Some(play_id) {
            return Err(Error::StaleResponse);
        }
        let selected = view
            .selected
            .as_ref()
            .ok_or(Error::State("no selected song"))?;
        let native = if let Some(value) = selected.resource.get("native_open_file") {
            let file: OpenedFile = serde_json::from_value(value.clone())?;
            let local: LocalResource = serde_json::from_value(selected.resource.clone())?;
            file.check_resource(&local)?;
            Some(NativeLyricSelection {
                file,
                play_id: play_id.into(),
            })
        } else {
            None
        };
        let target = if let Some(native) = &native {
            LyricTarget::Local(native.file.path.clone())
        } else if selected.resource["local_id"].is_string() {
            let resource: LocalResource = serde_json::from_value(selected.resource.clone())?;
            let context = self.local.context().await?;
            let path = tokio::task::spawn_blocking(move || {
                let item = context.library.item(&resource.local_id)?;
                let current = context.library.resource(&resource.local_id)?;
                if current.local_revision != resource.local_revision
                    || item.revision != resource.local_revision
                {
                    return Err(Error::QueueChanged);
                }
                Ok(item.path)
            })
            .await
            .map_err(|_| Error::State("local lyric worker stopped"))??;
            LyricTarget::Local(path)
        } else {
            let id = crate::protocol::api::identifier(&selected.resource["id"])?;
            if selected.resource["cloud_song_id"].as_str() == Some(id.as_str()) {
                LyricTarget::Cloud(id)
            } else {
                LyricTarget::Song(id)
            }
        };
        let download = match &target {
            LyricTarget::Song(id) | LyricTarget::Cloud(id) => {
                let cloud = matches!(target, LyricTarget::Cloud(_));
                selected.resource["path"].as_str().and_then(|path| {
                    let (task_id, forgotten) = match selected.resource.get("download_task_id") {
                        Some(task) => (task.as_str()?.to_owned(), None),
                        None => {
                            let owner = token.uid.as_deref()?;
                            if selected.resource["download_ledger_owner"].as_str() != Some(owner) {
                                return None;
                            }
                            let record: OfflineRecord = serde_json::from_value(
                                selected.resource.get("download_ledger")?.clone(),
                            )
                            .ok()?;
                            if record.path != Path::new(path) || record.song_id != *id {
                                return None;
                            }
                            (
                                record.task_id.clone(),
                                Some(ForgottenLyricSelection {
                                    record,
                                    entry: selected.clone(),
                                    play_id: play_id.to_owned(),
                                }),
                            )
                        }
                    };
                    Some(DownloadLyricSource {
                        task_id,
                        song_id: id.clone(),
                        cloud,
                        audio: path.into(),
                        forgotten,
                    })
                })
            }
            LyricTarget::Local(_) => None,
        };
        let mut lyrics = self.player.lyrics.lock().map_err(|_| Error::LockPoisoned)?;
        let current = controller.snapshot();
        if current.account != token
            || current.play_id.as_deref() != Some(play_id)
            || (native.is_some() && current.selected.as_ref() != Some(selected))
        {
            return Err(Error::StaleResponse);
        }
        // The remote cache remains keyed by song/cloud ID. A downloaded
        // sidecar must never be a cached UI document for a different source.
        if lyrics.as_ref().is_none_or(|old| {
            old.token != token || old.download != download || old.native != native
        }) {
            let service = if let Some(source) = &download {
                LyricsService::with_files(
                    self.session.clone(),
                    Arc::new(DownloadLyricFiles {
                        session: self.session.clone(),
                        token: token.clone(),
                        source: source.clone(),
                    }),
                )?
            } else if let Some(native) = &native {
                LyricsService::with_files(
                    self.session.clone(),
                    Arc::new(NativeOpenLyrics(native.file.clone())),
                )?
            } else {
                LyricsService::new(self.session.clone())?
            };
            *lyrics = Some(ActiveLyrics {
                token: token.clone(),
                download: download.clone(),
                native,
                service: Arc::new(service),
            });
        }
        Ok(LyricContext {
            token,
            service: lyrics.as_ref().expect("initialized").service.clone(),
            target,
            forgotten_entry: download.as_ref().and_then(|source| {
                source
                    .forgotten
                    .as_ref()
                    .map(|selection| selection.entry.clone())
            }),
            downloaded_audio: download.map(|source| source.audio),
        })
    }
    async fn lyric_result(
        &self,
        token: &SessionToken,
        play_id: &str,
        value: LyricSnapshot,
        forgotten_entry: Option<&QueueEntry>,
    ) -> Result<LyricsView> {
        self.session.check_generation(token)?;
        let player = self.player.controller().await?.snapshot();
        if player.account != *token || player.play_id.as_deref() != Some(play_id) {
            return Err(Error::StaleResponse);
        }
        if value.from_sidecar
            && let Some(expected) = forgotten_entry
        {
            // A completed operation may have updated SQLite before the actor
            // has observed the new queue. Never return that detached document.
            if player.selected.as_ref() != Some(expected) {
                return Err(Error::StaleResponse);
            }
            require_current_entry(&self.session, token, expected)?;
        }
        let lyric = value.lyric;
        let mut result = LyricsView {
            word_translation: auxiliary(lyric.yrc_info.yrc_trans.as_deref()),
            word_romanization: auxiliary(lyric.yrc_info.yrc_roma.as_deref()),
            word_lines: crate::lyrics::parse_yrc_words(
                lyric.yrc_info.yrc.as_deref().unwrap_or_default(),
            ),
            epoch: token.generation.to_string(),
            play_id: play_id.into(),
            lines: lyric.lyric_lines,
            translation: lyric.tlyric_lines,
            romanization: lyric.roma_lyric_lines,
            words: lyric.yrc_info,
            offset: lyric.offset,
            scrollable: lyric.scrollable,
            display_type: lyric.display_type,
            preferences: value.preferences,
            network_failed: value.network_failed,
            from_cache: value.from_cache,
            from_sidecar: value.from_sidecar,
            from_embedded: value.from_embedded,
        };
        if let Some((start, length)) = segment::bounds(&player)? {
            segment::apply(&mut result, start, length);
        }
        result.from_sidecar &= !result.lines.is_empty();
        result.from_embedded &= !result.lines.is_empty();
        Ok(result)
    }
    pub async fn player_lyrics(&self, epoch: &str, play_id: &str) -> Result<LyricsView> {
        let LyricContext {
            token,
            service,
            target,
            downloaded_audio,
            forgotten_entry,
        } = self.lyric_context(epoch, play_id).await?;
        if matches!(target, LyricTarget::Local(_)) || service.snapshot()?.network_failed {
            service.clear()?;
        }
        let value = service
            .fetch(LyricRequest {
                target,
                demand: LyricDemand {
                    detail_page: true,
                    ..Default::default()
                },
                downloaded_audio,
                track_mv: false,
            })
            .await?;
        self.lyric_result(&token, play_id, value, forgotten_entry.as_ref())
            .await
    }
    pub(in crate::client) async fn cached_player_lyrics(
        &self,
        epoch: &str,
        play_id: &str,
        observed: Option<&str>,
    ) -> Result<Option<(String, LyricsView, bool)>> {
        let LyricContext {
            token,
            service,
            target,
            forgotten_entry,
            ..
        } = self.lyric_context(epoch, play_id).await?;
        let Some((revision, value)) = service.snapshot_if_changed(&target, observed)? else {
            return Ok(None);
        };
        let failed = value.is_lyric_fetch_failed;
        Ok(Some((
            revision,
            self.lyric_result(&token, play_id, value, forgotten_entry.as_ref())
                .await?,
            failed,
        )))
    }
    /// Project the active shared document without refreshing its source.
    pub async fn player_cached_lyrics(&self, epoch: &str, play_id: &str) -> Result<LyricsView> {
        self.cached_player_lyrics(epoch, play_id, None)
            .await?
            .map(|(_, document, _)| document)
            .ok_or(Error::StaleResponse)
    }
    pub async fn lyric_preferences(
        &self,
        epoch: &str,
        play_id: &str,
        preferences: LyricPreferences,
    ) -> Result<LyricsView> {
        let LyricContext {
            token,
            service,
            target,
            forgotten_entry,
            ..
        } = self.lyric_context(epoch, play_id).await?;
        let value = service.set_target_preferences(&target, preferences)?;
        self.lyric_result(&token, play_id, value, forgotten_entry.as_ref())
            .await
    }
    pub async fn lyric_offset(&self, epoch: &str, play_id: &str, gain: f64) -> Result<LyricsView> {
        let LyricContext {
            token,
            service,
            target,
            forgotten_entry,
            ..
        } = self.lyric_context(epoch, play_id).await?;
        let value = service.adjust_target_offset(&target, gain)?;
        self.lyric_result(&token, play_id, value, forgotten_entry.as_ref())
            .await
    }
}
