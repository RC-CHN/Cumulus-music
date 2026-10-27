use super::{
    MembershipApi, MembershipState,
    clock::{MembershipClock, SystemMembershipClock},
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
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    data: Value,
    server_ms: f64,
    wall: f64,
}
struct Cached {
    record: Record,
    anchor: f64,
    token: SessionToken,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipSnapshot {
    #[serde(flatten)]
    pub state: MembershipState,
    pub server_now: f64,
    pub refresh_after_ms: Option<f64>,
}

pub struct MembershipManager {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    clock: Arc<dyn MembershipClock>,
    state: Mutex<Option<Cached>>,
    refresh_gate: tokio::sync::Mutex<()>,
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
        let _guard = self.refresh_gate.lock().await;
        let data = self
            .session
            .run_for(&token, |api, uid| async move {
                MembershipApi::new(api).info(Some(&json!(uid))).await
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
        self.session.with_token(&token, |uid| {
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            let candidate = Cached {
                record,
                anchor: clock.monotonic_seconds,
                token: token.clone(),
            };
            let snapshot = Self::snapshot_from(&candidate, clock.monotonic_seconds)?;
            self.store.put(
                uid,
                "membership",
                "current",
                &serde_json::to_value(&candidate.record)?,
            )?;
            *state = Some(candidate);
            Ok(snapshot)
        })
    }
    fn view(&self) -> Result<(SessionToken, MembershipSnapshot, [bool; 5])> {
        let token = self.token()?;
        let clock = self.clock.now()?.validate()?;
        let (snapshot, baseline) = self.session.with_token(&token, |uid| {
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            if !state.as_ref().is_some_and(|cached| cached.token == token) {
                let mut record: Record =
                    serde_json::from_value(self.store.get(uid, "membership", "current")?.ok_or(
                        Error::Response("membership has not been refreshed for this account"),
                    )?)?;
                if !record.wall.is_finite()
                    || !record.server_ms.is_finite()
                    || record.server_ms <= 0.0
                {
                    return Err(Error::Response("invalid stored membership clock"));
                }
                if let Some(id) = record.data.get("userId").filter(|v| !v.is_null())
                    && identifier(id)? != uid
                {
                    return Err(Error::Response(
                        "stored membership belongs to another account",
                    ));
                }
                let elapsed = clock.wall_seconds - record.wall;
                if elapsed < -5.0 {
                    return Err(Error::Response(
                        "clock moved backwards; refresh membership online",
                    ));
                }
                record.server_ms += elapsed.max(0.0) * 1000.0;
                membership_state(&record.data, record.server_ms)?;
                *state = Some(Cached {
                    record,
                    anchor: clock.monotonic_seconds,
                    token: token.clone(),
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
