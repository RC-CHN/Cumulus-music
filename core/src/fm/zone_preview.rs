//! A passive first batch never owns the playback actor or a persisted FM slot.
use super::{FmService, FmSource, FmTrack};
use crate::{Error, Result, account::SessionToken};
use std::{
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Mutex,
};

const MAX_RULES: usize = 32;
const MAX_BATCH_BYTES: usize = 64 * 1024;
const RULE_LANES: usize = 16;

#[derive(Default)]
pub(super) struct ZonePreviewState {
    slots: Mutex<Slots>,
    // Same-rule mount and explicit preload cannot race two first requests.
    // Fixed lanes bound concurrency without retaining arbitrary rule locks.
    gates: [tokio::sync::Mutex<()>; RULE_LANES],
}

#[derive(Default)]
struct Slots {
    owner: Option<SessionToken>,
    batches: VecDeque<(String, Vec<FmTrack>)>,
}

impl Slots {
    fn bind(&mut self, token: &SessionToken) {
        if self.owner.as_ref() != Some(token) {
            self.owner = Some(token.clone());
            self.batches.clear();
        }
    }
}

impl FmService {
    pub(crate) async fn zone_rule_gate(
        &self,
        token: &SessionToken,
        rule: &str,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        let mut hash = DefaultHasher::new();
        token.generation.hash(&mut hash);
        rule.hash(&mut hash);
        self.zone_preview.gates[hash.finish() as usize % RULE_LANES]
            .lock()
            .await
    }

    pub(super) fn zone_preview_batch_for(
        &self,
        token: &SessionToken,
        rule: &str,
    ) -> Result<Option<Vec<FmTrack>>> {
        self.session.with_token(token, |_| {
            let mut slots = self
                .zone_preview
                .slots
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            slots.bind(token);
            let Some(index) = slots.batches.iter().position(|(id, _)| id == rule) else {
                return Ok(None);
            };
            let batch = slots.batches.remove(index).expect("found preview index");
            let tracks = batch.1.clone();
            slots.batches.push_back(batch);
            Ok(Some(tracks))
        })
    }

    pub(crate) fn cached_zone_first(
        &self,
        token: &SessionToken,
        rule: &str,
        preferred_trial: Option<i32>,
    ) -> Result<Option<FmTrack>> {
        // Existing Rust playback slots are intentionally still keyed by the
        // full FmSource. For display, the official selector asks per rule.
        let choices = [preferred_trial, None, Some(121), Some(-1)];
        for (index, &trial_mode) in choices.iter().enumerate() {
            if choices[..index].contains(&trial_mode) {
                continue;
            }
            let source = FmSource::Zone {
                rule_id: rule.into(),
                trial_mode,
            };
            if let Some(first) = self
                .queue_for(token, &source)?
                .and_then(|queue| queue.tracks.into_iter().next())
            {
                return Ok(Some(first));
            }
        }
        Ok(self
            .zone_preview_batch_for(token, rule)?
            .and_then(|tracks| tracks.into_iter().next()))
    }

    /// Called only within the native card/view/account publication fence,
    /// while the caller holds the same rule gate used by explicit preload.
    pub(crate) fn remember_zone_preview(
        &self,
        token: &SessionToken,
        rule: String,
        tracks: Vec<FmTrack>,
    ) -> Result<()> {
        if tracks.is_empty() {
            return Ok(());
        }
        if tracks.len() > 3 || serde_json::to_vec(&tracks)?.len() > MAX_BATCH_BYTES {
            return Err(Error::Response("oversized zone FM preview batch"));
        }
        let mut slots = self
            .zone_preview
            .slots
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        slots.bind(token);
        slots.batches.retain(|(id, _)| *id != rule);
        slots.batches.push_back((rule, tracks));
        if slots.batches.len() > MAX_RULES {
            slots.batches.pop_front();
        }
        Ok(())
    }
}
