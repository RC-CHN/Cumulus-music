use super::{LocalLibrary, MAPS};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::{ListOptions, LocalRevision},
    protocol::api::identifier,
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
        if ids.len() > 500 {
            return Err(Error::InvalidInput("local mapping page exceeds 500"));
        }
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
            self.store.transaction(|tx| {
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
                Ok(mappings)
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
