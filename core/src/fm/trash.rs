//! Reduce recommendations and batch restoration. Mutations are serialized per
//! account; uncertain writes can only be reconciled by complete list reads.
use super::*;
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

const JOURNAL: &str = "fm-trash-mutations-v1";
fn valid_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() || ids.len() > 100 || ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(Error::InvalidInput("invalid FM trash selection"));
    }
    for id in ids {
        identifier(&json!(id))?;
    }
    Ok(())
}
impl FmService {
    fn trash_gate(&self, token: &SessionToken) -> Result<OperationLock> {
        OperationLock::acquire(self.store.directory(), JOURNAL, &self.owner(token)?)
    }
    fn trash_record(&self, owner: &str, id: &str) -> Result<Option<FmTrashMutation>> {
        self.store
            .get(owner, JOURNAL, id)?
            .map(|value| {
                let r: FmTrashMutation = serde_json::from_value(value)?;
                valid_ids(&r.song_ids)?;
                if r.id != id
                    || uuid::Uuid::parse_str(id).is_err()
                    || r.before.len() != r.song_ids.len()
                    || r.observed
                        .as_ref()
                        .is_some_and(|v| v.len() != r.song_ids.len())
                    || r.seconds > u32::MAX as u64
                    || (r.desired && (r.song_ids.len() != 1 || r.algorithm.is_none()))
                    || (r.artist_id.is_some() && !r.desired)
                {
                    return Err(Error::State("invalid FM trash journal identity"));
                }
                if let Some(id) = &r.artist_id {
                    identifier(&json!(id))?;
                }
                Ok(r)
            })
            .transpose()
    }
    pub fn trash_operation_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<FmTrashMutation>> {
        self.session
            .with_token(token, |owner| self.trash_record(owner, id))
    }
    pub fn trash_operations_for(&self, token: &SessionToken) -> Result<Vec<FmTrashMutation>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_keys()
                .map(|id| {
                    self.trash_record(owner, &id)?
                        .ok_or(Error::State("FM trash journal disappeared"))
                })
                .collect()
        })
    }
    pub async fn trash_page_for(
        &self,
        token: &SessionToken,
        page: usize,
        size: usize,
        add_time: Value,
    ) -> Result<FmTrashPage> {
        self.session
            .run_for(token, |api, _| async move {
                FmApi::new(api).trash_page(page, size, add_time).await
            })
            .await
    }
    pub async fn reasons_for(&self, token: &SessionToken, song_id: &str) -> Result<Value> {
        self.session
            .run_for(token, |api, _| async {
                FmApi::new(api).reasons(song_id).await
            })
            .await
    }
    async fn read_trash(&self, token: &SessionToken) -> Result<FmTrashSnapshot> {
        self.session
            .run_for(token, |api, _| async {
                FmApi::new(api).trash_snapshot().await
            })
            .await
    }
    pub async fn refresh_trash_for(&self, token: &SessionToken) -> Result<FmTrashSnapshot> {
        let _gate = self.trash_gate(token)?;
        let snapshot = self.read_trash(token).await?;
        self.session.with_token(token, |owner| {
            self.store.put(
                owner,
                service::TRASH_CACHE,
                "current",
                &serde_json::to_value(&snapshot)?,
            )
        })?;
        Ok(snapshot)
    }
    /// Resolve the track from the native FM revision. IPC cannot supply an
    /// algorithm or elapsed time in place of the native selection/clock.
    pub async fn trash_current_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
        observation: Option<&FmObservation>,
    ) -> Result<FmTrashMutation> {
        self.trash_reason_for(token, source, revision, observation, None)
            .await
    }
    pub async fn trash_reason_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        revision: u64,
        observation: Option<&FmObservation>,
        reason_key: Option<&str>,
    ) -> Result<FmTrashMutation> {
        let _queue_gate = self.gate(token, source)?;
        let queue = self.required(token, source, revision)?;
        let track = queue
            .current()
            .ok_or(Error::State("FM has no current track"))?;
        let seconds = observation
            .map(|o| o.seconds_for(track))
            .transpose()?
            .unwrap_or(0);
        let artist = reason_key
            .map(|key| self.reason_for(token, track, key))
            .transpose()?
            .and_then(|r| r.artist_id);
        self.change_trash(
            token,
            vec![track.id.clone()],
            true,
            Some(track.algorithm.clone()),
            seconds,
            artist,
        )
        .await
    }
    pub async fn restore_trash_for(
        &self,
        token: &SessionToken,
        song_ids: Vec<String>,
    ) -> Result<FmTrashMutation> {
        self.change_trash(token, song_ids, false, None, 0, None)
            .await
    }
    async fn change_trash(
        &self,
        token: &SessionToken,
        song_ids: Vec<String>,
        desired: bool,
        algorithm: Option<String>,
        seconds: u64,
        artist_id: Option<String>,
    ) -> Result<FmTrashMutation> {
        valid_ids(&song_ids)?;
        let owner = self.owner(token)?;
        let _gate = self.trash_gate(token)?;
        if self
            .trash_operations_for(token)?
            .iter()
            .any(|r| r.phase.pending() && r.song_ids.iter().any(|id| song_ids.contains(id)))
        {
            return Err(Error::MutationUncertain);
        }
        let snapshot = self.read_trash(token).await?;
        let before: Vec<bool> = song_ids
            .iter()
            .map(|id| snapshot.ids.contains(id))
            .collect();
        let unchanged = before.iter().all(|&v| v == desired);
        let mut record = FmTrashMutation {
            id: uuid::Uuid::new_v4().simple().to_string(),
            song_ids,
            desired,
            before: before.clone(),
            created_at: self.clock.now_ms()?,
            phase: if unchanged {
                MutationPhase::Unchanged
            } else {
                MutationPhase::Sending
            },
            observed: Some(before),
            algorithm,
            seconds,
            artist_id,
        };
        self.save_trash(token, &record, &snapshot)?;
        if unchanged {
            return Ok(record);
        }
        let result = self
            .session
            .run_for(token, |api, _| async {
                let api = FmApi::new(api);
                if desired {
                    api.trash(
                        &record.song_ids[0],
                        record.algorithm.as_deref().unwrap_or_default(),
                        seconds,
                    )
                    .await
                } else {
                    let changed: Vec<String> = record
                        .song_ids
                        .iter()
                        .zip(&record.before)
                        .filter(|(_, v)| **v)
                        .map(|(id, _)| id.clone())
                        .collect();
                    api.restore(&changed).await
                }
            })
            .await;
        record.phase = match &result {
            Ok(()) => MutationPhase::Verifying,
            Err(e) if feedback::rejected(e) => MutationPhase::Rejected,
            Err(_) => MutationPhase::Uncertain,
        };
        self.persist_original(&owner, JOURNAL, &record.id, serde_json::to_value(&record)?)?;
        result?;
        self.settle_trash(token, record).await
    }
    pub async fn reconcile_trash_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<FmTrashMutation> {
        let _gate = self.trash_gate(token)?;
        let record = self
            .trash_operation_for(token, id)?
            .ok_or(Error::State("FM trash operation missing"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        self.settle_trash(token, record).await
    }
    fn save_trash(
        &self,
        token: &SessionToken,
        record: &FmTrashMutation,
        snapshot: &FmTrashSnapshot,
    ) -> Result<()> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                tx.put(owner, JOURNAL, &record.id, &serde_json::to_value(record)?)?;
                tx.put(
                    owner,
                    service::TRASH_CACHE,
                    "current",
                    &serde_json::to_value(snapshot)?,
                )
            })
        })
    }
    async fn settle_trash(
        &self,
        token: &SessionToken,
        mut record: FmTrashMutation,
    ) -> Result<FmTrashMutation> {
        let owner = self.owner(token)?;
        let mut consecutive = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let snapshot = match self.read_trash(token).await {
                Ok(v) => v,
                Err(error) => {
                    record.phase = MutationPhase::Uncertain;
                    self.persist_original(
                        &owner,
                        JOURNAL,
                        &record.id,
                        serde_json::to_value(&record)?,
                    )?;
                    return Err(error);
                }
            };
            let observed: Vec<bool> = record
                .song_ids
                .iter()
                .map(|id| snapshot.ids.contains(id))
                .collect();
            consecutive = if observed.iter().all(|&v| v == record.desired) {
                consecutive + 1
            } else {
                0
            };
            record.observed = Some(observed);
            record.phase = if consecutive >= 2 {
                MutationPhase::Confirmed
            } else {
                MutationPhase::Uncertain
            };
            self.save_trash(token, &record, &snapshot)?;
            if record.phase == MutationPhase::Confirmed {
                return Ok(record);
            }
        }
        Err(Error::MutationUncertain)
    }
}
