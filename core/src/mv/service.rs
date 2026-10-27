//! Account-scoped metadata and playback checkpoints. Authorized URLs remain
//! ephemeral native values, never metadata-cache entries or frontend DTOs.
use super::{MvApi, MvDetail, MvPurpose};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const DETAILS: &str = "mv-details-v1";
const POSITIONS: &str = "mv-positions-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MvPosition {
    pub id: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub completed: bool,
    pub updated_ms: u64,
}

pub struct MvService {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
    pub(super) settle_delay: std::time::Duration,
}
impl MvService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            settle_delay: std::time::Duration::from_millis(500),
        })
    }
    pub fn with_settle_delay(mut self, delay: std::time::Duration) -> Self {
        self.settle_delay = delay;
        self
    }
    pub async fn detail_for(&self, token: &SessionToken, id: &Value) -> Result<MvDetail> {
        self.fetch_detail(token, id, false).await
    }
    pub async fn download_options_for(&self, token: &SessionToken, id: &Value) -> Result<MvDetail> {
        self.fetch_detail(token, id, true).await
    }
    async fn fetch_detail(
        &self,
        token: &SessionToken,
        id: &Value,
        options: bool,
    ) -> Result<MvDetail> {
        let id = identifier(id)?;
        let revision = self.detail_revision(token, &id)?;
        let mut detail = self
            .session
            .run_for(token, |api, _| async {
                let api = MvApi::new(api);
                if options {
                    api.download_options(&Value::String(id.clone())).await
                } else {
                    api.detail(&Value::String(id.clone())).await
                }
            })
            .await?;
        if options
            && detail
                .related_songs
                .iter()
                .any(|s| s["privilege"].is_null())
        {
            let ids = detail
                .related_songs
                .iter()
                .map(|s| identifier(&s["id"]))
                .collect::<Result<Vec<_>>>()?;
            let privileges = self
                .session
                .run_for(token, |api, _| async move {
                    MvApi::new(api).related_privileges(&ids).await
                })
                .await;
            match privileges {
                Ok(values) => {
                    for song in &mut detail.related_songs {
                        if let Some(value) = values.get(&identifier(&song["id"])?) {
                            song["privilege"] = value.clone();
                        }
                    }
                }
                Err(_) => {
                    self.session.with_token(token, |_| Ok(()))?;
                    detail.related_privilege_failed = true;
                }
            }
        }
        self.cache_detail(token, &id, revision, detail)
    }
    /// Explicit cached read: network errors are not silently reported as fresh data.
    pub fn cached_for(&self, token: &SessionToken, id: &Value) -> Result<Option<MvDetail>> {
        let id = identifier(id)?;
        self.session.with_token(token, |uid| {
            let value: Option<MvDetail> = self
                .store
                .get(uid, DETAILS, &id)?
                .map(serde_json::from_value)
                .transpose()?;
            if value.as_ref().is_some_and(|v| v.id != id) {
                return Err(Error::State("MV cache identity mismatch"));
            }
            Ok(value)
        })
    }
    pub async fn authorize_for(
        &self,
        token: &SessionToken,
        id: &Value,
        resolution: u32,
        purpose: MvPurpose,
    ) -> Result<Value> {
        let id = identifier(id)?;
        self.session
            .run_for(token, |api, _| async move {
                MvApi::new(api)
                    .authorize(&Value::String(id), resolution, purpose)
                    .await
            })
            .await
    }
    pub fn position_for(&self, token: &SessionToken, id: &Value) -> Result<Option<MvPosition>> {
        let id = identifier(id)?;
        self.session.with_token(token, |uid| {
            let value: Option<MvPosition> = self
                .store
                .get(uid, POSITIONS, &id)?
                .map(serde_json::from_value)
                .transpose()?;
            if value.as_ref().is_some_and(|v| {
                v.id != id || v.position_ms > v.duration_ms || v.completed && v.position_ms != 0
            }) {
                return Err(Error::State("invalid MV playback checkpoint"));
            }
            Ok(value)
        })
    }
    /// The native player supplies measured duration/position, after load succeeds.
    /// Old account players cannot write checkpoints into a newly selected account.
    pub fn save_position_for(
        &self,
        token: &SessionToken,
        id: &Value,
        position_ms: u64,
        duration_ms: u64,
        completed: bool,
    ) -> Result<MvPosition> {
        let id = identifier(id)?;
        if duration_ms == 0 || position_ms > duration_ms {
            return Err(Error::InvalidInput("invalid MV playback checkpoint"));
        }
        let updated_ms = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::State("invalid checkpoint clock"))?
                .as_millis(),
        )
        .map_err(|_| Error::State("checkpoint clock overflow"))?;
        let value = MvPosition {
            id,
            position_ms: if completed { 0 } else { position_ms },
            duration_ms,
            completed,
            updated_ms,
        };
        self.session
            .save_result(token, POSITIONS, &value.id, &serde_json::to_value(&value)?)?;
        Ok(value)
    }
}
