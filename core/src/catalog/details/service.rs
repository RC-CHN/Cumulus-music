use super::{CatalogQuery, request};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const CACHE: &str = "catalog-detail-pages-v1";

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedCatalogPage {
    pub query: CatalogQuery,
    pub fetched_at: u64,
    pub revision: String,
    /// Account-owned native metadata, never a frontend DTO.
    pub response: Value,
}
impl SavedCatalogPage {
    pub fn songs(&self) -> Option<&[Value]> {
        let field = match self.query {
            CatalogQuery::Artist { .. } => "hotSongs",
            CatalogQuery::ArtistSongs { .. } | CatalogQuery::Album { .. } => "songs",
            _ => return None,
        };
        self.response[field].as_array().map(Vec::as_slice)
    }
}
fn revision(query: &CatalogQuery, response: &Value) -> Result<String> {
    let field = match query {
        CatalogQuery::Artist { .. } => Some("hotSongs"),
        CatalogQuery::ArtistSongs { .. } | CatalogQuery::Album { .. } => Some("songs"),
        _ => None,
    };
    let content = if let Some(field) = field {
        json!(
            response[field]
                .as_array()
                .ok_or(Error::Response("missing catalog tracks"))?
                .iter()
                .map(|s| identifier(&s["id"]))
                .collect::<Result<Vec<_>>>()?
        )
    } else {
        response.clone()
    };
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&json!([
        query, content
    ]))?)))
}
pub struct CatalogDetails {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl CatalogDetails {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn key(query: &CatalogQuery) -> Result<String> {
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(query)?)))
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        query: &CatalogQuery,
    ) -> Result<Option<SavedCatalogPage>> {
        let key = Self::key(query)?;
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, CACHE, &key)?
                .map(|v| {
                    let saved: SavedCatalogPage = serde_json::from_value(v)?;
                    if saved.query != *query {
                        return Err(Error::State("catalog cache identity mismatch"));
                    }
                    request::validate(query, &saved.response)?;
                    if saved.revision != revision(query, &saved.response)? {
                        return Err(Error::State("catalog cache revision mismatch"));
                    }
                    Ok(saved)
                })
                .transpose()
        })
    }
    /// M/15's g has enable:false. A recovery snapshot must not suppress this
    /// request on the next page visit or authorize playback from old rights.
    pub async fn page_for(
        &self,
        token: &SessionToken,
        query: &CatalogQuery,
    ) -> Result<SavedCatalogPage> {
        let key = Self::key(query)?;
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let _lock =
            OperationLock::acquire(self.store.directory(), CACHE, &format!("{owner}:{key}"))?;
        let response = self
            .session
            .run_for(
                token,
                |api, _| async move { request::fetch(api, query).await },
            )
            .await?;
        let saved = SavedCatalogPage {
            query: query.clone(),
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            revision: revision(query, &response)?,
            response,
        };
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, CACHE, &key, &serde_json::to_value(&saved)?)
        })?;
        Ok(saved)
    }
}
