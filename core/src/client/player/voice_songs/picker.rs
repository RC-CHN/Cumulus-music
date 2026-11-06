//! The existing-owned-playlist completion of official 「下载到歌单」.
use super::{
    MusicClient,
    actions::Page,
    download::{Admission, Class, VoiceSongDownloadDecision},
    stale_credentials,
};
use crate::{
    Error, Result,
    client::dto::DownloadResult,
    download::service::DownloadOptions,
    playlist::{PlaylistCommand, PlaylistMutation, PlaylistPhase, PlaylistService, TrackChange},
    protocol::api::identifier,
};
use serde::Serialize;
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
struct Attempt {
    index: usize,
    playlist: String,
    operation: String,
    permit: String,
    consumed: Arc<AtomicBool>,
    busy: bool,
}

#[derive(Default)]
pub(super) struct Shelf(Mutex<Option<Attempt>>);

struct Lease<'a> {
    shelf: &'a Shelf,
    attempt: Attempt,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.shelf.0.lock()
            && let Some(active) = slot
                .as_mut()
                .filter(|active| active.operation == self.attempt.operation)
        {
            active.busy = false;
        }
    }
}

impl Shelf {
    fn choose<'a>(
        &'a self,
        service: &PlaylistService,
        page: &Page,
        index: usize,
        playlist: &str,
    ) -> Result<(Lease<'a>, Option<PlaylistMutation>)> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(active) = slot.as_mut() {
            if active.busy {
                return Err(Error::MutationBusy);
            }
            if let Some(record) = service.operation_for(&page.token, &active.operation)? {
                if active.index == index && active.playlist == playlist {
                    active.busy = true;
                    return Ok((
                        Lease {
                            shelf: self,
                            attempt: active.clone(),
                        },
                        Some(record),
                    ));
                }
                if record.phase.pending() {
                    return Err(Error::MutationUncertain);
                }
            }
        }
        let attempt = Attempt {
            index,
            playlist: playlist.into(),
            operation: uuid::Uuid::new_v4().simple().to_string(),
            permit: uuid::Uuid::new_v4().simple().to_string(),
            consumed: Arc::new(AtomicBool::new(false)),
            busy: true,
        };
        *slot = Some(attempt.clone());
        Ok((
            Lease {
                shelf: self,
                attempt,
            },
            None,
        ))
    }

    fn get(&self, index: usize, permit: Option<&str>) -> Result<Lease<'_>> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active = slot
            .as_mut()
            .filter(|active| {
                active.index == index
                    && permit.is_none_or(|value| {
                        value == active.permit && !active.consumed.load(Ordering::Acquire)
                    })
            })
            .ok_or(Error::StaleResponse)?;
        if active.busy {
            return Err(Error::MutationBusy);
        }
        active.busy = true;
        Ok(Lease {
            shelf: self,
            attempt: active.clone(),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongDownloadPicker {
    pub epoch: String,
    pub play_id: String,
    pub track_id: String,
    pub playlist_id: String,
    pub operation_id: String,
    pub phase: PlaylistPhase,
    pub continuation: Option<String>,
}

fn result(
    page: &Page,
    attempt: &Attempt,
    record: PlaylistMutation,
) -> Result<VoiceSongDownloadPicker> {
    let song = &page
        .rows
        .get(attempt.index)
        .ok_or(Error::StaleResponse)?
        .track
        .id;
    if record.operation_id != attempt.operation
        || record.playlist_id.as_deref() != Some(attempt.playlist.as_str())
        || !matches!(&record.command, PlaylistCommand::Tracks {id, change: TrackChange::Add, ids, ..} if id == &attempt.playlist && ids.as_slice() == [song.as_str()])
    {
        return Err(Error::State(
            "picker completion belongs to a different playlist action",
        ));
    }
    // FavouriteModal calls modalCallback(false) for an existing destination
    // even when the add result is false. A proved service rejection is that
    // case; a never-sent request or an unresolved transport loss is not.
    let completed = matches!(
        record.phase,
        PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
    ) || record.phase == PlaylistPhase::Rejected
        && record.checked_at.is_some()
        && record.error_class.as_deref() == Some("service")
        && record.codes.contains_key("main");
    Ok(VoiceSongDownloadPicker {
        epoch: page.token.generation.to_string(),
        play_id: page.play_id.clone(),
        track_id: song.clone(),
        playlist_id: attempt.playlist.clone(),
        operation_id: record.operation_id,
        phase: record.phase,
        continuation: (completed && !attempt.consumed.load(Ordering::Acquire))
            .then(|| attempt.permit.clone()),
    })
}

impl MusicClient {
    fn voice_song_picker_scope(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
    ) -> Result<(
        Arc<Page>,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        let (page, download_scope) = self.voice_song_download_scope(epoch, play_id, receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let revision = page.revision.clone();
        let check = move || {
            if api
                .song_card_playlist_scope_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            download_scope()
        };
        check()?;
        Ok((page, check))
    }

    pub async fn player_voice_song_download_choose_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        index: usize,
        playlist_id: &str,
    ) -> Result<VoiceSongDownloadPicker> {
        let (page, check) = self.voice_song_picker_scope(epoch, play_id, card_receipt)?;
        let playlist = identifier(&json!(playlist_id))?;
        let row = page
            .rows
            .get(index)
            .filter(|row| row.download == Class::Ordinary)
            .ok_or(Error::InvalidInput("invalid vinyl episode download row"))?;
        let service = PlaylistService::new(self.session.clone())?;
        let (lease, existing) = page.picker.choose(&service, &page, index, &playlist)?;
        let record = if let Some(record) = existing {
            record
        } else {
            let plan = self
                .player_voice_song_download_prepare(epoch, play_id, card_receipt, index)
                .await?;
            if plan.decision != VoiceSongDownloadDecision::PlaylistRequired {
                return Err(Error::State(
                    "this SongCard download does not require a playlist destination",
                ));
            }
            check()?;
            service
                .guarded_add_song(
                    &page.token,
                    &playlist,
                    &row.track.id,
                    &lease.attempt.operation,
                    &check,
                )
                .await
                .map_err(stale_credentials)?
        };
        check()?;
        result(&page, &lease.attempt, record)
    }

    /// Same current picker only; no externally supplied operation ID and no add replay.
    pub async fn player_voice_song_download_picker_status(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        index: usize,
        reconcile: bool,
    ) -> Result<VoiceSongDownloadPicker> {
        let (page, check) = self.voice_song_picker_scope(epoch, play_id, card_receipt)?;
        let lease = page.picker.get(index, None)?;
        let service = PlaylistService::new(self.session.clone())?;
        let record = if reconcile {
            service
                .reconcile_guarded_add(&page.token, &lease.attempt.operation, &check)
                .await
                .map_err(stale_credentials)?
        } else {
            service
                .operation_for(&page.token, &lease.attempt.operation)?
                .ok_or(Error::State("picker playlist operation is missing"))?
        };
        check()?;
        result(&page, &lease.attempt, record)
    }

    pub async fn player_voice_song_download_start_from_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        index: usize,
        continuation: &str,
        options: DownloadOptions,
    ) -> Result<DownloadResult> {
        let (page, check) = self.voice_song_picker_scope(epoch, play_id, card_receipt)?;
        let lease = page.picker.get(index, Some(continuation))?;
        let record = PlaylistService::new(self.session.clone())?
            .operation_for(&page.token, &lease.attempt.operation)?
            .ok_or(Error::StaleResponse)?;
        if result(&page, &lease.attempt, record)?
            .continuation
            .as_deref()
            != Some(continuation)
        {
            return Err(Error::StaleResponse);
        }
        self.start_resolved_song_download(
            epoch,
            &page,
            index,
            options,
            Admission::Picker(lease.attempt.consumed.clone()),
            check,
        )
        .await
    }
}
