use super::{Index, LocalLibrary};
use crate::{
    Error, Result,
    library::{LibraryItem, LocalRevision},
    playlist::PlaylistLocalRef,
    storage::OperationLock,
};
use std::collections::{BTreeMap, HashMap};

impl LocalLibrary {
    /// Resolve only exact, currently scoped index references. Stale or
    /// missing rows remain playlist placeholders rather than acquiring the
    /// replacement file's permission.
    pub(crate) fn playlist_local_items(
        &self,
        rows: &[PlaylistLocalRef],
    ) -> Result<BTreeMap<String, LibraryItem>> {
        if rows.is_empty() {
            return Ok(BTreeMap::new());
        }
        let index = self.load_with_hidden(true)?;
        let wanted: HashMap<_, _> = rows
            .iter()
            .map(|row| (row.id.as_str(), row.revision.as_str()))
            .collect();
        let mut found = BTreeMap::new();
        let insert = |found: &mut BTreeMap<String, LibraryItem>, item: &LibraryItem| {
            if let Some(&revision) = wanted.get(item.id.as_str())
                && item.revision.token() == revision
                && selectable(&index, item)
                && Self::check_current(item).is_ok()
            {
                found.insert(item.id.clone(), item.clone());
            }
        };
        for id in wanted.keys() {
            if let Some(item) = index.files.get(*id) {
                insert(&mut found, item);
            }
        }
        if found.len() < wanted.len() {
            for cue in index
                .cues
                .values()
                .filter(|cue| !cue.missing && !cue.unreadable)
            {
                for item in &cue.tracks {
                    insert(&mut found, item);
                }
            }
        }
        Ok(found)
    }

    pub(crate) fn hold_playlist_additions(
        &self,
        rows: &[PlaylistLocalRef],
    ) -> Result<OperationLock> {
        let lock = self.lock()?;
        if self.playlist_local_items(rows)?.len() != rows.len() {
            return Err(Error::QueueChanged);
        }
        Ok(lock)
    }
}

fn selectable(index: &Index, item: &LibraryItem) -> bool {
    let root_member = match &item.revision {
        LocalRevision::File { .. } => &item.path,
        LocalRevision::Cue { sheet, .. } => sheet,
    };
    index.directories.permits(root_member)
        && index.hidden.get(&item.id) != Some(&item.revision)
        && item.available()
}
