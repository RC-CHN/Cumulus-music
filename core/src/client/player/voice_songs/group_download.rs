//! M/3085 included-song batches with a liked trigger or explicit destination.
mod playlist;
use super::{
    MAX_TRACKS, MusicClient,
    actions::{Page, Row},
    collect::Privilege,
    download::Class,
    id,
    purchased::{Offer, purchased_quality_guide_url, vivid_enabled},
};
use crate::{
    Error, Result,
    account::resources::{AccountResources, ResourceKind},
    client::{downloads::GuardedSongBatch, dto::DownloadResult},
    download::service::SongBatchAdmission,
    membership::Quality,
    protocol::wire::serialization::js_truthy,
    storage::PlaybackDownloadKind,
};
pub use playlist::{VoiceSongGroupPlaylistCreation, VoiceSongGroupPlaylistResult};
use serde::Serialize;
use serde_json::Value;
use std::{
    cmp::Reverse,
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
    PurchasedQualityRequired,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub purchased_qualities: Vec<Quality>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceSongGroupDownloadConfirmation {
    Started { result: DownloadResult },
    PlaylistRequired,
    MembershipUnknown,
    DirectoryRequired,
}

pub(super) fn unsupported(row: &Value, song_id: &str, class: Class) -> bool {
    let privilege = &row["privilege"];
    // Do not treat a malformed/extended resource as a known ordinary row
    // merely because the single-song classifier declined to authorize it.
    matches!(class, Class::ExtendedRights | Class::Cloud)
        || [
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
        || (!privilege["id"].is_null() && id(&privilege["id"]).as_deref() != Some(song_id))
        || (!matches!(class, Class::Purchased(_))
            && (js_truthy(&privilege["payed"])
                || privilege["fee"]
                    .as_f64()
                    .or_else(|| privilege["fee"].as_str()?.trim().parse().ok())
                    .is_some_and(|fee| fee > 0.0)))
}

struct Commit {
    quality: Option<Quality>,
    ids: Vec<String>,
}

struct Selection {
    receipt: String,
    anchor: String,
    songs: Arc<[String]>,
    collection: Arc<[String]>,
    destination_enabled: bool,
    destination: Mutex<playlist::State>,
    purchased_qualities: Vec<Quality>,
    busy: AtomicBool,
    committed: OnceLock<Commit>,
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

fn membership(
    session: Arc<crate::account::AccountSession>,
    token: &crate::account::SessionToken,
    anchor: &str,
) -> Result<VoiceSongGroupDownloadDecision> {
    use VoiceSongGroupDownloadDecision as D;
    let (liked, operation) =
        AccountResources::new(session)?.membership_for(token, ResourceKind::Liked, anchor)?;
    Ok(
        match liked.filter(|_| !operation.is_some_and(|record| record.phase.pending())) {
            Some(true) => D::Ready,
            Some(false) => D::PlaylistRequired,
            None => D::MembershipUnknown,
        },
    )
}

fn purchased_qualities(offers: Vec<Offer>, vivid: bool) -> Vec<Quality> {
    let mut union = offers
        .into_iter()
        .flat_map(|offer| offer.qualities(vivid))
        .collect::<Vec<_>>();
    union.sort_unstable_by_key(|quality| Reverse(quality.bitrate()));
    union.dedup();
    union
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
        self.prepare_voice_song_group(epoch, play_id, card_receipt, anchor_index, indices, false)
            .await
    }

    /// Prepare a native picker. The existing desktop prepare intentionally
    /// continues to treat PlaylistRequired as a blocked plan.
    pub async fn player_voice_songs_download_prepare_with_destination(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        anchor_index: usize,
        indices: &[usize],
    ) -> Result<VoiceSongGroupDownloadPlan> {
        self.prepare_voice_song_group(epoch, play_id, card_receipt, anchor_index, indices, true)
            .await
    }

    async fn prepare_voice_song_group(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        anchor_index: usize,
        indices: &[usize],
        destination_enabled: bool,
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
        let (eligible, omitted, songs, collection, offers, decision) =
            tokio::task::spawn_blocking(move || {
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
                                let statuses = tx.playback_download_rows(
                                    owner,
                                    id,
                                    PlaybackDownloadKind::Song,
                                )?;
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
                let mut collection = Vec::new();
                let mut collected = BTreeSet::new();
                let mut survivors = BTreeMap::new();
                let mut seen = BTreeSet::new();
                let mut unsupported = false;
                for (index, row) in selected {
                    let id = row.track.id.as_str();
                    let (complete, exists) = statuses[id];
                    // M/3085 initial completed/local/copyright filtering occurs
                    // before any purchased group branch, not the postfilter.
                    if complete || local.contains(id) || row.download_copyright_blocked {
                        omitted.push(index);
                        continue;
                    } else if row.group_download_unsupported {
                        unsupported = true;
                        omitted.push(index);
                        continue;
                    } else {
                        // The download picker passes the initial survivors, not
                        // the task-ledger subset or ordinary collect selectedData.
                        // M/3085's second filter keeps absent/falsy/positive subPriv;
                        // the favorite's two reversals restore this relative order.
                        if matches!(
                            row.collection_privilege,
                            Privilege::Preferred | Privilege::FirstFallback
                        ) && collected.insert(id)
                        {
                            collection.push(id.to_owned());
                        }
                    }
                    if matches!(row.download, Class::Ordinary | Class::Purchased(_)) {
                        // M/3085 forms the purchase union from initial survivors,
                        // even if the ordinary task ledger later suppresses a row.
                        if survivors
                            .insert(id, row.download)
                            .is_some_and(|previous| previous != row.download)
                        {
                            unsupported = true;
                        }
                        if !exists && seen.insert(id) {
                            eligible.push(index);
                            songs.push(id.to_owned());
                        } else {
                            omitted.push(index);
                        }
                    } else {
                        omitted.push(index);
                    }
                }

                let offers = survivors
                    .values()
                    .filter_map(|class| match class {
                        Class::Purchased(offer) => Some(*offer),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let id = anchor.track.id.as_str();
                let decision = if statuses[id].0 || local.contains(id) {
                    D::Unavailable // M/556 menu hidden for this trigger.
                } else if unsupported || anchor.group_download_unsupported {
                    D::UnsupportedSelection
                } else if songs.is_empty() {
                    D::Unavailable
                } else if offers.is_empty() {
                    membership(session, &native.token, id)?
                } else if survivors.len() >= 2 {
                    // The official chooser precedes clicked-anchor membership
                    // and directory checks. No choice commits or queues yet.
                    D::PurchasedQualityRequired
                } else {
                    // The original single-survivor preflight also inspects the
                    // clicked resource; leave it to the existing single lane.
                    D::UnsupportedSelection
                };
                Ok::<_, Error>((eligible, omitted, songs, collection, offers, decision))
            })
            .await
            .map_err(|_| Error::State("download selection worker stopped"))??;
        check()?;
        let qualities = if decision == D::PurchasedQualityRequired {
            purchased_qualities(offers, vivid_enabled(&self.pc_experiments, &page)?)
        } else {
            Vec::new()
        };
        let decision = if (decision == D::PurchasedQualityRequired && qualities.is_empty())
            || (destination_enabled && decision == D::PlaylistRequired && collection.is_empty())
        {
            D::UnsupportedSelection
        } else if decision == D::Ready
            && !self.configured_download_directory_available(epoch).await?
        {
            D::DirectoryRequired
        } else {
            decision
        };
        check()?;
        let selection_receipt = page.group_download.install(
            ticket,
            (matches!(decision, D::Ready | D::PurchasedQualityRequired)
                || destination_enabled && decision == D::PlaylistRequired)
                .then(|| Selection {
                    receipt: uuid::Uuid::new_v4().simple().to_string(),
                    anchor: page.rows[anchor_index].track.id.clone(),
                    songs: songs.into(),
                    collection: collection.into(),
                    destination_enabled,
                    destination: Mutex::new(playlist::State::new(decision == D::PlaylistRequired)),
                    purchased_qualities: qualities.clone(),
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
            purchased_qualities: qualities,
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

    /// Authorize and durably enqueue the exact ordinary native group once.
    pub async fn player_voice_songs_download_start(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<DownloadResult> {
        let VoiceSongGroupDownloadConfirmation::Started { result } = self
            .start_voice_song_group(epoch, play_id, card_receipt, selection_receipt, None)
            .await?
        else {
            return Err(Error::State("ordinary group returned a purchase decision"));
        };
        Ok(result)
    }

    /// Resolve a fixed public browser handoff only for the live group chooser.
    /// This never reads the network or returns credentials.
    pub fn voice_song_group_purchased_quality_guide(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<&'static str> {
        let (page, check) = self.voice_song_download_scope(epoch, play_id, card_receipt)?;
        let lease = page.group_download.lease(selection_receipt)?;
        let vivid = vivid_enabled(&self.pc_experiments, &page)?;
        if lease.0.committed.get().is_some()
            || lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .quality
                .is_some()
            || !lease
                .0
                .purchased_qualities
                .iter()
                .any(|quality| *quality != Quality::Vivid || vivid)
        {
            return Err(Error::State(
                "group has no current purchased quality chooser",
            ));
        }
        check()?;
        Ok(purchased_quality_guide_url(vivid))
    }

    /// Explicitly confirm one of this native selection's purchased tiers.
    /// The same offered quality is frozen before any destination can be chosen.
    pub async fn player_voice_songs_download_confirm_quality(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        quality: Quality,
    ) -> Result<VoiceSongGroupDownloadConfirmation> {
        self.start_voice_song_group(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Some(quality),
        )
        .await
    }

    async fn start_voice_song_group(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        quality: Option<Quality>,
    ) -> Result<VoiceSongGroupDownloadConfirmation> {
        use VoiceSongGroupDownloadConfirmation as C;
        use VoiceSongGroupDownloadDecision as D;
        let (page, scope) = self.voice_song_download_scope(epoch, play_id, card_receipt)?;
        let lease = page.group_download.lease(selection_receipt)?;
        scope()?;
        let offered = &lease.0.purchased_qualities;
        if !quality.map_or(offered.is_empty(), |quality| offered.contains(&quality)) {
            return Err(Error::InvalidInput(
                "group download requires an offered quality",
            ));
        }
        if let Some(committed) = lease.0.committed.get() {
            if committed.quality != quality {
                return Err(Error::InvalidInput(
                    "group download quality is already committed",
                ));
            }
            return Ok(C::Started {
                result: DownloadResult {
                    epoch: epoch.into(),
                    accepted: committed.ids.clone(),
                    operation_id: Some(selection_receipt.into()),
                    ..Default::default()
                },
            });
        }
        if let Some(frozen) = lease
            .0
            .destination
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .quality
        {
            if frozen != quality {
                return Err(Error::InvalidInput(
                    "group destination quality is already selected",
                ));
            }
            return Ok(C::PlaylistRequired);
        }
        let experiments = self.pc_experiments.clone();
        let bound = page.clone();
        let check = move || {
            scope()?;
            if quality == Some(Quality::Vivid) && !vivid_enabled(&experiments, &bound)? {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        let session = self.session.clone();
        let token = page.token.clone();
        let anchor = lease.0.anchor.clone();
        if quality.is_some() {
            let decision = {
                let (session, token, anchor) = (session.clone(), token.clone(), anchor.clone());
                tokio::task::spawn_blocking(move || membership(session, &token, &anchor))
                    .await
                    .map_err(|_| Error::State("download membership worker stopped"))??
            };
            check()?;
            match decision {
                D::Ready => {}
                D::PlaylistRequired => {
                    if lease.0.destination_enabled {
                        if lease.0.collection.is_empty() {
                            return Err(Error::State("no group songs can be added to a playlist"));
                        }
                        lease
                            .0
                            .destination
                            .lock()
                            .map_err(|_| Error::LockPoisoned)?
                            .quality = Some(quality);
                    }
                    return Ok(C::PlaylistRequired);
                }
                D::MembershipUnknown => return Ok(C::MembershipUnknown),
                _ => return Err(Error::State("invalid group membership decision")),
            }
            let directory = self.configured_download_directory_available(epoch).await?;
            check()?;
            if !directory {
                return Ok(C::DirectoryRequired);
            }
        }
        let songs = lease.0.songs.clone();
        let result = self
            .enqueue_voice_song_group(
                epoch,
                &page,
                lease,
                songs,
                (quality, SongBatchAdmission::LikedAnchor(anchor)),
                check,
            )
            .await?;
        Ok(C::Started { result })
    }

    async fn enqueue_voice_song_group(
        &self,
        epoch: &str,
        page: &Page,
        lease: Lease,
        songs: Arc<[String]>,
        (quality, admission): (Option<Quality>, SongBatchAdmission),
        check: impl Fn() -> Result<()> + Send + 'static,
    ) -> Result<DownloadResult> {
        let local = self.local.context().await?;
        let session = self.session.clone();
        let token = page.token.clone();
        let checked_anchor = lease.0.anchor.clone();
        let selection_receipt = lease.0.receipt.clone();
        let mut options = self.download_quality_preferences(epoch)?.options;
        if let Some(quality) = quality {
            // M/754 sends the one chosen tier for every new authorized row.
            // The service, not a local per-track clamp, decides if it allows it.
            options.quality = quality;
        }
        self.start_guarded_song_batch(
            epoch,
            GuardedSongBatch {
                receipt: selection_receipt,
                admission,
                existing: Default::default(),
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
                let _ = lease.0.committed.set(Commit {
                    quality,
                    ids: ids.to_vec(),
                });
                drop(lease);
            },
        )
        .await
    }
}
