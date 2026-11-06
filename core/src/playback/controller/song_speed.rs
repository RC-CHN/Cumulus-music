//! Official PC song-speed selection. The existing voice and generic native rate
//! controls are separate; a presentation observation never grants media access.
use super::{PlaybackSpeedScope, state::Actor};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

const MIN: u8 = 5;
const MAX: u8 = 20;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SongSpeedSelection {
    Preset { tenths: u8 },
    Custom { tenths: u8 },
}

impl SongSpeedSelection {
    fn checked(self) -> Result<(u8, bool)> {
        match self {
            Self::Preset { tenths } if [20, 15, 10, 7, 5].contains(&tenths) => Ok((tenths, false)),
            Self::Custom { tenths } if (MIN..=MAX).contains(&tenths) => Ok((tenths, true)),
            _ => Err(Error::InvalidInput("invalid song speed selection")),
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct SongSpeedObservation {
    pub epoch: u64,
    pub revision: u64,
    pub enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(super) struct Saved {
    pub custom_tenths: u8,
    pub custom_selected: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSpeedView {
    pub enabled: bool,
    pub revision: String,
    pub custom_tenths: u8,
    pub custom_selected: bool,
    pub sync_failed: bool,
}

impl Actor {
    pub(super) fn song_speed_observation(&self) -> SongSpeedObservation {
        self.song_speed_observations
            .as_ref()
            .map(|source| *source.borrow())
            .filter(|source| source.epoch == self.token.generation)
            .unwrap_or_default()
    }

    pub(super) fn effective_speed(&self, scope: PlaybackSpeedScope) -> f64 {
        self.effective_speed_with(scope, self.prefs)
    }

    pub(super) fn effective_speed_with(
        &self,
        scope: PlaybackSpeedScope,
        prefs: super::types::Preferences,
    ) -> f64 {
        let enabled = self.song_speed_observation().enabled;
        if scope == PlaybackSpeedScope::Song
            && (enabled == Some(false) || (enabled.is_none() && prefs.song_speed.is_some()))
        {
            1.0
        } else {
            prefs.speed(scope)
        }
    }

    pub(super) fn song_speed_view(&self) -> Option<SongSpeedView> {
        let observed = self.song_speed_observation();
        if observed.enabled.is_none() && self.prefs.song_speed.is_none() {
            return None;
        }
        let saved = self.prefs.song_speed;
        let custom = saved.map_or_else(
            || {
                let scaled = self.prefs.speed * 10.0;
                if (f64::from(MIN)..=f64::from(MAX)).contains(&scaled) && scaled.round() == scaled {
                    scaled as u8
                } else {
                    10
                }
            },
            |saved| saved.custom_tenths,
        );
        Some(SongSpeedView {
            enabled: observed.enabled == Some(true),
            revision: observed.revision.to_string(),
            custom_tenths: custom,
            custom_selected: saved.is_some_and(|saved| saved.custom_selected)
                && f64::from(custom) / 10.0 == self.prefs.speed,
            sync_failed: self.song_speed_sync_failed,
        })
    }

    pub(super) async fn song_speed_apply_observation(&mut self) {
        if self.session.check_generation(&self.token).is_err()
            || !self.state.active()
            || PlaybackSpeedScope::for_entry(self.queue.selected().as_ref())
                != PlaybackSpeedScope::Song
        {
            return;
        }
        let wanted = self.effective_speed(PlaybackSpeedScope::Song);
        if self.applied_speed == Some(wanted) {
            self.song_speed_sync_failed = false;
            return;
        }
        match super::engine(self.engine.clone(), self.guard(), move |engine| {
            engine.speed(wanted)
        })
        .await
        {
            Ok(()) => {
                self.applied_speed = Some(wanted);
                self.song_speed_sync_failed = false;
            }
            Err(_) => self.song_speed_sync_failed = true,
        }
    }

    pub(super) async fn song_speed_select(
        &mut self,
        expected_revision: u64,
        expected_entry: &str,
        selection: SongSpeedSelection,
    ) -> Result<()> {
        let observation = self.song_speed_observation();
        if observation.enabled != Some(true)
            || observation.revision != expected_revision
            || self
                .queue
                .selected()
                .as_ref()
                .is_none_or(|entry| entry.entry_id != expected_entry)
            || PlaybackSpeedScope::for_entry(self.queue.selected().as_ref())
                != PlaybackSpeedScope::Song
        {
            return Err(Error::StaleResponse);
        }
        let (tenths, custom_selected) = selection.checked()?;
        let mut next = self.prefs;
        let remembered = self
            .song_speed_view()
            .ok_or(Error::StaleResponse)?
            .custom_tenths;
        next.song_speed = Some(Saved {
            custom_tenths: if custom_selected { tenths } else { remembered },
            custom_selected,
        });
        next.set_speed(PlaybackSpeedScope::Song, f64::from(tenths) / 10.0);
        self.save_preferences(
            next,
            false,
            PlaybackSpeedScope::Song,
            self.state.active(),
            Some(observation),
        )
        .await
    }
}

pub(super) async fn changed(source: &mut Option<watch::Receiver<SongSpeedObservation>>) {
    if let Some(receiver) = source {
        if receiver.changed().await.is_err() {
            *source = None;
        }
    } else {
        std::future::pending().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::AccountSession,
        playback::{
            engine::{AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia},
            source::SourceResolver,
        },
        protocol::api::{Api, ApiFuture},
        storage::StateStore,
    };
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex, atomic::AtomicBool};
    use tokio::sync::mpsc;

    struct Closed;
    impl Api for Closed {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async { panic!("song rate never requests an API") })
        }
    }

    struct Rates(Arc<Mutex<Vec<f64>>>);
    impl AudioEngine for Rates {
        fn load(&mut self, _: LoadRequest, _: &Cancellation) -> Result<LoadedMedia> {
            panic!("changing rate must not load a decoder")
        }
        fn pause(&mut self, _: bool) -> Result<()> {
            panic!("changing rate must not pause or resume")
        }
        fn seek(&mut self, _: f64) -> Result<()> {
            panic!("changing rate must not seek")
        }
        fn volume(&mut self, _: f64) -> Result<()> {
            panic!("changing rate must not change volume")
        }
        fn speed(&mut self, rate: f64) -> Result<()> {
            self.0.lock().unwrap().push(rate);
            Ok(())
        }
        fn stop(&mut self) -> Result<()> {
            Ok(())
        }
        fn poll(&mut self) -> Result<EngineSnapshot> {
            panic!("this test directly exercises observed transitions")
        }
    }

    #[tokio::test]
    async fn song_speed_keeps_saved_choice_across_cohort_but_never_crosses_account() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let api = Arc::new(Closed);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
            .unwrap();
        let first = session.token().unwrap();
        let (updates, receive) = watch::channel(SongSpeedObservation {
            epoch: first.generation,
            revision: 10,
            enabled: Some(true),
        });
        let resolver = Arc::new(SourceResolver::new(session.clone(), None));
        let (send, _) = mpsc::unbounded_channel();
        let rates = Arc::new(Mutex::new(Vec::new()));
        let mut actor = Actor::new(
            session.clone(),
            store.clone(),
            resolver,
            Arc::new(Mutex::new(Box::new(Rates(rates.clone())))),
            send,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        actor.song_speed_observations = Some(receive);
        actor
            .queue
            .replace(vec![json!({"id":1})], 0, json!({}))
            .unwrap();
        actor.state = super::super::PlaybackStatus::Paused;
        actor.applied_speed = Some(1.0);
        let entry = actor.queue.selected().unwrap().entry_id;
        let custom = SongSpeedSelection::Custom { tenths: 13 };
        actor.song_speed_select(10, &entry, custom).await.unwrap();
        let saved = store.get("71", "playback", "preferences").unwrap().unwrap();
        assert_eq!(saved["speed"], 1.3);
        assert_eq!(saved["song_speed"]["custom_tenths"], 13);
        assert_eq!(saved["voice_speed"], 1.0);
        updates.send_replace(SongSpeedObservation {
            epoch: first.generation,
            revision: 11,
            enabled: Some(false),
        });
        actor.song_speed_apply_observation().await;
        assert!(matches!(
            actor.song_speed_select(10, &entry, custom).await,
            Err(Error::StaleResponse)
        ));
        assert_eq!(
            store.get("71", "playback", "preferences").unwrap(),
            Some(saved.clone())
        );
        updates.send_replace(SongSpeedObservation {
            epoch: first.generation,
            revision: 12,
            enabled: Some(true),
        });
        actor.song_speed_apply_observation().await;
        assert_eq!(*rates.lock().unwrap(), [1.3, 1.0, 1.3]);

        session.adopt(api, json!({"userId":72}), &first).unwrap();
        actor.rebind().await.unwrap();
        assert!(actor.song_speed_view().is_none());
        assert_eq!(actor.effective_speed(PlaybackSpeedScope::Song), 1.0);
        assert_eq!(actor.effective_speed(PlaybackSpeedScope::Voice), 1.0);
        assert!(matches!(
            actor.song_speed_select(12, &entry, custom).await,
            Err(Error::StaleResponse)
        ));
        assert_eq!(*rates.lock().unwrap(), [1.3, 1.0, 1.3]);
        assert_eq!(
            store.get("71", "playback", "preferences").unwrap(),
            Some(saved)
        );
        assert!(
            store
                .get("72", "playback", "preferences")
                .unwrap()
                .is_none()
        );
    }
}
