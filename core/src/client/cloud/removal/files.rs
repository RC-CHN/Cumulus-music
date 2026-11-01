use super::*;
use crate::{
    download::{
        completed::{DeleteSource, fs as verified},
        file_identity::identity,
    },
    library::{ListOptions, LocalRevision},
    media::integrity::FileIntegrity,
};
use std::{collections::BTreeSet, path::Path};

pub(super) fn pin(path: &Path, integrity: FileIntegrity) -> Result<DeleteSource> {
    if !path.is_absolute()
        || path
            .ancestors()
            .any(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return Err(Error::State(
            "cloud removal source must be a regular absolute file",
        ));
    }
    let facts = verified::recorded_facts(path, integrity)?;
    let parent = identity(
        path.parent()
            .ok_or(Error::State("cloud removal source lacks parent"))?,
    )?;
    verified::parent(path, &parent)?;
    Ok(DeleteSource {
        path: path.to_owned(),
        facts,
        parent,
    })
}
pub(super) fn cue_sources(library: &crate::library::LocalLibrary) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    for item in library.list(ListOptions {
        include_containers: true,
        include_missing: true,
        ..Default::default()
    })? {
        if matches!(item.revision, LocalRevision::Cue { .. })
            && let Ok(id) = identity(&item.path)
        {
            found.insert(serde_json::to_string(&id)?);
        }
    }
    Ok(found)
}
pub(super) fn known_cue(path: &Path, cues: &BTreeSet<String>) -> Result<bool> {
    if path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("cue"))
    {
        return Ok(true);
    }
    match identity(path) {
        Ok(id) => Ok(cues.contains(&serde_json::to_string(&id)?)),
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
pub(super) fn upload(
    context: &Context,
    library: &crate::library::LocalLibrary,
    file: &FileRecord,
) -> Result<(CloudRemovalFileState, Option<Issue>)> {
    let Origin::Upload { id } = &file.origin else {
        return Err(Error::State("expected an upload source"));
    };
    let (path, integrity, cue) = match context.uploads.removal_source(id, &file.cloud_id) {
        Ok(value) => value,
        Err(Error::State(_) | Error::InvalidInput(_)) => {
            return Ok((CloudRemovalFileState::Retained, Some(Issue::Changed)));
        }
        Err(e) => return Err(e),
    };
    if path != file.source.path || integrity != file.source.facts.integrity {
        return Ok((CloudRemovalFileState::Retained, Some(Issue::Changed)));
    }
    let _index = library.lock()?;
    if cue || library.protects_cue_source(&path)? {
        return Ok((CloudRemovalFileState::Retained, Some(Issue::Cue)));
    }
    let missing = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => return Err(e.into()),
        Ok(_) => false,
    };
    if !missing {
        if verified::parent(&path, &file.source.parent)
            .and_then(|_| verified::matches(&path, &file.source.facts))
            .is_err()
        {
            return Ok((CloudRemovalFileState::Retained, Some(Issue::Changed)));
        }
        context.session.with_token(&context.token, |_| {
            verified::parent(&path, &file.source.parent)?;
            verified::unchanged(&path, &file.source.facts)?;
            std::fs::remove_file(&path)?;
            verified::sync(path.parent().expect("verified parent"))
        })?;
    }
    // If unlink succeeded but the process stopped before this transaction, a
    // later explicit continuation sees Missing and finishes index reconciliation.
    let s = store(context)?;
    context.session.with_token(&context.token, |_| {
        s.transaction(|tx| {
            crate::library::relocate_download(tx, &path, None, Some(&file.source.facts.fingerprint))
        })
    })?;
    Ok((
        if missing {
            CloudRemovalFileState::Missing
        } else {
            CloudRemovalFileState::Removed
        },
        None,
    ))
}

pub(super) fn download(
    context: &Context,
    file: &FileRecord,
) -> Result<(CloudRemovalFileState, Option<Issue>)> {
    use crate::download::completed::{CompletedDownloads, FilePhase};
    let Origin::Download { id, operation_id } = &file.origin else {
        return Err(Error::State("expected a download association"));
    };
    let managed = CompletedDownloads::new(context.session.clone())?;
    let result = managed
        .start_delete_once(id, operation_id, file.source.clone())
        .and_then(|_| managed.resume(operation_id));
    match result {
        Err(Error::State("CUE source is protected from cloud removal")) => {
            if managed.get(operation_id).is_ok() {
                managed.cancel(operation_id)?;
            }
            Ok((CloudRemovalFileState::Retained, Some(Issue::Cue)))
        }
        Err(e) => Err(e),
        Ok(op) => Ok(if op.phase != FilePhase::Complete {
            (CloudRemovalFileState::Failed, Some(Issue::Journal))
        } else if op.files.iter().any(|f| f.replacement_retained) {
            (CloudRemovalFileState::Retained, Some(Issue::Changed))
        } else if op.missing.iter().any(|v| v == "audio") {
            (CloudRemovalFileState::Missing, None)
        } else {
            (CloudRemovalFileState::Removed, None)
        }),
    }
}
