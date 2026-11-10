use super::super::recommendations::{
    RecommendationSource, daily_source, daily_subset, style_source,
};
use super::{prepared_selection::PreparedSelection, queue_click::RetainedRow, *};
use crate::playlist::MGC_TRIAL;
use crate::{catalog::CatalogApi, playlist::PlaylistApi, protocol::api::identifier};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;

impl MusicClient {
    pub async fn play_downloads(
        &self,
        epoch: &str,
        task_ids: Vec<String>,
        index: usize,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        if index >= task_ids.len() {
            return Err(Error::InvalidInput("invalid download selection index"));
        }
        let lease = self
            .claim_media(&token, super::super::focus::Owner::Audio)
            .await?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let resources = self.download_resources(epoch, task_ids).await?;
        let controller = self.player.controller().await?;
        if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        self.focus.check(lease, super::super::focus::Owner::Audio)?;
        let value = controller
            .command(
                &token,
                PlaybackCommand::Replace {
                    resources,
                    index,
                    source: json!({"kind":"downloads"}),
                },
            )
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        project(&value, &token)
    }
    pub async fn play_selection(&self, epoch: &str, input: PlaySelection) -> Result<PlayerView> {
        match input {
            PlaySelection::ProfileRankPlaylist { uid, quality } => {
                self.play_profile_rank_playlist(epoch, uid.as_deref(), quality)
                    .await
            }
            PlaySelection::VerticalZonePlaylistBatch {
                id,
                receipt,
                ids,
                revision,
                quality,
            } => {
                self.zone_playlist_queue_action(
                    epoch,
                    super::zone_playlist_batch::ZoneQueueSelection {
                        id,
                        receipt,
                        revision,
                        quality,
                        rows: super::zone_playlist_batch::ZoneQueueRows::Batch(ids),
                    },
                    None,
                )
                .await
            }
            PlaySelection::LocalBatch { .. } | PlaySelection::LocalBatchAdd { .. } => Err(
                Error::InvalidInput("local batch requires a preparation ticket"),
            ),
            PlaySelection::FollowLatest { revision, quality } => {
                self.play_follow_latest(epoch, &revision, quality).await
            }
            PlaySelection::SortedPlaylist {
                scope,
                start,
                double_click,
                quality,
            } => {
                self.play_sorted_playlist(epoch, scope, start, double_click, quality)
                    .await
            }
            PlaySelection::OfflinePlaylist {
                id,
                revision,
                cached_at_ms,
                start_id: None,
                ..
            } => {
                self.play_offline_playlist_header(epoch, &id, &revision, cached_at_ms)
                    .await
            }
            PlaySelection::PodcastSubscriptionRecommendation { .. } => Err(Error::InvalidInput(
                "recommended podcast playback requires a preparation ticket",
            )),
            PlaySelection::SearchArtistCard { .. } => Err(Error::InvalidInput(
                "search artist playback requires a preparation ticket",
            )),
            PlaySelection::PodcastBrowseVoice { .. } => Err(Error::InvalidInput(
                "podcast browse playback requires a preparation ticket",
            )),
            PlaySelection::SearchPodcastCard { .. } | PlaySelection::SearchPodcastRow { .. } => {
                Err(Error::InvalidInput(
                    "search podcast playback requires a preparation ticket",
                ))
            }
            PlaySelection::SearchVoiceMenu { .. } => Err(Error::InvalidInput(
                "search voice menu playback requires a preparation ticket",
            )),
            PlaySelection::HistoryRadioChapter { .. } => Err(Error::InvalidInput(
                "history chapter playback requires a preparation ticket",
            )),
            input => self.play_selection_inner(epoch, input, None).await,
        }
    }

    async fn play_offline_playlist_header(
        &self,
        epoch: &str,
        id: &str,
        revision: &str,
        cached_at_ms: u64,
    ) -> Result<PlayerView> {
        use super::super::focus::Owner;
        let token = self.player_token(epoch)?;
        let observed = self.player.selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        // Resolve the whole snapshot before claiming audio. An empty or
        // retired cache must not interrupt incumbent playback, including video.
        let queue = self
            .offline_playlist_queue(&token, id, revision, cached_at_ms, None, false)
            .await?;
        if queue.guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let (lease, attempt) = self.reserve_prepared_audio(&token, focus, || {
            let next = observed
                .checked_add(1)
                .ok_or(Error::State("playback selection exhausted"))?;
            self.player
                .selection
                .compare_exchange(observed, next, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| Error::StaleResponse)?;
            Ok(next)
        })?;
        self.finish_media_claim(&token, Owner::Audio, lease).await?;
        let state = self
            .submit_selection_with_source(
                &token,
                PlaybackCommand::Replace {
                    resources: queue.resources,
                    index: queue.index,
                    source: queue.source,
                },
                attempt,
                Some(lease),
                None,
                Some(queue.guard),
            )
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        project(&state, &token)
    }

    pub(super) async fn play_selection_inner(
        &self,
        epoch: &str,
        input: PlaySelection,
        prepared: Option<PreparedSelection>,
    ) -> Result<PlayerView> {
        self.play_selection_with_daily_origin(epoch, input, prepared, None)
            .await
    }

    pub(in crate::client) async fn play_daily_notification_selection(
        &self,
        epoch: &str,
        revision: String,
        quality: crate::membership::Quality,
        origin: super::super::daily_notification::PlaybackOrigin,
    ) -> Result<PlayerView> {
        self.play_selection_with_daily_origin(
            epoch,
            PlaySelection::DailyRecommendations {
                revision,
                index: 0,
                quality,
            },
            None,
            Some(origin),
        )
        .await
    }

    async fn play_selection_with_daily_origin(
        &self,
        epoch: &str,
        input: PlaySelection,
        prepared: Option<PreparedSelection>,
        daily_origin: Option<super::super::daily_notification::PlaybackOrigin>,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        if let PlaySelection::SearchVoiceMenu { target } = &input {
            return self
                .play_search_voice_menu(
                    &token,
                    target,
                    prepared.as_ref().ok_or(Error::StaleResponse)?,
                )
                .await;
        }
        let (input, zone_detail) = match input {
            PlaySelection::VerticalZonePlaylistDetail {
                id,
                receipt,
                revision,
                start_id,
                double_click,
                quality,
            } => {
                let snapshot = self
                    .vertical_zone_playlist_snapshot(&token, &receipt, &id, &revision)
                    .await?;
                let input = if double_click {
                    PlaySelection::SongRowDoubleClick {
                        selection: SongRowSelection::Playlist {
                            id: id.clone(),
                            start_id: start_id
                                .ok_or(Error::InvalidInput("MGC double-click requires a song"))?,
                            revision: Some(revision.clone()),
                            quality,
                        },
                    }
                } else {
                    PlaySelection::Playlist {
                        id: id.clone(),
                        start_id,
                        revision: Some(revision.clone()),
                        quality,
                    }
                };
                (input, Some((receipt, id, revision, snapshot)))
            }
            other => (other, None),
        };
        let (input, zone_playlist) = match input {
            PlaySelection::VerticalZonePlaylist {
                id,
                view_id,
                card_key,
                quality,
            } => {
                let trial = self
                    .vertical_zone_playlist_trial(&token, &view_id, &card_key, &id)
                    .await?;
                let playlist = PlaySelection::Playlist {
                    id: id.clone(),
                    start_id: None,
                    revision: None,
                    quality,
                };
                (playlist, Some((view_id, card_key, id, trial)))
            }
            other => (other, None),
        };
        let mut single_album_row = false;
        let mut album_double_click = false;
        let mut daily_row = None;
        let (input, retained) = match input {
            PlaySelection::SongRowDoubleClick { selection } => {
                let keep_queue = !self.replace_queue_on_double_click(&token)?;
                let input = match selection {
                    SongRowSelection::DailyRecommendation {
                        revision,
                        index,
                        track_id,
                        quality,
                    } => {
                        daily_row = Some(track_id);
                        PlaySelection::DailyRecommendations {
                            revision,
                            index,
                            quality,
                        }
                    }
                    SongRowSelection::LatestRelease {
                        area,
                        revision,
                        start_id,
                        quality,
                    } => {
                        let (ids, index) =
                            self.latest_release_rows_for(&token, area, &revision, &start_id)?;
                        PlaySelection::Songs {
                            ids,
                            index,
                            quality,
                        }
                    }
                    SongRowSelection::AlbumDetail {
                        album,
                        row,
                        quality,
                    } => {
                        album_double_click = true;
                        // M3085 uses only the clicked resource when the old queue
                        // is retained; replacement takes the visible album rows.
                        single_album_row = keep_queue;
                        PlaySelection::AlbumDetail {
                            album,
                            row: Some(row),
                            quality,
                        }
                    }
                    SongRowSelection::Catalog {
                        pages,
                        index,
                        quality,
                    } => PlaySelection::Catalog {
                        pages,
                        index,
                        quality,
                    },
                    SongRowSelection::Playlist {
                        id,
                        start_id,
                        revision,
                        quality,
                    } => PlaySelection::Playlist {
                        id,
                        start_id: Some(start_id),
                        revision,
                        quality,
                    },
                };
                let policy = if daily_row.is_some() {
                    RetainedRow::DailyRecommendation
                } else {
                    RetainedRow::Catalog
                };
                (input, keep_queue.then_some(policy))
            }
            input @ (PlaySelection::MixedPlaylist {
                double_click: true,
                start: Some(_),
                ..
            }
            | PlaySelection::OfflinePlaylist {
                double_click: true,
                start_id: Some(_),
                ..
            }) => {
                let keep_queue = !self.replace_queue_on_double_click(&token)?;
                (input, keep_queue.then_some(RetainedRow::Catalog))
            }
            input @ PlaySelection::AlbumDetail { row: Some(_), .. } => {
                (input, Some(RetainedRow::Catalog))
            }
            input @ PlaySelection::PodcastRecent { double_click, .. } => {
                let replace = double_click && self.replace_queue_on_double_click(&token)?;
                (input, (!replace).then_some(RetainedRow::SearchVoice))
            }
            input @ PlaySelection::SearchResultTrack { .. } => {
                (input, Some(RetainedRow::SearchCatalog))
            }
            input @ (PlaySelection::SearchHotTrack { .. } | PlaySelection::MessageTrack { .. }) => {
                (input, Some(RetainedRow::SearchSong))
            }
            input @ (PlaySelection::SearchResultVoice { .. }
            | PlaySelection::MessageVoice { .. }) => (input, Some(RetainedRow::SearchVoice)),
            input => (input, None),
        };
        if !matches!(
            input,
            PlaySelection::Local { .. }
                | PlaySelection::LocalBatch { .. }
                | PlaySelection::LocalBatchAdd { .. }
                | PlaySelection::History { .. }
                | PlaySelection::Downloads { .. }
        ) {
            self.session.with_token(&token, |_| Ok(()))?;
        }
        let (recommendation_ids, source_guard) = match &input {
            PlaySelection::StyleRecommendations {
                revision, index, ..
            } => {
                let (ids, guard) = self.recommendation_selection(
                    RecommendationSource::style_for_revision(revision),
                    &token,
                    revision,
                    *index,
                )?;
                (Some(ids), Some(guard))
            }
            PlaySelection::DailyRecommendations {
                revision, index, ..
            } => {
                let (ids, guard) = self.recommendation_selection(
                    RecommendationSource::Daily,
                    &token,
                    revision,
                    *index,
                )?;
                if let Some(track_id) = daily_row.as_deref()
                    && ids[*index].as_str() != Some(track_id)
                {
                    return Err(Error::QueueChanged);
                }
                (Some(ids), Some(guard))
            }
            PlaySelection::DailyRecommendationsBatch {
                revision, indices, ..
            } => {
                let (ids, guard) = self.recommendation_selection(
                    RecommendationSource::Daily,
                    &token,
                    revision,
                    0,
                )?;
                (Some(daily_subset(ids, Some(indices))?), Some(guard))
            }
            PlaySelection::AlbumDetail { album, .. } => {
                (None, Some(self.album_playback_guard(&token, album)?))
            }
            _ => (None, None),
        };
        if let PlaySelection::ListenedBookChapter { revision, key } = &input {
            self.check_listened_chapter(&token, revision, *key)?;
        }
        let retained_album =
            retained.is_some() && matches!(&input, PlaySelection::AlbumDetail { row: Some(_), .. });
        let preserve = if retained.is_some() {
            Some(self.queue_click_snapshot(&token).await?)
        } else {
            None
        };
        use super::super::focus::Owner;
        let (lease, attempt) = if let Some(prepared) = &prepared {
            (None, prepared.observed)
        } else if let Some(snapshot) = &preserve {
            let lease = self
                .claim_native_media(&token, Owner::Audio, snapshot.focus)
                .await?;
            (Some(lease), snapshot.attempt)
        } else {
            let lease = self.claim_media(&token, Owner::Audio).await?;
            (
                Some(lease),
                self.player.selection.fetch_add(1, Ordering::AcqRel) + 1,
            )
        };
        if let (
            PlaySelection::SearchResultTrack {
                id,
                toggle_current: true,
                ..
            },
            Some(snapshot),
            Some(lease),
        ) = (&input, &preserve, lease)
            && let Some(state) = self
                .toggle_retained_search_track(&token, snapshot, id, lease)
                .await?
        {
            self.session.with_token(&token, |_| Ok(()))?;
            return project(&state, &token);
        }
        let message_voice = matches!(&input, PlaySelection::MessageVoice { .. });
        let input = match input {
            PlaySelection::SearchArtistCard { id, quality } => {
                let (command, source_guard) =
                    self.search_artist_selection(&token, &id, quality).await?;
                let state = self
                    .submit_selection_with_source(
                        &token,
                        command,
                        attempt,
                        lease,
                        prepared.as_ref(),
                        Some(source_guard),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::PodcastBrowseVoice { receipt } => {
                let prepared = prepared.as_ref().ok_or(Error::StaleResponse)?;
                let (command, source_guard) = self
                    .podcast_browse_selection(&token, &receipt, prepared)
                    .await?;
                let state = self
                    .submit_selection_with_source(
                        &token,
                        command,
                        attempt,
                        lease,
                        Some(prepared),
                        Some(source_guard),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::SearchPodcastRow {
                id,
                receipt,
                gesture,
            } => {
                use super::search_podcast::SearchPodcastRowPlan;
                let prepared = prepared.as_ref().ok_or(Error::StaleResponse)?;
                let (plan, source_guard) = self
                    .search_podcast_row_selection(&token, &receipt, &id, gesture, prepared)
                    .await?;
                let state = match plan {
                    SearchPodcastRowPlan::Play(command) => {
                        self.submit_selection_with_source(
                            &token,
                            command,
                            attempt,
                            lease,
                            Some(prepared),
                            Some(source_guard),
                        )
                        .await?
                    }
                    SearchPodcastRowPlan::QueueOnly(update) => {
                        self.submit_prepared_queue_only(&token, prepared, update, source_guard)
                            .await?
                    }
                };
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::SearchPodcastCard { id, receipt } => {
                let prepared = prepared.as_ref().ok_or(Error::StaleResponse)?;
                let (command, source_guard) =
                    self.search_podcast_selection(&token, &receipt, &id).await?;
                let state = self
                    .submit_selection_with_source(
                        &token,
                        command,
                        attempt,
                        lease,
                        Some(prepared),
                        Some(source_guard),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::Downloads { query, start_id } => {
                let (resources, index) = self.download_selection(epoch, query, start_id).await?;
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"downloads"}),
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::History {
                history_kind,
                revision,
                ids,
                index,
                quality,
            } => {
                let (resources, index, source) = self
                    .history_selection(
                        &token,
                        super::super::history::Selection {
                            kind: history_kind,
                            revision: &revision,
                            ids: &ids,
                            index,
                            quality,
                        },
                        attempt,
                    )
                    .await?;
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source,
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::Voice { id }
            | PlaySelection::SearchResultVoice { id }
            | PlaySelection::MessageVoice { id } => {
                let id = identifier(&json!(id))?;
                let resource = self
                    .session
                    .run_for(&token, |api, _| async {
                        let episode = crate::podcast::PodcastApi::new(api)
                            .episode(&json!(id))
                            .await?;
                        let mut resource = episode.playback_resource()?;
                        resource["song"] = episode.download_metadata()?;
                        resource["level"] = json!("standard");
                        Ok(resource)
                    })
                    .await?;
                if let Some(snapshot) = preserve {
                    let state = self
                        .play_retained_queue_row(
                            &token,
                            snapshot,
                            (
                                resource,
                                if message_voice {
                                    json!({"kind":"voice","id":id,"scene":"message"})
                                } else {
                                    json!({"kind":"voice","id":id,"scene":"search","pldScene":"audio"})
                                },
                            ),
                            lease.ok_or(Error::StaleResponse)?,
                            RetainedRow::SearchVoice,
                        )
                        .await?;
                    self.session.with_token(&token, |_| Ok(()))?;
                    return project(&state, &token);
                }
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources: vec![resource],
                            index: 0,
                            source: json!({"kind":"voice","id":id,"scene":"comment"}),
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::ListenedBookChapter { revision, key } => {
                let (resources, index, source, cancellation) =
                    self.listened_book_queue(&token, &revision, key).await?;
                let state = self
                    .submit_selection_with_source(
                        &token,
                        PlaybackCommand::ReplaceListenedBook {
                            resources,
                            index,
                            source,
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                        Some(cancellation),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::HistoryRadioChapter {
                history_kind,
                revision,
                id,
            } => {
                let prepared = prepared.as_ref().ok_or(Error::StaleResponse)?;
                let (resources, index, source, cancellation) = self
                    .history_radio_chapter_queue(&token, history_kind, &revision, &id)
                    .await?;
                let state = self
                    .submit_selection_with_source(
                        &token,
                        PlaybackCommand::ReplaceListenedBook {
                            resources,
                            index,
                            source,
                        },
                        attempt,
                        lease,
                        Some(prepared),
                        Some(cancellation),
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::PodcastSubscriptionRecommendation {
                receipt,
                revision,
                id,
                action,
            } => {
                let prepared = prepared.as_ref().ok_or(Error::StaleResponse)?;
                let (command, source) = self
                    .recommended_podcast_command(&token, &receipt, &revision, &id, action)
                    .await?;
                let state = match command {
                    PlaybackCommand::UpdateQueue(update) => {
                        self.submit_prepared_queue_only(&token, prepared, update, source)
                            .await?
                    }
                    command => {
                        self.submit_selection_with_source(
                            &token,
                            command,
                            attempt,
                            lease,
                            Some(prepared),
                            Some(source),
                        )
                        .await?
                    }
                };
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::PodcastRecent { revision, id, .. } => {
                let mut selection = self
                    .podcast_recent_resources(&token, &revision, &id, preserve.is_none())
                    .await?;
                let source = super::origin::recent_podcast();
                let state = if let Some(snapshot) = preserve {
                    self.play_retained_queue_row_with_source(
                        &token,
                        snapshot,
                        (selection.resources.remove(selection.index), source),
                        lease.ok_or(Error::StaleResponse)?,
                        RetainedRow::SearchVoice,
                        Some(selection.source),
                    )
                    .await?
                } else {
                    self.submit_selection_with_source(
                        &token,
                        PlaybackCommand::Replace {
                            resources: selection.resources,
                            index: selection.index,
                            source,
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                        Some(selection.source),
                    )
                    .await?
                };
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::Podcast {
                scope,
                start_id,
                revision,
            } => {
                let (resources, index) = self
                    .podcast_resources(epoch, &scope, &revision, start_id.as_deref())
                    .await?;
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"podcast","scope":scope,"revision":revision}),
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            input @ (PlaySelection::LocalBatch { .. } | PlaySelection::LocalBatchAdd { .. }) => {
                use super::local_batch::LocalBatchAction;
                let action = if matches!(input, PlaySelection::LocalBatch { .. }) {
                    LocalBatchAction::Play
                } else {
                    LocalBatchAction::Add
                };
                let (query, revision, tracks) = match input {
                    PlaySelection::LocalBatch {
                        query,
                        revision,
                        tracks,
                    }
                    | PlaySelection::LocalBatchAdd {
                        query,
                        revision,
                        tracks,
                    } => (query, revision, tracks),
                    _ => unreachable!("matched local batch"),
                };
                let prepared = prepared.as_ref().ok_or(Error::StaleResponse)?;
                let (update, guard) = self
                    .local_batch_selection(&token, query, revision, tracks, action)
                    .await?;
                let state = if update.start {
                    self.submit_selection_with_source(
                        &token,
                        PlaybackCommand::ApplyLocalBatch(update),
                        attempt,
                        lease,
                        Some(prepared),
                        Some(guard),
                    )
                    .await?
                } else {
                    self.submit_prepared_local_queue_only(&token, prepared, update, guard)
                        .await?
                };
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::Local {
                mut query,
                start_id,
                revision,
            } => {
                query.revision = Some(revision.clone());
                let context = self.local.context().await?;
                let (resources, index) = tokio::task::spawn_blocking(move || {
                    context.library.catalog(&query)?.queue(start_id.as_deref())
                })
                .await
                .map_err(|_| Error::State("local selection worker stopped"))??;
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"local","revision":revision}),
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::OfflinePlaylist {
                id,
                revision,
                cached_at_ms,
                start_id,
                start_local,
                double_click,
            } => {
                let offline = self
                    .offline_playlist_queue(
                        &token,
                        &id,
                        &revision,
                        cached_at_ms,
                        start_id.as_deref().map(|id| (id, start_local)),
                        preserve.is_some(),
                    )
                    .await?;
                if double_click && let Some(lease) = lease {
                    let row = &offline.resources[offline.index];
                    let song = if row["local_id"].is_string() {
                        super::queue_click::TableSong::IndexedLocal(row)
                    } else {
                        super::queue_click::TableSong::OfflineDownload(row)
                    };
                    if let Some(state) = self
                        .toggle_active_song_row(
                            &token,
                            song,
                            attempt,
                            lease,
                            &offline.guard,
                            preserve.as_ref(),
                        )
                        .await?
                    {
                        self.session.check_generation(&token)?;
                        return project(&state, &token);
                    }
                }
                let state = if let Some(snapshot) = preserve {
                    let lease = lease.ok_or(Error::StaleResponse)?;
                    self.focus.check(lease, Owner::Audio)?;
                    let resource = offline
                        .resources
                        .into_iter()
                        .next()
                        .ok_or(Error::QueueChanged)?;
                    let policy = if resource["local_id"].is_string() {
                        RetainedRow::Local
                    } else {
                        RetainedRow::OfflineDownload
                    };
                    self.play_retained_queue_row_with_source(
                        &token,
                        snapshot,
                        (resource, offline.source),
                        lease,
                        policy,
                        Some(offline.guard),
                    )
                    .await?
                } else {
                    self.submit_selection_with_source(
                        &token,
                        PlaybackCommand::Replace {
                            resources: offline.resources,
                            index: offline.index,
                            source: offline.source,
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                        Some(offline.guard),
                    )
                    .await?
                };
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::MixedPlaylist {
                id,
                revision,
                start,
                quality,
                double_click,
            } => {
                let mixed = self
                    .mixed_playlist_selection(
                        &token,
                        super::mixed_playlist::MixedRequest {
                            id: &id,
                            revision: &revision,
                            start,
                            quality,
                            selected_only: preserve.is_some(),
                            attempt,
                        },
                    )
                    .await?;
                if double_click && let Some(lease) = lease {
                    let row = &mixed.resources[mixed.index];
                    let song = if row["local_id"].is_string() {
                        super::queue_click::TableSong::IndexedLocal(row)
                    } else {
                        super::queue_click::TableSong::Catalog(
                            row["id"].as_str().ok_or(Error::QueueChanged)?,
                        )
                    };
                    if let Some(state) = self
                        .toggle_active_song_row(
                            &token,
                            song,
                            attempt,
                            lease,
                            &mixed.guard,
                            preserve.as_ref(),
                        )
                        .await?
                    {
                        self.session.check_generation(&token)?;
                        return project(&state, &token);
                    }
                }
                let state = if let Some(snapshot) = preserve {
                    let lease = lease.ok_or(Error::StaleResponse)?;
                    self.focus.check(lease, Owner::Audio)?;
                    let resource = mixed
                        .resources
                        .into_iter()
                        .next()
                        .ok_or(Error::QueueChanged)?;
                    let policy = if resource["local_id"].is_string() {
                        RetainedRow::Local
                    } else {
                        RetainedRow::Catalog
                    };
                    self.play_retained_queue_row_with_source(
                        &token,
                        snapshot,
                        (resource, mixed.source),
                        lease,
                        policy,
                        Some(mixed.guard),
                    )
                    .await?
                } else {
                    self.submit_selection_with_source(
                        &token,
                        PlaybackCommand::Replace {
                            resources: mixed.resources,
                            index: mixed.index,
                            source: mixed.source,
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                        Some(mixed.guard),
                    )
                    .await?
                };
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::MvRelated {
                mv_id,
                song_id,
                quality,
            } => {
                let song_id = identifier(&json!(song_id))?;
                let detail = crate::mv::MvService::new(self.session.clone())?
                    .detail_for(&token, &json!(mv_id))
                    .await?;
                if !detail
                    .related_songs
                    .iter()
                    .any(|s| identifier(&s["id"]).ok().as_deref() == Some(song_id.as_str()))
                {
                    return Err(Error::InvalidInput("song is not related to this MV"));
                }
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(
                    lease.ok_or(Error::StaleResponse)?,
                    super::super::focus::Owner::Audio,
                )?;
                let current = controller.snapshot();
                if current.account == token
                    && current
                        .selected
                        .as_ref()
                        .is_some_and(|s| s.resource["id"].as_str() == Some(&song_id))
                {
                    let value = controller.command(&token, PlaybackCommand::Play).await?;
                    self.session.with_token(&token, |_| Ok(()))?;
                    return project(&value, &token);
                }
                PlaySelection::MvRelated {
                    mv_id: detail.id,
                    song_id,
                    quality,
                }
            }
            PlaySelection::Cloud {
                query,
                start_id,
                revision,
                quality,
            } => {
                let (resources, index) = self
                    .cloud_resources(epoch, query, &revision, start_id, quality)
                    .await?;
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"cloud","revision":revision}),
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::CloudBatch {
                ids,
                revision,
                quality,
                query,
            } => {
                let resources = self
                    .cloud_selected_resources(epoch, ids, &revision, quality, query.clone())
                    .await?;
                let state = self
                    .submit_selection(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index: 0,
                            source: json!({"kind":"cloud","revision":revision,"query":query}),
                        },
                        attempt,
                        lease,
                        prepared.as_ref(),
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            other => other,
        };
        let selection_token = token.clone();
        let clears_playlist = retained.is_none();
        let zone_snapshot = zone_detail
            .as_ref()
            .map(|(_, _, _, snapshot)| snapshot.clone());
        let zone_detail_trial = zone_snapshot.as_ref().map(|snapshot| snapshot.trial_mode);
        let zone_trial = zone_detail_trial.or_else(|| {
            zone_playlist
                .as_ref()
                .is_some_and(|(_, _, _, trial)| *trial)
                .then_some(MGC_TRIAL)
        });
        let (ids, index, quality, source) = self
            .session
            .run_for(&token, |api, uid| async move {
                match input {
                    PlaySelection::AlbumDetail {
                        album,
                        row,
                        quality,
                    } => {
                        let (ids, index, source) = self
                            .album_playback_selection(&selection_token, &album, row, single_album_row)
                            .await?;
                        Ok((ids, index, quality, source))
                    }
                    PlaySelection::Catalog {
                        pages,
                        index,
                        quality,
                    } => {
                        let (ids, index, source) = self
                            .catalog_song_selection(&selection_token, &pages, index)
                            .await?;
                        Ok((ids, index, quality, source))
                    }
                    PlaySelection::StyleRecommendations {
                        revision,
                        index,
                        quality,
                    } => Ok((
                        recommendation_ids.ok_or(Error::StaleResponse)?,
                        index,
                        quality,
                        style_source(&revision),
                    )),
                    PlaySelection::DailyRecommendations { revision, index, quality } => Ok((
                        recommendation_ids.ok_or(Error::StaleResponse)?, index, quality,
                        match daily_origin {
                            Some(origin) => origin.source(&revision, &uid),
                            None => daily_source(&revision),
                        },
                    )),
                    PlaySelection::DailyRecommendationsBatch { revision, quality, .. } => Ok((
                        recommendation_ids.ok_or(Error::StaleResponse)?, 0, quality,
                        daily_source(&revision),
                    )),
                    PlaySelection::Member {
                        query,
                        revision,
                        section,
                        index,
                        quality,
                    } => {
                        self.member_song_selection(
                            epoch,
                            query,
                            &revision,
                            section.as_deref(),
                            index,
                            quality,
                        )
                        .await
                    }
                    PlaySelection::Album {
                        id,
                        start_id,
                        revision,
                        quality,
                    } => {
                        let id = identifier(&json!(id))?;
                        let detail = CatalogApi::new(api).album(&json!(id)).await?;
                        let songs = detail["songs"]
                            .as_array()
                            .ok_or(Error::Response("missing album songs"))?;
                        let (ids, actual) = super::super::rankings::album_order(&id, songs)?;
                        if revision != actual {
                            return Err(Error::QueueChanged);
                        }
                        let index = match start_id {
                            Some(id) => ids
                                .iter()
                                .position(|v| v.as_str() == Some(&id))
                                .ok_or(Error::InvalidInput("song not in album"))?,
                            None => 0,
                        };
                        if ids.is_empty() {
                            return Err(Error::State("album has no songs"));
                        }
                        let source = super::origin::named(
                            json!({"kind":"album","id":id}),
                            super::origin::album_name(&detail["album"], &id),
                        );
                        Ok((ids, index, quality, source))
                    }
                    PlaySelection::MvRelated {
                        mv_id,
                        song_id,
                        quality,
                    } => Ok((
                        vec![json!(song_id)],
                        0,
                        quality,
                        json!({"kind":"mv_related","mvId":mv_id,"scene":"video"}),
                    )),
                    PlaySelection::SongRowDoubleClick { .. }
                    | PlaySelection::FollowLatest { .. }
                    | PlaySelection::ProfileRankPlaylist { .. }
                    | PlaySelection::VerticalZonePlaylist { .. }
                    | PlaySelection::VerticalZonePlaylistDetail { .. }
                    | PlaySelection::VerticalZonePlaylistBatch { .. }
                    | PlaySelection::Cloud { .. }
                    | PlaySelection::CloudBatch { .. }
                    | PlaySelection::Downloads { .. }
                    | PlaySelection::History { .. }
                    | PlaySelection::ListenedBookChapter { .. }
                    | PlaySelection::HistoryRadioChapter { .. }
                    | PlaySelection::Local { .. }
                    | PlaySelection::MixedPlaylist { .. }
                    | PlaySelection::SortedPlaylist { .. }
                    | PlaySelection::OfflinePlaylist { .. }
                    | PlaySelection::LocalBatch { .. }
                    | PlaySelection::LocalBatchAdd { .. }
                    | PlaySelection::Podcast { .. }
                    | PlaySelection::PodcastRecent { .. }
                    | PlaySelection::PodcastSubscriptionRecommendation { .. }
                    | PlaySelection::Voice { .. }
                    | PlaySelection::SearchArtistCard { .. }
                    | PlaySelection::SearchPodcastCard { .. }
                    | PlaySelection::SearchPodcastRow { .. }
                    | PlaySelection::SearchVoiceMenu { .. }
                    | PlaySelection::PodcastBrowseVoice { .. }
                    | PlaySelection::SearchResultVoice { .. }
                    | PlaySelection::MessageVoice { .. } => {
                        unreachable!("cloud handled before song detail requests")
                    }
                    PlaySelection::MessageTrack { id, quality } => Ok((
                        vec![json!(identifier(&json!(id))?)],
                        0,
                        quality,
                        json!({"kind":"songs","scene":"message"}),
                    )),
                    PlaySelection::SearchHotTrack { id, quality } => Ok((
                        vec![json!(identifier(&json!(id))?)],
                        0,
                        quality,
                        // HotExplainCard provides the search scene, but no
                        // search-row trialMode or playlist context.
                        json!({"kind":"songs","scene":"search"}),
                    )),
                    PlaySelection::SearchResultTrack { id, quality, .. } => Ok((
                        vec![json!(identifier(&json!(id))?)],
                        0,
                        quality,
                        super::next_play::search_track_source(),
                    )),
                    PlaySelection::SearchResults {
                        ids,
                        index,
                        quality,
                        context,
                    } => {
                        if ids.is_empty() || ids.len() > 500 || index >= ids.len() {
                            return Err(Error::InvalidInput("invalid search result selection"));
                        }
                        let ids = ids
                            .iter()
                            .map(|id| identifier(&json!(id)).map(Value::from))
                            .collect::<Result<Vec<_>>>()?;
                        // Recovered M/91 search trial and PC result action contexts.
                        let source = match context {
                            SearchResultPlayback::SongTabAll =>
                                json!({"kind":"songs","scene":"search","pldScene":"searchSongTab","trial_mode":42}),
                            SearchResultPlayback::ComprehensiveAll =>
                                json!({"kind":"songs","scene":"search"}),
                        };
                        Ok((ids, index, quality, source))
                    }
                    PlaySelection::SearchChart { ids, quality } => {
                        if ids.is_empty() || ids.len() > crate::catalog::search::CHART_ITEM_LIMIT {
                            return Err(Error::InvalidInput("invalid search chart selection"));
                        }
                        let ids = ids
                            .iter()
                            .map(|id| identifier(&json!(id)).map(Value::from))
                            .collect::<Result<Vec<_>>>()?;
                        Ok((
                            ids,
                            0,
                            quality,
                            json!({"kind":"songs","scene":"searchBox","trial_mode":42}),
                        ))
                    }
                    PlaySelection::Songs {
                        ids,
                        index,
                        quality,
                    } => {
                        if ids.is_empty() || ids.len() > 500 || index >= ids.len() {
                            return Err(Error::InvalidInput("invalid song selection"));
                        }
                        let ids = ids
                            .iter()
                            .map(|id| identifier(&json!(id)).map(Value::from))
                            .collect::<Result<Vec<_>>>()?;
                        Ok((ids, index, quality, json!({"kind":"songs"})))
                    }
                    PlaySelection::PlaylistBatch {
                        id,
                        ids,
                        revision,
                        quality,
                    } => {
                        let id = identifier(&json!(id))?;
                        let detail = PlaylistApi::new(api).detail(&json!(id)).await?;
                        let ids =
                            super::playlist_batch::selected_ids(&id, &detail, &ids, &revision)?;
                        let mut source = super::origin::named(
                            json!({"kind":"playlist","id":id}),
                            super::origin::resource_name(&detail, &id),
                        );
                        if let Some(trial) = self.ordinary_playlist_trial(&selection_token, &id, &detail)? {
                            source["trial_mode"] = json!(trial);
                        }
                        Ok((ids, 0, quality, source))
                    }
                    PlaySelection::Playlist {
                        id,
                        start_id,
                        revision,
                        quality,
                    } => {
                        let id = identifier(&json!(id))?;
                        let fetched;
                        let detail = if let Some(snapshot) = &zone_snapshot {
                            &snapshot.detail
                        } else {
                            fetched = PlaylistApi::new(api).detail(&json!(id)).await?;
                            &fetched
                        };
                        let tracks = detail["trackIds"]
                            .as_array()
                            .ok_or(Error::Response("missing playlist tracks"))?;
                        let actual =
                            hex::encode(Sha256::digest(serde_json::to_vec(&json!([id, tracks]))?));
                        if revision.is_some_and(|r| r != actual) {
                            return Err(Error::QueueChanged);
                        }
                        if tracks.len() > 50000 {
                            return Err(Error::InvalidInput("playlist too large"));
                        }
                        let ids = tracks
                            .iter()
                            .filter(|v| v["v"].as_i64() != Some(-1))
                            .map(|v| identifier(&v["id"]).map(Value::from))
                            .collect::<Result<Vec<_>>>()?;
                        let index = match start_id {
                            Some(id) => ids
                                .iter()
                                .position(|v| v.as_str() == Some(&id))
                                .ok_or(Error::InvalidInput("song not in playlist"))?,
                            None => 0,
                        };
                        if ids.is_empty() {
                            return Err(Error::State("playlist has no online tracks"));
                        }
                        let history = json!({"id":id,"name":detail["name"],
                            "coverImgUrl":detail["coverImgUrl"],"creator":detail["creator"],
                            "description":detail["description"],"trackCount":detail["trackCount"]});
                        let mut source = super::origin::named(
                            json!({"kind":"playlist","id":id,"history_resource":history}),
                            super::origin::resource_name(detail, &id),
                        );
                        let trial = match zone_trial {
                            Some(trial) => Some(trial),
                            None => self.ordinary_playlist_trial(&selection_token, &id, detail)?,
                        };
                        if let Some(trial) = trial {
                            source["trial_mode"] = json!(trial);
                        }
                        if clears_playlist {
                            self.attach_queue_more_playlist(
                                &selection_token, &uid, &id, detail, &mut source,
                            )?;
                        }
                        Ok((ids, index, quality, source))
                    }
                }
            })
            .await?;
        let (ids, index) = if retained.is_some() && !retained_album {
            let id = ids
                .get(index)
                .ok_or(Error::State("selected song is unavailable"))?;
            (vec![id.clone()], 0)
        } else {
            (ids, index)
        };
        if album_double_click || daily_row.is_some() {
            let id = ids
                .get(index)
                .and_then(Value::as_str)
                .ok_or(Error::QueueChanged)?;
            let guard = source_guard.as_ref().ok_or(Error::StaleResponse)?;
            // The album or Daily row was verified against its source snapshot.
            // Toggling the already-active entry does not resolve another song.
            if let Some(state) = self
                .toggle_active_song_row(
                    &token,
                    super::queue_click::TableSong::Catalog(id),
                    attempt,
                    lease.ok_or(Error::StaleResponse)?,
                    guard,
                    preserve.as_ref(),
                )
                .await?
            {
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
        }
        let mut resources = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(100) {
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            let songs = self
                .session
                .run_for(&token, |api, _| async {
                    CatalogApi::new(api)
                        .songs_with_trial(chunk, 100, zone_detail_trial)
                        .await
                })
                .await?;
            for (id, song) in chunk.iter().zip(songs.songs) {
                let song = song.as_ref().map(mapping::song_metadata).transpose()?;
                let mut resource = json!({"id":id,"level":quality.as_str(),"dt":song.as_ref().map(|s|&s["dt"]),"missing":song.is_none(),"song":song});
                if let Some(trial) = source.get("trial_mode") {
                    resource["trial_mode"] = trial.clone();
                }
                resources.push(resource);
            }
        }
        if let Some((view, card_key, id, _)) = zone_playlist {
            self.vertical_zone_playlist_trial(&token, &view, &card_key, &id)
                .await?;
        }
        if let Some((receipt, id, revision, previous)) = zone_detail {
            let current = self
                .vertical_zone_playlist_snapshot(&token, &receipt, &id, &revision)
                .await?;
            if !std::sync::Arc::ptr_eq(&previous, &current) {
                return Err(Error::StaleResponse);
            }
        }
        if resources.get(index).is_none_or(|v| v["missing"] == true) {
            return Err(Error::State("selected song is unavailable"));
        }
        let state = if let (Some(snapshot), Some(retained)) = (preserve, retained) {
            let lease = lease.ok_or(Error::StaleResponse)?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            self.focus.check(lease, Owner::Audio)?;
            if retained_album {
                self.play_retained_album_rows(
                    &token,
                    snapshot,
                    super::queue_click::AlbumTableRows {
                        resources,
                        index,
                        source,
                        double_click: album_double_click,
                    },
                    lease,
                    source_guard.ok_or(Error::StaleResponse)?,
                )
                .await?
            } else {
                let resource = resources
                    .pop()
                    .ok_or(Error::State("selected song is unavailable"))?;
                self.play_retained_queue_row_with_source(
                    &token,
                    snapshot,
                    (resource, source),
                    lease,
                    retained,
                    source_guard,
                )
                .await?
            }
        } else {
            self.submit_selection_with_source(
                &token,
                PlaybackCommand::Replace {
                    resources,
                    index,
                    source,
                },
                attempt,
                lease,
                prepared.as_ref(),
                source_guard,
            )
            .await?
        };
        self.session.with_token(&token, |_| Ok(()))?;
        project(&state, &token)
    }
}
