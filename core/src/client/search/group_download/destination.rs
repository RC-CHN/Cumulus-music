//! Verified playlist destinations for search group download.
pub(super) mod creation;
use super::*;
use crate::{
    client::content::PLAYLIST_COLLECT_LAST_PAGE,
    playlist::{
        GroupDownloadLikedOrigin, GroupDownloadPlaylistProof, PlaylistMutation, PlaylistPhase,
        PlaylistService,
    },
};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Destination {
    observed: BTreeMap<(&'static str, u32), Vec<String>>,
    liked: Option<LikedTarget>,
    attempt: Option<Attempt>,
    creation: Option<creation::CreationAttempt>,
    can_create: bool,
}

struct LikedTarget {
    playlist: String,
    receipt: String,
}

pub(super) enum Choice<'a> {
    Regular(&'a str),
    SystemLiked(&'a str),
}

#[derive(Clone)]
struct Attempt {
    playlist: String,
    liked_receipt: Option<String>,
    operation: String,
    continuation: String,
    creation: Option<String>,
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
    liked_ids: Vec<String>,
}

impl Attempt {
    fn proof(&self, plan: &Plan) -> GroupDownloadPlaylistProof {
        GroupDownloadPlaylistProof {
            operation: self.operation.clone(),
            playlist: self.playlist.clone(),
            songs: plan.collection.clone(),
            creation: self.creation.clone(),
            liked_origin: self
                .liked_receipt
                .is_some()
                .then_some(GroupDownloadLikedOrigin::Download),
        }
    }

    fn result(
        &self,
        plan: &Plan,
        record: &PlaylistMutation,
    ) -> Result<SearchGroupDownloadPlaylistResult> {
        let owner = plan
            .observed
            .owner
            .uid
            .as_deref()
            .ok_or(Error::StaleResponse)?;
        let proof = self.proof(plan);
        let (completed, add_failed) = proof.outcome(record, owner)?;
        let liked_ids = if proof.liked_origin.is_some()
            && completed
            && !add_failed
            && matches!(
                record.phase,
                PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
            ) {
            proof.songs.to_vec()
        } else {
            Vec::new()
        };
        Ok(SearchGroupDownloadPlaylistResult {
            epoch: plan.observed.owner.generation.to_string(),
            receipt: plan.receipt.clone(),
            playlist_id: self.playlist.clone(),
            phase: record.phase,
            add_failed,
            continuation: completed.then(|| self.continuation.clone()),
            liked_ids,
        })
    }
    fn view(&self, plan: &Plan, record: &PlaylistMutation) -> Result<SearchGroupDownloadReply> {
        Ok(SearchGroupDownloadReply::Playlist {
            result: self.result(plan, record)?,
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

pub(super) fn authorize_liked(observed: &Observed) -> Result<()> {
    if observed
        .api
        .download_system_liked_scope_revision()?
        .as_deref()
        != Some(&observed.revision)
    {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

pub(super) fn authorize_creation(observed: &Observed) -> Result<()> {
    if observed
        .api
        .download_collection_create_scope_revision()?
        .as_deref()
        != Some(observed.revision.as_str())
    {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

impl MusicClient {
    fn search_group_playlist_permission(
        &self,
        plan: &Plan,
        system_liked: bool,
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
            if system_liked {
                authorize_liked(&observed)?;
            } else {
                authorize_collection(&observed)?;
            }
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
        let check = self.search_group_playlist_permission(&lease.0, false)?;
        let key = (sort.wire(), page);
        {
            let mut destination = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if destination.attempt.is_some() || destination.creation.is_some() {
                return Err(Error::StaleResponse);
            }
            destination.observed.remove(&key);
            if page == 0 {
                destination.liked = None;
                destination.can_create = false;
            }
        }
        let reply = self
            .playlist_collect_page_scope(epoch, sort, page, true)
            .await?;
        // Only advertise the fixed account-owned system row if a current
        // signed observation identifies it. Missing capability or failure of
        // that independent read does not invalidate ordinary destinations.
        let liked = if page == 0
            && let Ok(liked_check) = self.search_group_playlist_permission(&lease.0, true)
        {
            PlaylistService::new(self.session.clone())?
                .download_system_liked_destination(&lease.0.observed.owner, &liked_check)
                .await
                .ok()
                .flatten()
                .map(|playlist| LikedTarget {
                    playlist,
                    receipt: uuid::Uuid::new_v4().simple().to_string(),
                })
        } else {
            None
        };
        check()?;
        let can_create = page == 0 && authorize_creation(&lease.0.observed).is_ok();
        let mut destination = lease
            .0
            .destination
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        destination.observed.insert(
            key,
            reply
                .value
                .playlists
                .iter()
                .map(|row| row.id.clone())
                .collect(),
        );
        if page == 0 {
            destination.liked = liked;
            destination.can_create = can_create;
        }
        Ok(SearchGroupDownloadReply::Destinations {
            page: reply,
            system_liked_receipt: destination.liked.as_ref().map(|row| row.receipt.clone()),
            can_create: destination.can_create,
        })
    }

    pub(super) async fn search_group_download_choose(
        &self,
        lease: &Lease,
        choice: Choice<'_>,
    ) -> Result<SearchGroupDownloadReply> {
        let system_liked = matches!(choice, Choice::SystemLiked(_));
        let check = self.search_group_playlist_permission(&lease.0, system_liked)?;
        let plan = &lease.0;
        let (attempt, first) = {
            let mut destination = plan.destination.lock().map_err(|_| Error::LockPoisoned)?;
            if destination.creation.is_some() {
                return Err(Error::StaleResponse);
            }
            let (id, liked_receipt) = match choice {
                Choice::Regular(playlist) => {
                    let id = identifier(&json!(playlist))?;
                    if !destination
                        .observed
                        .values()
                        .flatten()
                        .any(|row| row == &id)
                    {
                        return Err(Error::StaleResponse);
                    }
                    (id, None)
                }
                Choice::SystemLiked(receipt) => {
                    let liked = destination
                        .liked
                        .as_ref()
                        .filter(|target| target.receipt == receipt)
                        .ok_or(Error::StaleResponse)?;
                    (liked.playlist.clone(), Some(liked.receipt.clone()))
                }
            };
            if let Some(attempt) = &destination.attempt {
                if attempt.playlist != id || attempt.liked_receipt != liked_receipt {
                    return Err(Error::StaleResponse);
                }
                (attempt.clone(), false)
            } else {
                let attempt = Attempt {
                    playlist: id,
                    liked_receipt,
                    operation: uuid::Uuid::new_v4().simple().to_string(),
                    continuation: uuid::Uuid::new_v4().simple().to_string(),
                    creation: None,
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
                if system_liked {
                    service
                        .guarded_collect_download_liked_group(
                            token,
                            &attempt.playlist,
                            &plan.collection,
                            &attempt.operation,
                            &check,
                        )
                        .await?
                } else {
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
                }
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
        let attempt = current_attempt(plan)?;
        if attempt.creation.is_some() {
            return Err(Error::StaleResponse);
        }
        let system_liked = attempt.liked_receipt.is_some();
        let check = self.search_group_playlist_permission(plan, system_liked)?;
        let service = PlaylistService::new(self.session.clone())?;
        let record = service
            .reconcile_guarded_collect(
                &plan.observed.owner,
                &attempt.operation,
                &attempt.playlist,
                &plan.collection,
                system_liked,
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
        let attempt = current_attempt(plan)?;
        let check = self.search_group_playlist_permission(plan, attempt.liked_receipt.is_some())?;
        if attempt.creation.is_some() {
            self.search_group_creation_permission(plan)?()?;
        }
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
