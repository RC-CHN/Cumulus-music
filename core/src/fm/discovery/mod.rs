//! Official desktop M244 feature cards and M1211/M2986 vertical zones.
mod api;
mod normalize;
use super::FmSource;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
pub use api::FmDiscoveryApi;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const CACHE: &str = "fm-discovery-v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FmDiscoveryQuery {
    #[default]
    Home,
    Zone {
        page_code: String,
        tab_key: String,
    },
}
impl FmDiscoveryQuery {
    pub fn validate(&self) -> Result<()> {
        if let Self::Zone { page_code, tab_key } = self
            && (page_code.is_empty()
                || page_code.len() > 128
                || !page_code
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || tab_key.is_empty()
                || tab_key.len() > 256
                || tab_key.chars().any(char::is_control))
        {
            return Err(Error::InvalidInput("invalid FM zone query"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmCard {
    pub source: FmSource,
    pub title: String,
    pub subtitle: String,
    pub cover_url: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmZone {
    pub page_code: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmZoneTab {
    pub key: String,
    pub name: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmDiscoveryPage {
    pub cards: Vec<FmCard>,
    pub zones: Vec<FmZone>,
    pub tabs: Vec<FmZoneTab>,
    pub active_tab: Option<String>,
    /// The tab service failed/mismatched; official default-page fallback used.
    pub default_fallback: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFmDiscovery {
    pub query: FmDiscoveryQuery,
    pub fetched_at: u64,
    pub page: FmDiscoveryPage,
}
pub struct FmDiscovery {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl FmDiscovery {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn key(query: &FmDiscoveryQuery) -> Result<String> {
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(query)?)))
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        query: &FmDiscoveryQuery,
    ) -> Result<Option<SavedFmDiscovery>> {
        let key = Self::key(query)?;
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, CACHE, &key)?
                .map(|v| {
                    let saved: SavedFmDiscovery = serde_json::from_value(v)?;
                    if saved.query != *query {
                        return Err(Error::State("FM discovery cache identity mismatch"));
                    }
                    for card in &saved.page.cards {
                        card.source.validate()?;
                    }
                    Ok(saved)
                })
                .transpose()
        })
    }
    pub async fn page_for(
        &self,
        token: &SessionToken,
        query: &FmDiscoveryQuery,
    ) -> Result<SavedFmDiscovery> {
        let key = Self::key(query)?;
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let _gate =
            OperationLock::acquire(self.store.directory(), CACHE, &format!("{owner}:{key}"))?;
        let page = self
            .session
            .run_for(token, |api, _| async move {
                FmDiscoveryApi::new(api).page(query).await
            })
            .await?;
        let saved = SavedFmDiscovery {
            query: query.clone(),
            page,
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
        };
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, CACHE, &key, &serde_json::to_value(&saved)?)
        })?;
        Ok(saved)
    }
}
