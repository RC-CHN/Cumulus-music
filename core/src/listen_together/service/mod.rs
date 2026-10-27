//! Account-scoped durable HTTP control. A lost write is reconciled by reads,
//! never replayed. IM connection state is deliberately not inferred from HTTP.
use super::{
    CreatedRoom, ListCommand, ListCommandKind, PlayCommand, PlaybackSync, Room, RoomRole,
    TogetherApi, TogetherSnapshot, Version, versions_equal,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

const STATE: &str = "listen-together-v1";
const JOURNAL: &str = "listen-together-operations-v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TogetherPhase {
    Sending,
    Verifying,
    Uncertain,
    Confirmed,
    Rejected,
    RoomClosed,
}
impl TogetherPhase {
    pub fn pending(self) -> bool {
        matches!(self, Self::Sending | Self::Verifying | Self::Uncertain)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WriteIntent {
    Create {
        refer: String,
    },
    Accept {
        room_id: String,
        inviter: String,
        refer: String,
    },
    End {
        room_id: String,
        count: Option<u64>,
    },
    Queue {
        room_id: String,
        command: ListCommand,
        expected_display: Vec<String>,
    },
    Play {
        room_id: String,
        command: PlayCommand,
    },
}
impl WriteIntent {
    fn room_id(&self) -> Option<&str> {
        match self {
            Self::Create { .. } => None,
            Self::Accept { room_id, .. }
            | Self::End { room_id, .. }
            | Self::Queue { room_id, .. }
            | Self::Play { room_id, .. } => Some(room_id),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TogetherOperation {
    pub id: String,
    pub intent: WriteIntent,
    pub phase: TogetherPhase,
    pub acknowledged: bool,
    /// Only a NEW_ROOM reply proves ownership by this creation attempt.
    pub created_room: Option<Room>,
    pub observed_room: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TogetherState {
    pub room: Option<Room>,
    pub role: Option<RoomRole>,
    pub snapshot: TogetherSnapshot,
    pub playback: PlaybackSync,
    pub waiting_until_ms: Option<u64>,
    pub next_heartbeat_ms: Option<u64>,
    pub checked_at_ms: Option<u64>,
    #[serde(default)]
    pub incoming: Option<IncomingPlayback>,
    #[serde(default)]
    pub shared: Option<SharedReception>,
    #[serde(default)]
    pub end_notice: Option<EndNotice>,
}
pub struct TogetherService {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    settle_delay: Duration,
}
impl TogetherService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            settle_delay: Duration::from_millis(400),
        })
    }
    pub fn with_settle_delay(mut self, delay: Duration) -> Self {
        self.settle_delay = delay;
        self
    }
    fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |owner| Ok(owner.into()))
    }
    fn lock(&self, owner: &str) -> Result<OperationLock> {
        OperationLock::acquire(self.store.directory(), owner, "listen-together-control")
    }
    pub fn state_for(&self, token: &SessionToken) -> Result<TogetherState> {
        self.session.with_token(token, |owner| {
            Ok(self
                .store
                .get(owner, STATE, "current")?
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default())
        })
    }
    pub fn operations_for(&self, token: &SessionToken) -> Result<Vec<TogetherOperation>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_values()
                .map(|v| serde_json::from_value(v).map_err(Into::into))
                .collect()
        })
    }
    fn save(
        &self,
        owner: &str,
        state: &TogetherState,
        operation: Option<&TogetherOperation>,
    ) -> Result<()> {
        self.store.transaction(|tx| {
            tx.put(owner, STATE, "current", &serde_json::to_value(state)?)?;
            if let Some(op) = operation {
                tx.put(owner, JOURNAL, &op.id, &serde_json::to_value(op)?)?;
            }
            Ok(())
        })
    }
    fn save_operation(&self, owner: &str, operation: &TogetherOperation) -> Result<()> {
        self.store.put(
            owner,
            JOURNAL,
            &operation.id,
            &serde_json::to_value(operation)?,
        )
    }
    fn require_resolved(&self, token: &SessionToken) -> Result<()> {
        if self
            .operations_for(token)?
            .iter()
            .any(|o| o.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
    fn require_outgoing_ready(&self, token: &SessionToken) -> Result<()> {
        self.require_active(token)?;
        self.require_resolved(token)?;
        self.require_shared_applied(token)?;
        if self
            .state_for(token)?
            .incoming
            .is_some_and(|v| matches!(v.phase, IncomingPhase::Pending | IncomingPhase::Failed))
        {
            return Err(Error::State("remote playback requires recovery"));
        }
        Ok(())
    }
    fn require_active(&self, token: &SessionToken) -> Result<()> {
        if self.state_for(token)?.end_notice.is_some() {
            return Err(Error::State("together room is ending"));
        }
        Ok(())
    }
    fn require_shared_applied(&self, token: &SessionToken) -> Result<()> {
        if self
            .state_for(token)?
            .shared
            .is_some_and(|v| v.phase != IncomingPhase::Applied)
        {
            return Err(Error::State("shared queue requires recovery"));
        }
        Ok(())
    }
    async fn status(&self, token: &SessionToken) -> Result<Option<Room>> {
        self.session
            .run_for(token, |api, owner| async move {
                let room = TogetherApi::new(api).status().await?;
                if let Some(r) = &room {
                    r.role_for(&owner)?;
                }
                Ok(room)
            })
            .await
    }
    fn require_room<'a>(&self, state: &'a TogetherState, expected: &str) -> Result<&'a Room> {
        state
            .room
            .as_ref()
            .filter(|r| r.room_id == expected)
            .ok_or(Error::State("together room changed; refresh first"))
    }
    fn observe(state: &mut TogetherState, room: Option<Room>, owner: &str, now: u64) -> Result<()> {
        let changed = state.room.as_ref().map(|r| &r.room_id) != room.as_ref().map(|r| &r.room_id);
        if changed {
            *state = TogetherState::default();
        }
        state.role = room.as_ref().map(|r| r.role_for(owner)).transpose()?;
        state.waiting_until_ms = if state.role == Some(RoomRole::Waiting) {
            Some(
                state
                    .waiting_until_ms
                    .unwrap_or(now.saturating_add(600_000)),
            )
        } else {
            None
        };
        state.next_heartbeat_ms = room.as_ref().map(|_| {
            state
                .next_heartbeat_ms
                .unwrap_or(now.saturating_add(20_000))
        });
        state.room = room;
        state.checked_at_ms = Some(now);
        Ok(())
    }
    async fn snapshot(&self, token: &SessionToken, room: &str) -> Result<TogetherSnapshot> {
        self.session
            .run_for(token, |api, _| async move {
                TogetherApi::new(api).snapshot(room).await
            })
            .await
    }
    /// Explicit read refresh; never ends an existing single-member room.
    pub async fn refresh_for(&self, token: &SessionToken, now: u64) -> Result<TogetherState> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut state = self.state_for(token)?;
        let room = self.status(token).await?;
        Self::observe(&mut state, room, &owner, now)?;
        if let Some(room) = &state.room {
            state.snapshot = self.snapshot(token, &room.room_id).await?;
            if let Some(command) = &state.snapshot.play_command {
                state.playback.restore(command.clone(), &owner, now)?;
            }
        }
        self.session
            .with_token(token, |owner| self.save(owner, &state, None))?;
        Ok(state)
    }
}

mod commands;
mod incoming;
mod lifecycle;
pub use lifecycle::EndNotice;
mod shared;
pub use incoming::{IncomingPhase, IncomingPlayback};
pub use shared::SharedReception;
mod reconcile;
mod timer;
