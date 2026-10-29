//! M/163 playback authorization, account-scoped URL cache and shared pending
//! reads. Signed URLs live in memory only. Local resolution uses LocalLibrary.
mod cache;
mod fetch;
mod loudness;
use super::{Timeline, engine::MediaInput};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::{LocalLibrary, LocalResource},
    membership::Quality,
    protocol::api::identifier,
};
use cache::{CacheState, Key, Outcome, Owner, PendingKey};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// No Debug or Serialize: info includes native authorization facts and URLs.
#[derive(Clone)]
pub struct ResolvedSource {
    pub input: MediaInput,
    pub info: Value,
}
impl ResolvedSource {
    pub fn timeline(&self, duration: Option<f64>) -> Result<Timeline> {
        Timeline::from_source(&self.info, duration)
    }
    pub fn actual_quality(&self) -> Option<&str> {
        self.info["resource_info"]["level"].as_str()
    }
}

pub trait ResolverClock: Send + Sync {
    fn now(&self) -> Duration;
}
struct Monotonic(Instant);
impl ResolverClock for Monotonic {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

pub struct SourceResolver {
    session: Arc<AccountSession>,
    library: Option<Arc<LocalLibrary>>,
    state: Arc<Mutex<CacheState>>,
    clock: Arc<dyn ResolverClock>,
}
impl SourceResolver {
    pub(crate) async fn voice_resume_position(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Option<f64>> {
        crate::podcast::progress::position(&self.session, token, resource).await
    }
    pub(crate) fn now(&self) -> Duration {
        self.clock.now()
    }
    pub fn new(session: Arc<AccountSession>, library: Option<Arc<LocalLibrary>>) -> Self {
        Self::with_clock(session, library, Arc::new(Monotonic(Instant::now())))
    }
    pub fn with_clock(
        session: Arc<AccountSession>,
        library: Option<Arc<LocalLibrary>>,
        clock: Arc<dyn ResolverClock>,
    ) -> Self {
        Self {
            session,
            library,
            state: Arc::new(Mutex::new(CacheState::default())),
            clock,
        }
    }
    pub fn revision(&self) -> Result<u64> {
        Ok(self.state.lock().map_err(|_| Error::LockPoisoned)?.revision)
    }
    pub fn invalidate(&self, id: Option<&str>) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or(Error::State("source revision exhausted"))?;
        match id {
            None => state.cache.clear(),
            Some(id) => state.cache.retain(|k, _| k.id != id),
        }
        Ok(())
    }
    fn key(&self, resource: &Value, token: &SessionToken) -> Result<Key> {
        self.session.with_token(token, |_| Ok(()))?;
        let level = Quality::parse(
            resource
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("standard"),
        )?
        .as_str()
        .into();
        let encode = resource
            .get("encode_type")
            .and_then(Value::as_str)
            .unwrap_or("aac");
        if encode.is_empty()
            || encode.len() > 32
            || !encode
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(Error::InvalidInput("invalid playback encoding"));
        }
        let trial = match resource.get("trial_mode") {
            Some(value) => value
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or(Error::InvalidInput("invalid trial mode"))?,
            None => -1,
        };
        Ok(Key {
            token: token.clone(),
            id: identifier(&resource["id"])?,
            voice_id: resource
                .get("voice_id")
                .filter(|v| !v.is_null())
                .map(identifier)
                .transpose()?,
            level,
            encode: encode.into(),
            trial,
        })
    }
    pub fn cached_until(&self, resource: &Value, token: &SessionToken) -> Result<Option<Duration>> {
        let key = self.key(resource, token)?;
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .cache
            .get(&key)
            .map(|entry| entry.deadline))
    }
    fn finish_outcome(
        &self,
        outcome: Outcome,
        token: &SessionToken,
    ) -> Result<Arc<ResolvedSource>> {
        let current = self.session.snapshot()?;
        // Keep a 301 which expired this exact generation, but never deliver
        // that old failure over a subsequently authenticated account.
        if outcome.as_ref().is_err_and(|e| e.is_service_code(301))
            && current.status == crate::account::SessionStatus::Expired
            && token.generation.checked_add(1) == Some(current.token.generation)
            && current.token.uid.is_none()
        {
            return outcome.map_err(Error::Shared);
        }
        self.session.with_token(token, |_| Ok(()))?;
        outcome.map_err(Error::Shared)
    }
    async fn local(&self, resource: Value, token: &SessionToken) -> Result<Arc<ResolvedSource>> {
        if self.session.token()? != *token {
            return Err(Error::StaleResponse);
        }
        // Persisted downloaded resources revalidate the account ledger on every
        // selection, queue advance and restore, including encrypted membership.
        let mut quality = None;
        let mut downloaded_gain = None;
        if let Some(id) = resource.get("download_task_id") {
            let id = id
                .as_str()
                .ok_or(Error::InvalidInput("invalid download task reference"))?;
            let task =
                crate::download::service::DownloadService::new(self.session.clone())?.get(id)?;
            let path = std::path::Path::new(
                resource["path"]
                    .as_str()
                    .ok_or(Error::InvalidInput("invalid downloaded path"))?,
            );
            if task.status != crate::download::service::DownloadStatus::Complete
                || task.target.as_deref() != Some(path)
            {
                return Err(Error::State("download reference changed"));
            }
            let check = crate::download::offline::OfflineLibrary::new(self.session.clone())?
                .check(token, path, false)
                .await?;
            if check.record.task_id != id || !check.access.can_play || !check.integrity_verified {
                return Err(Error::State("download is unavailable for this account"));
            }
            quality = if task.origin.is_cloud() {
                None
            } else {
                Some(check.record.quality.as_str().to_owned())
            };
            downloaded_gain = task
                .resource
                .as_ref()
                .and_then(|value| value.get("gain"))
                .cloned();
        } else if let Some(ledger) = resource.get("download_ledger") {
            self.session.with_token(token, |owner| {
                if resource["download_ledger_owner"].as_str() != Some(owner) {
                    return Err(Error::StaleResponse);
                }
                Ok(())
            })?;
            let record: crate::download::offline::OfflineRecord =
                serde_json::from_value(ledger.clone())?;
            if resource["path"].as_str().map(std::path::Path::new) != Some(record.path.as_path()) {
                return Err(Error::State("detached download path changed"));
            }
            let check = crate::download::offline::OfflineLibrary::new(self.session.clone())?
                .check_detached(token, record)
                .await?;
            if !check.access.can_play || !check.integrity_verified {
                return Err(Error::State("download is unavailable for this account"));
            }
            quality = if resource["cloud_song_id"].as_str() == Some(&check.record.song_id) {
                None
            } else {
                Some(check.record.quality.as_str().to_owned())
            };
        }
        let library = self
            .library
            .clone()
            .ok_or(Error::State("local playback requires library"))?;
        let mut source = tokio::task::spawn_blocking(move || -> Result<ResolvedSource> {
            if resource.get("local_id").is_some() {
                let local: LocalResource = serde_json::from_value(resource)?;
                let item = library.item(&local.local_id)?;
                // A container's gain does not identify the separate CUE tracks.
                let gain = if matches!(
                    local.local_revision,
                    crate::library::LocalRevision::Cue { .. }
                ) {
                    (Value::Null, false)
                } else {
                    loudness::file_gain(&item.path)
                };
                let source = library.resolve(&local)?;
                Ok(ResolvedSource {
                    input: MediaInput::File(source.source),
                    info: loudness::with_gain(
                        json!({"kind":"local","metadata":source.metadata,"segment":source.segment}),
                        gain,
                    ),
                })
            } else {
                let path = resource["path"]
                    .as_str()
                    .ok_or(Error::InvalidInput("invalid local audio path"))?;
                let gain = loudness::file_gain(std::path::Path::new(path));
                let path = library.playable_path(path)?;
                Ok(ResolvedSource {
                    input: MediaInput::File(path),
                    info: loudness::with_gain(json!({"kind":"local"}), gain),
                })
            }
        })
        .await
        .map_err(|_| Error::State("local source worker failed"))??;
        if let Some(quality) = quality {
            source.info["resource_info"] = json!({"level": quality});
        }
        if let Some(gain) = downloaded_gain {
            source.info["volume_delta"] = gain;
            source.info["loudness_invalid"] = json!(false);
        }
        if self.session.token()? != *token {
            return Err(Error::StaleResponse);
        }
        Ok(Arc::new(source))
    }
    pub async fn resolve(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Arc<ResolvedSource>> {
        if resource.get("local_id").is_some() || resource.get("path").is_some() {
            return self.local(resource.clone(), token).await;
        }
        let key = self.key(resource, token)?;
        let (pending, mut receiver, owner) = {
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            let now = self.clock.now();
            state
                .cache
                .retain(|k, v| k.token == *token && v.deadline > now);
            if let Some(cached) = state.cache.get(&key) {
                let value = cached.value.clone();
                drop(state);
                self.session.with_token(token, |_| Ok(()))?;
                return Ok(value);
            }
            let pending = PendingKey {
                revision: state.revision,
                key: key.clone(),
            };
            if let Some(sender) = state.pending.get(&pending) {
                (pending, sender.subscribe(), false)
            } else {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                state.pending.insert(pending.clone(), sender);
                (pending, receiver, true)
            }
        };
        if owner {
            let guard = Owner::new(self.state.clone(), pending.clone());
            let result = self.fetch(resource, &key).await.and_then(|value| {
                self.session.with_token(token, |_| Ok(()))?;
                if self.state.lock().map_err(|_| Error::LockPoisoned)?.revision != pending.revision
                {
                    return Err(Error::StaleResponse);
                }
                Ok(Arc::new(value))
            });
            let outcome: Outcome = result.map_err(Arc::new);
            if let Ok(value) = &outcome {
                let ttl_value = &value.info["resource_info"]["expi"];
                let ttl = ttl_value
                    .as_f64()
                    .or_else(|| ttl_value.as_str().and_then(|s| s.parse().ok()))
                    .filter(|n| n.is_finite() && *n > 5.0);
                let deadline = ttl
                    .and_then(|n| Duration::try_from_secs_f64(n - 5.0).ok())
                    .and_then(|d| self.clock.now().checked_add(d));
                if let Some(deadline) = deadline {
                    let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                    // Invalidation must win even after the fetch consistency
                    // check, before publication into the shared cache.
                    if state.revision != pending.revision {
                        return Err(Error::StaleResponse);
                    }
                    if state.cache.len() >= 128
                        && let Some(oldest) = state
                            .cache
                            .iter()
                            .min_by_key(|(_, v)| v.deadline)
                            .map(|(k, _)| k.clone())
                    {
                        state.cache.remove(&oldest);
                    }
                    state.cache.insert(
                        key,
                        cache::Cached {
                            value: value.clone(),
                            deadline,
                        },
                    );
                }
            }
            let outcome = guard.finish(outcome)?;
            self.finish_outcome(outcome, token)
        } else {
            let outcome = loop {
                if let Some(outcome) = receiver.borrow().clone() {
                    break outcome;
                }
                receiver.changed().await.map_err(|_| Error::Cancelled)?;
            };
            if self.revision()? != pending.revision {
                return Err(Error::StaleResponse);
            }
            self.finish_outcome(outcome, token)
        }
    }
}
