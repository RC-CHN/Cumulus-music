//! Search AudioCard overlays: M3095 -> M537 -> M3085/M404.
use super::{
    MusicClient, PlaybackCommand,
    podcast_recommended::{
        RadioPlaybackPlan, RadioPlaybackStart, RadioPlaybackTarget, same_radio_transport,
    },
};
use crate::{
    Error, Result, account::SessionToken, client::search::podcast_playback::PodcastPlaybackLane,
    playback::engine::Cancellation,
};
use serde_json::json;

impl MusicClient {
    pub(super) async fn search_podcast_selection(
        &self,
        token: &SessionToken,
        receipt: &str,
        id: &str,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let api = self.session.native_api(token)?;
        let (radio, observed) = self.search_podcast_playback_source(
            token,
            &api,
            PodcastPlaybackLane::Card,
            receipt,
            id,
        )?;
        let room = self.together_detached_guard(token);
        let pending = self.auth.pending_guard();
        let guard = Cancellation::new(move || {
            !observed.is_cancelled() && !room.is_cancelled() && pending().is_ok()
        });
        guard.check()?;
        let controller = self.player.controller().await?;
        if let Some(command) = same_radio_transport(&controller.snapshot(), token, id) {
            return Ok((command, guard));
        }
        let RadioPlaybackPlan { resources, start } = self
            .prepare_radio_playback(
                token,
                &api,
                &radio,
                &guard,
                RadioPlaybackStart::Overlay {
                    select_checkpoint: true,
                },
            )
            .await?;
        let RadioPlaybackTarget {
            index, position, ..
        } = start.ok_or(Error::QueueChanged)?;
        guard.check()?;
        // Authorize the chosen episode before replacing the old queue. Keep
        // the fresh source native and pass it only to the exact fenced load.
        let first_source = controller
            .source_resolver()
            .prepare_scoped_voice(&resources[index], token, &api, &guard)
            .await?;
        guard.check()?;
        let sequential = !matches!(radio.raw["categoryId"].as_i64(), Some(3001 | 10002 | 2001));
        Ok((
            PlaybackCommand::ReplacePodcastCollection {
                resources,
                index,
                position,
                sequential,
                first_source: Some(first_source),
                source: json!({"kind":"search_podcast_card","id":radio.id,"scene":"unknown"}),
            },
            guard,
        ))
    }
}

pub(super) enum SearchPodcastRowPlan {
    Play(PlaybackCommand),
    QueueOnly(crate::playback::controller::QueueUpdate),
}

impl MusicClient {
    pub(super) async fn search_podcast_row_selection(
        &self,
        token: &SessionToken,
        receipt: &str,
        id: &str,
        gesture: super::SearchPodcastRowGesture,
        prepared: &super::prepared_selection::PreparedSelection,
    ) -> Result<(SearchPodcastRowPlan, Cancellation)> {
        use super::SearchPodcastRowGesture;
        use crate::playback::{
            PlaybackMode,
            controller::{PlaybackStatus, QueueTransport, QueueUpdate},
        };

        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let api = self.session.native_api(token)?;
        let (radio, observed) = self.search_podcast_playback_source(
            token,
            &api,
            PodcastPlaybackLane::Row,
            receipt,
            id,
        )?;
        let room = self.together_detached_guard(token);
        let pending = self.auth.pending_guard();
        let guard = Cancellation::new(move || {
            !observed.is_cancelled() && !room.is_cancelled() && pending().is_ok()
        });
        guard.check()?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(token).await?;
        prepared.check(self, &before)?;
        if before.fm || queue.intelligence.is_some() {
            return Err(Error::State("search podcast requires an ordinary queue"));
        }
        if gesture == SearchPodcastRowGesture::Index
            && let Some(command) = same_radio_transport(&before, token, id)
        {
            return Ok((SearchPodcastRowPlan::Play(command), guard));
        }
        // M622's menu label 播放 is PlayAll; it is not the index's Play.
        let replace = gesture == SearchPodcastRowGesture::Menu
            || self.replace_queue_on_double_click(token)?;
        let mode =
            if replace && !matches!(radio.raw["categoryId"].as_i64(), Some(3001 | 10002 | 2001)) {
                PlaybackMode::Sequential
            } else {
                queue.mode
            };
        let preparing = {
            let prepared = self.pending_preparation_guard(prepared, controller.clone());
            let authority = guard.clone();
            Cancellation::new(move || !authority.is_cancelled() && !prepared.is_cancelled())
        };
        let RadioPlaybackPlan { resources, start } = self
            .prepare_radio_playback(
                token,
                &api,
                &radio,
                &preparing,
                RadioPlaybackStart::Table { mode },
            )
            .await?;
        preparing.check()?;
        if start.is_none()
            && replace
            && matches!(
                before.state,
                PlaybackStatus::Preparing
                    | PlaybackStatus::Loading
                    | PlaybackStatus::Playing
                    | PlaybackStatus::Paused
                    | PlaybackStatus::Buffering
            )
        {
            // The official queue can keep current audio outside its visible list.
            // The ordinary Rust actor cannot clear that entry without stopping it.
            return Err(Error::State(
                "podcast checkpoint is not in the playback list",
            ));
        }
        let first_source = if let Some(start) = &start {
            Some(
                controller
                    .source_resolver()
                    .prepare_scoped_episode(
                        &resources[start.index],
                        token,
                        &api,
                        &preparing,
                        start.detail.as_ref(),
                    )
                    .await?,
            )
        } else {
            None
        };
        preparing.check()?;
        let transport = match (start, first_source.as_ref()) {
            (Some(start), Some(first)) => {
                let timeline = first.timeline(None)?;
                let position = if timeline.to_media(start.position).is_ok() {
                    start.position
                } else {
                    timeline.snapshot().start
                };
                Some((
                    resources[start.index]["voice_id"]
                        .as_str()
                        .ok_or(Error::QueueChanged)?
                        .to_owned(),
                    QueueTransport {
                        position: Some(position),
                        paused: false,
                    },
                ))
            }
            _ => None,
        };
        let source =
            json!({"kind":"search_podcast_row","id":radio.id,"scene":"unknown","pldScene":"audio"});
        let queue = search_podcast_row_queue(
            queue,
            resources,
            source,
            replace,
            mode,
            transport.as_ref().map(|(id, _)| id.as_str()),
        )?;
        let update = QueueUpdate {
            expected: before.fence(),
            queue,
            transport: transport.map(|(_, transport)| transport),
        };
        let plan = match first_source {
            Some(first_source) => SearchPodcastRowPlan::Play(PlaybackCommand::PlayPodcastBrowse {
                update,
                first_source,
            }),
            None => SearchPodcastRowPlan::QueueOnly(update),
        };
        Ok((plan, guard))
    }
}

fn search_podcast_row_queue(
    mut queue: crate::playback::QueueSnapshot,
    resources: Vec<serde_json::Value>,
    source: serde_json::Value,
    replace: bool,
    mode: crate::playback::PlaybackMode,
    selected: Option<&str>,
) -> Result<crate::playback::QueueSnapshot> {
    use super::{
        local_batch::random_order,
        queue_playlist::{QUEUE_LIMIT, insert_collection_bound},
    };
    use crate::playback::{PlaybackMode, QueueSnapshot};
    use std::collections::HashSet;

    let resources: Vec<_> = resources.into_iter().take(QUEUE_LIMIT).collect();
    let ids: HashSet<_> = resources
        .iter()
        .filter_map(|row| row["voice_id"].as_str().map(str::to_owned))
        .collect();
    let first = resources
        .first()
        .and_then(|row| row["voice_id"].as_str())
        .ok_or(Error::QueueChanged)?
        .to_owned();
    let retained = (!replace && selected.is_none())
        .then(|| queue.selected())
        .flatten();
    if replace || selected.is_some() && resources.len() == QUEUE_LIMIT {
        queue = QueueSnapshot {
            mode,
            ..QueueSnapshot::default()
        };
    }
    queue = insert_collection_bound(queue, resources, source.clone(), false, true, true);
    if let Some(selected) = selected {
        let entry = queue
            .items
            .iter()
            .find(|entry| entry.resource["voice_id"] == selected && entry.source == source)
            .ok_or(Error::QueueChanged)?;
        queue.current = Some(entry.entry_id.clone());
    } else {
        queue.current = retained.as_ref().map(|entry| entry.entry_id.clone());
        if let Some(retained) = retained {
            let existing = queue
                .items
                .iter_mut()
                .find(|entry| entry.entry_id == retained.entry_id)
                .ok_or(Error::QueueChanged)?;
            *existing = retained;
        }
    }
    if mode == PlaybackMode::Random {
        let in_batch: HashSet<_> = queue
            .items
            .iter()
            .filter(|entry| {
                entry.resource["voice_id"]
                    .as_str()
                    .is_some_and(|id| ids.contains(id))
            })
            .map(|entry| entry.entry_id.as_str())
            .collect();
        let first = queue
            .items
            .iter()
            .find(|entry| entry.resource["voice_id"] == first)
            .ok_or(Error::QueueChanged)?;
        // This remains official Play (l.play=true), even when the bookmark
        // resolves no current entry; only Add uses setRandomNext.
        queue.order = random_order(&queue, &in_batch, &first.entry_id, false, &mut rand::rng());
    } else {
        queue.order = queue
            .items
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect();
    }
    Ok(queue)
}
