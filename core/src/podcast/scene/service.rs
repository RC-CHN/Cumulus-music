use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    podcast::PodcastApi,
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
const STATE: &str = "scene-fm-v1";
pub struct SceneFmService {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl SceneFmService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn gate(&self, t: &SessionToken) -> Result<OperationLock> {
        let owner = self.session.with_token(t, |u| Ok(u.to_owned()))?;
        OperationLock::acquire(self.store.directory(), STATE, &owner)
    }
    pub fn cached_for(&self, t: &SessionToken) -> Result<Option<SceneFmSnapshot>> {
        self.session.with_token(t, |u| {
            self.store
                .get(u, STATE, "current")?
                .map(|v| {
                    let v: SceneFmSnapshot = serde_json::from_value(v)?;
                    v.validate()?;
                    Ok(v)
                })
                .transpose()
        })
    }
    fn save(&self, t: &SessionToken, expected: u64, value: &SceneFmSnapshot) -> Result<()> {
        value.validate()?;
        self.session.with_token(t, |u| {
            self.store.transaction(|tx| {
                let rev = tx
                    .get(u, STATE, "current")?
                    .map(|v| serde_json::from_value::<SceneFmSnapshot>(v).map(|s| s.revision))
                    .transpose()?
                    .unwrap_or(0);
                if rev != expected {
                    return Err(Error::QueueChanged);
                }
                tx.put(u, STATE, "current", &serde_json::to_value(value)?)
            })
        })
    }
    fn heard(&self, t: &SessionToken) -> Result<BTreeSet<String>> {
        // These records are committed only after the native engine successfully
        // loads a voice. They are local account history, not remote play counts.
        self.session.with_token(t, |u| {
            Ok(self
                .store
                .values(u, "podcast-positions-v1")?
                .into_keys()
                .chain(self.store.values(u, "podcast-heard-v1")?.into_keys())
                .collect())
        })
    }
    pub fn autoplay_for(&self, t: &SessionToken) -> Result<bool> {
        self.session.with_token(t, |u| {
            Ok(self
                .store
                .get(u, STATE, "autoplay")?
                .and_then(|v| v.as_bool())
                .unwrap_or(false))
        })
    }
    pub fn set_autoplay_for(&self, t: &SessionToken, enabled: bool) -> Result<()> {
        self.session
            .with_token(t, |u| self.store.put(u, STATE, "autoplay", &json!(enabled)))
    }
    pub async fn refresh_for(&self, t: &SessionToken) -> Result<SceneFmSnapshot> {
        let _gate = self.gate(t)?;
        let old = self.cached_for(t)?;
        let revision = old.as_ref().map(|v| v.revision).unwrap_or(0);
        if old.as_ref().is_some_and(|s| s.pending) {
            return Err(Error::State("scene FM selection requires recovery"));
        }
        let mut scenes = self
            .session
            .run_for(t, |api, _| async { SceneFmApi::new(api).home().await })
            .await?;
        let heard = self.heard(t)?;
        for s in &mut scenes {
            s.choose_unheard(&heard);
            if let Some(previous) = old
                .as_ref()
                .and_then(|o| o.scenes.iter().find(|p| p.id == s.id))
            {
                if s.voices.is_empty() {
                    s.voices = previous.voices.clone();
                    s.choose_unheard(&heard);
                }
                if previous
                    .selected
                    .as_ref()
                    .is_some_and(|id| s.voices.iter().any(|v| &v.id == id))
                {
                    s.selected = previous.selected.clone();
                }
            }
        }
        let active_id = old
            .as_ref()
            .and_then(|o| o.active_id.clone())
            .filter(|id| scenes.iter().any(|s| &s.id == id))
            .or_else(|| scenes.first().map(|s| s.id.clone()));
        let mut value = SceneFmSnapshot {
            revision,
            scenes,
            active_id,
            pending: false,
        };
        value.bump()?;
        self.save(t, revision, &value)?;
        Ok(value)
    }
    pub async fn select_for(
        &self,
        t: &SessionToken,
        expected: u64,
        id: &str,
    ) -> Result<SceneFmSnapshot> {
        scene_id(id)?;
        let _gate = self.gate(t)?;
        let mut value = self.required(t, expected)?;
        let scene = value
            .scenes
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(Error::InvalidInput("scene FM category not offered"))?;
        if scene.voices.is_empty() {
            scene.voices = self
                .session
                .run_for(t, |api, _| async {
                    SceneFmApi::new(api).programs(id).await
                })
                .await?;
        }
        scene.choose_unheard(&self.heard(t)?);
        value.active_id = Some(id.into());
        value.pending = true;
        value.bump()?;
        self.save(t, expected, &value)?;
        Ok(value)
    }
    fn required(&self, t: &SessionToken, revision: u64) -> Result<SceneFmSnapshot> {
        let v = self
            .cached_for(t)?
            .ok_or(Error::State("scene FM not loaded"))?;
        if v.revision != revision {
            return Err(Error::QueueChanged);
        }
        Ok(v)
    }
    pub fn cancel_for(&self, t: &SessionToken, revision: u64) -> Result<SceneFmSnapshot> {
        let _gate = self.gate(t)?;
        let mut v = self.required(t, revision)?;
        v.pending = false;
        v.bump()?;
        self.save(t, revision, &v)?;
        Ok(v)
    }
    /// Return a full native voice queue, independent of visible UI pagination.
    pub async fn prepare_for(
        &self,
        t: &SessionToken,
        expected: u64,
    ) -> Result<(SceneFmSnapshot, Vec<Value>, usize)> {
        let _gate = self.gate(t)?;
        let mut state = self.required(t, expected)?;
        let id = state
            .active_id
            .clone()
            .ok_or(Error::State("scene FM has no categories"))?;
        // Commit the user's intent before any lazy network preparation. A
        // dropped task or failed program detail remains explicitly recoverable.
        state.pending = true;
        state.bump()?;
        self.save(t, expected, &state)?;
        let expected = state.revision;
        let scene = state
            .scenes
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(Error::State("scene FM category missing"))?;
        if scene.voices.is_empty() {
            scene.voices = self
                .session
                .run_for(t, |api, _| async {
                    SceneFmApi::new(api).programs(&id).await
                })
                .await?;
            scene.choose_unheard(&self.heard(t)?);
        }
        let index = scene
            .selected
            .as_ref()
            .and_then(|id| scene.voices.iter().position(|v| &v.id == id))
            .ok_or(Error::State("scene FM has no playable voices"))?;
        let mut resources = Vec::with_capacity(scene.voices.len());
        for episode in &mut scene.voices {
            if episode.song_id.is_none() {
                *episode = self
                    .session
                    .run_for(t, |api, _| async {
                        PodcastApi::new(api).episode(&json!(episode.id)).await
                    })
                    .await?;
            }
            let mut resource = episode.playback_resource()?;
            resource["song"] = episode.download_metadata()?;
            resource["song"]["voice_id"] = json!(episode.id);
            resource["level"] = json!("standard");
            resources.push(resource);
        }
        state.pending = true;
        state.bump()?;
        self.save(t, expected, &state)?;
        Ok((state, resources, index))
    }
    /// Acknowledging actual queue identity also repairs a lost local completion
    /// write. A different old queue must not overwrite a pending selection.
    pub fn sync_for(
        &self,
        t: &SessionToken,
        scene_id: &str,
        voice_id: &str,
        applied_revision: Option<u64>,
    ) -> Result<Option<SceneFmSnapshot>> {
        let _gate = self.gate(t)?;
        let Some(mut state) = self.cached_for(t)? else {
            return Ok(None);
        };
        if state.pending
            && (state.active_id.as_deref() != Some(scene_id)
                || applied_revision != Some(state.revision))
        {
            return Ok(Some(state));
        }
        let old = state.revision;
        let preferred = state.scenes.iter().position(|s| {
            Some(&s.id) == state.active_id.as_ref() && s.voices.iter().any(|v| v.id == voice_id)
        });
        let found = preferred.or_else(|| {
            state
                .scenes
                .iter()
                .position(|s| s.voices.iter().any(|v| v.id == voice_id))
        });
        if let Some(i) = found {
            if state.pending && state.scenes[i].selected.as_deref() != Some(voice_id) {
                return Ok(Some(state));
            }
            if state.pending
                || state.active_id.as_deref() != Some(state.scenes[i].id.as_str())
                || state.scenes[i].selected.as_deref() != Some(voice_id)
            {
                state.active_id = Some(state.scenes[i].id.clone());
                state.scenes[i].selected = Some(voice_id.into());
                state.pending = false;
                state.bump()?;
                self.save(t, old, &state)?;
            }
        }
        Ok(Some(state))
    }
}
