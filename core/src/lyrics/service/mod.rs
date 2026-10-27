//! M/3081 request/cache/selection workflow, isolated from renderer and host.
mod cache;
mod request;
mod types;
pub use types::*;

use super::{
    files::{LocalLyrics, SidecarLyrics},
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
            state.view.is_loading = false;
            state.view.is_lyric_fetch_failed = true;
            state.view.resource_key = None;
        }
    }
}

impl LyricsService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_files(session, Arc::new(SidecarLyrics))
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
    pub fn clear(&self) -> Result<LyricSnapshot> {
        self.with_state(|state, _| {
            state.attempt = Uuid::new_v4();
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
        self.with_state(|state, uid| {
            self.store.put(
                uid,
                "lyric_preferences",
                "state",
                &serde_json::to_value(preferences)?,
            )?;
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
        if !gain.is_finite() {
            return Err(Error::InvalidInput("lyric offset gain must be finite"));
        }
        let key = self
            .snapshot()?
            .resource_key
            .ok_or(Error::State("no selected lyrics"))?;
        let _lock = self.resource_lock(&key)?;
        self.with_state(|state, uid| {
            if state.view.resource_key.as_ref() != Some(&key) || state.view.is_loading {
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
            if !key.starts_with("local:") {
                let stored = cache::read(&self.store, uid, &key)?;
                if stored.as_ref() != state.raw.as_ref() {
                    return Err(Error::State("lyric cache changed; clear and reload"));
                }
            }
            self.store
                .put(uid, cache::KIND, &key, &cache::record(&raw))?;
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
        let mut from_cache = false;
        let mut cache_value = None;
        let mut raw = match &input.target {
            LyricTarget::Local(path) => {
                from_sidecar = true;
                let text = self.files.read(path).await?.unwrap_or_default();
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
        if !present && let Some(path) = input.downloaded_audio {
            let text = self.files.read(&path).await?.unwrap_or_default();
            raw = response::local_response(&text);
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
            state.raw = present.then_some(raw);
            state.view = LyricSnapshot {
                resource_key: present.then_some(key),
                is_loading: false,
                is_lyric_fetch_failed: !present && network_failed,
                network_failed,
                from_cache,
                from_sidecar,
                preferences: state.view.preferences,
                lyric: parsed,
            };
            Ok(state.view.clone())
        })
    }
}
