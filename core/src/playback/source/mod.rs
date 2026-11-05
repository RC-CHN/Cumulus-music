//! M/163 playback authorization, account-scoped URL cache and shared pending
//! reads. Signed URLs live in memory only. Local resolution uses LocalLibrary.
mod cache;
mod fetch;
#[cfg(test)]
mod fresh_tests;
mod loudness;
mod streaming;
use super::{Timeline, engine::MediaInput};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::service::{DownloadService, MissingCatalogDownload},
    library::{LocalLibrary, LocalResource},
    membership::Quality,
    protocol::api::{Api, identifier},
};
use cache::{CacheState, Key, Outcome, Owner, PendingKey};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
const AUTHORIZATION_REUSE_MARGIN: Duration = Duration::from_secs(5);

/// No Debug or Serialize: info includes native authorization facts and URLs.
#[derive(Clone)]
pub struct ResolvedSource {
    pub input: MediaInput,
    pub info: Value,
    expires_at: Option<Duration>,
}
impl ResolvedSource {
    pub fn timeline(&self, duration: Option<f64>) -> Result<Timeline> {
        Timeline::from_source(&self.info, duration)
    }
    pub fn actual_quality(&self) -> Option<&str> {
        self.info["resource_info"]["level"].as_str()
    }
    fn reuse_until(&self) -> Option<Duration> {
        self.expires_at
            .map(|deadline| deadline.saturating_sub(AUTHORIZATION_REUSE_MARGIN))
    }
    pub(crate) fn needs_renewal_for_reopen(&self, now: Duration) -> bool {
        self.reuse_until().is_some_and(|deadline| deadline <= now)
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
    pub(crate) streaming_cache: Option<Arc<super::stream_cache::StreamingCache>>,
}
impl SourceResolver {
    pub(crate) async fn confirmed_plain_skip_denial(
        &self,
        token: &SessionToken,
        item: &super::QueueEntry,
    ) -> Result<bool> {
        super::preflight::confirmed_plain_skip_denial(&self.session, token, item).await
    }
    pub(super) fn check_account_generation(&self, token: &SessionToken) -> Result<()> {
        self.session.check_generation(token)
    }

    pub(crate) async fn ordinary_queue_preflight(
        &self,
        token: &SessionToken,
        queue: &super::QueueSnapshot,
        environment: super::preflight::OrdinaryQueueEnvironment,
    ) -> Result<super::preflight::OrdinaryQueuePreflight> {
        super::preflight::inspect(self.session.clone(), token, queue, environment).await
    }
    pub(crate) async fn voice_resume_position(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Option<f64>> {
        crate::podcast::progress::position(&self.session, token, resource).await
    }
    pub(crate) async fn missing_catalog_download(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Option<MissingCatalogDownload>> {
        let Some(task) = resource["download_task_id"].as_str() else {
            return Ok(None);
        };
        if resource.get("local_id").is_some()
            || !resource["cloud_song_id"].is_null()
            || !resource["voice_id"].is_null()
            || !resource["song"]["voice_id"].is_null()
            || resource.get("download_ledger").is_some()
            || !resource["playback_constraint"].is_null()
        {
            return Ok(None);
        }
        let (Some(path), Some(song)) = (resource["path"].as_str(), resource["id"].as_str()) else {
            return Ok(None);
        };
        let path = std::path::PathBuf::from(path);
        let (task, song) = (task.to_owned(), song.to_owned());
        let session = self.session.clone();
        let token = token.clone();
        tokio::task::spawn_blocking(move || {
            DownloadService::new(session)?.missing_catalog_playback(&token, &task, &path, &song)
        })
        .await
        .map_err(|_| Error::State("missing download check worker stopped"))?
    }
    pub(crate) fn now(&self) -> Duration {
        self.clock.now()
    }
    fn authorization_expiry(&self, ttl: &Value) -> Option<Duration> {
        let ttl = ttl
            .as_f64()
            .or_else(|| ttl.as_str()?.parse().ok())
            .filter(|value| value.is_finite() && *value > 0.0)?;
        self.now()
            .checked_add(Duration::try_from_secs_f64(ttl).ok()?)
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
            streaming_cache: None,
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
    /// Prepare only the already published account-owned catalog file. This
    /// does not consult online URLs, import local matches or renew membership.
    pub(crate) async fn resolve_follow_latest_download(
        &self,
        resource: &Value,
        token: &SessionToken,
        song: &str,
    ) -> Result<Arc<ResolvedSource>> {
        self.local(resource.clone(), token, Some(song)).await
    }

    async fn local(
        &self,
        resource: Value,
        token: &SessionToken,
        catalog_song: Option<&str>,
    ) -> Result<Arc<ResolvedSource>> {
        if self.session.token()? != *token {
            return Err(Error::StaleResponse);
        }
        // Persisted downloaded resources revalidate the account ledger on every
        // selection, queue advance and restore, including encrypted membership.
        let mut quality = None;
        let mut downloaded_gain = None;
        let mut download_recheck = None;
        if catalog_song.is_some_and(|song| {
            !resource["download_task_id"].is_string()
                || resource["id"].as_str() != Some(song)
                || resource.get("local_id").is_some()
                || !resource["cloud_song_id"].is_null()
                || !resource["voice_id"].is_null()
        }) {
            return Err(Error::State("catalog download identity changed"));
        }
        if let Some(id) = resource.get("download_task_id") {
            let id = id
                .as_str()
                .ok_or(Error::InvalidInput("invalid download task reference"))?;
            let service = crate::download::service::DownloadService::new(self.session.clone())?;
            let task = service.get(id)?;
            if catalog_song.is_some_and(|song| {
                task.song_id != song
                    || task.origin != crate::download::service::DownloadOrigin::Song
            }) {
                return Err(Error::State("catalog download identity changed"));
            }
            let path = std::path::Path::new(
                resource["path"]
                    .as_str()
                    .ok_or(Error::InvalidInput("invalid downloaded path"))?,
            );
            if task.playback_target()? != path {
                return Err(Error::State("download reference changed"));
            }
            let check = match crate::download::offline::OfflineLibrary::new(self.session.clone())?
                .check(token, path, false)
                .await
            {
                Err(error @ Error::Io(_)) if matches!(&error, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound) => {
                    match self.missing_catalog_download(&resource, token).await {
                        Ok(Some(_)) => return Err(Error::DownloadedFileMissing),
                        Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                        _ => return Err(error),
                    }
                }
                other => other?,
            };
            if catalog_song.is_some_and(|song| {
                check.record.song_id != song
                    || task.output_integrity.as_ref() != Some(&check.record.integrity)
                    || task.encrypted != check.record.encrypted
            }) {
                return Err(Error::State("catalog download identity changed"));
            }
            if check.record.task_id != id || !check.access.can_play || !check.integrity_verified {
                return Err(Error::State("download is unavailable for this account"));
            }
            quality = if task.origin.is_cloud() {
                None
            } else {
                Some(check.record.quality.as_str().to_owned())
            };
            download_recheck = Some((
                service,
                id.to_owned(),
                path.to_owned(),
                task.output_integrity,
            ));
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
            if let Some(opened) = resource.get("native_open_file") {
                let opened: crate::library::native_open::OpenedFile =
                    serde_json::from_value(opened.clone())?;
                let local: LocalResource = serde_json::from_value(resource)?;
                opened.check_resource(&local)?;
                Ok(ResolvedSource {
                    input: MediaInput::NativeFile {
                        path: opened.path,
                        fingerprint: opened.fingerprint,
                    },
                    info: json!({"kind":"local","metadata":local.metadata}),
                    expires_at: None,
                })
            } else if resource.get("local_id").is_some() {
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
                    expires_at: None,
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
                    expires_at: None,
                })
            }
        })
        .await
        .map_err(|_| Error::State("local source worker failed"))??;
        if let Some((service, id, path, integrity)) = download_recheck {
            let latest = service.get(&id)?;
            if latest.playback_target()? != path || latest.output_integrity != integrity {
                return Err(Error::State("download reference changed"));
            }
        }
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
            return self.local(resource.clone(), token, None).await;
        }
        self.resolve_online(resource, token, false).await
    }
    pub(crate) async fn resolve_fresh_online(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Arc<ResolvedSource>> {
        if resource.get("local_id").is_some() || resource.get("path").is_some() {
            return Err(Error::InvalidInput(
                "fresh authorization requires online media",
            ));
        }
        self.resolve_online(resource, token, true).await
    }
    async fn resolve_online(
        &self,
        resource: &Value,
        token: &SessionToken,
        fresh: bool,
    ) -> Result<Arc<ResolvedSource>> {
        self.resolve_online_observed(resource, token, fresh, None)
            .await
            .map(|(source, _)| source)
    }

    async fn resolve_online_observed(
        &self,
        resource: &Value,
        token: &SessionToken,
        fresh: bool,
        expected: Option<&Arc<dyn Api>>,
    ) -> Result<(Arc<ResolvedSource>, Option<Arc<dyn Api>>)> {
        let key = self.key(resource, token)?;
        let (pending, mut receiver, owner) = {
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            let now = self.clock.now();
            state
                .cache
                .retain(|k, v| k.token == *token && v.deadline > now);
            let pending = PendingKey {
                revision: state.revision,
                key: key.clone(),
            };
            if fresh {
                state.cache.remove(&key);
                if let Some(previous) = state.pending.remove(&pending) {
                    previous.send_replace(Some(Err(Arc::new(Error::StaleResponse))));
                }
            }
            if let Some(cached) = state.cache.get(&key) {
                let value = cached.value.clone();
                drop(state);
                self.session.with_token(token, |_| Ok(()))?;
                return Ok((value, None));
            }
            if let Some(sender) = state.pending.get(&pending) {
                (pending, sender.subscribe(), None)
            } else {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                state.pending.insert(pending.clone(), sender.clone());
                (pending, receiver, Some(sender))
            }
        };
        if let Some(owner) = owner {
            let guard = Owner::new(self.state.clone(), pending.clone(), owner);
            let mut provenance = None;
            let result = self
                .fetch(resource, &key, expected)
                .await
                .and_then(|(value, api)| {
                    self.session.with_token(token, |_| Ok(()))?;
                    let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                    if !guard.current(&state) {
                        return Err(Error::StaleResponse);
                    }
                    provenance = Some(api);
                    Ok(Arc::new(value))
                });
            let outcome: Outcome = result.map_err(Arc::new);
            if let Ok(value) = &outcome {
                let deadline = value
                    .reuse_until()
                    .filter(|deadline| *deadline > self.now());
                if let Some(deadline) = deadline {
                    let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                    // Invalidation must win even after the fetch consistency
                    // check, before publication into the shared cache.
                    if !guard.current(&state) {
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
                .map(|source| (source, provenance))
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
                .map(|source| (source, None))
        }
    }
}
