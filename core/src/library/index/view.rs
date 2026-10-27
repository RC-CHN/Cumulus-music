use super::{Index, LocalLibrary};
use crate::{
    Error, Result,
    library::{LibraryItem, ListOptions, LocalResource, LocalRevision, identity::fingerprint},
};
use std::{collections::HashSet, path::PathBuf};
use unicode_casefold::UnicodeCaseFold;

fn cue_items(index: &Index, include_missing: bool) -> Vec<LibraryItem> {
    index
        .cues
        .values()
        .filter(|cue| include_missing || (!cue.missing && !cue.unreadable))
        .flat_map(|cue| {
            cue.tracks.iter().cloned().map(|mut item| {
                item.missing = cue.missing;
                item.unreadable = cue.unreadable;
                item
            })
        })
        .collect()
}
fn fold(value: &str) -> String {
    value.case_fold().collect()
}

impl LocalLibrary {
    pub fn list(&self, options: ListOptions<'_>) -> Result<Vec<LibraryItem>> {
        let index = self.load()?;
        let virtual_tracks = cue_items(&index, options.include_missing);
        let containers: HashSet<PathBuf> = cue_items(&index, false)
            .into_iter()
            .map(|t| t.path)
            .collect();
        let query = fold(options.query);
        let mut items = Vec::new();
        for item in index
            .files
            .into_values()
            .filter(|item| options.include_containers || !containers.contains(&item.path))
            .chain(virtual_tracks)
        {
            if (options.include_missing || item.available())
                && fold(&serde_json::to_string(&item.metadata)?).contains(&query)
            {
                items.push(item);
            }
        }
        items.sort_by(|a, b| {
            fold(&a.metadata.title)
                .cmp(&fold(&b.metadata.title))
                .then(a.path.cmp(&b.path))
                .then(a.id.cmp(&b.id))
        });
        Ok(items)
    }
    pub fn item(&self, id: &str) -> Result<LibraryItem> {
        let index = self.load()?;
        index
            .files
            .get(id)
            .cloned()
            .or_else(|| {
                cue_items(&index, true)
                    .into_iter()
                    .find(|item| item.id == id)
            })
            .filter(LibraryItem::available)
            .ok_or(Error::State("local track is not available"))
    }
    pub fn remove(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        let old = self.load()?;
        let mut new = Index {
            files: old.files.clone(),
            cues: old.cues.clone(),
        };
        new.files.remove(id);
        for cue in new.cues.values_mut() {
            if cue.tracks.iter().any(|t| t.id == id) {
                cue.tracks.retain(|t| t.id != id);
                cue.needs_scan = true;
            }
        }
        self.commit(&old, &new, &|| false)
    }
    pub(crate) fn check_current(item: &LibraryItem) -> Result<()> {
        if !item.available() {
            return Err(Error::State("local track is unavailable"));
        }
        let matches = match &item.revision {
            LocalRevision::File {
                fingerprint: expected,
            } => fingerprint(&item.path)? == *expected,
            LocalRevision::Cue {
                sheet,
                sheet_fingerprint,
                source_fingerprint,
                ..
            } => {
                fingerprint(&item.path)? == *source_fingerprint
                    && fingerprint(sheet)? == *sheet_fingerprint
            }
        };
        if !matches {
            return Err(Error::State("local source changed; rescan before playing"));
        }
        Ok(())
    }
    pub fn resource(&self, id: &str) -> Result<LocalResource> {
        let item = self.item(id)?;
        Self::check_current(&item)?;
        Ok(LocalResource {
            local_id: item.id,
            local_revision: item.revision,
            metadata: item.metadata,
        })
    }
}
