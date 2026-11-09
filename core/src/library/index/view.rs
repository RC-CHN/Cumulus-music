use super::{HIDDEN, Index, LocalLibrary, OWNER};
use crate::{
    Error, Result,
    library::{
        LibraryItem, ListOptions, LocalResource, LocalRevision,
        catalog::{self, LocalQuery},
        identity::fingerprint,
    },
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
    /// Caller holds the shared index lock when using this as an unlink guard.
    pub(crate) fn protects_cue_source(&self, path: &std::path::Path) -> Result<bool> {
        let index = self.load()?;
        if index
            .cues
            .values()
            .any(|cue| cue.sources.contains_key(path))
        {
            return Ok(true);
        }
        let candidate = match crate::download::file_identity::identity(path) {
            Ok(value) => value,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        Ok(index
            .cues
            .values()
            .flat_map(|cue| cue.sources.keys())
            .any(|source| {
                crate::download::file_identity::identity(source)
                    .is_ok_and(|identity| identity == candidate)
            }))
    }
    pub fn list(&self, options: ListOptions<'_>) -> Result<Vec<LibraryItem>> {
        self.list_with_visibility(options, false)
    }

    /// File-management safeguards must inspect even rows hidden in the local page.
    pub(crate) fn list_indexed(&self, options: ListOptions<'_>) -> Result<Vec<LibraryItem>> {
        self.list_with_visibility(options, true)
    }

    fn list_with_visibility(
        &self,
        options: ListOptions<'_>,
        include_hidden: bool,
    ) -> Result<Vec<LibraryItem>> {
        Self::visible_items(self.load_with_hidden(!include_hidden)?, options)
    }

    fn visible_items(index: Index, options: ListOptions<'_>) -> Result<Vec<LibraryItem>> {
        let scope = &index.directories;
        let cue_in_scope = |item: &LibraryItem| matches!(&item.revision, LocalRevision::Cue { sheet, .. } if scope.permits(sheet));
        let virtual_tracks = cue_items(&index, options.include_missing)
            .into_iter()
            .filter(cue_in_scope);
        let containers: HashSet<PathBuf> = cue_items(&index, false)
            .into_iter()
            .filter(cue_in_scope)
            .map(|t| t.path)
            .collect();
        let query = fold(options.query);
        let mut items = Vec::new();
        for item in index
            .files
            .into_values()
            .filter(|item| scope.permits(&item.path))
            .filter(|item| options.include_containers || !containers.contains(&item.path))
            .chain(virtual_tracks)
        {
            if (options.include_missing || item.available())
                && index.hidden.get(&item.id) != Some(&item.revision)
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
    /// Hide this exact indexed revision from lists without touching the file,
    /// its CUE siblings, or resources that were already placed on a queue.
    pub(crate) fn hide_revision(&self, id: &str, token: &str) -> Result<()> {
        let _lock = self.lock()?;
        let index = self.load()?;
        let item = index
            .files
            .get(id)
            .cloned()
            .or_else(|| {
                cue_items(&index, true)
                    .into_iter()
                    .find(|item| item.id == id)
            })
            .ok_or(Error::QueueChanged)?;
        if item.revision.token() != token {
            return Err(Error::QueueChanged);
        }
        self.store
            .put(OWNER, HIDDEN, id, &serde_json::to_value(item.revision)?)
    }

    /// Hide a full group against the exact displayed catalog revision. The
    /// shared index lock and one write transaction cover snapshot and commit.
    pub(crate) fn hide_group(&self, query: &LocalQuery, revision: &str) -> Result<()> {
        let _lock = self.lock()?;
        self.store.transaction(|tx| {
            let items = Self::visible_items(
                Self::load_index(tx, true)?,
                ListOptions {
                    include_missing: true,
                    ..Default::default()
                },
            )?;
            if catalog::revision(&items)? != revision {
                return Err(Error::QueueChanged);
            }
            let targets = catalog::group_hide_targets(&items, query)?;
            for index in targets {
                let item = &items[index];
                tx.put(
                    OWNER,
                    HIDDEN,
                    &item.id,
                    &serde_json::to_value(&item.revision)?,
                )?;
            }
            Ok(())
        })
    }

    /// Resolve the whole selected set in one locked index snapshot. The
    /// command affects list visibility only, including for virtual CUE rows.
    pub(crate) fn hide_selection(
        &self,
        query: &LocalQuery,
        revision: &str,
        references: &[(&str, &str)],
    ) -> Result<()> {
        let _lock = self.lock()?;
        self.store.transaction(|tx| {
            let items = Self::visible_items(
                Self::load_index(tx, true)?,
                ListOptions {
                    include_missing: true,
                    ..Default::default()
                },
            )?;
            if catalog::revision(&items)? != revision {
                return Err(Error::QueueChanged);
            }
            let targets = catalog::batch_hide_targets(&items, query, references)?;
            for item in targets {
                tx.put(
                    OWNER,
                    HIDDEN,
                    &item.id,
                    &serde_json::to_value(&item.revision)?,
                )?;
            }
            Ok(())
        })
    }

    /// A user-requested rescan may restore list entries. Automatic monitoring
    /// never clears this state.
    pub(crate) fn restore_hidden(&self) -> Result<()> {
        let _lock = self.lock()?;
        self.store.transaction(|tx| {
            for id in tx.values(OWNER, HIDDEN)?.into_keys() {
                tx.delete(OWNER, HIDDEN, &id)?;
            }
            Ok(())
        })
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        let old = self.load()?;
        let mut new = old.clone();
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
