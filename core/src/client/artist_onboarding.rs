mod api;

use super::{
    MusicClient,
    dto::{AccountData, TrackView},
};
use crate::{
    Error, Result,
    account::{SessionToken, artist_onboarding::ArtistFavorScene as Scene},
    client::dto::PlayerView,
    membership::Quality,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashSet,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const KIND: &str = "artist_onboarding_v1";
const WEEK: u64 = 7 * 24 * 60 * 60;
const MAX_CANDIDATES: usize = 500;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCandidate {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistOffer {
    pub id: String,
    pub scene: Scene,
    pub resumed: bool,
    pub save_blocked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistSave {
    Confirmed,
    Uncertain,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistResultAction {
    PlayNow,
    Later,
}

#[derive(Serialize, Deserialize)]
struct Resume {
    scene: Scene,
    aborted: u64,
}

struct Run {
    token: SessionToken,
    offer: ArtistOffer,
    candidates: HashSet<String>,
    selected: Vec<u64>,
    save: Option<ArtistSave>,
    saving: bool,
    tracks: Vec<String>,
    closed: bool,
}

#[derive(Default)]
pub(super) struct Host(Mutex<Option<Run>>);

impl Host {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<Run>>> {
        self.0.lock().map_err(|_| Error::LockPoisoned)
    }
    fn run<'a>(slot: &'a mut Option<Run>, token: &SessionToken, id: &str) -> Result<&'a mut Run> {
        slot.as_mut()
            .filter(|run| &run.token == token && run.offer.id == id && !run.closed)
            .ok_or(Error::StaleResponse)
    }
}

struct SaveGuard<'a> {
    host: &'a Host,
    token: &'a SessionToken,
    id: &'a str,
}
impl Drop for SaveGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.host.lock()
            && let Ok(run) = Host::run(&mut slot, self.token, self.id)
        {
            run.saving = false;
        }
    }
}

fn time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| Error::State("artist onboarding clock unavailable"))
}

fn packet<T>(token: &SessionToken, value: T) -> AccountData<T> {
    AccountData {
        epoch: token.generation.to_string(),
        value,
    }
}

impl MusicClient {
    pub fn artist_onboarding_offer(&self, epoch: &str) -> Result<AccountData<Option<ArtistOffer>>> {
        let token = self.player_token(epoch)?;
        let Some(login) = self.session.artist_login(&token)? else {
            return Ok(packet(&token, None));
        };
        let store = self.registry.store();
        let value = self.session.with_token(&token, |uid| {
            let mut slot = self.artist_onboarding.lock()?;
            if let Some(run) = slot.as_ref().filter(|run| run.token == token) {
                return Ok((!run.closed).then(|| run.offer.clone()));
            }
            let offer = store.transaction(|tx| {
                let resume: Option<Resume> = tx
                    .get(uid, KIND, "resume")?
                    .and_then(|value| serde_json::from_value(value).ok());
                let current = time()?;
                let resume = resume.filter(|item| {
                    current
                        .checked_sub(item.aborted)
                        .is_some_and(|age| age <= WEEK)
                });
                tx.delete(uid, KIND, "resume")?;
                let Some(scene) = login.scene.or_else(|| resume.as_ref().map(|row| row.scene))
                else {
                    return Ok(None);
                };
                Ok(Some(ArtistOffer {
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    scene,
                    resumed: resume.is_some(),
                    save_blocked: tx.get(uid, KIND, "save")?.is_some(),
                }))
            })?;
            *slot = offer.as_ref().map(|offer| Run {
                token: token.clone(),
                offer: offer.clone(),
                candidates: HashSet::new(),
                selected: Vec::new(),
                save: None,
                saving: false,
                tracks: Vec::new(),
                closed: false,
            });
            Ok(offer)
        })?;
        Ok(packet(&token, value))
    }

    pub async fn artist_onboarding_candidates(
        &self,
        epoch: &str,
        id: &str,
        similar_to: Option<&str>,
    ) -> Result<AccountData<Vec<ArtistCandidate>>> {
        let token = self.player_token(epoch)?;
        let (scene, similar) = {
            let mut slot = self.artist_onboarding.lock()?;
            let run = Host::run(&mut slot, &token, id)?;
            if run.save.is_some() {
                return Err(Error::State("artist selection is complete"));
            }
            let similar = similar_to
                .map(|id| {
                    if !run.candidates.contains(id) {
                        return Err(Error::InvalidInput("artist was not offered"));
                    }
                    api::numeric_id(id)
                })
                .transpose()?;
            (run.offer.scene, similar)
        };
        let rows = self
            .session
            .run_for(&token, |api, _| async move {
                match similar {
                    Some(artist) => api::similar(&*api, scene, artist).await,
                    None => api::candidates(&*api, scene).await,
                }
            })
            .await?;
        let mut slot = self.artist_onboarding.lock()?;
        let run = Host::run(&mut slot, &token, id)?;
        if run.save.is_some() {
            return Err(Error::StaleResponse);
        }
        let rows = rows
            .into_iter()
            .filter(|row| {
                if run.candidates.contains(&row.id) {
                    similar.is_none()
                } else if run.candidates.len() < MAX_CANDIDATES {
                    run.candidates.insert(row.id.clone())
                } else {
                    false
                }
            })
            .collect();
        Ok(packet(&token, rows))
    }

    /// Persist uncertainty before sending; a stale/failed receipt must not
    /// permit a second write after a relogin or crash.
    pub async fn artist_onboarding_save(
        &self,
        epoch: &str,
        id: &str,
        artists: Vec<String>,
    ) -> Result<AccountData<ArtistSave>> {
        let token = self.player_token(epoch)?;
        let selected = api::selection(&artists)?;
        let store = self.registry.store();
        self.session.with_token(&token, |uid| {
            let mut slot = self.artist_onboarding.lock()?;
            let run = Host::run(&mut slot, &token, id)?;
            if run.saving || run.save.is_some() || run.offer.save_blocked {
                return Err(Error::State("artist preferences save cannot be repeated"));
            }
            if artists.iter().any(|id| !run.candidates.contains(id)) {
                return Err(Error::InvalidInput("artist was not offered"));
            }
            store.transaction(|tx| {
                if tx.get(uid, KIND, "save")?.is_some() {
                    return Err(Error::State("artist preferences save cannot be repeated"));
                }
                tx.put(uid, KIND, "save", &json!({"attempt":id}))
            })?;
            run.save = Some(ArtistSave::Uncertain);
            run.saving = true;
            run.selected.clone_from(&selected);
            Ok(())
        })?;
        let _guard = SaveGuard {
            host: &self.artist_onboarding,
            token: &token,
            id,
        };
        let result = self
            .session
            .run_for(&token, |api, _| async move {
                api.call(api::SAVE_PATH, json!({"artistIds":selected}), false, &[200])
                    .await
            })
            .await;
        self.session.with_token(&token, |uid| {
            let mut slot = self.artist_onboarding.lock()?;
            let run = Host::run(&mut slot, &token, id)?;
            let phase = if result.is_ok() {
                store.transaction(|tx| {
                    if tx.get(uid, KIND, "save")? != Some(json!({"attempt":id})) {
                        return Err(Error::StaleResponse);
                    }
                    tx.delete(uid, KIND, "save")
                })?;
                ArtistSave::Confirmed
            } else {
                ArtistSave::Uncertain
            };
            run.save = Some(phase);
            Ok(packet(&token, phase))
        })
    }

    pub async fn artist_onboarding_preview(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<Vec<TrackView>>> {
        let token = self.player_token(epoch)?;
        let (scene, selected) = {
            let mut slot = self.artist_onboarding.lock()?;
            let run = Host::run(&mut slot, &token, id)?;
            if run.saving || run.save.is_none() {
                return Err(Error::State(
                    "confirm artist selection before reading songs",
                ));
            }
            (run.offer.scene, run.selected.clone())
        };
        let tracks = self
            .session
            .run_for(&token, |api, _| async move {
                api::recommendations(api, scene, &selected).await
            })
            .await?;
        let mut slot = self.artist_onboarding.lock()?;
        let run = Host::run(&mut slot, &token, id)?;
        run.tracks = tracks
            .iter()
            .filter(|track| !track.missing)
            .map(|track| track.id.clone())
            .collect();
        Ok(packet(&token, tracks))
    }

    pub fn artist_onboarding_close(&self, epoch: &str, id: &str) -> Result<()> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |uid| {
            let mut slot = self.artist_onboarding.lock()?;
            let run = Host::run(&mut slot, &token, id)?;
            if run.saving {
                return Err(Error::MutationBusy);
            }
            if run.save.is_none() && !run.offer.resumed && !run.offer.save_blocked {
                self.registry.store().put(
                    uid,
                    KIND,
                    "resume",
                    &serde_json::to_value(Resume {
                        scene: run.offer.scene,
                        aborted: time()?,
                    })?,
                )?;
            }
            run.closed = true;
            Ok(())
        })
    }

    pub async fn artist_onboarding_result(
        &self,
        epoch: &str,
        id: &str,
        action: ArtistResultAction,
        queue_revision: String,
        quality: Quality,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let ids = {
            let mut slot = self.artist_onboarding.lock()?;
            let run = Host::run(&mut slot, &token, id)?;
            if run.tracks.is_empty() {
                return Err(Error::State("no recommended songs"));
            }
            run.tracks.clone()
        };
        match action {
            ArtistResultAction::PlayNow => {
                self.play_artist_favor(epoch, ids, quality, queue_revision)
                    .await
            }
            ArtistResultAction::Later => {
                self.queue_artist_favor(epoch, ids, quality, queue_revision)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests;
