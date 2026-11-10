//! A search download picker has separate durable create and ordered add writes.
use super::*;
use crate::{playlist::PlaylistCommand, protocol::api::playlist_name};

#[derive(Clone)]
pub(super) struct CreationAttempt {
    name: String,
    private: bool,
    operation: String,
    add: String,
    continuation: String,
}

#[derive(Clone, Copy)]
pub(in crate::client::search::group_download) enum Request<'a> {
    Create { name: &'a str, private: bool },
    Status { reconcile: bool },
    Add,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchGroupDownloadCreation {
    epoch: String,
    receipt: String,
    create_phase: PlaylistPhase,
    playlist_id: Option<String>,
    can_add: bool,
    add: Option<SearchGroupDownloadPlaylistResult>,
}

impl CreationAttempt {
    fn new(name: String, private: bool) -> Self {
        let id = || uuid::Uuid::new_v4().simple().to_string();
        Self {
            name,
            private,
            operation: id(),
            add: id(),
            continuation: id(),
        }
    }

    fn verify(&self, create: &PlaylistMutation) -> Result<Option<Attempt>> {
        if create.operation_id != self.operation
            || create.system_liked_group
            || !matches!(&create.command, PlaylistCommand::Create { name, private }
                if name == &self.name && *private == self.private)
        {
            return Err(Error::StaleResponse);
        }
        Ok(create
            .confirmed_created_target()
            .map(|(playlist, _, _)| Attempt {
                playlist: playlist.to_owned(),
                liked_receipt: None,
                operation: self.add.clone(),
                continuation: self.continuation.clone(),
                creation: Some(self.operation.clone()),
            }))
    }
}

impl MusicClient {
    pub(super) fn search_group_creation_permission(
        &self,
        plan: &Plan,
    ) -> Result<impl Fn() -> Result<()> + Send + Sync + 'static + use<>> {
        let destination = self.search_group_playlist_permission(plan, false)?;
        let observed = plan.observed.clone();
        let check = move || {
            destination()?;
            authorize_creation(&observed)
        };
        check()?;
        Ok(check)
    }

    fn require_unliked_search_anchor(&self, plan: &Plan) -> Result<()> {
        let (liked, pending) = AccountResources::new(self.session.clone())?.membership_for(
            &plan.observed.owner,
            ResourceKind::Liked,
            &plan.observed.anchor,
        )?;
        if liked != Some(false) || pending.is_some_and(|record| record.phase.pending()) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(in crate::client::search::group_download) async fn search_group_download_creation(
        &self,
        lease: &Lease,
        request: Request<'_>,
    ) -> Result<SearchGroupDownloadReply> {
        let plan = &lease.0;
        let check = self.search_group_creation_permission(plan)?;
        let mutation = || {
            check()?;
            self.require_unliked_search_anchor(plan)
        };
        let name = match request {
            Request::Create { name, .. } => Some(playlist_name(name)?),
            _ => None,
        };
        let (creation, first) = {
            let mut destination = plan.destination.lock().map_err(|_| Error::LockPoisoned)?;
            if !destination.can_create
                || (destination.creation.is_none() && destination.attempt.is_some())
            {
                return Err(Error::StaleResponse);
            }
            match (&destination.creation, request, name) {
                (None, Request::Create { private, .. }, Some(name)) => {
                    self.require_unliked_search_anchor(plan)?;
                    let attempt = CreationAttempt::new(name, private);
                    destination.creation = Some(attempt.clone());
                    (attempt, true)
                }
                (Some(saved), Request::Create { private, .. }, Some(name))
                    if saved.name == name && saved.private == private =>
                {
                    (saved.clone(), false)
                }
                (Some(saved), Request::Status { .. } | Request::Add, _) => (saved.clone(), false),
                _ => return Err(Error::StaleResponse),
            }
        };
        let service = PlaylistService::new(self.session.clone())?;
        let token = &plan.observed.owner;
        let mut create = if first {
            match service
                .guarded_create_download_collection(
                    token,
                    &creation.operation,
                    &creation.name,
                    creation.private,
                    &mutation,
                )
                .await
            {
                Ok(record) => record,
                Err(error) => match service.operation_for(token, &creation.operation)? {
                    Some(record) => record,
                    None if matches!(error.root(), Error::Transport(_)) => {
                        // The guarded service persists Sending before dispatch.
                        // No record means this failed preflight sent no create.
                        check()?;
                        let mut destination =
                            plan.destination.lock().map_err(|_| Error::LockPoisoned)?;
                        if destination
                            .creation
                            .as_ref()
                            .is_none_or(|v| v.operation != creation.operation)
                        {
                            return Err(Error::StaleResponse);
                        }
                        destination.creation = None;
                        return Err(Error::State("download collection create not sent"));
                    }
                    None => return Err(error),
                },
            }
        } else {
            service
                .operation_for(token, &creation.operation)?
                .ok_or(Error::StaleResponse)?
        };
        creation.verify(&create)?;
        check()?;
        let readback = matches!(request, Request::Status { reconcile: true });
        if readback && create.phase.pending() {
            create = service
                .reconcile_guarded_create(token, &creation.operation, &check)
                .await?;
        }
        let attempt = creation.verify(&create)?;
        let mut add = service.operation_for(token, &creation.add)?;
        if (first || matches!(request, Request::Add)) && add.is_none() {
            if attempt.is_some() {
                let result = service
                    .guarded_collect_created_download(
                        token,
                        &creation.operation,
                        &plan.collection,
                        &creation.add,
                        &mutation,
                    )
                    .await;
                add = Some(match result {
                    Ok(record) => record,
                    Err(error) => service.operation_for(token, &creation.add)?.ok_or(error)?,
                });
            } else if !first {
                return Err(Error::StaleResponse);
            }
        } else if readback && add.as_ref().is_some_and(|record| record.phase.pending()) {
            let attempt = attempt.as_ref().ok_or(Error::StaleResponse)?;
            add = Some(
                service
                    .reconcile_guarded_collect(
                        token,
                        &creation.add,
                        &attempt.playlist,
                        &plan.collection,
                        false,
                        &check,
                    )
                    .await?,
            );
        }
        check()?;
        let add = add
            .as_ref()
            .map(|record| {
                attempt
                    .as_ref()
                    .ok_or(Error::StaleResponse)?
                    .result(plan, record)
            })
            .transpose()?;
        let can_add = attempt.is_some() && add.is_none();
        let playlist_id = attempt.as_ref().map(|attempt| attempt.playlist.clone());
        if let Some(attempt) = attempt {
            let mut destination = plan.destination.lock().map_err(|_| Error::LockPoisoned)?;
            if destination
                .creation
                .as_ref()
                .is_none_or(|v| v.operation != creation.operation)
            {
                return Err(Error::StaleResponse);
            }
            destination.attempt = Some(attempt);
        }
        Ok(SearchGroupDownloadReply::Creation {
            result: SearchGroupDownloadCreation {
                epoch: token.generation.to_string(),
                receipt: plan.receipt.clone(),
                create_phase: create.phase,
                playlist_id,
                can_add,
                add,
            },
        })
    }
}
