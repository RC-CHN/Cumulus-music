//! Atomic download-file reconciliation. Caller holds the library-index lock.
use super::{CUES, FILES, MAPS, OWNER};
use crate::{
    Error, Result,
    library::{
        LibraryItem, LocalRevision,
        identity::{FileFingerprint, path_id},
        records::CueRecord,
    },
    playback::QueueSnapshot,
    storage::StateTransaction,
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

pub(crate) fn relocate_download(
    tx: &mut StateTransaction<'_>,
    old: &Path,
    new: Option<(&Path, FileFingerprint)>,
    expected: Option<&FileFingerprint>,
) -> Result<()> {
    let old_id = path_id(old)?;
    let old_item: Option<LibraryItem> = tx
        .get(OWNER, FILES, &old_id)?
        .map(serde_json::from_value)
        .transpose()?;
    let expected_revision = expected.map(|f| LocalRevision::File {
        fingerprint: f.clone(),
    });
    let mut new_item = None;
    if let (Some(item), Some((path, fp))) = (&old_item, &new)
        && item.available()
        && Some(&item.revision) == expected_revision.as_ref()
    {
        let mut item = item.clone();
        item.id = path_id(path)?;
        item.path = path.to_path_buf();
        item.revision = LocalRevision::File {
            fingerprint: fp.clone(),
        };
        if tx.get(OWNER, FILES, &item.id)?.is_some() {
            return Err(Error::State("destination already has a local index record"));
        }
        tx.put(OWNER, FILES, &item.id, &serde_json::to_value(&item)?)?;
        new_item = Some(item);
    }
    tx.delete(OWNER, FILES, &old_id)?;
    for row in tx.rows(MAPS)? {
        if row.key != old_id {
            continue;
        }
        if let (Some(old), Some(new)) = (&old_item, &new_item)
            && row.value.get("revision") == Some(&serde_json::to_value(&old.revision)?)
        {
            let mut mapping = row.value.clone();
            mapping["revision"] = serde_json::to_value(&new.revision)?;
            tx.put(&row.owner, MAPS, &new.id, &mapping)?;
        }
        tx.delete(&row.owner, MAPS, &row.key)?;
    }
    let mut cue_ids = HashSet::new();
    for row in tx.rows(CUES)? {
        if row.owner != OWNER {
            continue;
        }
        let mut cue: CueRecord = serde_json::from_value(row.value)?;
        if cue.sources.contains_key(old) {
            cue.unreadable = true;
            cue.needs_scan = true;
            cue_ids.extend(cue.tracks.iter().map(|t| t.id.clone()));
            tx.put(&row.owner, CUES, &row.key, &serde_json::to_value(cue)?)?;
        }
    }
    for row in tx.rows("playback")? {
        let position_key = match row.key.as_str() {
            "queue" => "position",
            crate::playback::FM_BACKUP_QUEUE => crate::playback::FM_BACKUP_POSITION,
            _ => continue,
        };
        let mut queue: QueueSnapshot = serde_json::from_value(row.value)?;
        queue.validate()?;
        let mut removed = HashSet::new();
        let mut changed = false;
        for entry in &mut queue.items {
            let resource = &mut entry.resource;
            let local_id = resource.get("local_id").and_then(Value::as_str);
            if local_id.is_some_and(|id| cue_ids.contains(id)) {
                removed.insert(entry.entry_id.clone());
                changed = true;
                continue;
            }
            let path_match = resource
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|value| {
                    let path = Path::new(value);
                    path == old
                        || (path.is_absolute()
                            && path.canonicalize().is_ok_and(|resolved| resolved == old))
                });
            if local_id != Some(old_id.as_str()) && !path_match {
                continue;
            }
            changed = true;
            if let Some((path, _)) = &new {
                if local_id.is_some() {
                    if let (Some(old), Some(new)) = (&old_item, &new_item)
                        && resource.get("local_revision")
                            == Some(&serde_json::to_value(&old.revision)?)
                    {
                        resource["local_id"] = json!(new.id);
                        resource["local_revision"] = serde_json::to_value(&new.revision)?;
                        resource["metadata"] = serde_json::to_value(&new.metadata)?;
                    } else {
                        removed.insert(entry.entry_id.clone());
                    }
                }
                if path_match {
                    resource["path"] = json!(path);
                }
            } else {
                removed.insert(entry.entry_id.clone());
            }
        }
        if !changed {
            continue;
        }
        if queue
            .current
            .as_ref()
            .is_some_and(|id| removed.contains(id))
        {
            let index = queue
                .order
                .iter()
                .position(|id| Some(id) == queue.current.as_ref())
                .expect("validated current");
            queue.current = queue.order[index + 1..]
                .iter()
                .chain(&queue.order[..index])
                .find(|id| !removed.contains(*id))
                .cloned();
            tx.delete(&row.owner, "playback", position_key)?;
        }
        queue.items.retain(|e| !removed.contains(&e.entry_id));
        queue.order.retain(|id| !removed.contains(id));
        queue.validate()?;
        tx.put(
            &row.owner,
            "playback",
            &row.key,
            &serde_json::to_value(queue)?,
        )?;
    }
    Ok(())
}
