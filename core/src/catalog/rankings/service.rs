use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const CACHE: &str = "ranking-pages-v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedRanking {
    pub query: RankingQuery,
    pub fetched_at: u64,
    pub page: RankingPage,
}
impl SavedRanking {
    /// Official M460 -> M32.n: default three HOURS, minus the smaller of 5%
    /// or three seconds. A backwards local clock forces revalidation here.
    pub fn fresh_at(&self, now: u64) -> bool {
        matches!(self.query, RankingQuery::Home)
            && now >= self.fetched_at
            && now - self.fetched_at <= 10_797_000
    }
}
pub struct Rankings {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl Rankings {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn key(query: &RankingQuery) -> Result<String> {
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(query)?)))
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        query: &RankingQuery,
    ) -> Result<Option<SavedRanking>> {
        let key = Self::key(query)?;
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, CACHE, &key)?
                .map(|v| {
                    let saved: SavedRanking = serde_json::from_value(v)?;
                    if saved.query != *query {
                        return Err(Error::State("ranking cache identity mismatch"));
                    }
                    Ok(saved)
                })
                .transpose()
        })
    }
    pub async fn page_for(
        &self,
        token: &SessionToken,
        query: &RankingQuery,
    ) -> Result<SavedRanking> {
        let key = Self::key(query)?;
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let _lock =
            OperationLock::acquire(self.store.directory(), CACHE, &format!("{owner}:{key}"))?;
        let page = self
            .session
            .run_for(token, |api, _| async move {
                RankingsApi::new(api).page(query).await
            })
            .await?;
        let saved = SavedRanking {
            query: query.clone(),
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            page,
        };
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, CACHE, &key, &serde_json::to_value(&saved)?)
        })?;
        Ok(saved)
    }
}
