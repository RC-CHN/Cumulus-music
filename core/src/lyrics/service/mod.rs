//! M/3081 request/cache/selection workflow, isolated from renderer and host.
mod cache;
mod request;
mod types;
pub use types::*;

use super::{
    files::{LibraryLyrics, LocalLyricSource, LocalLyrics},
    parse_lyrics, response,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct State {
    attempt: Uuid,
    revision: Uuid,
    selected: Option<String>,
    sidecar_key: Option<String>,
    view: LyricSnapshot,
    raw: Option<Value>,
}

pub struct LyricsService {
    session: Arc<AccountSession>,
    token: SessionToken,
    store: Arc<StateStore>,
    files: Arc<dyn LocalLyrics>,
    state: Mutex<State>,
}

struct Loading<'a> {
    service: &'a LyricsService,
    attempt: Uuid,
}
impl Drop for Loading<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.service.state.lock()
            && state.attempt == self.attempt
            && state.view.is_loading
        {
            state.revision = Uuid::new_v4();
            state.view.is_loading = false;
            state.view.is_lyric_fetch_failed = true;
            state.view.resource_key = None;
        }
    }
}

impl LyricsService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_files(session, Arc::new(LibraryLyrics))
    }
    pub fn with_files(session: Arc<AccountSession>, files: Arc<dyn LocalLyrics>) -> Result<Self> {
        let snapshot = session.snapshot()?;
        let store = session.persistent_store()?;
        let token = snapshot.token;
        let preferences = session.with_local_owner(&token, |uid| {
            store
                .get(uid, "lyric_preferences", "state")?
                .map(serde_json::from_value)
                .transpose()
                .map(|v| v.unwrap_or_default())
                .map_err(Error::from)
        })?;
        Ok(Self {
            session,
            token,
            store,
            files,
            state: Mutex::new(State {
                attempt: Uuid::new_v4(),
                revision: Uuid::new_v4(),
                selected: None,
                sidecar_key: None,
                view: LyricSnapshot {
                    preferences,
                    ..Default::default()
                },
                raw: None,
            }),
        })
    }
    fn with_state<T>(&self, operation: impl FnOnce(&mut State, &str) -> Result<T>) -> Result<T> {
        self.session.with_local_owner(&self.token, |uid| {
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            operation(&mut state, uid)
        })
    }
    fn resource_lock(&self, key: &str) -> Result<OperationLock> {
        OperationLock::acquire(
            self.store.directory(),
            self.token.uid.as_deref().unwrap_or("local-guest"),
            &format!("lyric:{key}"),
        )
    }
    pub fn snapshot(&self) -> Result<LyricSnapshot> {
        self.with_state(|state, _| Ok(state.view.clone()))
    }
    /// Reads a changed document without HTTP or cloning unchanged lyrics. The
    /// selected key survives an empty/failed result, unlike resource_key (which
    /// means successful content). It is cleared when the selection is retired.
    pub fn snapshot_if_changed(
        &self,
        target: &LyricTarget,
        observed: Option<&str>,
    ) -> Result<Option<(String, LyricSnapshot)>> {
        let key = request::key(target)?;
        self.with_state(|state, _| {
            if state.selected.as_deref() != Some(&key) {
                return Err(Error::StaleResponse);
            }
            if state.view.is_loading {
                return Err(Error::MutationBusy);
            }
            let revision = state.revision.to_string();
            Ok((observed != Some(&revision)).then(|| (revision, state.view.clone())))
        })
    }
    pub fn clear(&self) -> Result<LyricSnapshot> {
        self.with_state(|state, _| {
            state.attempt = Uuid::new_v4();
            state.revision = Uuid::new_v4();
            state.selected = None;
            state.sidecar_key = None;
            state.view = LyricSnapshot {
                preferences: state.view.preferences,
                ..Default::default()
            };
            state.view.lyric.display_type = super::DisplayType::Ask;
            state.raw = None;
            Ok(state.view.clone())
        })
    }
    pub fn set_preferences(&self, preferences: LyricPreferences) -> Result<LyricSnapshot> {
        self.set_preferences_inner(None, preferences)
    }
    /// Apply a preference gesture only while its displayed lyric is current.
    /// The persisted preference is still account-wide.
    pub fn set_target_preferences(
        &self,
        target: &LyricTarget,
        preferences: LyricPreferences,
    ) -> Result<LyricSnapshot> {
        self.set_preferences_inner(Some(&request::key(target)?), preferences)
    }
    fn set_preferences_inner(
        &self,
        expected: Option<&str>,
        preferences: LyricPreferences,
    ) -> Result<LyricSnapshot> {
        self.with_state(|state, uid| {
            if let Some(expected) = expected {
                require_document(state, expected)?;
            }
            self.store.put(
                uid,
                "lyric_preferences",
                "state",
                &serde_json::to_value(preferences)?,
            )?;
            state.revision = Uuid::new_v4();
            state.view.preferences = preferences;
            Ok(state.view.clone())
        })
    }
    pub fn import_official_cache(&self, target: &LyricTarget, value: Value) -> Result<bool> {
        let key = request::key(target)?;
        let _lock = self.resource_lock(&key)?;
        let Some(value) = cache::legacy(value)? else {
            return Ok(false);
        };
        parse_lyrics(&value)?;
        self.with_state(|_, uid| {
            self.store.transaction(|tx| {
                if tx.get(uid, cache::KIND, &key)?.is_some() {
                    return Err(Error::State("lyric cache already exists"));
                }
                tx.put(uid, cache::KIND, &key, &cache::record(&value))
            })?;
            Ok(true)
        })
    }
    pub fn adjust_offset(&self, gain: f64) -> Result<LyricSnapshot> {
        self.adjust_offset_inner(None, gain)
    }
    /// Unlike a change to whichever lyric happens to be loaded, a gesture
    /// from either desktop window always names the displayed resource.
    pub fn adjust_target_offset(&self, target: &LyricTarget, gain: f64) -> Result<LyricSnapshot> {
        self.adjust_offset_inner(Some(&request::key(target)?), gain)
    }
    fn adjust_offset_inner(&self, expected: Option<&str>, gain: f64) -> Result<LyricSnapshot> {
        if !gain.is_finite() {
            return Err(Error::InvalidInput("lyric offset gain must be finite"));
        }
        let (key, sidecar_key, attempt) = self.with_state(|state, _| {
            if let Some(expected) = expected {
                require_document(state, expected)?;
            }
            Ok((
                state
                    .view
                    .resource_key
                    .clone()
                    .ok_or(Error::State("no selected lyrics"))?,
                state.sidecar_key.clone(),
                state.attempt,
            ))
        })?;
        let storage_key = sidecar_key.as_deref().unwrap_or(&key);
        let _lock = self.resource_lock(storage_key)?;
        self.with_state(|state, uid| {
            require_document(state, &key)?;
            if state.attempt != attempt || state.sidecar_key != sidecar_key {
                return Err(Error::StaleResponse);
            }
            if !state.view.lyric.scrollable {
                return Err(Error::State("lyrics are not scrollable"));
            }
            let mut raw = state
                .raw
                .clone()
                .ok_or(Error::State("missing lyric source"))?;
            let offset = state.view.lyric.offset + 0.5 * gain;
            if !offset.is_finite() {
                return Err(Error::InvalidInput("lyric offset overflow"));
            }
            // Scrollable sources currently select LRC in M/370.
            raw["lrc"]["offset"] = json!(offset);
            let parsed = parse_lyrics(&raw)?;
            if !storage_key.starts_with("local:") {
                let stored = cache::read(&self.store, uid, storage_key)?;
                if stored.as_ref() != state.raw.as_ref() {
                    return Err(Error::State("lyric cache changed; clear and reload"));
                }
            }
            self.store
                .put(uid, cache::KIND, storage_key, &cache::record(&raw))?;
            state.revision = Uuid::new_v4();
            state.raw = Some(raw);
            state.view.lyric = parsed;
            Ok(state.view.clone())
        })
    }

    pub async fn fetch(&self, input: LyricRequest) -> Result<LyricSnapshot> {
        if !matches!(input.target, LyricTarget::Local(_)) {
            self.session.with_token(&self.token, |_| Ok(()))?;
        }
        if input.track_mv || !input.demand.any() {
            return self.clear();
        }
        let key = request::key(&input.target)?;
        if let Some(view) = self.with_state(|state, _| {
            Ok(
                (!state.view.is_loading && state.view.resource_key.as_ref() == Some(&key))
                    .then(|| state.view.clone()),
            )
        })? {
            return Ok(view);
        }
        let _lock = self.resource_lock(&key)?;
        let (attempt, cached) = self.with_state(|state, uid| {
            let cached = cache::read(&self.store, uid, &key)?;
            state.attempt = Uuid::new_v4();
            state.revision = Uuid::new_v4();
            state.selected = Some(key.clone());
            state.sidecar_key = None;
            state.view = LyricSnapshot {
                is_loading: true,
                preferences: state.view.preferences,
                ..Default::default()
            };
            state.raw = None;
            Ok((state.attempt, cached))
        })?;
        let _loading = Loading {
            service: self,
            attempt,
        };
        let mut network_failed = false;
        let mut from_sidecar = false;
        let mut from_embedded = false;
        let mut from_cache = false;
        let mut cache_value = None;
        let mut raw = match &input.target {
            LyricTarget::Local(path) => {
                let local = self.files.read_local(path).await?;
                let text = if let Some((text, source)) = local {
                    from_sidecar = source == LocalLyricSource::Sidecar;
                    from_embedded = source == LocalLyricSource::Embedded;
                    text
                } else {
                    String::new()
                };
                let mut raw = response::local_response(&text);
                // Preserve a local user offset only if the source text is unchanged.
                if let Some(old) = &cached
                    && old["lrc"]["lyric"] == raw["lrc"]["lyric"]
                {
                    raw["lrc"]["offset"] = old["lrc"]["offset"].clone();
                }
                raw
            }
            _ => {
                let target = &input.target;
                let cached_ref = cached.as_ref();
                let incoming = self
                    .session
                    .run_for(&self.token, |api, uid| async move {
                        request::fetch(api.as_ref(), &uid, target, cached_ref).await
                    })
                    .await;
                let incoming = match incoming {
                    Ok(value) => Some(value),
                    Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                    Err(error) if error.is_service_code(301) => return Err(error),
                    Err(_) => {
                        network_failed = true;
                        from_cache = cached.is_some();
                        None
                    }
                };
                let value = response::merge(cached, incoming);
                cache_value = Some(value.clone());
                value
            }
        };
        let mut present = raw["code"] == 200 && raw["noLyric"] == false;
        let mut sidecar_key = None;
        if !present
            && let Some(path) = input.downloaded_audio
            && let Some(text) = self.files.read(&path).await?
        {
            let local = request::key(&LyricTarget::Local(path))?;
            raw = response::local_response(&text);
            let old = self.with_state(|_, uid| cache::read(&self.store, uid, &local))?;
            if let Some(old) = old
                && old["lrc"]["lyric"] == raw["lrc"]["lyric"]
            {
                raw["lrc"]["offset"] = old["lrc"]["offset"].clone();
            }
            sidecar_key = Some(local);
            from_sidecar = true;
            present = true;
        }
        let mut parsed = if present {
            parse_lyrics(&raw)?
        } else {
            super::ParsedLyrics::default()
        };
        if !present {
            parsed.display_type = super::DisplayType::Ask;
        }
        self.with_state(|state, uid| {
            if state.attempt != attempt {
                return Err(Error::StaleResponse);
            }
            if let Some(value) = cache_value {
                self.store
                    .put(uid, cache::KIND, &key, &cache::record(&value))?;
            }
            state.revision = Uuid::new_v4();
            state.sidecar_key = sidecar_key;
            state.raw = present.then_some(raw);
            state.view = LyricSnapshot {
                resource_key: present.then_some(key),
                is_loading: false,
                is_lyric_fetch_failed: !present && network_failed,
                network_failed,
                from_cache,
                from_sidecar,
                from_embedded,
                preferences: state.view.preferences,
                lyric: parsed,
            };
            Ok(state.view.clone())
        })
    }
}

fn require_document(state: &State, expected: &str) -> Result<()> {
    if state.view.resource_key.as_deref() == Some(expected) && !state.view.is_loading {
        Ok(())
    } else {
        Err(Error::StaleResponse)
    }
}
