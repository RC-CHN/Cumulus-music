use super::{LocalLibrary, MAPS};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::{ListOptions, LocalRevision},
    protocol::api::identifier,
    storage::StateTransaction,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingSource {
    FileMetadata,
    Search,
    Manual,
}

/// Native persistence record. The revision can contain a CUE path; renderer
/// adapters must project a separate DTO instead of serializing this record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalMapping {
    pub song_id: String,
    pub revision: LocalRevision,
    #[serde(default)]
    pub song: Option<Value>,
    #[serde(default)]
    pub source: Option<MappingSource>,
}

impl LocalLibrary {
    /// Reverse lookup for native resource menus. Only explicit, account-owned
    /// mappings at the current file/CUE revision count; filenames are not IDs.
    pub(crate) fn files_for_online_song(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        song_id: &str,
    ) -> Result<Vec<crate::library::LibraryItem>> {
        identifier(&serde_json::json!(song_id))?;
        let _lock = self.lock()?;
        let mappings = session.with_token(token, |uid| self.store.values(uid, MAPS))?;
        if mappings.len() > 50_000 {
            return Err(Error::State("local file associations exceed 50000"));
        }
        let wanted = mappings
            .into_iter()
            .map(|(id, value)| Ok((id, serde_json::from_value::<LocalMapping>(value)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let found = self
            .list(ListOptions {
                include_containers: true,
                ..Default::default()
            })?
            .into_iter()
            .filter(|item| {
                wanted.get(&item.id).is_some_and(|mapping| {
                    mapping.song_id == song_id && mapping.revision == item.revision
                }) && Self::check_current(item).is_ok()
            })
            .collect();
        session.with_token(token, |_| Ok(found))
    }

    pub fn map_online(&self, session: &AccountSession, id: &str, song_id: &Value) -> Result<()> {
        let song_id = identifier(song_id)?;
        let token = session.token()?;
        let _lock = self.lock()?;
        let item = self.item(id)?;
        Self::check_current(&item)?;
        session.with_token(&token, |uid| {
            self.store.put(
                uid,
                MAPS,
                id,
                &serde_json::to_value(LocalMapping {
                    song_id,
                    revision: item.revision,
                    song: None,
                    source: Some(MappingSource::Manual),
                })?,
            )
        })
    }

    /// One index read for the requested page; account and file revisions are
    /// independently checked. Missing/changed files never inherit an old link.
    pub fn online_mappings(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        ids: &[String],
    ) -> Result<BTreeMap<String, LocalMapping>> {
        self.with_online_mappings_snapshot(session, token, ids, |_, _, mappings| Ok(mappings))
    }

    /// Observe another account-owned record alongside the current mappings.
    /// The callback may read only the supplied snapshot, not re-enter the
    /// account, index, or store locks.
    pub(in crate::library) fn with_online_mappings_snapshot<T>(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        ids: &[String],
        observe: impl FnOnce(&StateTransaction<'_>, &str, BTreeMap<String, LocalMapping>) -> Result<T>,
    ) -> Result<T> {
        if ids.len() > 500 {
            return Err(Error::InvalidInput("local mapping page exceeds 500"));
        }
        self.online_mappings_inner(session, token, ids, observe)
    }

    /// Native collection uses one index scan for the complete queue, retaining
    /// the public page limit and the same account/file identity checks.
    pub(crate) fn online_mappings_for_collection(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        ids: &[String],
    ) -> Result<BTreeMap<String, LocalMapping>> {
        if ids.len() > 50_000 {
            return Err(Error::InvalidInput(
                "local collection exceeds queue capacity",
            ));
        }
        self.online_mappings_inner(session, token, ids, |_, _, mappings| Ok(mappings))
    }

    fn online_mappings_inner<T>(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        ids: &[String],
        observe: impl FnOnce(&StateTransaction<'_>, &str, BTreeMap<String, LocalMapping>) -> Result<T>,
    ) -> Result<T> {
        let requested = ids.iter().collect::<BTreeSet<_>>();
        let _lock = self.lock()?;
        let current = self
            .list(ListOptions {
                include_containers: true,
                ..Default::default()
            })?
            .into_iter()
            .filter(|i| requested.contains(&i.id))
            .filter(|i| Self::check_current(i).is_ok())
            .map(|i| (i.id, i.revision))
            .collect::<BTreeMap<_, _>>();
        session.with_token(token, |uid| {
            self.store.read_snapshot(|tx| {
                let mut mappings = BTreeMap::new();
                for (id, revision) in current {
                    let mapping: Option<LocalMapping> = tx
                        .get(uid, MAPS, &id)?
                        .map(serde_json::from_value)
                        .transpose()?;
                    if let Some(mapping) = mapping.filter(|m| m.revision == revision) {
                        mappings.insert(id, mapping);
                    }
                }
                observe(tx, uid, mappings)
            })
        })
    }

    pub fn online_id(&self, session: &AccountSession, id: &str) -> Result<Option<String>> {
        let token = session.token()?;
        Ok(self
            .online_mappings(session, &token, &[id.to_owned()])?
            .remove(id)
            .map(|m| m.song_id))
    }
}
