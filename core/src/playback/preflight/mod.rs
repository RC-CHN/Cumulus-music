//! Official PC M/59.j ordinary-song candidate scan. This is a read-only
//! decision input, never a playback authorization or a skip controller.
mod download;
mod lookup;

use super::{QueueSnapshot, intelligence::eligibility::online_candidate};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::offline::{OfflineLibrary, PlaylistDownload},
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Supply OnlineWithoutMvExperiment only when a trusted native caller has
/// observed M/119 online and verified the PC MV experiment is off. Redux's
/// separate app online state controls whether M/59 requests an alternative.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OrdinaryQueueEnvironment {
    #[default]
    Unresolved,
    OnlineWithoutMvExperiment {
        app_online: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrdinaryCandidate {
    AccountDownload,
    Online,
    /// Exact type 1; M/59 found this ID in the formatted detail response.
    SameRecording {
        id: String,
    },
    /// M/59 retains the original item if the declared alternative is missing.
    /// This retention does not grant either resource playback rights.
    OriginalRetained {
        missing_alternative: String,
    },
    Unavailable,
    Undetermined(Undetermined),
    NotScanned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Undetermined {
    CatalogMetadataMissing,
    DownloadPublication,
    Environment,
    MalformedPrivilege,
    MalformedAlternative,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryCandidateRow {
    pub entry_id: String,
    pub original_song_id: String,
    pub candidate: OrdinaryCandidate,
    pub(crate) no_download_publication: bool,
    pub(crate) publication: Option<PlaylistDownload>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryQueuePreflight {
    pub rows: Vec<OrdinaryCandidateRow>,
    /// M/59 returns the first occurrence of each final ID, in display order.
    /// Indices refer to rows, not a new persisted queue. May include explicitly
    /// retained originals; only a real source resolver can authorize playback.
    pub candidate_order: Vec<usize>,
}

impl OrdinaryQueuePreflight {
    pub fn fully_classified(&self) -> bool {
        self.rows.iter().all(|row| {
            !matches!(
                row.candidate,
                OrdinaryCandidate::Undetermined(_) | OrdinaryCandidate::NotScanned
            )
        })
    }
}

pub(super) async fn inspect(
    session: Arc<AccountSession>,
    token: &SessionToken,
    queue: &QueueSnapshot,
    environment: OrdinaryQueueEnvironment,
) -> Result<OrdinaryQueuePreflight> {
    session.with_token(token, |_| Ok(()))?;
    queue.validate()?;
    if queue.fm_source.is_some()
        || queue.intelligence.is_some()
        || queue.items.len() > super::intelligence::MAX_QUEUE_TRACKS
    {
        return Err(Error::InvalidInput(
            "preflight requires a bounded ordinary song queue",
        ));
    }

    // M/308 passes curPlayingList, which is display order, not shuffled
    // transport order. Rust's items are in display order; order is traversal.
    let mut inputs = Vec::with_capacity(queue.items.len());
    for entry in &queue.items {
        let row = &entry.resource;
        if !entry.visible_in_queue()
            || [
                "local_id",
                "path",
                "download_task_id",
                "download_ledger",
                "cloud_song_id",
                "voice_id",
                "playback_constraint",
            ]
            .into_iter()
            .any(|key| !row[key].is_null())
            || js_truthy(&row["song"]["voiceId"])
            || js_truthy(&row["song"]["voice_id"])
        {
            return Err(Error::InvalidInput(
                "preflight requires an ordinary catalog song queue",
            ));
        }
        let id = identifier(&row["id"])?;
        let song = &row["song"];
        if id.len() > 32 || (!song.is_null() && identifier(&song["id"])? != id) {
            return Err(Error::State("queue preflight song identity changed"));
        }
        inputs.push((entry.entry_id.clone(), id, song));
    }
    let ids: HashSet<_> = inputs
        .iter()
        .take_while(|(_, _, song)| !song.is_null())
        .map(|(_, id, _)| id.clone())
        .collect();
    let publications = download::snapshot(&session, token, ids.clone()).await?;
    let offline = OfflineLibrary::new(session.clone())?;
    let mut stopped_at_hole = false;
    let mut requests = Vec::new();
    let mut rows = Vec::with_capacity(inputs.len());
    let mut checked_downloads: HashMap<String, download::Assessment> = HashMap::new();
    for (entry_id, id, song) in inputs {
        session.check_generation(token)?;
        let mut publication = None;
        let candidate = if stopped_at_hole {
            OrdinaryCandidate::NotScanned
        } else if song.is_null() {
            stopped_at_hole = true;
            OrdinaryCandidate::Undetermined(Undetermined::CatalogMetadataMissing)
        } else {
            let download = if let Some(assessment) = checked_downloads.get(&id) {
                assessment.clone()
            } else {
                let assessment = download::check(&offline, token, publications.get(&id)).await?;
                checked_downloads.insert(id.clone(), assessment.clone());
                assessment
            };
            match download {
                download::Assessment::Authorized(exact) => {
                    publication = Some(exact);
                    OrdinaryCandidate::AccountDownload
                }
                download::Assessment::Unknown => {
                    OrdinaryCandidate::Undetermined(Undetermined::DownloadPublication)
                }
                download::Assessment::FallThrough => {
                    let (choice, backup) = classify_online(song, environment);
                    if let Some(backup) = backup {
                        requests.push((rows.len(), backup));
                    }
                    choice
                }
            }
        };
        rows.push(OrdinaryCandidateRow {
            entry_id,
            no_download_publication: !publications.contains_key(&id),
            original_song_id: id,
            candidate,
            publication,
        });
    }

    if !requests.is_empty() {
        let ids: Vec<_> = requests.iter().map(|(_, id)| id.clone()).collect();
        let found = session
            .run_for(token, |api, _| async move {
                lookup::same_recordings(api, &ids).await
            })
            .await?;
        for (index, id) in requests {
            rows[index].candidate = if found.contains(&id) {
                OrdinaryCandidate::SameRecording { id }
            } else {
                OrdinaryCandidate::OriginalRetained {
                    missing_alternative: id,
                }
            };
        }
    }

    // A refile, removal, duplicate publication, or account change during a
    // slow alternative lookup cannot leave the returned evidence current.
    if publications != download::snapshot(&session, token, ids).await? {
        return Err(Error::StaleResponse);
    }
    session.check_generation(token)?;
    let mut seen = HashSet::new();
    let candidate_order = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let final_id = match &row.candidate {
                OrdinaryCandidate::AccountDownload
                | OrdinaryCandidate::Online
                | OrdinaryCandidate::OriginalRetained { .. } => &row.original_song_id,
                OrdinaryCandidate::SameRecording { id } => id,
                _ => return None,
            };
            seen.insert(final_id).then_some(index)
        })
        .collect();
    Ok(OrdinaryQueuePreflight {
        rows,
        candidate_order,
    })
}

/// M/308 runs the selected resource check before it calls the M/59 scan. Only
/// prove the branch when no managed publication could turn this into the
/// separate local/noPath workflow. This check performs no alternative lookup.
pub(super) async fn confirmed_plain_skip_denial(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    item: &super::QueueEntry,
) -> Result<bool> {
    if !super::skip_policy::plain_copyright_denial(item) {
        return Ok(false);
    }
    no_managed_publication(session, token, identifier(&item.resource["id"])?).await
}

/// M/3085 checks the clicked catalog song before M/308 adds it to the queue.
/// Only prove the common no-copyright case. Purchases, trial, substitutes,
/// cloud/local tracks and even ambiguous download publications are not denials
/// in this predicate; they still need their own source and entitlement checks.
pub(crate) async fn confirmed_plain_search_menu_denial(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    id: &str,
    song: &Value,
    privilege: Option<&Value>,
) -> Result<bool> {
    let privilege = privilege.unwrap_or(&song["privilege"]);
    if id.len() > 32
        || identifier(&song["id"]).ok().as_deref() != Some(id)
        || (!privilege["id"].is_null() && identifier(&privilege["id"]).ok().as_deref() != Some(id))
        || privilege["st"].as_i64() != Some(-200)
        || privilege["pl"].as_i64() != Some(0)
        || privilege["flag"].as_u64() != Some(0)
        || js_truthy(&privilege["cs"])
        || [
            "programId",
            "audio",
            "radio",
            "offlineId",
            "noCopyrightRcmds",
        ]
        .into_iter()
        .any(|field| js_truthy(&song[field]))
        || !super::skip_policy::plain_copyright_metadata(song, privilege)
    {
        return Ok(false);
    }
    no_managed_publication(session, token, id.to_owned()).await
}

async fn no_managed_publication(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    id: String,
) -> Result<bool> {
    let publications = download::snapshot(session, token, HashSet::from([id])).await?;
    session.check_generation(token)?;
    Ok(publications.is_empty())
}

fn classify_online(
    song: &Value,
    environment: OrdinaryQueueEnvironment,
) -> (OrdinaryCandidate, Option<String>) {
    use OrdinaryCandidate::*;
    let OrdinaryQueueEnvironment::OnlineWithoutMvExperiment { app_online } = environment else {
        return (Undetermined(self::Undetermined::Environment), None);
    };
    match online_candidate(song) {
        Ok(true) => return (Online, None),
        Ok(false) => {}
        Err(_) => return (Undetermined(self::Undetermined::MalformedPrivilege), None),
    }
    let backup = &song["noCopyrightRcmd"];
    if app_online && backup["type"].as_f64() == Some(1.0) && js_truthy(&backup["songId"]) {
        let Some(id) = identifier(&backup["songId"])
            .ok()
            .filter(|id| id.len() <= 32)
        else {
            return (Undetermined(self::Undetermined::MalformedAlternative), None);
        };
        // This temporary marker is replaced only after a successful detail
        // response. An HTTP/service failure propagates instead of selecting.
        return (Unavailable, Some(id));
    }
    (Unavailable, None)
}
