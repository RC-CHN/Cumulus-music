//! M3160's per-radio observations. This presentation cache never authorizes
//! playback, supplies a default to a wire read, or changes a mutation result.
use super::PodcastApi;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::{
        Api, PODCAST_RESUME_READ_PATH, PlayerPodcastResumeRead, VinylPlaybackReadPermission,
        identifier, validate_response,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) const EXPERIMENT: &str = "Podcast_ResumePlaySwitch";
const CAPACITY: usize = 1024;

#[derive(Clone, Copy)]
pub(crate) struct PlayerBinding<'a> {
    pub(crate) revision: &'a str,
    pub(crate) permission: &'a VinylPlaybackReadPermission<'a>,
}

struct AccountObservations {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    radios: HashMap<String, (u64, bool)>,
}

impl AccountObservations {
    fn owns(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: &str) -> bool {
        self.token == *token
            && self.credentials == credentials
            && self.api.upgrade().is_some_and(|old| Arc::ptr_eq(&old, api))
    }
}

#[derive(Default)]
pub(crate) struct Observations {
    sequence: AtomicU64,
    current: Mutex<Option<AccountObservations>>,
}

impl Observations {
    fn issue(&self) -> Option<u64> {
        self.sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .ok()
    }

    fn record(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: String,
        id: String,
        sequence: u64,
        enabled: bool,
    ) -> Result<()> {
        let mut lock = self.current.lock().map_err(|_| Error::LockPoisoned)?;
        if !lock
            .as_ref()
            .is_some_and(|current| current.owns(token, api, &credentials))
        {
            *lock = Some(AccountObservations {
                token: token.clone(),
                api: Arc::downgrade(api),
                credentials,
                radios: HashMap::new(),
            });
        }
        let radios = &mut lock.as_mut().expect("scope installed").radios;
        if let Some(value) = radios.get_mut(&id) {
            // A reply begun before a newer observation cannot overwrite it.
            if sequence > value.0 {
                *value = (sequence, enabled);
            }
            return Ok(());
        }
        if radios.len() == CAPACITY {
            let (oldest, &(old_sequence, _)) = radios
                .iter()
                .min_by_key(|(_, value)| value.0)
                .expect("capacity is nonzero");
            if sequence <= old_sequence {
                return Ok(());
            }
            let oldest = oldest.clone();
            radios.remove(&oldest);
        }
        radios.insert(id, (sequence, enabled));
        Ok(())
    }

    pub(crate) fn known(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
        id: &str,
    ) -> Result<Option<bool>> {
        Ok(self
            .current
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|current| current.owns(token, api, credentials))
            .and_then(|current| current.radios.get(id))
            .map(|&(_, enabled)| enabled))
    }

    pub(crate) fn known_off(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
    ) -> Result<HashSet<String>> {
        let lock = self.current.lock().map_err(|_| Error::LockPoisoned)?;
        let Some(current) = lock
            .as_ref()
            .filter(|old| old.owns(token, api, credentials))
        else {
            return Ok(HashSet::new());
        };
        Ok(current
            .radios
            .iter()
            .filter(|(_, (_, enabled))| !enabled)
            .map(|(id, _)| id.clone())
            .collect())
    }
}

/// Observe only the exact boolean returned by a read that was already needed.
/// Rotation invalidates the presentation observation without changing the
/// ordinary read's established return/transport semantics.
pub(crate) async fn read_for(
    session: &AccountSession,
    token: &SessionToken,
    id: &Value,
) -> Result<bool> {
    read_observed(session, token, identifier(id)?, None).await
}

pub(super) async fn read_player_for(
    session: &AccountSession,
    token: &SessionToken,
    id: &str,
    binding: PlayerBinding<'_>,
) -> Result<bool> {
    read_observed(session, token, id.into(), Some(binding)).await
}

async fn read_observed(
    session: &AccountSession,
    token: &SessionToken,
    id: String,
    player: Option<PlayerBinding<'_>>,
) -> Result<bool> {
    session
        .run_for(token, |api, _| async move {
            let observation = api
                .podcast_resume_observation_revision()
                .ok()
                .flatten()
                .zip(session.podcast_resume.issue());
            let enabled = if let Some(binding) = player {
                (binding.permission)()?;
                let reply = api
                    .request_player_podcast_resume_read(
                        PlayerPodcastResumeRead::new(&id)?,
                        binding.revision,
                        binding.permission,
                    )
                    .await?;
                (binding.permission)()?;
                PodcastApi::resume_from_reply(&validate_response(
                    PODCAST_RESUME_READ_PATH,
                    reply,
                    &[200],
                )?)?
            } else {
                PodcastApi::new(api.clone())
                    .resume_enabled(&json!(id))
                    .await?
            };
            if let Some((credentials, sequence)) = observation {
                let _ = session.with_verified_profile(token, &api, |_, _| {
                    if api.podcast_resume_observation_revision()?.as_deref() == Some(&credentials) {
                        session.podcast_resume.record(
                            token,
                            &api,
                            credentials,
                            id,
                            sequence,
                            enabled,
                        )?;
                    }
                    Ok(())
                });
            }
            Ok(enabled)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::api::ApiFuture;

    struct Closed;
    impl Api for Closed {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async { panic!("observation storage must not use HTTP") })
        }
    }

    #[test]
    fn late_off_cannot_overwrite_a_newer_on_for_the_same_radio() {
        let observations = Observations::default();
        let api: Arc<dyn Api> = Arc::new(Closed);
        let token = SessionToken {
            generation: 1,
            uid: Some("2".into()),
        };
        let older = observations.issue().unwrap();
        let newer = observations.issue().unwrap();
        observations
            .record(&token, &api, "rev".into(), "7".into(), newer, true)
            .unwrap();
        observations
            .record(&token, &api, "rev".into(), "7".into(), older, false)
            .unwrap();
        assert!(
            observations
                .known_off(&token, &api, "rev")
                .unwrap()
                .is_empty()
        );
    }
}
