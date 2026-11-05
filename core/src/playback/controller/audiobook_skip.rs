//! Bind skip intervals to one actually authorized, online full episode.
use super::{Message, PlaybackController, PlaybackStatus, state::Actor};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{QueueEntry, Timeline, TimelineMode, source::ResolvedSource},
    podcast::{AudiobookIntervals, AudiobookSkip, PodcastApi, is_audiobook},
    protocol::api::identifier,
};
use std::{sync::Arc, time::Duration};

pub(super) fn radio_id(item: &QueueEntry) -> Option<String> {
    let radio = &item.resource["voice"]["audio"];
    if item.resource["voice_id"].is_null()
        || !item.resource["path"].is_null()
        || !item.resource["local_id"].is_null()
        || !is_audiobook(radio)
    {
        return None;
    }
    identifier(&radio["id"]).ok()
}

pub(super) async fn prepare(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    item: &QueueEntry,
    source: &ResolvedSource,
    timeline: Timeline,
) -> Option<AudiobookIntervals> {
    if timeline.snapshot().mode != TimelineMode::Full || source.info["kind"] != "online" {
        return None;
    }
    let radio = radio_id(item)?;
    let episode = identifier(&item.resource["voice_id"]).ok()?;
    let duration = timeline.display_end()?;
    let api = session.native_api(token).ok()?;
    let store = session.persistent_store().ok()?;
    let preference = session
        .with_verified_profile(token, &api, |uid, _| {
            AudiobookSkip::read(&store, uid, &radio)
        })
        .ok()?;
    if preference == AudiobookSkip::Close {
        return None;
    }
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        preference.resolve(&PodcastApi::new(api.clone()), &episode, duration),
    )
    .await
    .ok()?
    .ok()?;
    let current = session
        .with_verified_profile(token, &api, |uid, _| {
            AudiobookSkip::read(&store, uid, &radio)
        })
        .ok()?;
    (current == preference && result.fits(duration)).then_some(result)
}

pub(super) struct Refreshed {
    generation: u64,
    sequence: u64,
    token: SessionToken,
    radio: String,
    intervals: Option<AudiobookIntervals>,
}

impl PlaybackController {
    pub fn refresh_audiobook_skip(&self, token: &SessionToken, radio_id: &str) -> Result<()> {
        self.sender
            .send(Message::AudiobookSkipChanged {
                token: token.clone(),
                radio: radio_id.into(),
            })
            .map_err(|_| Error::Cancelled)
    }
}

impl Actor {
    pub(super) fn refresh_audiobook_skip(&mut self, token: SessionToken, radio: String) {
        if token != self.token || self.session.check_generation(&token).is_err() {
            return;
        }
        let loaded = self
            .loaded
            .as_mut()
            .filter(|entry| radio_id(&entry.item).as_deref() == Some(radio.as_str()));
        if loaded.is_none()
            && self
                .pending
                .as_ref()
                .is_none_or(|pending| radio_id(&pending.item).as_deref() != Some(radio.as_str()))
        {
            return;
        }
        self.audiobook_skip_sequence = self.audiobook_skip_sequence.wrapping_add(1);
        if let Some(task) = self.audiobook_skip_task.take() {
            task.abort();
        }
        let Some(loaded) = loaded else { return };
        loaded.audiobook_skip = None;
        let (item, source, timeline) =
            (loaded.item.clone(), loaded.source.clone(), loaded.timeline);
        let session = self.session.clone();
        let sender = self.sender.clone();
        let generation = self.generation();
        let sequence = self.audiobook_skip_sequence;
        self.audiobook_skip_task = Some(tokio::spawn(async move {
            let intervals = prepare(&session, &token, &item, &source, timeline).await;
            let _ = sender.send(Message::AudiobookSkipRefreshed(Refreshed {
                generation,
                sequence,
                token,
                radio,
                intervals,
            }));
        }));
    }

    pub(super) fn refreshed_audiobook_skip(&mut self, value: Refreshed) {
        if value.generation != self.generation()
            || value.sequence != self.audiobook_skip_sequence
            || self.token != value.token
            || self.session.check_generation(&value.token).is_err()
        {
            return;
        }
        self.audiobook_skip_task = None;
        if !matches!(
            self.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        ) {
            return;
        }
        if let Some(loaded) = &mut self.loaded
            && radio_id(&loaded.item).as_deref() == Some(value.radio.as_str())
        {
            loaded.audiobook_skip = value.intervals;
        }
    }
}
