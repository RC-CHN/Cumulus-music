//! New download destination. Its create and add are separate durable writes.
use super::{
    Attempt, Lease, MusicClient, Page, PlaylistCommand, PlaylistMutation, PlaylistPhase,
    PlaylistService, Target, VoiceSongGroupPlaylistResult, current_attempt, fresh_id, membership,
};
use crate::{
    Error, Result,
    protocol::api::{PlaylistAddPermission, playlist_name},
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongGroupPlaylistCreation {
    pub epoch: String,
    pub play_id: String,
    pub selection_receipt: String,
    pub create_operation_id: String,
    pub create_phase: PlaylistPhase,
    pub playlist_id: Option<String>,
    pub can_add: bool,
    pub add: Option<VoiceSongGroupPlaylistResult>,
}

fn creation_id(attempt: &Attempt) -> Result<&str> {
    match &attempt.target {
        Target::Created { operation, .. } => Ok(operation),
        Target::Existing(_) => Err(Error::StaleResponse),
    }
}

fn creation_view(
    page: &Page,
    lease: &Lease,
    attempt: &Attempt,
    create: PlaylistMutation,
    add: Option<PlaylistMutation>,
) -> Result<VoiceSongGroupPlaylistCreation> {
    let Target::Created {
        name,
        private,
        operation,
    } = &attempt.target
    else {
        return Err(Error::StaleResponse);
    };
    if create.operation_id != *operation
        || create.system_liked_group
        || !matches!(&create.command, PlaylistCommand::Create { name: target, private: visibility }
            if target == name && visibility == private)
    {
        return Err(Error::StaleResponse);
    }
    let playlist = create.confirmed_created_target().map(|(id, _, _)| id);
    let add = add
        .map(|record| attempt.view(page, lease, playlist.ok_or(Error::StaleResponse)?, record))
        .transpose()?;
    let can_add = playlist.is_some() && add.is_none();
    Ok(VoiceSongGroupPlaylistCreation {
        epoch: page.token.generation.to_string(),
        play_id: page.play_id.clone(),
        selection_receipt: lease.0.receipt.clone(),
        playlist_id: playlist.map(str::to_owned),
        create_operation_id: create.operation_id,
        create_phase: create.phase,
        can_add,
        add,
    })
}

async fn add_once(
    service: &PlaylistService,
    page: &Page,
    lease: &Lease,
    attempt: &Attempt,
    check: &PlaylistAddPermission<'_>,
) -> Result<PlaylistMutation> {
    if let Some(record) = service.operation_for(&page.token, &attempt.operation)? {
        return Ok(record);
    }
    check()?;
    service
        .guarded_collect_created_songs(
            &page.token,
            creation_id(attempt)?,
            &lease.0.collection,
            &attempt.operation,
            check,
        )
        .await
        .map_err(super::super::super::stale_credentials)
}

impl MusicClient {
    fn group_creation_scope(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<(
        Arc<Page>,
        Lease,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        let (page, lease, _, destination) =
            self.group_destination_scope(epoch, play_id, card_receipt, selection_receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let revision = page.revision.clone();
        let check = move || {
            destination()?;
            if api
                .song_card_collection_create_scope_revision()
                .map_err(super::super::super::stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        Ok((page, lease, check))
    }

    /// Explicit new destination; only an acknowledged, verified creation can
    /// chain the initially requested add, never a recovered lost receipt.
    pub async fn player_voice_songs_download_create_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        name: &str,
        private: bool,
    ) -> Result<VoiceSongGroupPlaylistCreation> {
        let name = playlist_name(name)?;
        let (page, lease, check) =
            self.group_creation_scope(epoch, play_id, card_receipt, selection_receipt)?;
        let (attempt, first) = {
            let mut state = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if let Some(attempt) = &state.attempt {
                if !matches!(&attempt.target, Target::Created { name: saved, private: p, .. } if saved == &name && *p == private)
                {
                    return Err(Error::StaleResponse);
                }
                (attempt.clone(), false)
            } else {
                let attempt = Attempt {
                    target: Target::Created {
                        name: name.clone(),
                        private,
                        operation: fresh_id(),
                    },
                    operation: fresh_id(),
                    continuation: fresh_id(),
                };
                state.attempt = Some(attempt.clone());
                (attempt, true)
            }
        };
        let service = PlaylistService::new(self.session.clone())?;
        let create = if first {
            let decision = membership(self.session.clone(), &page.token, &lease.0.anchor)?;
            if decision != super::super::VoiceSongGroupDownloadDecision::PlaylistRequired {
                return Err(Error::StaleResponse);
            }
            check()?;
            service
                .guarded_create_collect_playlist(
                    &page.token,
                    creation_id(&attempt)?,
                    &name,
                    private,
                    &check,
                )
                .await
                .map_err(super::super::super::stale_credentials)?
        } else {
            service
                .operation_for(&page.token, creation_id(&attempt)?)?
                .ok_or(Error::StaleResponse)?
        };
        check()?;
        let add = if first && create.confirmed_created_target().is_some() {
            Some(add_once(&service, &page, &lease, &attempt, &check).await?)
        } else {
            service.operation_for(&page.token, &attempt.operation)?
        };
        check()?;
        creation_view(&page, &lease, &attempt, create, add)
    }

    /// Local journal or one explicitly requested read-back, never dispatch.
    pub async fn player_voice_songs_download_creation_status(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        reconcile: bool,
    ) -> Result<VoiceSongGroupPlaylistCreation> {
        let (page, lease, check) =
            self.group_creation_scope(epoch, play_id, card_receipt, selection_receipt)?;
        let attempt = current_attempt(&lease)?;
        let service = PlaylistService::new(self.session.clone())?;
        let id = creation_id(&attempt)?;
        let mut create = service
            .operation_for(&page.token, id)?
            .ok_or(Error::StaleResponse)?;
        if reconcile && create.phase.pending() {
            create = service
                .reconcile_guarded_create(&page.token, id, &check)
                .await
                .map_err(super::super::super::stale_credentials)?;
        }
        let mut add = service.operation_for(&page.token, &attempt.operation)?;
        if reconcile && add.as_ref().is_some_and(|record| record.phase.pending()) {
            let (id, _, _) = create
                .confirmed_created_target()
                .ok_or(Error::StaleResponse)?;
            add = Some(
                service
                    .reconcile_guarded_collect(
                        &page.token,
                        &attempt.operation,
                        id,
                        &lease.0.collection,
                        false,
                        &check,
                    )
                    .await
                    .map_err(super::super::super::stale_credentials)?,
            );
        }
        check()?;
        creation_view(&page, &lease, &attempt, create, add)
    }

    /// Explicitly resume a never-dispatched add after a verified create.
    pub async fn player_voice_songs_download_add_created_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<VoiceSongGroupPlaylistCreation> {
        let (page, lease, check) =
            self.group_creation_scope(epoch, play_id, card_receipt, selection_receipt)?;
        let attempt = current_attempt(&lease)?;
        let service = PlaylistService::new(self.session.clone())?;
        let create = service
            .operation_for(&page.token, creation_id(&attempt)?)?
            .ok_or(Error::StaleResponse)?;
        create
            .confirmed_created_target()
            .ok_or(Error::StaleResponse)?;
        let add = add_once(&service, &page, &lease, &attempt, &check).await?;
        check()?;
        creation_view(&page, &lease, &attempt, create, Some(add))
    }
}
