//! M/2120 subscribed-row count and the M/3117 explicit detail-entry clear.
//! Neither a generic read nor playback is a writer in this lane.
use super::{MusicClient, PodcastPage};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    podcast::{PodcastLibrary, PodcastSnapshot, Radio},
    protocol::api::{Api, PodcastBadgeClear},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Badge {
    count: u64,
    latest: Option<u64>,
}

fn number(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|value| *value <= 9_007_199_254_740_991)
}

fn badge_count(radio: &Radio) -> Option<u64> {
    // M/21.Z takes the first JS-truthy field. A malformed truthy value
    // is not evidence for a badge or permission to send a write.
    let raw = &radio.raw;
    if crate::protocol::wire::serialization::js_truthy(&raw["newVoiceCount"]) {
        number(&raw["newVoiceCount"])
    } else if raw["newProgramCount"].is_null() {
        number(&raw["newVoiceCount"])
    } else {
        number(&raw["newProgramCount"])
    }
}

fn badge(radio: &Radio) -> Option<Badge> {
    if radio.id.len() > 64 {
        return None;
    }
    let count = badge_count(radio)?;
    let raw = &radio.raw;
    (count > 0 && raw["grayDisplay"] != true && raw["underShelf"] != true).then(|| Badge {
        count,
        latest: number(&raw["lastProgramCreateTime"]),
    })
}

#[derive(Clone)]
struct Authority {
    api: Arc<dyn Api>,
    credential: String,
}

pub(super) struct Read {
    serial: u64,
    authority: Authority,
    fresh: bool,
}

struct Observed {
    receipt: String,
    snapshot: String,
    rows: BTreeMap<String, Badge>,
    authority: Authority,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attempt {
    Pending,
    Acknowledged,
    Uncertain,
}

struct Consumed {
    id: Uuid,
    fact: Badge,
    result: Attempt,
}

#[derive(Default)]
struct State {
    owner: Option<SessionToken>,
    serial: u64,
    native_snapshot: Option<(String, Authority)>,
    observed: Option<Observed>,
    consumed: BTreeMap<String, Consumed>,
}

#[derive(Default)]
pub(crate) struct Host(Mutex<State>);

impl Host {
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.0.lock().map_err(|_| Error::LockPoisoned)
    }

    pub(super) fn begin(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        fresh: bool,
    ) -> Result<Option<Read>> {
        let api = session.native_api(token)?;
        let Some(credential) = api.podcast_badge_revision()? else {
            return Ok(None);
        };
        let mut state = self.lock()?;
        if state.owner.as_ref() != Some(token) {
            *state = State {
                owner: Some(token.clone()),
                ..State::default()
            };
        }
        state.serial = state
            .serial
            .checked_add(1)
            .ok_or(Error::State("podcast badge read generation exhausted"))?;
        state.observed = None;
        Ok(Some(Read {
            serial: state.serial,
            authority: Authority { api, credential },
            fresh,
        }))
    }

    pub(super) fn project(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        read: Option<Read>,
        snapshot: &PodcastSnapshot,
        page: &mut PodcastPage,
    ) -> Result<()> {
        let (Some(read), Some(snapshot_id)) = (read, &snapshot.badge_observation) else {
            return Ok(());
        };
        let Authority { api, credential } = &read.authority;
        session.require_api(token, api)?;
        if api.podcast_badge_revision()?.as_deref() != Some(credential.as_str()) {
            return Ok(());
        }
        let mut state = self.lock()?;
        if state.owner.as_ref() != Some(token) || state.serial != read.serial {
            return Ok(());
        }
        if read.fresh {
            state.native_snapshot = Some((snapshot_id.clone(), read.authority.clone()));
            for radio in &snapshot.radios {
                if badge_count(radio) == Some(0) {
                    // Only an authoritative zero/new observation re-arms an
                    // old acknowledged/uncertain fact; never release a live write.
                    if state
                        .consumed
                        .get(&radio.id)
                        .is_some_and(|entry| entry.result != Attempt::Pending)
                    {
                        state.consumed.remove(&radio.id);
                    }
                }
            }
        } else if !state
            .native_snapshot
            .as_ref()
            .is_some_and(|(previous, authority)| {
                previous == snapshot_id
                    && Arc::ptr_eq(&authority.api, api)
                    && authority.credential == *credential
            })
        {
            return Ok(());
        }
        let mut rows = BTreeMap::new();
        let displayed: std::collections::BTreeSet<_> =
            page.cards.iter().map(|card| card.id.clone()).collect();
        for radio in snapshot
            .radios
            .iter()
            .filter(|radio| displayed.contains(&radio.id))
        {
            let Some(fact) = badge(radio) else { continue };
            if state
                .consumed
                .get(&radio.id)
                .is_some_and(|seen| seen.fact == fact && seen.result == Attempt::Acknowledged)
            {
                continue;
            }
            rows.insert(radio.id.clone(), fact);
        }
        if !rows.is_empty() {
            for card in &mut page.cards {
                card.new_episode_count = rows.get(&card.id).map(|fact| fact.count);
            }
            let receipt = Uuid::new_v4().simple().to_string();
            page.subscribed_badge_receipt = Some(receipt.clone());
            state.observed = Some(Observed {
                receipt,
                snapshot: snapshot_id.clone(),
                rows,
                authority: read.authority,
            });
        }
        Ok(())
    }

    fn claim<'a>(&'a self, token: &SessionToken, receipt: &str, radio: &str) -> Result<Claim<'a>> {
        let mut state = self.lock()?;
        if state.owner.as_ref() != Some(token) {
            return Err(Error::StaleResponse);
        }
        let page = state
            .observed
            .as_ref()
            .filter(|page| page.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        let fact = *page.rows.get(radio).ok_or(Error::StaleResponse)?;
        if state
            .consumed
            .get(radio)
            .is_some_and(|old| old.result == Attempt::Pending || old.fact == fact)
        {
            return Err(Error::StaleResponse);
        }
        let snapshot = page.snapshot.clone();
        let authority = page.authority.clone();
        if state.consumed.len() >= 50_000 && !state.consumed.contains_key(radio) {
            return Err(Error::State("podcast badge attempt limit reached"));
        }
        let id = Uuid::new_v4();
        state.consumed.insert(
            radio.into(),
            Consumed {
                id,
                fact,
                result: Attempt::Pending,
            },
        );
        Ok(Claim {
            host: self,
            token: token.clone(),
            receipt: receipt.into(),
            radio: radio.into(),
            snapshot,
            authority,
            id,
            dispatched: AtomicBool::new(false),
            acknowledged: false,
        })
    }
}

struct Claim<'a> {
    host: &'a Host,
    token: SessionToken,
    receipt: String,
    radio: String,
    snapshot: String,
    authority: Authority,
    id: Uuid,
    dispatched: AtomicBool,
    acknowledged: bool,
}

impl Claim<'_> {
    fn check(&self, session: &AccountSession, library: &PodcastLibrary) -> Result<()> {
        session.require_api(&self.token, &self.authority.api)?;
        if self.authority.api.podcast_badge_revision()?.as_deref()
            != Some(self.authority.credential.as_str())
        {
            return Err(Error::CredentialChanged);
        }
        if !library.badge_observation_is_current(&self.token, &self.snapshot)? {
            return Err(Error::StaleResponse);
        }
        let state = self.host.lock()?;
        if state.owner.as_ref() != Some(&self.token)
            || !state
                .observed
                .as_ref()
                .is_some_and(|page| page.receipt == self.receipt)
            || !state
                .consumed
                .get(&self.radio)
                .is_some_and(|attempt| attempt.id == self.id)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        let Ok(mut state) = self.host.lock() else {
            return;
        };
        if state.owner.as_ref() != Some(&self.token) {
            return;
        }
        let Some(attempt) = state
            .consumed
            .get_mut(&self.radio)
            .filter(|attempt| attempt.id == self.id)
        else {
            return;
        };
        if self.acknowledged {
            attempt.result = Attempt::Acknowledged;
        } else if self.dispatched.load(Ordering::Relaxed) {
            attempt.result = Attempt::Uncertain;
        } else {
            state.consumed.remove(&self.radio);
        }
    }
}

impl MusicClient {
    pub async fn podcast_clear_subscribed_badge(
        &self,
        epoch: &str,
        receipt: &str,
        radio: &str,
    ) -> Result<()> {
        let token = self.podcast_token(epoch)?;
        let write = PodcastBadgeClear::new(radio)?;
        let mut claim = self.podcast_badges.claim(&token, receipt, radio)?;
        let library = PodcastLibrary::new(self.session.clone())?;
        let store = self.session.persistent_store()?;
        let _lock = self.session.with_token(&token, |owner| {
            crate::storage::OperationLock::acquire(
                store.directory(),
                owner,
                &format!("podcast-badge:{radio}"),
            )
        })?;
        claim.check(&self.session, &library)?;
        let mut changes = self.session.subscribe()?;
        let result = {
            let permission = || claim.check(&self.session, &library);
            tokio::select! {
                biased;
                _ = changes.wait_for(|current| current != &token) => Err(Error::StaleResponse),
                result = claim.authority.api.clear_podcast_badge(
                    write, &claim.authority.credential, &permission, &claim.dispatched,
                ) => result,
            }
        };
        result?;
        self.session.require_api(&token, &claim.authority.api)?;
        claim.acknowledged = true;
        Ok(())
    }
}
