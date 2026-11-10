//! Native receipts for the exact episode offered by a podcast browse read.
use super::EpisodeView;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    podcast::Episode,
    protocol::api::Api,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

const LANES: usize = 16;
const ROWS: usize = 2_048;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::client) enum Lane {
    Weekly,
    Rank {
        id: String,
        chart_type: String,
        preview: bool,
    },
}

impl Lane {
    pub(in crate::client) fn is_full_rank(&self) -> bool {
        matches!(self, Self::Rank { preview: false, .. })
    }

    pub(in crate::client) fn source(&self) -> &'static str {
        match self {
            Self::Weekly => "podcast_weekly",
            Self::Rank { preview: true, .. } => "podcast_rank_preview",
            Self::Rank { preview: false, .. } => "podcast_rank",
        }
    }
}

#[derive(Clone)]
pub(in crate::client) struct Target {
    pub(in crate::client) id: String,
    pub(in crate::client) song: Option<String>,
    pub(in crate::client) radio: Option<String>,
}

#[derive(Clone)]
pub(in crate::client) struct Observation {
    pub(in crate::client) lane: Lane,
    pub(super) visit: uuid::Uuid,
    token: SessionToken,
    api: Weak<dyn Api>,
    credential: Option<String>,
    live: Arc<AtomicBool>,
}

impl Observation {
    pub(in crate::client) fn current(&self, token: &SessionToken, api: &Arc<dyn Api>) -> bool {
        self.live.load(Ordering::Acquire)
            && api
                .playback_source_credential_revision()
                .is_ok_and(|credential| self.matches(token, api, &credential))
    }

    fn matches(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credential: &Option<String>,
    ) -> bool {
        &self.token == token
            && self.api.ptr_eq(&Arc::downgrade(api))
            && credential == &self.credential
    }
}

struct Visit {
    observation: Observation,
    rows: HashMap<String, Target>,
    // Only the full voice table offers double click with its read order.
    chart: Vec<Episode>,
}

impl Drop for Visit {
    fn drop(&mut self) {
        self.observation.live.store(false, Ordering::Release);
    }
}

#[derive(Default)]
pub(in crate::client) struct Host(Mutex<VecDeque<Visit>>);

impl Host {
    pub(super) fn begin(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        lane: Lane,
        continue_existing: bool,
    ) -> Result<Observation> {
        // Serialize issuance with the native account/profile. A delayed
        // older epoch may never evict the newer account's live browse shelf.
        session.with_verified_profile(token, api, |_, _| {
            let credential = api.playback_source_credential_revision()?;
            let mut visits = self.0.lock().map_err(|_| Error::LockPoisoned)?;
            visits.retain(|visit| visit.observation.matches(token, api, &credential));
            if continue_existing {
                return visits
                    .iter()
                    .find(|visit| visit.observation.lane == lane)
                    .map(|visit| visit.observation.clone())
                    .ok_or(Error::StaleResponse);
            }
            visits.retain(|visit| {
                visit.observation.lane != lane
                    && !(lane.is_full_rank() && visit.observation.lane.is_full_rank())
            });
            if visits.len() == LANES {
                visits.pop_front();
            }
            let observation = Observation {
                lane,
                visit: uuid::Uuid::new_v4(),
                token: token.clone(),
                api: Arc::downgrade(api),
                credential,
                live: Arc::new(AtomicBool::new(true)),
            };
            visits.push_back(Visit {
                observation: observation.clone(),
                rows: HashMap::new(),
                chart: Vec::new(),
            });
            Ok(observation)
        })
    }

    pub(super) fn retire(&self, observation: &Observation) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .retain(|visit| !Arc::ptr_eq(&visit.observation.live, &observation.live));
        Ok(())
    }

    pub(super) fn retire_full_rank(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<()> {
        session.with_verified_profile(token, api, |_, _| {
            self.0
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .retain(|visit| !visit.observation.lane.is_full_rank());
            Ok(())
        })
    }

    pub(super) fn bind(
        &self,
        observation: &Observation,
        episode: &Episode,
        view: &mut EpisodeView,
    ) -> Option<()> {
        if view.unavailable {
            return Some(());
        }
        let mut visits = self.0.lock().ok()?;
        let visit = visits
            .iter_mut()
            .find(|visit| Arc::ptr_eq(&visit.observation.live, &observation.live))?;
        if visit.rows.len() < ROWS {
            let receipt = uuid::Uuid::new_v4().simple().to_string();
            visit.rows.insert(
                receipt.clone(),
                Target {
                    id: episode.id.clone(),
                    song: episode.song_id.clone(),
                    radio: episode.radio.as_ref().map(|radio| radio.id.clone()),
                },
            );
            if observation.lane.is_full_rank() {
                visit.chart.push(episode.clone());
            }
            view.browse_playback_receipt = Some(receipt);
        }
        Some(())
    }

    pub(in crate::client) fn resolve(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        receipt: &str,
    ) -> Result<(Observation, Target)> {
        if receipt.len() != 32 {
            return Err(Error::StaleResponse);
        }
        let value = self
            .0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .find_map(|visit| {
                visit
                    .rows
                    .get(receipt)
                    .map(|target| (visit.observation.clone(), target.clone()))
            })
            .ok_or(Error::StaleResponse)?;
        if !value.0.current(token, api) {
            return Err(Error::StaleResponse);
        }
        Ok(value)
    }

    pub(in crate::client) fn full_rank(&self, observation: &Observation) -> Result<Vec<Episode>> {
        if !observation.lane.is_full_rank() {
            return Err(Error::InvalidInput(
                "double click requires a full voice rank",
            ));
        }
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .find(|visit| Arc::ptr_eq(&visit.observation.live, &observation.live))
            .map(|visit| visit.chart.clone())
            .ok_or(Error::StaleResponse)
    }
}
