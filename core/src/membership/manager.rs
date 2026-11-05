use super::{
    MembershipApi, MembershipState,
    clock::{ClockReading, MembershipClock, SystemMembershipClock},
    membership_state,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionStatus, SessionToken},
    protocol::api::identifier,
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Record {
    pub(super) data: Value,
    pub(super) server_ms: f64,
    wall: f64,
}
impl Record {
    pub(super) fn restore(
        mut self,
        uid: &str,
        clock: ClockReading,
        rollback_tolerance: f64,
    ) -> Result<Self> {
        if !self.wall.is_finite() || !self.server_ms.is_finite() || self.server_ms <= 0.0 {
            return Err(Error::Response("invalid stored membership clock"));
        }
        if let Some(id) = self.data.get("userId").filter(|v| !v.is_null())
            && identifier(id)? != uid
        {
            return Err(Error::Response(
                "stored membership belongs to another account",
            ));
        }
        let elapsed = clock.wall_seconds - self.wall;
        if elapsed < -rollback_tolerance {
            return Err(Error::Response(
                "clock moved backwards; refresh membership online",
            ));
        }
        self.server_ms += elapsed.max(0.0) * 1000.0;
        membership_state(&self.data, self.server_ms)?;
        Ok(self)
    }
}
struct Cached {
    record: Record,
    anchor: f64,
    token: SessionToken,
    credentials: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipSnapshot {
    #[serde(flatten)]
    pub state: MembershipState,
    pub server_now: f64,
    pub refresh_after_ms: Option<f64>,
}

#[derive(Clone)]
pub(crate) struct MembershipCommit {
    pub(crate) token: SessionToken,
    pub(crate) expiries: [f64; 5],
    pub(crate) server_now: f64,
    pub(crate) at: tokio::time::Instant,
}

pub(super) fn expiries(state: &MembershipState) -> [f64; 5] {
    [
        state.vinyl_vip_expire_time,
        state.plus_vip_expire_time,
        state.music_package_expire_time,
        state.album_vip_expire_time,
        state.voice_book_vip_expire_time,
    ]
}

pub struct MembershipManager {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    clock: Arc<dyn MembershipClock>,
    state: Mutex<Option<Cached>>,
    refresh_gate: tokio::sync::Mutex<()>,
    completed: AtomicU64,
}
impl MembershipManager {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_clock(session, Arc::new(SystemMembershipClock::default()))
    }
    pub fn with_clock(
        session: Arc<AccountSession>,
        clock: Arc<dyn MembershipClock>,
    ) -> Result<Self> {
        let store = session.persistent_store()?;
        Ok(Self {
            session,
            store,
            clock,
            state: Mutex::new(None),
            refresh_gate: tokio::sync::Mutex::new(()),
            completed: AtomicU64::new(0),
        })
    }
    fn token(&self) -> Result<SessionToken> {
        let snapshot = self.session.snapshot()?;
        if snapshot.status != SessionStatus::Authenticated || snapshot.token.uid.is_none() {
            return Err(Error::StaleResponse);
        }
        Ok(snapshot.token)
    }
    fn snapshot_from(cached: &Cached, monotonic: f64) -> Result<MembershipSnapshot> {
        let now = cached.record.server_ms + (monotonic - cached.anchor).max(0.0) * 1000.0;
        let state = membership_state(&cached.record.data, now)?;
        let next = state
            .delta_expire_time_map
            .values()
            .copied()
            .filter(|v| *v > 0.0)
            .reduce(f64::min)
            .map(|v| v + 1000.0);
        Ok(MembershipSnapshot {
            state,
            server_now: now,
            refresh_after_ms: next,
        })
    }
    pub async fn refresh(&self) -> Result<MembershipSnapshot> {
        self.refresh_for(self.token()?).await
    }
    pub(crate) async fn refresh_for(&self, token: SessionToken) -> Result<MembershipSnapshot> {
        let observed = self.completed.load(Ordering::Acquire);
        let _guard = self.refresh_gate.lock().await;
        let api = self.session.native_api(&token)?;
        let credentials = api.membership_credential_revision()?;
        if observed != self.completed.load(Ordering::Acquire) {
            let clock = self.clock.now()?.validate()?;
            let current = self.session.with_verified_profile(&token, &api, |_, _| {
                let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                state
                    .as_ref()
                    .filter(|cached| cached.token == token && cached.credentials == credentials)
                    .map(|cached| Self::snapshot_from(cached, clock.monotonic_seconds))
                    .transpose()
            })?;
            if let Some(current) = current {
                return Ok(current);
            }
        }
        let expected_api = &api;
        let expected_credentials = credentials.as_deref();
        let data = self
            .session
            .run_for(&token, |current, uid| async move {
                if !Arc::ptr_eq(expected_api, &current)
                    || current.membership_credential_revision()?.as_deref() != expected_credentials
                {
                    return Err(Error::StaleResponse);
                }
                MembershipApi::new(current).info(Some(&json!(uid))).await
            })
            .await?;
        let now = data["now"]
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or(Error::Response("membership has no valid server timestamp"))?;
        if let Some(uid) = data.get("userId").filter(|v| !v.is_null())
            && Some(identifier(uid)?) != token.uid
        {
            return Err(Error::Response("membership returned another account"));
        }
        membership_state(&data, now)?;
        let clock = self.clock.now()?.validate()?;
        let record = Record {
            data,
            server_ms: now,
            wall: clock.wall_seconds,
        };
        let candidate = Cached {
            record,
            anchor: clock.monotonic_seconds,
            token: token.clone(),
            credentials: credentials.clone(),
        };
        let snapshot = Self::snapshot_from(&candidate, clock.monotonic_seconds)?;
        let rights = &snapshot.state;
        let commit = MembershipCommit {
            token: token.clone(),
            expiries: expiries(rights),
            server_now: snapshot.server_now,
            at: tokio::time::Instant::now(),
        };
        // M/3095 watches active booleans and vipUserId, not dates.
        let tuple = (
            [
                rights.is_album_vip,
                rights.is_plus_vip,
                rights.is_vinyl_vip,
                rights.is_music_package,
                rights.is_voice_book_vip,
            ],
            rights.user_id.clone(),
        );
        self.session.commit_membership(
            &token,
            &api,
            credentials.as_deref(),
            commit,
            tuple,
            |uid| {
                let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                self.store.put(
                    uid,
                    "membership",
                    "current",
                    &serde_json::to_value(&candidate.record)?,
                )?;
                *state = Some(candidate);
                Ok(())
            },
        )?;
        self.completed.fetch_add(1, Ordering::Release);
        Ok(snapshot)
    }
    fn view(&self) -> Result<(SessionToken, MembershipSnapshot, [bool; 5])> {
        let token = self.token()?;
        let clock = self.clock.now()?.validate()?;
        let (snapshot, baseline) = self.session.with_token(&token, |uid| {
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            if !state.as_ref().is_some_and(|cached| cached.token == token) {
                let record: Record =
                    serde_json::from_value(self.store.get(uid, "membership", "current")?.ok_or(
                        Error::Response("membership has not been refreshed for this account"),
                    )?)?;
                let record = record.restore(uid, clock, 5.0)?;
                *state = Some(Cached {
                    record,
                    anchor: clock.monotonic_seconds,
                    token: token.clone(),
                    credentials: None,
                });
            }
            let cached = state
                .as_ref()
                .ok_or(Error::State("missing membership anchor"))?;
            Ok((
                Self::snapshot_from(cached, clock.monotonic_seconds)?,
                membership_state(&cached.record.data, cached.record.server_ms)?.active_flags(),
            ))
        })?;
        Ok((token, snapshot, baseline))
    }
    pub fn snapshot(&self) -> Result<MembershipSnapshot> {
        Ok(self.view()?.1)
    }
    pub async fn refresh_if_due(&self) -> Result<MembershipSnapshot> {
        let (token, snapshot, baseline) = self.view()?;
        if baseline != snapshot.state.active_flags() {
            self.refresh_for(token).await
        } else {
            Ok(snapshot)
        }
    }
}
