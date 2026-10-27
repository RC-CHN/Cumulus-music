use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
pub(super) const QUEUES: &str = "fm-queues-v1";
pub(super) const BLACKLIST: &str = "fm-blacklist-v1";
pub(super) const TRASH_CACHE: &str = "fm-trash-snapshot-v1";
pub trait FmClock: Send + Sync {
    fn now_ms(&self) -> Result<u64>;
}
pub struct SystemFmClock;
impl FmClock for SystemFmClock {
    fn now_ms(&self) -> Result<u64> {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::State("clock precedes epoch"))?
                .as_millis(),
        )
        .map_err(|_| Error::State("FM clock overflow"))
    }
}
pub struct FmService {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
    pub(super) clock: Arc<dyn FmClock>,
}
impl FmService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_clock(session, Arc::new(SystemFmClock))
    }
    pub fn with_clock(session: Arc<AccountSession>, clock: Arc<dyn FmClock>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            clock,
        })
    }
    pub(super) fn key(source: &FmSource) -> Result<String> {
        source.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(source)?)))
    }
    pub(super) fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |uid| Ok(uid.into()))
    }
    pub(super) fn gate(&self, token: &SessionToken, source: &FmSource) -> Result<OperationLock> {
        OperationLock::acquire(
            self.store.directory(),
            QUEUES,
            &format!("{}:{}", self.owner(token)?, Self::key(source)?),
        )
    }
    pub fn queue_for(&self, token: &SessionToken, source: &FmSource) -> Result<Option<FmQueue>> {
        let key = Self::key(source)?;
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, QUEUES, &key)?
                .map(|v| {
                    let state: FmQueue = serde_json::from_value(v)?;
                    state.validate()?;
                    if state.source != *source {
                        return Err(Error::State("FM cache source mismatch"));
                    }
                    Ok(state)
                })
                .transpose()
        })
    }
    pub(super) fn required(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
    ) -> Result<FmQueue> {
        let queue = self
            .queue_for(token, source)?
            .ok_or(Error::State("FM has not been loaded"))?;
        if queue.revision != revision {
            return Err(Error::QueueChanged);
        }
        Ok(queue)
    }
    fn save(
        &self,
        token: &SessionToken,
        expected: u64,
        queue: &FmQueue,
        skip: Option<&SkipReport>,
    ) -> Result<()> {
        queue.validate()?;
        let key = Self::key(&queue.source)?;
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                let revision = tx
                    .get(uid, QUEUES, &key)?
                    .map(|v| serde_json::from_value::<FmQueue>(v).map(|v| v.revision))
                    .transpose()?
                    .unwrap_or(0);
                if revision != expected {
                    return Err(Error::QueueChanged);
                }
                tx.put(uid, QUEUES, &key, &serde_json::to_value(queue)?)?;
                if let Some(skip) = skip {
                    tx.put(uid, feedback::SKIPS, &skip.id, &serde_json::to_value(skip)?)?;
                }
                Ok(())
            })
        })
    }
    pub async fn refresh_blacklist_for(&self, token: &SessionToken) -> Result<FmBlacklist> {
        let _gate = self.artist_gate(token)?;
        let value = self
            .session
            .run_for(token, |api, _| async { FmApi::new(api).blacklist().await })
            .await?;
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, BLACKLIST, "current", &serde_json::to_value(&value)?)
        })?;
        Ok(value)
    }
    pub fn blacklist_for(&self, token: &SessionToken) -> Result<FmBlacklist> {
        self.session.with_token(token, |uid| {
            let mut value: FmBlacklist = self
                .store
                .get(uid, BLACKLIST, "current")?
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default();
            if let Some(v) = self.store.get(uid, TRASH_CACHE, "current")? {
                let trash: FmTrashSnapshot = serde_json::from_value(v)?;
                value.songs.extend(trash.ids);
            }
            Ok(value)
        })
    }
    async fn fill(&self, token: &SessionToken, queue: &mut FmQueue, preload: bool) -> Result<()> {
        let tracks = self
            .session
            .run_for(token, |api, _| async {
                let api = FmApi::new(api);
                let mut first = api.batch(&queue.source, preload).await?;
                if first.len() < 3 && !matches!(queue.source, FmSource::Zone { .. }) {
                    first.extend(api.batch(&queue.source, preload).await?);
                }
                Ok(first)
            })
            .await?;
        queue.append(tracks);
        queue.fetched_at = self.clock.now_ms()?;
        Ok(())
    }
    /// Native caller supplies the current server-derived membership identity.
    /// A fresh ordinary preload preserves cursor. Expired and alternate-mode
    /// preloads append to existing history, as M1384.loadMore does.
    pub async fn preload_for(
        &self,
        token: &SessionToken,
        source: FmSource,
        rights: FmRights,
    ) -> Result<FmQueue> {
        let _gate = self.gate(token, &source)?;
        let old = self.queue_for(token, &source)?;
        let now = self.clock.now_ms()?;
        if let Some(old) = &old
            && old.fresh_at(rights, now)
            && old.current().is_some()
        {
            return Ok(old.clone());
        }
        let revision = old.as_ref().map(|q| q.revision).unwrap_or(0);
        let mut queue = old.unwrap_or_else(|| FmQueue::empty(source, rights));
        queue.rights = rights;
        self.fill(token, &mut queue, true).await?;
        let blacklist = self.blacklist_for(token)?;
        queue.index = queue
            .find(queue.index, false, &blacklist)
            .unwrap_or(queue.tracks.len());
        queue.bump()?;
        self.save(token, revision, &queue, None)?;
        Ok(queue)
    }
    pub fn repeat_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
        repeat: FmRepeat,
    ) -> Result<FmQueue> {
        let _gate = self.gate(token, source)?;
        let mut queue = self.required(token, source, revision)?;
        if queue.repeat == repeat {
            return Ok(queue);
        }
        queue.repeat = repeat;
        queue.bump()?;
        self.save(token, revision, &queue, None)?;
        Ok(queue)
    }
    /// Official switchTrack(CUR) also filters a newly refreshed blacklist.
    pub async fn playable_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
    ) -> Result<FmQueue> {
        let _gate = self.gate(token, source)?;
        let mut queue = self.required(token, source, revision)?;
        let blacklist = self.blacklist_for(token)?;
        if queue.current().is_some_and(|t| !blacklist.blocks(t)) {
            return Ok(queue);
        }
        let mut selected = queue.find(queue.index, false, &blacklist);
        if selected.is_none() {
            self.fill(token, &mut queue, false).await?;
            selected = queue.find(queue.index, false, &blacklist);
        }
        queue.select(selected.unwrap_or(queue.tracks.len()));
        queue.bump()?;
        self.save(token, revision, &queue, None)?;
        Ok(queue)
    }
    /// Returns a committed queue intent and an optional *prepared*, unsent skip
    /// report. The native coordinator dispatches it once, independently of audio.
    pub async fn step_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
        step: FmStep,
        observation: Option<&FmObservation>,
    ) -> Result<FmTransition> {
        let _gate = self.gate(token, source)?;
        let mut queue = self.required(token, source, revision)?;
        let now = self.clock.now_ms()?;
        if queue
            .last_step_at
            .is_some_and(|t| now >= t && now - t < 250)
        {
            return Err(Error::State("FM step cooldown"));
        }
        let previous = queue.current().cloned();
        let report = if step == FmStep::ManualNext {
            previous
                .as_ref()
                .map(|track| {
                    Ok::<_, Error>(SkipReport {
                        id: uuid::Uuid::new_v4().simple().to_string(),
                        song_id: track.id.clone(),
                        algorithm: track.algorithm.clone(),
                        seconds: observation
                            .map(|o| o.seconds_for(track))
                            .transpose()?
                            .unwrap_or(0),
                        created_at: now,
                        phase: SkipPhase::Prepared,
                    })
                })
                .transpose()?
        } else {
            None
        };
        let mut refill_failed = false;
        let repeat = step == FmStep::Ended && queue.repeat == FmRepeat::One && previous.is_some();
        let blacklist = self.blacklist_for(token)?;
        let selected = if repeat {
            Some(queue.index)
        } else if step == FmStep::Previous {
            if queue.index == 0 {
                return Err(Error::State("FM has no previous track"));
            }
            queue.find(queue.index - 1, true, &blacklist)
        } else {
            if queue.needs_refill()
                && let Err(error) = self.fill(token, &mut queue, false).await
            {
                if error.is_service_code(301)
                    || matches!(error, Error::StaleResponse | Error::CredentialChanged)
                {
                    return Err(error);
                }
                refill_failed = true;
            }
            queue.find(
                queue.index + usize::from(previous.is_some()),
                false,
                &blacklist,
            )
        };
        if let Some(index) = selected {
            queue.select(index);
        } else if step != FmStep::Previous {
            queue.select(queue.tracks.len());
        }
        queue.last_step_at = Some(now);
        queue.bump()?;
        self.save(token, revision, &queue, report.as_ref())?;
        Ok(FmTransition {
            queue,
            skip_report: report.map(|s| s.id),
            play: selected.is_some(),
            exhausted: selected.is_none(),
            refill_failed,
        })
    }
    /// Local part of acknowledged "reduce recommendations". Never removes a
    /// current track while the remote mutation is still uncertain.
    pub async fn apply_trash_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
        operation: &str,
    ) -> Result<FmTransition> {
        let record = self
            .trash_operation_for(token, operation)?
            .ok_or(Error::State("FM trash operation missing"))?;
        if !record.desired
            || !matches!(
                record.phase,
                crate::account::resources::MutationPhase::Confirmed
                    | crate::account::resources::MutationPhase::Unchanged
            )
        {
            return Err(Error::MutationUncertain);
        }
        let _gate = self.gate(token, source)?;
        let mut queue = self.required(token, source, revision)?;
        let current = queue.current().cloned();
        let mut refill_failed = false;
        let removed_current = current
            .as_ref()
            .is_some_and(|t| record.song_ids.contains(&t.id));
        if removed_current
            && queue.needs_refill()
            && let Err(error) = self.fill(token, &mut queue, false).await
        {
            if error.is_service_code(301)
                || matches!(error, Error::StaleResponse | Error::CredentialChanged)
            {
                return Err(error);
            }
            refill_failed = true;
        }
        let old_index = queue.index;
        let removed_before = queue
            .tracks
            .iter()
            .take(old_index)
            .filter(|t| record.song_ids.contains(&t.id))
            .count();
        queue.tracks.retain(|t| !record.song_ids.contains(&t.id));
        let start = old_index
            .saturating_sub(removed_before)
            .min(queue.tracks.len());
        let selected = if removed_current {
            queue.find(start, false, &self.blacklist_for(token)?)
        } else {
            current.and_then(|c| queue.tracks.iter().position(|t| t.entry_id == c.entry_id))
        };
        queue.select(selected.unwrap_or(queue.tracks.len()));
        queue.bump()?;
        self.save(token, revision, &queue, None)?;
        Ok(FmTransition {
            queue,
            skip_report: None,
            play: removed_current && selected.is_some(),
            exhausted: selected.is_none(),
            refill_failed,
        })
    }
    pub(super) fn persist_original(
        &self,
        owner: &str,
        kind: &str,
        key: &str,
        value: Value,
    ) -> Result<()> {
        self.store.put(owner, kind, key, &value)
    }
}
