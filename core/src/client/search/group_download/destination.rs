//! Verified existing ordinary playlist destination for search group download.
use super::*;
use crate::{
    client::content::PLAYLIST_COLLECT_LAST_PAGE,
    playlist::{GroupDownloadPlaylistProof, PlaylistMutation, PlaylistPhase, PlaylistService},
};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Destination {
    observed: BTreeMap<(&'static str, u32), Vec<String>>,
    attempt: Option<Attempt>,
}

#[derive(Clone)]
struct Attempt {
    playlist: String,
    operation: String,
    continuation: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchGroupDownloadPlaylistResult {
    epoch: String,
    receipt: String,
    playlist_id: String,
    phase: PlaylistPhase,
    add_failed: bool,
    continuation: Option<String>,
}

impl Attempt {
    fn proof(&self, plan: &Plan) -> GroupDownloadPlaylistProof {
        GroupDownloadPlaylistProof {
            operation: self.operation.clone(),
            playlist: self.playlist.clone(),
            songs: plan.collection.clone(),
            creation: None,
            system_liked: false,
        }
    }

    fn view(&self, plan: &Plan, record: &PlaylistMutation) -> Result<SearchGroupDownloadReply> {
        let owner = plan
            .observed
            .owner
            .uid
            .as_deref()
            .ok_or(Error::StaleResponse)?;
        let (completed, add_failed) = self.proof(plan).outcome(record, owner)?;
        Ok(SearchGroupDownloadReply::Playlist {
            result: SearchGroupDownloadPlaylistResult {
                epoch: plan.observed.owner.generation.to_string(),
                receipt: plan.receipt.clone(),
                playlist_id: self.playlist.clone(),
                phase: record.phase,
                add_failed,
                continuation: completed.then(|| self.continuation.clone()),
            },
        })
    }
}

pub(super) fn authorize_collection(observed: &Observed) -> Result<()> {
    if observed
        .api
        .song_card_collection_scope_revision()?
        .as_deref()
        != Some(&observed.revision)
        || observed
            .api
            .download_collection_scope_revision()?
            .as_deref()
            != Some(&observed.revision)
    {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

impl MusicClient {
    fn search_group_playlist_permission(
        &self,
        plan: &Plan,
    ) -> Result<impl Fn() -> Result<()> + Send + Sync + 'static + use<>> {
        if plan.purpose != SearchGroupDownloadDecision::PlaylistRequired
            || plan.collection.is_empty()
        {
            return Err(Error::StaleResponse);
        }
        let observed = plan.observed.clone();
        let session = self.session.clone();
        let pages = self.search.group_downloads.clone();
        let login = self.auth.pending_guard();
        let check = move || {
            login()?;
            authorization(&session, &observed)?;
            authorize_collection(&observed)?;
            pages.current(&observed)
        };
        check()?;
        Ok(check)
    }

    pub(super) async fn search_group_download_destinations(
        &self,
        epoch: &str,
        lease: &Lease,
        sort: PlaylistCollectSort,
        page: u32,
    ) -> Result<SearchGroupDownloadReply> {
        if page > PLAYLIST_COLLECT_LAST_PAGE {
            return Err(Error::InvalidInput("invalid download playlist page"));
        }
        let check = self.search_group_playlist_permission(&lease.0)?;
        let key = (sort.wire(), page);
        {
            let mut destination = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if destination.attempt.is_some() {
                return Err(Error::StaleResponse);
            }
            destination.observed.remove(&key);
        }
        let reply = self
            .playlist_collect_page_scope(epoch, sort, page, true)
            .await?;
        check()?;
        lease
            .0
            .destination
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .observed
            .insert(
                key,
                reply
                    .value
                    .playlists
                    .iter()
                    .map(|row| row.id.clone())
                    .collect(),
            );
        Ok(SearchGroupDownloadReply::Destinations { page: reply })
    }

    pub(super) async fn search_group_download_choose(
        &self,
        lease: &Lease,
        playlist: &str,
    ) -> Result<SearchGroupDownloadReply> {
        let check = self.search_group_playlist_permission(&lease.0)?;
        let id = identifier(&json!(playlist))?;
        let plan = &lease.0;
        let (attempt, first) = {
            let mut destination = plan.destination.lock().map_err(|_| Error::LockPoisoned)?;
            if let Some(attempt) = &destination.attempt {
                if attempt.playlist != id {
                    return Err(Error::StaleResponse);
                }
                (attempt.clone(), false)
            } else {
                if !destination
                    .observed
                    .values()
                    .flatten()
                    .any(|row| row == &id)
                {
                    return Err(Error::StaleResponse);
                }
                let attempt = Attempt {
                    playlist: id,
                    operation: uuid::Uuid::new_v4().simple().to_string(),
                    continuation: uuid::Uuid::new_v4().simple().to_string(),
                };
                destination.attempt = Some(attempt.clone());
                (attempt, true)
            }
        };
        let token = &plan.observed.owner;
        let service = PlaylistService::new(self.session.clone())?;
        let record =
            if first {
                let (liked, pending) = AccountResources::new(self.session.clone())?
                    .membership_for(token, ResourceKind::Liked, &plan.observed.anchor)?;
                if liked != Some(false) || pending.is_some_and(|record| record.phase.pending()) {
                    return Err(Error::StaleResponse);
                }
                service
                    .guarded_collect_download(
                        token,
                        &attempt.playlist,
                        &plan.collection,
                        true,
                        &attempt.operation,
                        &check,
                    )
                    .await?
            } else {
                service
                    .operation_for(token, &attempt.operation)?
                    .ok_or(Error::StaleResponse)?
            };
        check()?;
        attempt.view(plan, &record)
    }

    pub(super) async fn search_group_download_reconcile(
        &self,
        lease: &Lease,
    ) -> Result<SearchGroupDownloadReply> {
        let plan = &lease.0;
        let check = self.search_group_playlist_permission(plan)?;
        let attempt = current_attempt(plan)?;
        let service = PlaylistService::new(self.session.clone())?;
        let record = service
            .reconcile_guarded_collect(
                &plan.observed.owner,
                &attempt.operation,
                &attempt.playlist,
                &plan.collection,
                false,
                &check,
            )
            .await?;
        check()?;
        attempt.view(plan, &record)
    }

    pub(super) async fn search_group_download_start_from_playlist(
        &self,
        epoch: &str,
        lease: Lease,
        continuation: &str,
    ) -> Result<SearchGroupDownloadReply> {
        let plan = &lease.0;
        let check = self.search_group_playlist_permission(plan)?;
        let attempt = current_attempt(plan)?;
        if attempt.continuation != continuation {
            return Err(Error::StaleResponse);
        }
        let service = PlaylistService::new(self.session.clone())?;
        let token = &plan.observed.owner;
        let record = service
            .operation_for(token, &attempt.operation)?
            .ok_or(Error::StaleResponse)?;
        let proof = attempt.proof(plan);
        if !proof
            .outcome(&record, token.uid.as_deref().ok_or(Error::StaleResponse)?)?
            .0
        {
            return Err(Error::StaleResponse);
        }
        if plan.committed.get().is_none()
            && !self.configured_download_directory_available(epoch).await?
        {
            check()?;
            return Ok(SearchGroupDownloadReply::DirectoryRequired);
        }
        check()?;
        // The picker reverses the resource array, but the actual playlist
        // add reverses its separate copy back to the gesture order.
        let songs = plan.songs.iter().rev().cloned().collect();
        let anchor = plan.observed.anchor.clone();
        self.enqueue_search_download(
            epoch,
            lease,
            songs,
            SongBatchAdmission::SearchPlaylist { proof, anchor },
            true,
        )
        .await
    }
}

fn current_attempt(plan: &Plan) -> Result<Attempt> {
    plan.destination
        .lock()
        .map_err(|_| Error::LockPoisoned)?
        .attempt
        .clone()
        .ok_or(Error::StaleResponse)
}
