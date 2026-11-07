//! Native download destination, separate from the general SongCard collect flow.
mod creation;
mod liked;
use super::super::collect::VoiceSongLikedDestination;
use super::{Lease, MusicClient, Page, Quality, membership};
use crate::{
    Error, Result,
    client::{
        content::PLAYLIST_COLLECT_LAST_PAGE,
        dto::{AccountData, DownloadResult, PlaylistCollectPage, PlaylistCollectSort},
    },
    download::service::SongBatchAdmission,
    playlist::{
        GroupDownloadPlaylistProof, PlaylistCommand, PlaylistMutation, PlaylistPhase,
        PlaylistService,
    },
    protocol::api::{PlaylistAddPermission, identifier},
};
pub use creation::VoiceSongGroupPlaylistCreation;
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

type DestinationScope<Check> = (Arc<Page>, Lease, Option<Quality>, Check);

#[derive(Clone, PartialEq, Eq)]
enum Target {
    Existing(String),
    SystemLiked {
        id: String,
        receipt: String,
    },
    Created {
        name: String,
        private: bool,
        operation: String,
    },
}

#[derive(Clone, Copy)]
enum Authority {
    Regular,
    SystemLiked,
    Attempt,
}

#[derive(Clone)]
struct Attempt {
    target: Target,
    operation: String,
    continuation: String,
}

pub(super) struct State {
    // None: choose a purchased tier or use direct admission; Some(None):
    // ordinary destination; Some(Some(tier)): confirmed purchased destination.
    pub(super) quality: Option<Option<Quality>>,
    observed: BTreeMap<(&'static str, u32), Vec<String>>,
    liked: Option<VoiceSongLikedDestination>,
    attempt: Option<Attempt>,
}

impl State {
    pub(super) fn new(ordinary_picker: bool) -> Self {
        Self {
            quality: ordinary_picker.then_some(None),
            observed: BTreeMap::new(),
            liked: None,
            attempt: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongGroupPlaylistResult {
    pub epoch: String,
    pub play_id: String,
    pub selection_receipt: String,
    pub playlist_id: String,
    pub operation_id: String,
    pub phase: PlaylistPhase,
    pub add_failed: bool,
    pub continuation: Option<String>,
}

impl Attempt {
    fn playlist(&self, service: &PlaylistService, page: &Page) -> Result<String> {
        match &self.target {
            Target::Existing(id) | Target::SystemLiked { id, .. } => Ok(id.clone()),
            Target::Created { operation, .. } => service
                .operation_for(&page.token, operation)?
                .ok_or(Error::StaleResponse)?
                .confirmed_created_target()
                .map(|(id, _, _)| id.to_owned())
                .ok_or(Error::StaleResponse),
        }
    }

    fn proof(&self, lease: &Lease, playlist: String) -> GroupDownloadPlaylistProof {
        GroupDownloadPlaylistProof {
            operation: self.operation.clone(),
            playlist,
            songs: lease.0.collection.clone(),
            creation: match &self.target {
                Target::Created { operation, .. } => Some(operation.clone()),
                Target::Existing(_) | Target::SystemLiked { .. } => None,
            },
            system_liked: matches!(self.target, Target::SystemLiked { .. }),
        }
    }

    fn view(
        &self,
        page: &Page,
        lease: &Lease,
        id: &str,
        record: PlaylistMutation,
    ) -> Result<VoiceSongGroupPlaylistResult> {
        let owner = page.token.uid.as_deref().ok_or(Error::StaleResponse)?;
        let (completed, add_failed) = self.proof(lease, id.into()).outcome(&record, owner)?;
        Ok(VoiceSongGroupPlaylistResult {
            epoch: page.token.generation.to_string(),
            play_id: page.play_id.clone(),
            selection_receipt: lease.0.receipt.clone(),
            playlist_id: id.into(),
            operation_id: record.operation_id,
            phase: record.phase,
            add_failed,
            continuation: (completed && lease.0.committed.get().is_none())
                .then(|| self.continuation.clone()),
        })
    }
}

impl MusicClient {
    fn group_destination_scope(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        authority: Authority,
    ) -> Result<DestinationScope<impl Fn() -> Result<()> + Send + Sync + 'static + use<>>> {
        let (page, scope) = self.voice_song_download_scope(epoch, play_id, card_receipt)?;
        let lease = page.group_download.lease(selection_receipt)?;
        let (quality, system_liked) = {
            let state = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let quality = state.quality.ok_or(Error::StaleResponse)?;
            let system_liked = match authority {
                Authority::Regular => false,
                Authority::SystemLiked => true,
                Authority::Attempt => matches!(
                    state.attempt.as_ref().ok_or(Error::StaleResponse)?.target,
                    Target::SystemLiked { .. }
                ),
            };
            (quality, system_liked)
        };
        if lease.0.collection.is_empty() {
            return Err(Error::State("no group songs can be added to a playlist"));
        }
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let revision = page.revision.clone();
        let experiments = self.pc_experiments.clone();
        let bound = page.clone();
        let check = move || {
            scope()?;
            let current = if system_liked {
                api.song_card_system_liked_scope_revision()
            } else {
                api.song_card_collection_scope_revision()
            };
            if current.map_err(super::super::stale_credentials)?.as_deref()
                != Some(revision.as_str())
                || quality == Some(Quality::Vivid) && !super::vivid_enabled(&experiments, &bound)?
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        Ok((page, lease, quality, check))
    }

    /// Read the owned regular destination page into this exact group picker.
    pub async fn player_voice_songs_download_destinations(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        sort: PlaylistCollectSort,
        number: u32,
    ) -> Result<AccountData<PlaylistCollectPage>> {
        if number > PLAYLIST_COLLECT_LAST_PAGE {
            return Err(Error::InvalidInput("invalid playlist collection page"));
        }
        let (_, lease, _, check) = self.group_destination_scope(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Authority::Regular,
        )?;
        let key = (sort.wire(), number);
        {
            let mut state = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.attempt.is_some() {
                return Err(Error::StaleResponse);
            }
            state.observed.remove(&key);
        }
        let result = self.playlist_collect_page(epoch, sort, number).await?;
        check()?;
        lease
            .0
            .destination
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .observed
            .insert(
                key,
                result
                    .value
                    .playlists
                    .iter()
                    .map(|item| item.id.clone())
                    .collect(),
            );
        Ok(result)
    }

    /// A caller can choose only a native-observed destination. Repetition
    /// reads the original journal and never resends its add.
    pub async fn player_voice_songs_download_choose_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        playlist_id: &str,
    ) -> Result<VoiceSongGroupPlaylistResult> {
        let (page, lease, _, check) = self.group_destination_scope(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Authority::Regular,
        )?;
        let id = identifier(&json!(playlist_id))?;
        let (attempt, first) = {
            let mut state = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if let Some(attempt) = &state.attempt {
                if attempt.target != Target::Existing(id.clone()) {
                    return Err(Error::StaleResponse);
                }
                (attempt.clone(), false)
            } else {
                if !state.observed.values().flatten().any(|seen| seen == &id) {
                    return Err(Error::StaleResponse);
                }
                let attempt = Attempt {
                    target: Target::Existing(id),
                    operation: fresh_id(),
                    continuation: fresh_id(),
                };
                state.attempt = Some(attempt.clone());
                (attempt, true)
            }
        };
        self.apply_group_playlist_choice(&page, &lease, &attempt, first, &check)
            .await
    }

    async fn apply_group_playlist_choice(
        &self,
        page: &Page,
        lease: &Lease,
        attempt: &Attempt,
        first: bool,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<VoiceSongGroupPlaylistResult> {
        let service = PlaylistService::new(self.session.clone())?;
        let playlist = attempt.playlist(&service, page)?;
        let record = if first {
            let decision = membership(self.session.clone(), &page.token, &lease.0.anchor)?;
            if decision != super::VoiceSongGroupDownloadDecision::PlaylistRequired {
                return Err(Error::StaleResponse);
            }
            check()?;
            match attempt.target {
                Target::Existing(_) => {
                    service
                        .guarded_collect_songs(
                            &page.token,
                            &playlist,
                            &lease.0.collection,
                            &attempt.operation,
                            check,
                        )
                        .await
                }
                Target::SystemLiked { .. } => {
                    service
                        .guarded_collect_system_liked(
                            &page.token,
                            &playlist,
                            &lease.0.collection,
                            &attempt.operation,
                            check,
                        )
                        .await
                }
                Target::Created { .. } => return Err(Error::StaleResponse),
            }
            .map_err(super::super::stale_credentials)?
        } else {
            service
                .operation_for(&page.token, &attempt.operation)?
                .ok_or(Error::StaleResponse)?
        };
        check()?;
        attempt.view(page, lease, &playlist, record)
    }

    /// Current picker journal, optionally reconciled through one read only.
    pub async fn player_voice_songs_download_playlist_status(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        reconcile: bool,
    ) -> Result<VoiceSongGroupPlaylistResult> {
        let (page, lease, _, check) = self.group_destination_scope(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Authority::Attempt,
        )?;
        let attempt = current_attempt(&lease)?;
        let service = PlaylistService::new(self.session.clone())?;
        let playlist = attempt.playlist(&service, &page)?;
        let record = if reconcile {
            service
                .reconcile_guarded_collect(
                    &page.token,
                    &attempt.operation,
                    &playlist,
                    &lease.0.collection,
                    matches!(attempt.target, Target::SystemLiked { .. }),
                    &check,
                )
                .await
                .map_err(super::super::stale_credentials)?
        } else {
            service
                .operation_for(&page.token, &attempt.operation)?
                .ok_or(Error::StaleResponse)?
        };
        check()?;
        attempt.view(&page, &lease, &playlist, record)
    }

    /// The official picker reverses its download array in place, while the
    /// separate playlist add keeps its original relative order. Only the
    /// already-verified journal can authorize this one durable admission.
    pub async fn player_voice_songs_download_start_from_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        continuation: &str,
    ) -> Result<DownloadResult> {
        let (page, lease, quality, check) = self.group_destination_scope(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Authority::Attempt,
        )?;
        let attempt = current_attempt(&lease)?;
        if attempt.continuation != continuation {
            return Err(Error::StaleResponse);
        }
        if let Some(committed) = lease.0.committed.get() {
            return Ok(DownloadResult {
                epoch: epoch.into(),
                accepted: committed.ids.clone(),
                operation_id: Some(selection_receipt.into()),
                ..Default::default()
            });
        }
        let service = PlaylistService::new(self.session.clone())?;
        let playlist = attempt.playlist(&service, &page)?;
        let record = service
            .operation_for(&page.token, &attempt.operation)?
            .ok_or(Error::StaleResponse)?;
        if attempt
            .view(&page, &lease, &playlist, record)?
            .continuation
            .as_deref()
            != Some(continuation)
        {
            return Err(Error::StaleResponse);
        }
        check()?;
        let songs = lease.0.songs.iter().rev().cloned().collect::<Arc<[_]>>();
        let proof = attempt.proof(&lease, playlist);
        self.enqueue_voice_song_group(
            epoch,
            &page,
            lease,
            songs,
            (quality, SongBatchAdmission::Playlist(proof)),
            check,
        )
        .await
    }
}

fn current_attempt(lease: &Lease) -> Result<Attempt> {
    lease
        .0
        .destination
        .lock()
        .map_err(|_| Error::LockPoisoned)?
        .attempt
        .clone()
        .ok_or(Error::StaleResponse)
}

fn fresh_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
