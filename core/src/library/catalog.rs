//! Official M/461 list/group/filter/full-list selection semantics over the
//! recovered local index. No online calls and no renderer-supplied file facts.
use super::{LibraryItem, ListOptions, LocalLibrary, LocalResource};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use unicode_casefold::UnicodeCaseFold;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalSort {
    #[default]
    Title,
    Artist,
    Album,
    Duration,
    Format,
    Folder,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalGroup {
    #[default]
    List,
    Artist,
    Album,
    Folder,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalQuery {
    pub keyword: String,
    pub sort: LocalSort,
    pub descending: bool,
    pub group: LocalGroup,
    pub group_key: Option<String>,
    pub include_missing: bool,
    pub offset: usize,
    pub limit: usize,
    pub revision: Option<String>,
}
impl Default for LocalQuery {
    fn default() -> Self {
        Self {
            keyword: String::new(),
            sort: LocalSort::Title,
            descending: true,
            group: LocalGroup::List,
            group_key: None,
            include_missing: true,
            offset: 0,
            limit: 200,
            revision: None,
        }
    }
}
impl LocalQuery {
    pub fn validate(&self) -> Result<()> {
        if self.keyword.len() > 1024
            || self.group_key.as_ref().is_some_and(|s| s.len() > 8192)
            || !(1..=500).contains(&self.limit)
            || self.offset > 1_000_000
            || (self.group == LocalGroup::List && self.group_key.is_some())
        {
            return Err(Error::InvalidInput("invalid local library query"));
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalGroupEntry {
    pub key: String,
    pub name: String,
    pub count: usize,
}

pub struct LocalSnapshot {
    pub revision: String,
    pub items: Vec<LibraryItem>,
    pub groups: Vec<LocalGroupEntry>,
}
fn folded(value: &str) -> String {
    value.case_fold().collect()
}
fn folder(item: &LibraryItem) -> String {
    item.path
        .parent()
        .unwrap_or(&item.path)
        .to_string_lossy()
        .into_owned()
}
fn keys(item: &LibraryItem, group: LocalGroup) -> BTreeSet<String> {
    match group {
        LocalGroup::List => BTreeSet::new(),
        LocalGroup::Album => BTreeSet::from([item.metadata.album.clone()]),
        LocalGroup::Folder => BTreeSet::from([folder(item)]),
        LocalGroup::Artist if item.metadata.artists.is_empty() => BTreeSet::from([String::new()]),
        LocalGroup::Artist => item.metadata.artists.iter().cloned().collect(),
    }
}
fn text_key(item: &LibraryItem, sort: LocalSort) -> String {
    folded(&match sort {
        LocalSort::Title | LocalSort::Duration => item.metadata.title.clone(),
        LocalSort::Artist => item.metadata.artists.join(" / "),
        LocalSort::Album => item.metadata.album.clone(),
        LocalSort::Format => item.metadata.file_format().into(),
        LocalSort::Folder => folder(item),
    })
}
impl LocalLibrary {
    /// One index transaction supplies both the revision and the complete list.
    pub fn catalog(&self, query: &LocalQuery) -> Result<LocalSnapshot> {
        query.validate()?;
        let all = self.list(ListOptions {
            include_missing: true,
            ..Default::default()
        })?;
        let revision = hex::encode(Sha256::digest(serde_json::to_vec(&all)?));
        if query.revision.as_ref().is_some_and(|v| v != &revision) {
            return Err(Error::QueueChanged);
        }
        let needle = folded(query.keyword.trim());
        let mut items = all
            .into_iter()
            .filter(|item| {
                (query.include_missing || item.available())
                    && (needle.is_empty()
                        || [&item.metadata.title, &item.metadata.album]
                            .into_iter()
                            .chain(item.metadata.artists.iter())
                            .any(|v| folded(v).contains(&needle))
                        || folded(&item.path.to_string_lossy()).contains(&needle))
            })
            .collect::<Vec<_>>();
        let mut groups = BTreeMap::<String, usize>::new();
        for item in &items {
            for key in keys(item, query.group) {
                *groups.entry(key).or_default() += 1;
            }
        }
        if let Some(key) = &query.group_key {
            items.retain(|item| keys(item, query.group).contains(key));
        }
        let mut keyed = items
            .into_iter()
            .map(|item| (text_key(&item, query.sort), item))
            .collect::<Vec<_>>();
        keyed.sort_by(|(a_key, a), (b_key, b)| {
            let order = match query.sort {
                LocalSort::Duration => a.metadata.duration.total_cmp(&b.metadata.duration),
                _ => a_key.cmp(b_key),
            };
            let order = if query.descending {
                order.reverse()
            } else {
                order
            };
            order.then(a.path.cmp(&b.path)).then(a.id.cmp(&b.id))
        });
        let mut groups = groups
            .into_iter()
            .map(|(key, count)| LocalGroupEntry {
                name: if key.is_empty() {
                    "未知".into()
                } else {
                    key.clone()
                },
                key,
                count,
            })
            .collect::<Vec<_>>();
        groups.sort_by_cached_key(|v| (folded(&v.name), v.key.clone()));
        Ok(LocalSnapshot {
            revision,
            items: keyed.into_iter().map(|(_, i)| i).collect(),
            groups,
        })
    }
}
impl LocalSnapshot {
    /// Build every playable row, irrespective of UI pagination. Files are
    /// fingerprint-checked by the resolver on selection, advance and restore.
    pub fn queue(&self, start: Option<&str>) -> Result<(Vec<serde_json::Value>, usize)> {
        let items = self
            .items
            .iter()
            .filter(|i| i.available())
            .collect::<Vec<_>>();
        if items.is_empty() || items.len() > 50_000 {
            return Err(Error::InvalidInput("local queue is empty or too large"));
        }
        let index = match start {
            Some(id) => items
                .iter()
                .position(|i| i.id == id)
                .ok_or(Error::InvalidInput(
                    "local selection is unavailable or outside query",
                ))?,
            None => 0,
        };
        let resources = items
            .into_iter()
            .map(|i| {
                serde_json::to_value(LocalResource {
                    local_id: i.id.clone(),
                    local_revision: i.revision.clone(),
                    metadata: i.metadata.clone(),
                })
                .map_err(Error::from)
            })
            .collect::<Result<_>>()?;
        Ok((resources, index))
    }
}
