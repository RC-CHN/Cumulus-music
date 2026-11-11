//! In-memory purchase evidence from the exact native ordinary playback read.
//! A saved queue, playback fee, or renderer DTO cannot mint this observation.
use super::{MusicClient, PlaybackSnapshot, PlaybackStatus, identity};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::PlayerPurchaseDetail,
    client::{
        downloads::purchased::{Offer, vivid_available},
        focus::Owner,
        history::song_download::download_picker_collectable,
    },
    membership::Quality,
    playback::TimelineMode,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy)]
pub(super) enum Rights {
    Ordinary,
    Purchased(Offer),
}

pub(super) struct Candidate {
    api: Arc<dyn Api>,
    revision: String,
    id: String,
    rights: Rights,
    picker_collectable: bool,
}

fn exact_id(value: &Value, id: &str) -> bool {
    !id.starts_with('0') && id.len() <= 128 && identifier(value).ok().as_deref() == Some(id)
}

fn external(row: &Value) -> bool {
    [
        "cs",
        "cloudSong",
        "local_id",
        "localTrack",
        "localId",
        "fileId",
        "filename",
        "path",
        "cloud_song_id",
        "cloudSongId",
        "voice_id",
        "voiceId",
        "programId",
        "audio",
        "radio",
        "mainTrackId",
        "download_task_id",
        "offlineId",
        "onlineTrack",
        "isOfflineDownOnline",
    ]
    .into_iter()
    .any(|key| js_truthy(&row[key]))
        || ["t", "songType"]
            .into_iter()
            .any(|key| row[key].as_i64() == Some(1) || row[key].as_str() == Some("1"))
}

fn download_not_blocked(value: &Value) -> bool {
    value.is_null() || value.as_bool() == Some(false) || value.as_u64() == Some(0)
}

fn ordinary_download_right(song: &Value, privilege: &Value) -> bool {
    // M/7:85–92 defines the six catalogue fee bits. M/27.e requires
    // actual dl > 0, M/21.f tests fee && !payed, and M/35.h tests
    // payed & 6 for the *separate* purchase chooser. A positive fee
    // with a nonpurchase payed bit can therefore reach stock download.
    let (Some(fee), Some(payed)) = (
        privilege["fee"].as_u64().filter(|fee| *fee <= 0b11_1111),
        privilege["payed"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok()),
    ) else {
        return false;
    };
    let nested = &song["privilege"];
    payed & 6 == 0
        && (fee == 0 || payed != 0)
        && privilege["st"].as_i64().is_some_and(|status| status >= 0)
        && privilege["dl"].as_u64().is_some_and(|level| level > 0)
        && (nested["dl"].is_null() || nested["dl"].as_u64().is_some_and(|level| level > 0))
        && [("fee", fee), ("payed", u64::from(payed))]
            .into_iter()
            .all(|(key, value)| nested[key].is_null() || nested[key].as_u64() == Some(value))
}

impl Candidate {
    pub(super) fn from_detail(
        id: &str,
        api: Arc<dyn Api>,
        detail: PlayerPurchaseDetail,
    ) -> Option<Self> {
        let song = &detail.song;
        let privilege = &detail.privilege;
        if detail.revision.is_empty()
            || !exact_id(&song["id"], id)
            || !exact_id(&privilege["id"], id)
            || !download_not_blocked(&privilege["bd"])
            || !download_not_blocked(&song["privilege"]["bd"])
            || [song, privilege, &song["privilege"]]
                .into_iter()
                .any(|row| {
                    external(row)
                        || (js_truthy(&row["onlineTrackId"])
                            && !exact_id(&row["onlineTrackId"], id))
                        || ["st", "status"].into_iter().any(|key| {
                            row[key]
                                .as_i64()
                                .or_else(|| row[key].as_str()?.parse().ok())
                                .is_some_and(|value| value < 0)
                        })
                })
        {
            return None;
        }
        let rights = if let Some(offer) = Offer::from_privilege(privilege) {
            Rights::Purchased(offer)
        } else if ordinary_download_right(song, privilege) {
            Rights::Ordinary
        } else {
            return None;
        };
        Some(Self {
            rights,
            picker_collectable: download_picker_collectable(
                &json!({"id": id, "privilege": privilege}),
            ),
            api,
            revision: detail.revision,
            id: id.into(),
        })
    }
}

#[derive(Clone)]
pub(super) struct Observation {
    pub(super) account: SessionToken,
    pub(super) api: Weak<dyn Api>,
    pub(super) revision: String,
    pub(super) selection: u64,
    pub(super) play_id: String,
    pub(super) entry_id: String,
    pub(super) song_id: String,
    pub(super) rights: Rights,
    pub(super) picker_collectable: bool,
}

pub(super) fn ordinary_entry<'a>(
    state: &'a PlaybackSnapshot,
    account: &SessionToken,
    id: &str,
) -> Option<&'a str> {
    let entry = state.selected.as_ref()?;
    (&state.account == account
        && account.uid.is_some()
        && !state.fm
        && state.intelligence.is_none()
        && !state.catalog_file_source
        && entry.visible_in_queue()
        && entry.source["kind"] != "listen_together"
        && entry.resource["shared_room"].is_null()
        && entry.resource["playback_constraint"].is_null()
        && entry.resource["missing"] != true
        && identity::online_song(&entry.resource, id)
        && exact_id(&entry.resource["song"]["id"], id))
    .then_some(entry.entry_id.as_str())
}

#[derive(Default)]
pub(super) struct Host(Mutex<Option<Observation>>);

impl Host {
    pub(super) fn observe(
        &self,
        generation: &AtomicU64,
        state: &PlaybackSnapshot,
        account: &SessionToken,
        selection: u64,
        candidate: Candidate,
    ) {
        let (Some(play_id), Some(entry_id)) = (
            state.play_id.as_ref(),
            ordinary_entry(state, account, &candidate.id),
        ) else {
            return;
        };
        if let Ok(mut slot) = self.0.lock()
            && generation.load(Ordering::Acquire) == selection
        {
            *slot = Some(Observation {
                account: account.clone(),
                api: Arc::downgrade(&candidate.api),
                revision: candidate.revision,
                selection,
                play_id: play_id.clone(),
                entry_id: entry_id.into(),
                song_id: candidate.id,
                rights: candidate.rights,
                picker_collectable: candidate.picker_collectable,
            });
        }
    }

    pub(super) fn current(
        &self,
        account: &SessionToken,
        selection: u64,
        play_id: &str,
        entry_id: &str,
    ) -> Result<Option<Observation>> {
        let slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        // A reader may see the old watch publication just as a new selection
        // records its evidence. A nonmatching read may never retire that slot.
        Ok(slot
            .as_ref()
            .filter(|record| {
                &record.account == account
                    && record.selection == selection
                    && record.play_id == play_id
                    && record.entry_id == entry_id
            })
            .cloned())
    }

    /// A completed pause or seek can keep the verified catalogue row. Its
    /// transport intent still cancels older work and expires any chooser.
    pub(super) fn retain_after_transport(
        &self,
        generation: &AtomicU64,
        state: &PlaybackSnapshot,
        account: &SessionToken,
        intent: u64,
    ) {
        if let Ok(mut slot) = self.0.lock()
            && let Some(record) = slot.as_mut()
            && &record.account == account
            && record.selection.checked_add(1) == Some(intent)
            && state.play_id.as_deref() == Some(record.play_id.as_str())
            && ordinary_entry(state, account, &record.song_id) == Some(record.entry_id.as_str())
            && generation.load(Ordering::Acquire) == intent
        {
            record.selection = intent;
        }
    }

    pub(super) fn retire(&self, record: &Observation) {
        if let Ok(mut slot) = self.0.lock()
            && slot.as_ref().is_some_and(|current| {
                current.account == record.account
                    && current.selection == record.selection
                    && current.play_id == record.play_id
                    && current.entry_id == record.entry_id
                    && current.revision == record.revision
            })
        {
            *slot = None;
        }
    }
}

/// Read-only view for a future native chooser. This is deliberately not a
/// mutation receipt or a serializable renderer value.
pub struct CurrentSongPurchase {
    pub song_id: String,
    pub qualities: Vec<Quality>,
    /// The M3085 Song filter only, never a playlist or mutation entitlement.
    pub picker_collectable: bool,
}

impl MusicClient {
    pub fn current_player_song_purchase(
        &self,
        epoch: &str,
        play_id: &str,
        entry_id: &str,
    ) -> Result<Option<CurrentSongPurchase>> {
        self.auth.pending_guard()()?;
        let account = self.player_token(epoch)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let selection = self.player.selection.load(Ordering::Acquire);
        let check_player = |state: &PlaybackSnapshot| {
            if state.account != account
                || state.play_id.as_deref() != Some(play_id)
                || state.selected.as_ref().map(|entry| entry.entry_id.as_str()) != Some(entry_id)
                || self.player.selection.load(Ordering::Acquire) != selection
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        let state = controller.snapshot();
        check_player(&state)?;
        let Some(record) = self
            .player
            .song_purchase
            .current(&account, selection, play_id, entry_id)?
        else {
            return Ok(None);
        };
        let Rights::Purchased(offer) = record.rights else {
            return Ok(None);
        };
        let eligible = |state: &PlaybackSnapshot| {
            matches!(
                state.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            ) && state
                .timeline
                .as_ref()
                .is_some_and(|timeline| timeline.mode == TimelineMode::Full)
                && ordinary_entry(state, &account, &record.song_id) == Some(entry_id)
        };
        let focus = self.focus.snapshot()?;
        if !eligible(&state)
            || focus.owner != Some(Owner::Audio)
            || self.together_attached(&account, None)?
        {
            return Ok(None);
        }
        let api = record.api.upgrade().ok_or(Error::StaleResponse)?;
        let result = self.session.with_verified_profile(&account, &api, |_, _| {
            if api
                .player_song_purchase_credential_revision()
                .map_err(|_| Error::StaleResponse)?
                .as_deref()
                != Some(record.revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            let groups = self
                .pc_experiments
                .observed_groups(&account, &api, &record.revision)?;
            let state = controller.snapshot();
            check_player(&state)?;
            if !eligible(&state) {
                return Ok(None);
            }
            let qualities = offer.qualities(vivid_available(groups.as_ref()));
            if qualities.is_empty() {
                Ok(None)
            } else {
                Ok(Some(CurrentSongPurchase {
                    song_id: record.song_id.clone(),
                    qualities,
                    picker_collectable: record.picker_collectable,
                }))
            }
        });
        if matches!(&result, Err(Error::StaleResponse)) {
            self.player.song_purchase.retire(&record);
        }
        let purchase = result?;
        if self.focus.snapshot()? != focus || self.together_attached(&account, None)? {
            return Err(Error::StaleResponse);
        }
        self.auth.pending_guard()()?;
        let state = controller.snapshot();
        check_player(&state)?;
        self.session.check_generation(&account)?;
        Ok(purchase.filter(|_| eligible(&state)))
    }
}
