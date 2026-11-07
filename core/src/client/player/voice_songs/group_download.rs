//! M/3085 ordinary included-song batch with an authoritatively liked trigger.
use super::{MAX_TRACKS, MusicClient, actions::Row, download::Class, id};
use crate::{
    Error, Result,
    account::resources::{AccountResources, ResourceKind},
    client::{downloads::GuardedSongBatch, dto::DownloadResult},
    protocol::wire::serialization::js_truthy,
    storage::PlaybackDownloadKind,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSongGroupDownloadDecision {
    Ready,
    PlaylistRequired,
    MembershipUnknown,
    Unavailable,
    UnsupportedSelection,
    DirectoryRequired,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongGroupDownloadPlan {
    pub epoch: String,
    pub play_id: String,
    pub anchor_index: usize,
    pub eligible_indices: Vec<usize>,
    pub omitted_indices: Vec<usize>,
    pub decision: VoiceSongGroupDownloadDecision,
    pub selection_receipt: Option<String>,
}

pub(super) fn unsupported(row: &Value, song_id: &str, class: Class) -> bool {
    let privilege = &row["privilege"];
    // Do not treat a malformed/extended resource as a known ordinary row
    // merely because the single-song classifier declined to authorize it.
    matches!(
        class,
        Class::Purchased(_) | Class::ExtendedRights | Class::Cloud
    ) || [
        "programId",
        "audio",
        "radio",
        "mainTrackId",
        "offlineId",
        "filename",
        "localTrack",
        "onlineTrackId",
        "onlineTrack",
    ]
    .iter()
    .any(|field| js_truthy(&row[*field]))
        || js_truthy(&privilege["cs"])
        || js_truthy(&privilege["payed"])
        || (!privilege["id"].is_null() && id(&privilege["id"]).as_deref() != Some(song_id))
        || privilege["fee"]
            .as_f64()
            .or_else(|| privilege["fee"].as_str()?.trim().parse().ok())
            .is_some_and(|fee| fee > 0.0)
}

struct Selection {
    receipt: String,
    anchor: String,
    songs: Arc<[String]>,
    busy: AtomicBool,
    committed: OnceLock<Vec<String>>,
}

struct Lease(Arc<Selection>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

#[derive(Default)]
struct Slot {
    ticket: u64,
    selection: Option<Arc<Selection>>,
}

#[derive(Default)]
pub(super) struct Shelf(Mutex<Slot>);

impl Shelf {
    fn begin(&self) -> Result<u64> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot
            .selection
            .as_ref()
            .is_some_and(|value| value.busy.load(Ordering::Acquire))
        {
            return Err(Error::MutationBusy);
        }
        slot.ticket = slot
            .ticket
            .checked_add(1)
            .ok_or(Error::State("download selection exhausted"))?;
        slot.selection = None;
        Ok(slot.ticket)
    }

    fn install(&self, ticket: u64, value: Option<Selection>) -> Result<Option<String>> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.ticket != ticket {
            return Err(Error::StaleResponse);
        }
        let receipt = value.as_ref().map(|value| value.receipt.clone());
        slot.selection = value.map(Arc::new);
        Ok(receipt)
    }

    fn cancel(&self, receipt: &str) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let selection = slot
            .selection
            .as_ref()
            .filter(|value| value.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        if selection.busy.load(Ordering::Acquire) {
            return Err(Error::MutationBusy);
        }
        slot.selection = None;
        Ok(())
    }

    fn lease(&self, receipt: &str) -> Result<Lease> {
        let slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let value = slot
            .selection
            .as_ref()
            .filter(|value| value.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        if value.busy.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        Ok(Lease(value.clone()))
    }
}

fn ordered_rows<'a>(
    rows: &'a [Row],
    anchor: usize,
    indices: &[usize],
) -> Result<Vec<(usize, &'a Row)>> {
    const INVALID: Error = Error::InvalidInput("invalid vinyl episode song download selection");
    if !(2..=MAX_TRACKS).contains(&indices.len()) {
        return Err(INVALID);
    }
    let mut seen = BTreeSet::new();
    let selected = indices
        .iter()
        .map(|&index| {
            let row = rows
                .get(index)
                .filter(|_| seen.insert(index))
                .ok_or(INVALID)?;
            Ok((index, row))
        })
        .collect::<Result<Vec<_>>>()?;
    if !seen.contains(&anchor) {
        return Err(INVALID);
    }
    Ok(selected)
}

impl MusicClient {
    /// Freeze one ordered, native SongCard selection. No media or playlist
    /// request is issued and no local download task is written by this method.
    pub async fn player_voice_songs_download_prepare(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        anchor_index: usize,
        indices: &[usize],
    ) -> Result<VoiceSongGroupDownloadPlan> {
        use VoiceSongGroupDownloadDecision as D;
        let (page, check) = self.voice_song_download_scope(epoch, play_id, card_receipt)?;
        ordered_rows(&page.rows, anchor_index, indices)?;
        let ticket = page.group_download.begin()?;
        let local = self.local.context().await?;
        let session = self.session.clone();
        let store = session.persistent_store()?;
        let native = page.clone();
        let indices = indices.to_vec();
        let (eligible, omitted, songs, decision) = tokio::task::spawn_blocking(move || {
            let selected = ordered_rows(&native.rows, anchor_index, &indices)?;
            let anchor = &native.rows[anchor_index];
            let ids: BTreeSet<_> = selected
                .iter()
                .map(|(_, row)| row.track.id.as_str())
                .collect();
            let local = local
                .library
                .matched_online_song_ids(&session, &native.token, &ids)?;
            let statuses = session.with_token(&native.token, |owner| {
                store.read_snapshot(|tx| {
                    ids.iter()
                        .map(|&id| {
                            let statuses =
                                tx.playback_download_rows(owner, id, PlaybackDownloadKind::Song)?;
                            let complete = statuses.iter().any(|row| row.status == "complete");
                            let exists = statuses.iter().any(|row| row.status != "cancelled");
                            Ok((id, (complete, exists)))
                        })
                        .collect::<Result<BTreeMap<_, _>>>()
                })
            })?;

            let mut eligible = Vec::new();
            let mut omitted = Vec::new();
            let mut songs = Vec::new();
            let mut seen = BTreeSet::new();
            let mut unsupported = false;
            for (index, row) in selected {
                let id = row.track.id.as_str();
                let (complete, exists) = statuses[id];
                // M/3085 initial completed/local/copyright filtering occurs
                // before any purchased group branch, not the postfilter.
                if complete || local.contains(id) || row.download_copyright_blocked {
                    omitted.push(index);
                } else if row.group_download_unsupported {
                    unsupported = true;
                    omitted.push(index);
                } else if row.download == Class::Ordinary && !exists && seen.insert(id) {
                    eligible.push(index);
                    songs.push(id.to_owned());
                } else {
                    omitted.push(index);
                }
            }

            let id = anchor.track.id.as_str();
            let decision = if statuses[id].0 || local.contains(id) {
                D::Unavailable // M/556 menu hidden for this trigger.
            } else if unsupported || anchor.group_download_unsupported {
                D::UnsupportedSelection
            } else if songs.is_empty() {
                D::Unavailable
            } else {
                let (liked, operation) = AccountResources::new(session)?.membership_for(
                    &native.token,
                    ResourceKind::Liked,
                    id,
                )?;
                if operation.is_some_and(|record| record.phase.pending()) {
                    D::MembershipUnknown
                } else {
                    match liked {
                        Some(true) => D::Ready,
                        Some(false) => D::PlaylistRequired,
                        None => D::MembershipUnknown,
                    }
                }
            };
            Ok::<_, Error>((eligible, omitted, songs, decision))
        })
        .await
        .map_err(|_| Error::State("download selection worker stopped"))??;
        check()?;
        let decision = if decision == D::Ready
            && !self.configured_download_directory_available(epoch).await?
        {
            D::DirectoryRequired
        } else {
            decision
        };
        check()?;
        let selection_receipt = page.group_download.install(
            ticket,
            (decision == D::Ready).then(|| Selection {
                receipt: uuid::Uuid::new_v4().simple().to_string(),
                anchor: page.rows[anchor_index].track.id.clone(),
                songs: songs.into(),
                busy: AtomicBool::new(false),
                committed: OnceLock::new(),
            }),
        )?;
        Ok(VoiceSongGroupDownloadPlan {
            epoch: epoch.into(),
            play_id: play_id.into(),
            anchor_index,
            eligible_indices: eligible,
            omitted_indices: omitted,
            decision,
            selection_receipt,
        })
    }

    /// Discard a prepared group without starting any work.
    pub fn player_voice_songs_download_cancel(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<()> {
        let (page, _) = self.voice_song_download_scope(epoch, play_id, card_receipt)?;
        page.group_download.cancel(selection_receipt)
    }

    /// Authorize and durably enqueue the exact frozen native group once.
    pub async fn player_voice_songs_download_start(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<DownloadResult> {
        let (page, check) = self.voice_song_download_scope(epoch, play_id, card_receipt)?;
        let lease = page.group_download.lease(selection_receipt)?;
        check()?;
        if let Some(ids) = lease.0.committed.get() {
            return Ok(DownloadResult {
                epoch: epoch.into(),
                accepted: ids.clone(),
                operation_id: Some(selection_receipt.into()),
                ..Default::default()
            });
        }
        let local = self.local.context().await?;
        let session = self.session.clone();
        let token = page.token.clone();
        let songs = lease.0.songs.clone();
        let anchor = lease.0.anchor.clone();
        let checked_anchor = anchor.clone();
        let options = self.download_quality_preferences(epoch)?.options;
        self.start_guarded_song_batch(
            epoch,
            GuardedSongBatch {
                receipt: selection_receipt.into(),
                anchor,
                songs: songs.clone(),
                options,
            },
            move || {
                let ids = songs
                    .iter()
                    .map(String::as_str)
                    .chain([checked_anchor.as_str()])
                    .collect();
                let lock = local
                    .library
                    .hold_unmatched_online_songs(&session, &token, &ids)?;
                check()?;
                Ok(lock)
            },
            move |ids| {
                let _ = lease.0.committed.set(ids.to_vec());
                drop(lease);
            },
        )
        .await
    }
}
