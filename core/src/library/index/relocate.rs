//! Atomic download-file reconciliation. Caller holds the library-index lock.
use super::{CUES, FILES, MAPS, OWNER};
use crate::{
    Error, Result,
    library::{
        LibraryItem, LocalRevision,
        identity::{FileFingerprint, path_id},
        records::CueRecord,
    },
    playback::{QueueSnapshot, intelligence},
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
    reconcile_download(tx, old, new, expected, false)
}

pub(crate) fn delete_completed_download(tx: &mut StateTransaction<'_>, old: &Path) -> Result<()> {
    reconcile_download(tx, old, None, None, true)
}

fn reconcile_download(
    tx: &mut StateTransaction<'_>,
    old: &Path,
    new: Option<(&Path, FileFingerprint)>,
    expected: Option<&FileFingerprint>,
    completed_delete: bool,
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
    let relocation = Relocation {
        old,
        new: new.as_ref().map(|(path, _)| *path),
        old_id: &old_id,
        old_item: old_item.as_ref(),
        new_item: new_item.as_ref(),
        cue_ids: &cue_ids,
        completed_delete,
    };
    for row in tx.rows("playback")? {
        let position_key = match row.key.as_str() {
            "queue" => "position",
            crate::playback::FM_BACKUP_QUEUE => crate::playback::FM_BACKUP_POSITION,
            _ => continue,
        };
        let mut queue: QueueSnapshot = serde_json::from_value(row.value)?;
        queue.validate()?;
        let updated = if row.key == "queue" && queue.intelligence.is_some() {
            reconcile_intelligence_queue(tx, &row.owner, &mut queue, &relocation)?
        } else {
            relocation.reconcile(&mut queue)?
        };
        if let Some(selected_removed) = updated {
            queue.validate()?;
            if selected_removed {
                tx.delete(&row.owner, "playback", position_key)?;
            }
            tx.put(
                &row.owner,
                "playback",
                &row.key,
                &serde_json::to_value(queue)?,
            )?;
        }
    }
    Ok(())
}

fn reconcile_intelligence_queue(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    active: &mut QueueSnapshot,
    relocation: &Relocation<'_>,
) -> Result<Option<bool>> {
    let mut session: intelligence::IntelligenceSession = serde_json::from_value(
        tx.get(owner, intelligence::KIND, intelligence::KEY)?
            .ok_or(Error::State("missing intelligence session"))?,
    )?;
    session.validate(active)?;
    let backup_changed = relocation.reconcile(&mut session.backup)?.is_some();
    let active_changed = relocation.reconcile(active)?;
    if relocation.completed_delete && active_changed == Some(true) && active.items.is_empty() {
        // There is no live intelligence selection to keep. Filter the fallback
        // before exiting so an identical downloaded seed cannot be restored.
        relocation.reconcile(&mut session.fallback)?;
        *active = intelligence::ordinary_after_deleted_selection(session)?;
        tx.delete(owner, intelligence::KIND, intelligence::KEY)?;
    } else if backup_changed {
        session.revision = uuid::Uuid::new_v4().to_string();
        session.validate(active)?;
        tx.put(
            owner,
            intelligence::KIND,
            intelligence::KEY,
            &serde_json::to_value(session)?,
        )?;
    }
    Ok(active_changed)
}

struct Relocation<'a> {
    old: &'a Path,
    new: Option<&'a Path>,
    old_id: &'a str,
    old_item: Option<&'a LibraryItem>,
    new_item: Option<&'a LibraryItem>,
    cue_ids: &'a HashSet<String>,
    completed_delete: bool,
}

impl Relocation<'_> {
    fn path_matches(&self, value: Option<&Value>) -> bool {
        value.and_then(Value::as_str).is_some_and(|value| {
            let path = Path::new(value);
            path == self.old
                || (path.is_absolute()
                    && path
                        .canonicalize()
                        .is_ok_and(|resolved| resolved == self.old))
        })
    }

    /// `None` means no reference matched; otherwise returns whether the
    /// selected entry disappeared. Only current/FM queues persist a position.
    fn reconcile(&self, queue: &mut QueueSnapshot) -> Result<Option<bool>> {
        let mut removed = HashSet::new();
        let mut changed = false;
        for entry in &mut queue.items {
            let resource = &mut entry.resource;
            let local_id = resource.get("local_id").and_then(Value::as_str);
            if local_id.is_some_and(|id| self.cue_ids.contains(id)) {
                removed.insert(entry.entry_id.clone());
                changed = true;
                continue;
            }
            let path_match = self.path_matches(resource.get("path"));
            let ledger_match = self.completed_delete
                && self.path_matches(
                    resource
                        .get("download_ledger")
                        .and_then(|value| value.get("path")),
                );
            if local_id != Some(self.old_id) && !path_match && !ledger_match {
                continue;
            }
            changed = true;
            if let Some(path) = self.new {
                if local_id.is_some() {
                    if let (Some(old), Some(new)) = (self.old_item, self.new_item)
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
            return Ok(None);
        }
        let selected_removed = queue
            .current
            .as_ref()
            .is_some_and(|id| removed.contains(id));
        if selected_removed {
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
        }
        queue.items.retain(|e| !removed.contains(&e.entry_id));
        queue.order.retain(|id| !removed.contains(id));
        Ok(Some(selected_removed))
    }
}
