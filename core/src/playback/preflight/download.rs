//! Map the official M/59.j trackMap/DB check to account-owned publications.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::{offline::OfflineLibrary, service::DownloadService},
    media::integrity::FileIntegrity,
};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Publication {
    id: String,
    song_id: String,
    target: Option<PathBuf>,
    integrity: Option<FileIntegrity>,
    encrypted: bool,
}

pub(super) async fn snapshot(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    wanted: HashSet<String>,
) -> Result<BTreeMap<String, Vec<Publication>>> {
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let session = session.clone();
    let token = token.clone();
    tokio::task::spawn_blocking(move || {
        let tasks = DownloadService::queue_publications(
            &session,
            &token,
            &wanted.iter().map(String::as_str).collect(),
        )?;
        let mut results: BTreeMap<String, Vec<Publication>> = BTreeMap::new();
        for task in tasks {
            let target = task.playback_target().ok().map(ToOwned::to_owned);
            results
                .entry(task.song_id.clone())
                .or_default()
                .push(Publication {
                    id: task.id,
                    song_id: task.song_id,
                    target,
                    integrity: task.output_integrity,
                    encrypted: task.encrypted,
                });
        }
        for matches in results.values_mut() {
            matches.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        }
        Ok(results)
    })
    .await
    .map_err(|_| Error::State("download preflight worker stopped"))?
}

#[derive(Clone, Copy)]
pub(super) enum Assessment {
    FallThrough,
    Authorized,
    Unknown,
}

pub(super) async fn check(
    offline: &OfflineLibrary,
    token: &SessionToken,
    matches: Option<&Vec<Publication>>,
) -> Result<Assessment> {
    let Some(matches) = matches else {
        return Ok(Assessment::FallThrough);
    };
    // The recovered JS maps one row per song. When Cumulus has two completed
    // publications for the same song, there is no proven native tie-break.
    let [publication] = matches.as_slice() else {
        return Ok(Assessment::Unknown);
    };
    let (Some(target), Some(expected_integrity)) = (&publication.target, &publication.integrity)
    else {
        return Ok(Assessment::Unknown);
    };
    let check = match offline.check(token, target, false).await {
        Ok(check) => check,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Assessment::FallThrough);
        }
        Err(error @ (Error::StaleResponse | Error::Cancelled | Error::LockPoisoned)) => {
            return Err(error);
        }
        Err(_) => return Ok(Assessment::Unknown),
    };
    let record = &check.record;
    if record.task_id != publication.id
        || record.song_id != publication.song_id
        || record.encrypted != publication.encrypted
        || &record.integrity != expected_integrity
    {
        return Ok(Assessment::Unknown);
    }
    Ok(if !check.access.can_play {
        Assessment::FallThrough
    } else if check.integrity_verified {
        Assessment::Authorized
    } else {
        Assessment::Unknown
    })
}
