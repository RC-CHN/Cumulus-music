//! Owned-playlist and system-liked destinations of the episode SongCard FavouriteModal.
use super::{MAX_TRACKS, MusicClient, actions::Page, id, stale_credentials};
use crate::{
    Error, Result,
    client::{
        content::PLAYLIST_COLLECT_LAST_PAGE,
        dto::{AccountData, PlaylistCollectPage, PlaylistCollectSort},
    },
    playlist::{PlaylistCommand, PlaylistMutation, PlaylistPhase, PlaylistService, TrackChange},
    protocol::wire::serialization::js_truthy,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Privilege {
    Preferred,
    FirstFallback,
    UnsupportedPreferred,
    Unavailable,
}

impl Privilege {
    pub(super) fn from_song(row: &Value, track_id: &str) -> Self {
        let p = &row["privilege"];
        let ordinary = (p.is_object() || p.is_null())
            && (p["id"].is_null() || id(&p["id"]).as_deref() == Some(track_id))
            && ![
                "programId",
                "audio",
                "radio",
                "mainTrackId",
                "localTrack",
                "onlineTrackId",
            ]
            .into_iter()
            .any(|key| js_truthy(&row[key]))
            && !js_truthy(&p["cs"]);
        // Preserve the existence of an original preferred row even if we
        // cannot safely write its native identity. Omitting one must not
        // accidentally promote a different, zero-rights row to the fallback.
        let preferred = if ordinary {
            Self::Preferred
        } else {
            Self::UnsupportedPreferred
        };
        let Some(value) = p.get("subp") else {
            return preferred;
        };
        let number = value
            .as_f64()
            .or_else(|| value.as_str()?.trim().parse().ok());
        if number.is_some_and(|number| number.is_finite() && number > 0.0) {
            preferred
        } else if value.is_null() || value.as_f64() == Some(0.0) || value.as_str() == Some("") {
            if ordinary {
                Self::FirstFallback
            } else {
                Self::Unavailable
            }
        } else if number.is_some_and(|number| number.is_finite() && number <= 0.0) {
            Self::Unavailable
        } else {
            // An untyped value may coerce positive in the original JS.
            // It cannot grant native authority or activate the fallback.
            Self::UnsupportedPreferred
        }
    }

    fn preferred_in_group(self) -> bool {
        matches!(self, Self::Preferred | Self::UnsupportedPreferred)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongCollectPlan {
    pub epoch: String,
    pub play_id: String,
    pub selection_receipt: Option<String>,
    pub selected_count: usize,
    pub eligible_count: usize,
    pub omitted_indices: Vec<usize>,
    pub used_first_fallback: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongCollectResult {
    pub epoch: String,
    pub play_id: String,
    pub selection_receipt: String,
    pub playlist_id: String,
    pub operation_id: String,
    pub phase: PlaylistPhase,
}

struct Selection {
    receipt: String,
    songs: Arc<[String]>,
    destinations: BTreeMap<(&'static str, u32), DestinationPage>,
    next_destination_ticket: u64,
    attempt: Option<Attempt>,
    busy: bool,
}

struct DestinationPage {
    ticket: u64,
    ids: Vec<String>,
}

#[derive(Clone)]
struct Attempt {
    destination: Destination,
    operation: String,
}

#[derive(Clone, PartialEq, Eq)]
enum Destination {
    Owned(String),
    SystemLiked,
}

impl Destination {
    fn owned_id(&self) -> Option<&str> {
        match self {
            Self::Owned(id) => Some(id),
            Self::SystemLiked => None,
        }
    }
}

#[derive(Default)]
pub(super) struct Shelf(Mutex<Option<Selection>>);

struct Lease<'a> {
    shelf: &'a Shelf,
    receipt: &'a str,
    songs: Arc<[String]>,
    attempt: Attempt,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.shelf.0.lock()
            && let Some(active) = slot
                .as_mut()
                .filter(|active| active.receipt == self.receipt)
        {
            active.busy = false;
        }
    }
}

fn uuid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl Shelf {
    fn replace(
        &self,
        service: &PlaylistService,
        page: &Page,
        songs: Vec<String>,
    ) -> Result<Option<String>> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(active) = slot.as_ref() {
            if active.busy {
                return Err(Error::MutationBusy);
            }
            if let Some(attempt) = &active.attempt
                && service
                    .operation_for(&page.token, &attempt.operation)?
                    .is_some_and(|record| record.phase.pending())
            {
                return Err(Error::MutationUncertain);
            }
        }
        *slot = (!songs.is_empty()).then(|| Selection {
            receipt: uuid(),
            songs: songs.into(),
            destinations: BTreeMap::new(),
            next_destination_ticket: 0,
            attempt: None,
            busy: false,
        });
        Ok(slot.as_ref().map(|active| active.receipt.clone()))
    }

    fn with_selection<T>(
        &self,
        receipt: &str,
        action: impl FnOnce(&mut Selection) -> Result<T>,
    ) -> Result<T> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active = slot
            .as_mut()
            .filter(|active| active.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        action(active)
    }

    fn lease<'a>(
        &'a self,
        service: &PlaylistService,
        page: &Page,
        receipt: &'a str,
        destination: Option<Destination>,
    ) -> Result<(Lease<'a>, Option<PlaylistMutation>)> {
        self.with_selection(receipt, |active| {
            if active.busy {
                return Err(Error::MutationBusy);
            }
            let previous = active
                .attempt
                .as_ref()
                .map(|attempt| service.operation_for(&page.token, &attempt.operation))
                .transpose()?
                .flatten();
            if let Some(destination) = destination {
                if let Some(playlist) = destination.owned_id()
                    && !active
                        .destinations
                        .values()
                        .flat_map(|page| &page.ids)
                        .any(|id| id == playlist)
                {
                    return Err(Error::StaleResponse);
                }
                if let Some(attempt) = &active.attempt
                    && attempt.destination != destination
                    && previous.is_some()
                {
                    // Once journaled, a selection can never silently choose
                    // another destination. A new explicit modal is needed.
                    return Err(Error::StaleResponse);
                }
                if previous.is_none() {
                    active.attempt = Some(Attempt {
                        destination,
                        operation: uuid(),
                    });
                }
            }
            let attempt = active.attempt.clone().ok_or(Error::StaleResponse)?;
            active.busy = true;
            Ok((
                Lease {
                    shelf: self,
                    receipt,
                    songs: active.songs.clone(),
                    attempt,
                },
                previous,
            ))
        })
    }
}

fn result(
    page: &Page,
    lease: &Lease<'_>,
    record: PlaylistMutation,
) -> Result<VoiceSongCollectResult> {
    let attempt = &lease.attempt;
    let playlist_id = record
        .playlist_id
        .as_ref()
        .ok_or(Error::State("collection playlist missing"))?;
    if record.operation_id != attempt.operation
        || record.system_liked_group != (attempt.destination == Destination::SystemLiked)
        || attempt
            .destination
            .owned_id()
            .is_some_and(|owned| owned != playlist_id)
        || !matches!(&record.command, PlaylistCommand::Tracks { id, change: TrackChange::Add, ids, .. }
            if id == playlist_id && ids.as_slice() == lease.songs.as_ref())
    {
        return Err(Error::State(
            "collection completion belongs to a different playlist action",
        ));
    }
    Ok(VoiceSongCollectResult {
        epoch: page.token.generation.to_string(),
        play_id: page.play_id.clone(),
        selection_receipt: lease.receipt.into(),
        playlist_id: playlist_id.clone(),
        operation_id: record.operation_id,
        phase: record.phase,
    })
}

impl MusicClient {
    fn voice_song_collect_scope(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
    ) -> Result<(
        Arc<Page>,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        self.voice_song_write_scope(epoch, play_id, receipt, |api| {
            api.song_card_collection_scope_revision()
        })
    }

    /// Freeze the renderer's ordered row selection. All song IDs and rights
    /// come from the current native SongCard, never from the caller.
    pub fn player_voice_songs_collect_prepare(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        indices: &[usize],
    ) -> Result<VoiceSongCollectPlan> {
        let (page, check) = self.voice_song_collect_scope(epoch, play_id, card_receipt)?;
        let mut seen_indices = HashSet::new();
        if !(2..=MAX_TRACKS).contains(&indices.len())
            || !indices
                .iter()
                .all(|&i| i < page.rows.len() && seen_indices.insert(i))
        {
            return Err(Error::InvalidInput("invalid vinyl episode song selection"));
        }
        let preferred = indices
            .iter()
            .any(|&i| page.rows[i].collection_privilege.preferred_in_group());
        let used_first_fallback =
            !preferred && page.rows[indices[0]].collection_privilege == Privilege::FirstFallback;
        let mut seen_songs = HashSet::new();
        let mut songs = Vec::new();
        let mut omitted_indices = Vec::new();
        for (position, &index) in indices.iter().enumerate() {
            let row = &page.rows[index];
            if (row.collection_privilege == Privilege::Preferred
                || (used_first_fallback && position == 0))
                && seen_songs.insert(&row.track.id)
            {
                songs.push(row.track.id.clone());
            } else {
                omitted_indices.push(index);
            }
        }
        let eligible_count = songs.len();
        let service = PlaylistService::new(self.session.clone())?;
        check()?;
        let selection_receipt = page.collect.replace(&service, &page, songs)?;
        check()?;
        Ok(VoiceSongCollectPlan {
            epoch: epoch.into(),
            play_id: play_id.into(),
            selection_receipt,
            selected_count: indices.len(),
            eligible_count,
            omitted_indices,
            used_first_fallback,
        })
    }

    /// Record only owned regular destinations actually observed by this
    /// selection. Loading a page cannot cause a write or consume the receipt.
    pub async fn player_voice_songs_collect_destinations(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        sort: PlaylistCollectSort,
        number: u32,
    ) -> Result<AccountData<PlaylistCollectPage>> {
        let (page, check) = self.voice_song_collect_scope(epoch, play_id, card_receipt)?;
        if number > PLAYLIST_COLLECT_LAST_PAGE {
            return Err(Error::InvalidInput("invalid playlist collection page"));
        }
        let key = (sort.wire(), number);
        let ticket = page.collect.with_selection(selection_receipt, |active| {
            if active.busy {
                return Err(Error::MutationBusy);
            }
            let ticket = active
                .next_destination_ticket
                .checked_add(1)
                .ok_or(Error::State("collection destination reads exhausted"))?;
            active.next_destination_ticket = ticket;
            active.destinations.insert(
                key,
                DestinationPage {
                    ticket,
                    ids: Vec::new(),
                },
            );
            Ok(ticket)
        })?;
        let destinations = self.playlist_collect_page(epoch, sort, number).await?;
        check()?;
        page.collect.with_selection(selection_receipt, |active| {
            let current = active
                .destinations
                .get_mut(&key)
                .filter(|pending| pending.ticket == ticket)
                .ok_or(Error::StaleResponse)?;
            current.ids = destinations
                .value
                .playlists
                .iter()
                .map(|item| item.id.clone())
                .collect();
            Ok(())
        })?;
        Ok(destinations)
    }

    pub async fn player_voice_songs_collect_existing(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        playlist_id: &str,
    ) -> Result<VoiceSongCollectResult> {
        let (page, check) = self.voice_song_collect_scope(epoch, play_id, card_receipt)?;
        let service = PlaylistService::new(self.session.clone())?;
        let (lease, previous) = page.collect.lease(
            &service,
            &page,
            selection_receipt,
            Some(Destination::Owned(playlist_id.into())),
        )?;
        let record = if let Some(previous) = previous {
            previous
        } else {
            service
                .guarded_collect_songs(
                    &page.token,
                    playlist_id,
                    &lease.songs,
                    &lease.attempt.operation,
                    &check,
                )
                .await
                .map_err(stale_credentials)?
        };
        check()?;
        result(&page, &lease, record)
    }

    /// The system-liked destination is resolved by native account reads. A
    /// renderer cannot substitute a PID or turn this into individual hearts.
    pub async fn player_voice_songs_collect_liked(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<VoiceSongCollectResult> {
        let (page, check) = self.voice_song_write_scope(epoch, play_id, card_receipt, |api| {
            api.song_card_system_liked_scope_revision()
        })?;
        let service = PlaylistService::new(self.session.clone())?;
        let (lease, previous) = page.collect.lease(
            &service,
            &page,
            selection_receipt,
            Some(Destination::SystemLiked),
        )?;
        let record = if let Some(previous) = previous {
            previous
        } else {
            service
                .guarded_collect_system_liked(
                    &page.token,
                    &lease.songs,
                    &lease.attempt.operation,
                    &check,
                )
                .await
                .map_err(stale_credentials)?
        };
        check()?;
        result(&page, &lease, record)
    }

    /// Same current selection and owned destination only. This never sends
    /// another add, including after an ambiguous first response.
    pub async fn player_voice_songs_collect_status(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        reconcile: bool,
    ) -> Result<VoiceSongCollectResult> {
        let token = self.player_token(epoch)?;
        let pending = self
            .player
            .voice_songs
            .page(&token, play_id, card_receipt)?;
        let system_liked = pending
            .collect
            .with_selection(selection_receipt, |active| {
                Ok(active
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.destination == Destination::SystemLiked))
            })?;
        let (page, check) = self.voice_song_write_scope(
            epoch,
            play_id,
            card_receipt,
            if system_liked {
                |api| api.song_card_system_liked_scope_revision()
            } else {
                |api| api.song_card_collection_scope_revision()
            },
        )?;
        let service = PlaylistService::new(self.session.clone())?;
        let (lease, previous) = page
            .collect
            .lease(&service, &page, selection_receipt, None)?;
        let previous = previous.ok_or(Error::State("collection playlist operation is missing"))?;
        let record = if reconcile && previous.phase.pending() {
            service
                .reconcile_guarded_collect(
                    &page.token,
                    &lease.attempt.operation,
                    lease.attempt.destination.owned_id(),
                    &lease.songs,
                    &check,
                )
                .await
                .map_err(stale_credentials)?
        } else {
            previous
        };
        check()?;
        result(&page, &lease, record)
    }
}
