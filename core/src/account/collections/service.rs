use super::{CollectionApi, CollectionData, CollectionQuery};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const CACHE: &str = "account-collection-pages-v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedCollectionPage {
    pub query: CollectionQuery,
    pub fetched_at: u64,
    pub data: CollectionData,
}
pub struct Collections {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl Collections {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn key(query: &CollectionQuery) -> Result<String> {
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(query)?)))
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        query: &CollectionQuery,
    ) -> Result<Option<SavedCollectionPage>> {
        let key = Self::key(query)?;
        self.session.with_token(token, |owner| {
            self.store
                .get(owner, CACHE, &key)?
                .map(|value| {
                    let saved: SavedCollectionPage = serde_json::from_value(value)?;
                    if saved.query != *query {
                        return Err(Error::State("collection cache identity mismatch"));
                    }
                    saved.data.validate(query)?;
                    Ok(saved)
                })
                .transpose()
        })
    }
    pub async fn page_for(
        &self,
        token: &SessionToken,
        query: &CollectionQuery,
    ) -> Result<SavedCollectionPage> {
        let key = Self::key(query)?;
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lock =
            OperationLock::acquire(self.store.directory(), CACHE, &format!("{owner}:{key}"))?;
        let data = self
            .session
            .run_for(token, |api, _| async move {
                CollectionApi::new(api).page(query).await
            })
            .await?;
        let saved = SavedCollectionPage {
            query: query.clone(),
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            data,
        };
        self.session.with_token(token, |owner| {
            self.store
                .put(owner, CACHE, &key, &serde_json::to_value(&saved)?)
        })?;
        Ok(saved)
    }
}
