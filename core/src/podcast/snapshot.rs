//! Complete account-scoped lists. Page slicing and playback share one identity
//! snapshot; a failed/inconsistent refresh never publishes a partial queue.
use super::{Episode, LibraryKind, PodcastApi, Radio, purchased::Purchases};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::{OperationLock, StateStore, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};
const CACHE: &str = "podcast-lists-v1";
const VERSION: &str = "podcast-cache-generation-v1";
const BADGE_OBSERVATION: &str = "podcast-badge-observation-v1";
const PURCHASE_OBSERVATION: &str = "podcast-purchase-observation-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastScope {
    Podcasts,
    Audiobooks,
    Created,
    Purchased,
    Liked,
    Radio { id: String, ascending: bool },
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PodcastSnapshot {
    pub scope: PodcastScope,
    pub revision: String,
    pub radios: Vec<Radio>,
    pub voices: Vec<Episode>,
    pub source_ids: Vec<String>,
    pub remote_total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) purchases: Option<Purchases>,
    generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) badge_observation: Option<String>,
}
pub struct PodcastLibrary {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
pub(super) fn invalidate(tx: &mut StateTransaction<'_>, owner: &str) -> Result<()> {
    let before = tx
        .get(owner, VERSION, "current")?
        .map(|v| {
            v.as_u64()
                .ok_or(Error::State("invalid podcast cache version"))
        })
        .transpose()?
        .unwrap_or(0);
    tx.put(
        owner,
        VERSION,
        "current",
        &json!(
            before
                .checked_add(1)
                .ok_or(Error::State("podcast cache version exhausted"))?
        ),
    )
}
fn generation(store: &StateStore, owner: &str) -> Result<u64> {
    store
        .get(owner, VERSION, "current")?
        .map(|v| {
            v.as_u64()
                .ok_or(Error::State("invalid podcast cache version"))
        })
        .transpose()
        .map(|v| v.unwrap_or(0))
}
impl PodcastLibrary {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    pub(crate) fn radio_order_for(&self, token: &SessionToken, id: &str) -> Result<Option<bool>> {
        let id = identifier(&json!(id))?;
        self.session.with_token(token, |owner| {
            self.store
                .read_snapshot(|tx| super::radio_order::read(tx, owner, &id))
        })
    }
    pub(crate) fn set_radio_order_for(
        &self,
        token: &SessionToken,
        scope: &PodcastScope,
        revision: &str,
        ascending: bool,
    ) -> Result<()> {
        let PodcastScope::Radio {
            id,
            ascending: displayed,
        } = scope
        else {
            return Err(Error::InvalidInput("podcast order requires a radio view"));
        };
        identifier(&json!(id))?;
        if *displayed == ascending {
            return Err(Error::InvalidInput("podcast order must change"));
        }
        let key = serde_json::to_string(scope)?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let version = tx
                    .get(owner, VERSION, "current")?
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                let valid_view = tx
                    .get(owner, CACHE, &key)?
                    .map(serde_json::from_value::<PodcastSnapshot>)
                    .transpose()?
                    .is_some_and(|snapshot| {
                        &snapshot.scope == scope
                            && snapshot.revision == revision
                            && snapshot.generation == version
                            && snapshot.radios.first().is_some_and(|radio| {
                                &radio.id == id
                                    && radio.raw["grayDisplay"] != true
                                    && radio.raw["underShelf"] != true
                            })
                    });
                // No side effects on cached snapshots or the active player.
                // A differently sorted detail in another process is stale.
                if !valid_view
                    || super::radio_order::read(tx, owner, id)?
                        .is_some_and(|saved| saved != *displayed)
                {
                    return Err(Error::QueueChanged);
                }
                super::radio_order::write(tx, owner, id, ascending)
            })
        })
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        scope: &PodcastScope,
    ) -> Result<Option<PodcastSnapshot>> {
        self.session.with_token(token, |owner| {
            let key = serde_json::to_string(scope)?;
            let snapshot: Option<PodcastSnapshot> = self
                .store
                .get(owner, CACHE, &key)?
                .map(serde_json::from_value)
                .transpose()?;
            if snapshot.as_ref().is_some_and(|s| s.scope != *scope) {
                return Err(Error::State("podcast snapshot identity mismatch"));
            }
            let version = generation(&self.store, owner)?;
            Ok(snapshot.filter(|s| s.generation == version && s.has_current_shape()))
        })
    }
    pub(crate) fn badge_observation_is_current(
        &self,
        token: &SessionToken,
        observed: &str,
    ) -> Result<bool> {
        self.session.with_token(token, |owner| {
            self.store.read_snapshot(|tx| {
                let generation = tx
                    .get(owner, VERSION, "current")?
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                Ok(tx.get(owner, BADGE_OBSERVATION, "current")?
                    == Some(json!([generation, observed])))
            })
        })
    }
    pub(crate) fn purchase_observation_is_current(
        &self,
        token: &SessionToken,
        observed: &str,
    ) -> Result<bool> {
        self.session.with_token(token, |owner| {
            self.store.read_snapshot(|tx| {
                let generation = tx
                    .get(owner, VERSION, "current")?
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                Ok(tx.get(owner, PURCHASE_OBSERVATION, "current")?
                    == Some(json!([generation, observed])))
            })
        })
    }
    pub async fn read_for(
        &self,
        token: &SessionToken,
        scope: &PodcastScope,
        refresh: bool,
    ) -> Result<PodcastSnapshot> {
        if let PodcastScope::Radio { id, .. } = scope {
            identifier(&json!(id))?;
        }
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let key = serde_json::to_string(scope)?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            &owner,
            &format!("podcast-list:{key}"),
        )?;
        let version = generation(&self.store, &owner)?;
        if !refresh && let Some(value) = self.store.get(&owner, CACHE, &key)? {
            let snapshot: PodcastSnapshot = serde_json::from_value(value)?;
            if snapshot.scope != *scope {
                return Err(Error::State("podcast snapshot identity mismatch"));
            }
            if snapshot.generation == version
                && snapshot.has_current_shape()
                && (!matches!(scope, PodcastScope::Purchased)
                    || self.purchase_observation_is_current(token, &snapshot.revision)?)
            {
                self.session.with_token(token, |_| Ok(()))?;
                return Ok(snapshot);
            }
        }
        let mut snapshot = self
            .session
            .run_for(token, |api, _| async move {
                fetch(PodcastApi::new(api), scope).await
            })
            .await?;
        snapshot.generation = version;
        if matches!(scope, PodcastScope::Podcasts) {
            snapshot.badge_observation = Some(uuid::Uuid::new_v4().simple().to_string());
        }
        snapshot.revise()?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let current = tx
                    .get(owner, VERSION, "current")?
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if current != version {
                    return Err(Error::StaleResponse);
                }
                if let Some(observed) = &snapshot.badge_observation {
                    tx.put(
                        owner,
                        BADGE_OBSERVATION,
                        "current",
                        &json!([version, observed]),
                    )?;
                }
                snapshot.persist(tx, owner, &key)
            })
        })?;
        Ok(snapshot)
    }
    pub(crate) fn lock_purchases_for(&self, token: &SessionToken) -> Result<OperationLock> {
        self.lock_list_for(token, PodcastScope::Purchased)
    }

    pub(crate) fn lock_liked_for(&self, token: &SessionToken) -> Result<OperationLock> {
        self.lock_list_for(token, PodcastScope::Liked)
    }

    fn lock_list_for(&self, token: &SessionToken, scope: PodcastScope) -> Result<OperationLock> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let key = serde_json::to_string(&scope)?;
        OperationLock::acquire(
            self.store.directory(),
            &owner,
            &format!("podcast-list:{key}"),
        )
    }

    pub(crate) async fn extend_purchased_for(
        &self,
        token: &SessionToken,
        revision: &str,
        offset: usize,
    ) -> Result<PodcastSnapshot> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let key = serde_json::to_string(&PodcastScope::Purchased)?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            &owner,
            &format!("podcast-list:{key}"),
        )?;
        let mut snapshot = self
            .cached_for(token, &PodcastScope::Purchased)?
            .filter(|value| value.revision == revision)
            .ok_or(Error::QueueChanged)?;
        let previous = snapshot.purchases.take().ok_or(Error::QueueChanged)?;
        let next = self
            .session
            .run_for(token, |api, _| async move {
                PodcastApi::new(api)
                    .extend_purchases(Some(previous), offset)
                    .await
            })
            .await?;
        snapshot.update_purchases(next);
        snapshot.revise()?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let version = tx
                    .get(owner, VERSION, "current")?
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                if version != snapshot.generation {
                    return Err(Error::StaleResponse);
                }
                snapshot.persist(tx, owner, &key)
            })
        })?;
        Ok(snapshot)
    }
}

impl PodcastSnapshot {
    fn persist(&self, tx: &mut StateTransaction<'_>, owner: &str, key: &str) -> Result<()> {
        if matches!(self.scope, PodcastScope::Purchased) {
            tx.put(
                owner,
                PURCHASE_OBSERVATION,
                "current",
                &json!([self.generation, self.revision]),
            )?;
        }
        tx.put(owner, CACHE, key, &serde_json::to_value(self)?)
    }

    fn has_current_shape(&self) -> bool {
        !matches!(self.scope, PodcastScope::Purchased) || self.purchases.is_some()
    }

    fn update_purchases(&mut self, purchases: Purchases) {
        self.remote_total = purchases.total.unwrap_or(purchases.items.len() as u64);
        self.purchases = Some(purchases);
    }

    fn revise(&mut self) -> Result<()> {
        if let Some(purchases) = &self.purchases {
            self.revision = hex::encode(Sha256::digest(serde_json::to_vec(&(
                self.generation,
                purchases,
            ))?));
            return Ok(());
        }
        let mut authority = vec![
            json!(self.scope),
            json!(self.source_ids),
            json!(
                self.voices
                    .iter()
                    .map(|v| (&v.id, &v.song_id, v.duration_ms))
                    .collect::<Vec<_>>()
            ),
            json!(self.remote_total),
        ];
        if self.scope == PodcastScope::Liked {
            authority.push(json!(
                self.voices
                    .iter()
                    .map(|voice| (
                        voice.display_status_withdrawn(),
                        voice.raw["grayDisplay"] == true
                    ))
                    .collect::<Vec<_>>()
            ));
        }
        self.revision = hex::encode(Sha256::digest(serde_json::to_vec(&authority)?));
        Ok(())
    }
}
async fn fetch(api: PodcastApi, scope: &PodcastScope) -> Result<PodcastSnapshot> {
    let mut result = PodcastSnapshot {
        scope: scope.clone(),
        revision: String::new(),
        radios: vec![],
        voices: vec![],
        source_ids: vec![],
        remote_total: 0,
        purchases: None,
        generation: 0,
        badge_observation: None,
    };
    match scope {
        PodcastScope::Podcasts | PodcastScope::Audiobooks => {
            let list = api
                .my_library(if matches!(scope, PodcastScope::Podcasts) {
                    LibraryKind::Podcast
                } else {
                    LibraryKind::Audiobook
                })
                .await?;
            result.source_ids = list.ids.iter().map(identifier).collect::<Result<_>>()?;
            result.radios = list.items;
        }
        PodcastScope::Created => result.radios = api.legacy_library(true).await?,
        PodcastScope::Purchased => result.update_purchases(api.extend_purchases(None, 0).await?),
        PodcastScope::Liked => result.voices = api.liked_voices().await?,
        PodcastScope::Radio { id, ascending } => {
            let radio = api.radio(&json!(id)).await?;
            result.radios.push(radio);
            let mut expected = None;
            let mut seen = BTreeSet::new();
            loop {
                let page = api
                    .programs(&json!(id), 200, result.voices.len(), *ascending)
                    .await?;
                if expected.is_some_and(|n| n != page.total) {
                    return Err(Error::QueueChanged);
                }
                expected = Some(page.total);
                result.remote_total = page.total;
                for voice in page.items {
                    if !seen.insert(voice.id.clone()) {
                        return Err(Error::QueueChanged);
                    }
                    result.voices.push(voice);
                }
                if result.voices.len() > 50_000 {
                    return Err(Error::Response("podcast queue exceeds limit"));
                }
                if !page.more {
                    break;
                }
            }
            result.source_ids = result.voices.iter().map(|v| v.id.clone()).collect();
            if result.remote_total < result.voices.len() as u64 {
                return Err(Error::QueueChanged);
            }
        }
    }
    if !matches!(scope, PodcastScope::Radio { .. } | PodcastScope::Purchased) {
        if result.source_ids.is_empty() {
            result.source_ids = if matches!(scope, PodcastScope::Liked) {
                result.voices.iter().map(|v| v.id.clone()).collect()
            } else {
                result.radios.iter().map(|v| v.id.clone()).collect()
            };
        }
        result.remote_total = result.source_ids.len() as u64;
    }
    Ok(result)
}
