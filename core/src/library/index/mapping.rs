use super::{LocalLibrary, MAPS};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::{ListOptions, LocalRevision},
    protocol::{api::identifier, wire::serialization::js_truthy},
    storage::{OperationLock, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

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
        self.files_for_online_songs(session, token, &[song_id].into_iter().collect())
            .map(|matches| matches.into_iter().map(|(_, item)| item).collect())
    }

    /// One reverse lookup for the complete native group, not one index scan per song.
    pub(crate) fn matched_online_song_ids(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        song_ids: &BTreeSet<&str>,
    ) -> Result<BTreeSet<String>> {
        let _lock = self.lock()?;
        self.files_for_online_songs(session, token, song_ids)
            .map(|matches| matches.into_iter().map(|(id, _)| id).collect())
    }

    /// Hold the native index stable through an immediately following durable
    /// admission. The caller must not reenter LocalLibrary while holding it.
    pub(crate) fn hold_unmatched_online_songs(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        song_ids: &BTreeSet<&str>,
    ) -> Result<OperationLock> {
        let lock = self.lock()?;
        if !self
            .files_for_online_songs(session, token, song_ids)?
            .is_empty()
        {
            return Err(Error::State("download selection gained a local file"));
        }
        Ok(lock)
    }

    fn files_for_online_songs(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        song_ids: &BTreeSet<&str>,
    ) -> Result<Vec<(String, crate::library::LibraryItem)>> {
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
            .filter_map(|item| {
                let mapping = wanted.get(&item.id)?;
                (song_ids.contains(mapping.song_id.as_str())
                    && mapping.revision == item.revision
                    && Self::check_current(&item).is_ok())
                .then(|| (mapping.song_id.clone(), item))
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

    /// Lyric source for an exact queued local resource. An account's mapping
    /// is looked up by that one index ID, never by scanning all song mappings.
    pub(crate) fn indexed_lyric_source(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        id: &str,
        revision: &LocalRevision,
    ) -> Result<(PathBuf, Option<String>)> {
        let item = self.item(id)?;
        if &item.revision != revision {
            return Err(Error::QueueChanged);
        }
        let song_id = if token.uid.is_some() {
            session.with_token(token, |owner| {
                let mapped = self
                    .store
                    .get(owner, MAPS, id)?
                    .map(serde_json::from_value::<LocalMapping>)
                    .transpose()?;
                Ok(mapped
                    .filter(|mapping| &mapping.revision == revision)
                    .and_then(ordinary_lyric_song))
            })?
        } else {
            None
        };
        Self::check_current(&item)?;
        session.check_generation(token)?;
        Ok((item.path, song_id))
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

    /// Resolve the frozen flat selection in native display order. Keep the
    /// index lock across both the current file checks and account mapping read.
    pub(crate) fn batch_collection_mappings(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        query: &crate::library::catalog::LocalQuery,
        references: &[(&str, &str)],
    ) -> Result<Vec<LocalMapping>> {
        let _index = self.lock()?;
        let snapshot = self.catalog(query)?;
        let mut selected = references.iter().copied().collect::<BTreeMap<_, _>>();
        if selected.len() != references.len() {
            return Err(Error::InvalidInput("duplicate local selection row"));
        }
        let mut rows = Vec::with_capacity(references.len());
        for row in snapshot.items {
            if let Some(revision) = selected.remove(row.id.as_str()) {
                if row.revision.token() != revision {
                    return Err(Error::QueueChanged);
                }
                Self::check_current(&row).map_err(|_| Error::QueueChanged)?;
                rows.push(row);
            }
        }
        if !selected.is_empty() {
            return Err(Error::QueueChanged);
        }
        session.with_token(token, |uid| {
            self.store.read_snapshot(|tx| {
                rows.into_iter()
                    .map(|row| {
                        let raw = tx.get(uid, MAPS, &row.id)?.ok_or(Error::InvalidInput(
                            "every selected local track requires a current catalog match",
                        ))?;
                        let mapping: LocalMapping = serde_json::from_value(raw)?;
                        if mapping.revision != row.revision {
                            return Err(Error::QueueChanged);
                        }
                        Ok(mapping)
                    })
                    .collect()
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

/// A current plain-ID legacy link is supported. Typed podcast/cloud mappings
/// cannot silently be turned into a catalogue lyric read.
fn ordinary_lyric_song(mapping: LocalMapping) -> Option<String> {
    let id = identifier(&Value::String(mapping.song_id)).ok()?;
    if let Some(song) = mapping.song
        && (identifier(&song["id"]).ok().as_deref() != Some(id.as_str())
            || song["songType"] == 1
            || [
                "voice",
                "voice_id",
                "voiceId",
                "djProgramId",
                "_cloud_public_id",
                "_cloud_catalog_id",
            ]
            .into_iter()
            .any(|field| js_truthy(&song[field]))
            || ["cs", "cloudSong"]
                .into_iter()
                .any(|field| js_truthy(&song["privilege"][field])))
    {
        return None;
    }
    Some(id)
}
