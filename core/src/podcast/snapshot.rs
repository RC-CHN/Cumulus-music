//! Complete account-scoped lists. Page slicing and playback share one identity
//! snapshot; a failed/inconsistent refresh never publishes a partial queue.
use super::{Episode, LibraryKind, PodcastApi, Radio};
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
    generation: u64,
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
            Ok(snapshot.filter(|s| s.generation == version))
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
            if snapshot.generation == version {
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
        snapshot.revision = hex::encode(Sha256::digest(serde_json::to_vec(&json!([
            scope,
            snapshot.source_ids,
            snapshot
                .voices
                .iter()
                .map(|v| (&v.id, &v.song_id, v.duration_ms))
                .collect::<Vec<_>>(),
            snapshot.remote_total
        ]))?));
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let current = tx
                    .get(owner, VERSION, "current")?
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if current != version {
                    return Err(Error::StaleResponse);
                }
                tx.put(owner, CACHE, &key, &serde_json::to_value(&snapshot)?)
            })
        })?;
        Ok(snapshot)
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
        generation: 0,
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
        PodcastScope::Purchased => result.radios = api.purchased().await?,
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
    if !matches!(scope, PodcastScope::Radio { .. }) {
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
