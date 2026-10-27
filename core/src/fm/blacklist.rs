//! Independent artist blacklist writes. A song-trash operation may own one
//! such write, but its acknowledged song result never implies artist success.
use super::*;
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde_json::{Value, json};
use std::time::Duration;
const JOURNAL: &str = "fm-artist-mutations-v1";
const LATEST: &str = "fm-artist-latest-v1";
impl FmService {
    pub(super) fn artist_gate(&self, token: &SessionToken) -> Result<OperationLock> {
        OperationLock::acquire(self.store.directory(), JOURNAL, &self.owner(token)?)
    }
    fn decode_artist(value: Value, id: &str) -> Result<FmArtistMutation> {
        let record: FmArtistMutation = serde_json::from_value(value)?;
        identifier(&json!(record.artist_id))?;
        if record.id != id || uuid::Uuid::parse_str(id).is_err() {
            return Err(Error::State("invalid FM artist journal identity"));
        }
        Ok(record)
    }
    pub fn artist_operation_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<FmArtistMutation>> {
        self.session.with_token(token, |owner| {
            self.store
                .get(owner, JOURNAL, id)?
                .map(|v| Self::decode_artist(v, id))
                .transpose()
        })
    }
    pub fn artist_operations_for(&self, token: &SessionToken) -> Result<Vec<FmArtistMutation>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_iter()
                .map(|(id, value)| Self::decode_artist(value, &id))
                .collect()
        })
    }
    pub fn artist_latest_for(&self, token: &SessionToken, id: &str) -> Result<Option<String>> {
        self.session.with_token(token, |owner| {
            Ok(self
                .store
                .get(owner, LATEST, id)?
                .and_then(|v| v["id"].as_str().map(str::to_owned)))
        })
    }
    fn save_artist(
        &self,
        token: &SessionToken,
        r: &FmArtistMutation,
        snapshot: &FmBlacklist,
        new: bool,
    ) -> Result<()> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                tx.put(owner, JOURNAL, &r.id, &serde_json::to_value(r)?)?;
                tx.put(
                    owner,
                    service::BLACKLIST,
                    "current",
                    &serde_json::to_value(snapshot)?,
                )?;
                if new {
                    tx.put(owner, LATEST, &r.artist_id, &json!({"id":r.id}))?;
                }
                Ok(())
            })
        })
    }
    async fn read_artists(&self, token: &SessionToken) -> Result<FmBlacklist> {
        self.session
            .run_for(token, |api, _| async { FmApi::new(api).blacklist().await })
            .await
    }
    pub async fn change_artist_for(
        &self,
        token: &SessionToken,
        artist_id: &str,
        desired: bool,
    ) -> Result<FmArtistMutation> {
        self.change_artist_id(
            token,
            &uuid::Uuid::new_v4().simple().to_string(),
            artist_id,
            desired,
        )
        .await
    }
    async fn change_artist_id(
        &self,
        token: &SessionToken,
        id: &str,
        artist_id: &str,
        desired: bool,
    ) -> Result<FmArtistMutation> {
        identifier(&json!(artist_id))?;
        uuid::Uuid::parse_str(id)
            .map_err(|_| Error::InvalidInput("invalid FM artist operation"))?;
        let owner = self.owner(token)?;
        let _gate = self.artist_gate(token)?;
        if let Some(r) = self.artist_operation_for(token, id)? {
            if r.artist_id != artist_id || r.desired != desired {
                return Err(Error::InvalidInput("FM artist operation changed"));
            }
            return Ok(r);
        }
        if self
            .artist_operations_for(token)?
            .iter()
            .any(|r| r.artist_id == artist_id && r.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let snapshot = self.read_artists(token).await?;
        let before = snapshot.artists.contains(artist_id);
        let mut r = FmArtistMutation {
            id: id.into(),
            artist_id: artist_id.into(),
            desired,
            before,
            observed: Some(before),
            created_at: self.clock.now_ms()?,
            phase: if before == desired {
                MutationPhase::Unchanged
            } else {
                MutationPhase::Sending
            },
        };
        self.save_artist(token, &r, &snapshot, true)?;
        if before == desired {
            return Ok(r);
        }
        let result = self
            .session
            .run_for(token, |api, _| async {
                FmApi::new(api).artist_blacklist(artist_id, desired).await
            })
            .await;
        r.phase = match &result {
            Ok(()) => MutationPhase::Verifying,
            Err(e) if feedback::rejected(e) => MutationPhase::Rejected,
            Err(_) => MutationPhase::Uncertain,
        };
        self.persist_original(&owner, JOURNAL, id, serde_json::to_value(&r)?)?;
        result?;
        self.settle_artist(token, r).await
    }
    pub async fn reconcile_artist_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<FmArtistMutation> {
        let _gate = self.artist_gate(token)?;
        let r = self
            .artist_operation_for(token, id)?
            .ok_or(Error::State("FM artist operation missing"))?;
        if !r.phase.pending() {
            return Ok(r);
        }
        self.settle_artist(token, r).await
    }
    async fn settle_artist(
        &self,
        token: &SessionToken,
        mut r: FmArtistMutation,
    ) -> Result<FmArtistMutation> {
        let owner = self.owner(token)?;
        let mut consecutive = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let snapshot = match self.read_artists(token).await {
                Ok(v) => v,
                Err(e) => {
                    r.phase = MutationPhase::Uncertain;
                    self.persist_original(&owner, JOURNAL, &r.id, serde_json::to_value(&r)?)?;
                    return Err(e);
                }
            };
            let observed = snapshot.artists.contains(&r.artist_id);
            consecutive = if observed == r.desired {
                consecutive + 1
            } else {
                0
            };
            r.observed = Some(observed);
            r.phase = if consecutive >= 2 {
                MutationPhase::Confirmed
            } else {
                MutationPhase::Uncertain
            };
            self.save_artist(token, &r, &snapshot, false)?;
            if r.phase == MutationPhase::Confirmed {
                return Ok(r);
            }
        }
        Err(Error::MutationUncertain)
    }
    /// Called by the native coordinator, or explicit recovery. Never replays
    /// a write whose deterministic child ID already has a journal entry.
    pub async fn finish_trash_artist_for(
        &self,
        token: &SessionToken,
        r: &FmTrashMutation,
    ) -> Result<()> {
        let Some(artist) = &r.artist_id else {
            return Ok(());
        };
        if !matches!(r.phase, MutationPhase::Confirmed | MutationPhase::Unchanged) {
            return Err(Error::MutationUncertain);
        }
        let child = match self.artist_operation_for(token, &r.id)? {
            Some(child) if child.phase.pending() => self.reconcile_artist_for(token, &r.id).await?,
            Some(child) => child,
            None => self.change_artist_id(token, &r.id, artist, true).await?,
        };
        match child.phase {
            MutationPhase::Confirmed | MutationPhase::Unchanged => Ok(()),
            MutationPhase::Rejected => Err(Error::State("FM artist change rejected")),
            _ => Err(Error::MutationUncertain),
        }
    }
}
