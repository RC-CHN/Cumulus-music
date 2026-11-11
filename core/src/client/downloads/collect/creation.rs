//! A new destination requires its own acknowledged create, then one guarded add.
use super::{Lease, MusicClient, Source, playlists, with_visit};
use crate::{
    Error, Result,
    client::dto::{AccountData, PlaylistMutationView},
    playlist::{
        HistoryCollectDestination, PlaylistCommand, PlaylistMutation, PlaylistService, TrackChange,
    },
    protocol::api::playlist_name,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub(super) struct CreationAttempt {
    name: String,
    private: bool,
    create_id: String,
    add_id: String,
}

impl CreationAttempt {
    fn new(name: String, private: bool) -> Self {
        Self {
            name,
            private,
            create_id: uuid::Uuid::new_v4().simple().to_string(),
            add_id: uuid::Uuid::new_v4().simple().to_string(),
        }
    }

    fn check_create(&self, record: &PlaylistMutation) -> Result<()> {
        if record.operation_id != self.create_id
            || record.system_liked_group
            || !matches!(&record.command, PlaylistCommand::Create { name, private }
                if name == &self.name && *private == self.private)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn check_add(&self, record: &PlaylistMutation, playlist: &str, songs: &[String]) -> Result<()> {
        if record.operation_id != self.add_id
            || record.system_liked_group
            || record.playlist_id.as_deref() != Some(playlist)
            || !matches!(&record.command, PlaylistCommand::Tracks { id, change: TrackChange::Add, ids, .. }
                if id == playlist && ids == songs)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DownloadCollectCreationCommand {
    Create {
        name: String,
        private_playlist: bool,
    },
    Status {
        reconcile: bool,
    },
    AddCreated {},
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCollectCreation {
    pub receipt: String,
    pub create: PlaylistMutationView,
    pub add: Option<PlaylistMutationView>,
    pub can_add: bool,
}

impl Source {
    fn require_creation(&self) -> Result<()> {
        self.require()?;
        if self
            .api
            .download_collection_create_scope_revision()?
            .as_deref()
            != Some(self.credentials.as_str())
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

impl MusicClient {
    pub async fn download_collect_creation(
        &self,
        epoch: &str,
        receipt: &str,
        command: DownloadCollectCreationCommand,
    ) -> Result<AccountData<DownloadCollectCreation>> {
        use DownloadCollectCreationCommand as Command;
        let source = self.download_collection(epoch, receipt)?;
        source.require_creation()?;
        let songs = source.alternate_destination_songs()?;
        let slot = &self.downloads.inner.collect;
        let requested = match &command {
            Command::Create {
                name,
                private_playlist,
            } => Some((playlist_name(name)?, *private_playlist)),
            _ => None,
        };
        let (attempt, first) = with_visit(slot, &source, |visit| {
            if visit.busy {
                return Err(Error::MutationBusy);
            }
            if visit.attempt.is_some() {
                return Err(Error::StaleResponse);
            }
            let (attempt, first) = match (&visit.creation, requested) {
                (None, Some((name, private))) => {
                    let attempt = CreationAttempt::new(name, private);
                    visit.creation = Some(attempt.clone());
                    (attempt, true)
                }
                (Some(attempt), Some((name, private)))
                    if attempt.name == name && attempt.private == private =>
                {
                    (attempt.clone(), false)
                }
                (Some(attempt), None) => (attempt.clone(), false),
                _ => return Err(Error::StaleResponse),
            };
            visit.busy = true;
            Ok((attempt, first))
        })?;
        let _lease = Lease {
            slot,
            source: source.clone(),
        };
        let check = || {
            source.require_creation()?;
            with_visit(slot, &source, |_| Ok(()))
        };
        let service = PlaylistService::new(source.session.clone())?;
        let mut create = if first {
            let result = service
                .guarded_create_download_collection(
                    &source.token,
                    &attempt.create_id,
                    &attempt.name,
                    attempt.private,
                    &check,
                )
                .await;
            match result {
                Ok(record) => record,
                Err(error) => match service.operation_for(&source.token, &attempt.create_id)? {
                    Some(record) => record,
                    None if matches!(error.root(), Error::Transport(_)) => {
                        // A guarded create saves its Sending record before
                        // dispatch. A transport error during preflight with no
                        // record is definitely unsent, so this visit can retry.
                        check()?;
                        with_visit(slot, &source, |visit| {
                            if visit.creation.as_ref().map(|v| &v.create_id)
                                != Some(&attempt.create_id)
                            {
                                return Err(Error::StaleResponse);
                            }
                            visit.creation = None;
                            Ok(())
                        })?;
                        return Err(Error::State("download collection create not sent"));
                    }
                    None => return Err(error),
                },
            }
        } else {
            service
                .operation_for(&source.token, &attempt.create_id)?
                .ok_or(Error::StaleResponse)?
        };
        attempt.check_create(&create)?;
        check()?;
        let readback = matches!(command, Command::Status { reconcile: true });
        if readback && create.phase.pending() {
            create = service
                .reconcile_guarded_create(&source.token, &attempt.create_id, &check)
                .await?;
            attempt.check_create(&create)?;
        }
        let confirmed = create
            .confirmed_created_target()
            .map(|(id, _, _)| id.to_owned());
        let mut add = service.operation_for(&source.token, &attempt.add_id)?;
        if let Some(record) = &add {
            attempt.check_add(
                record,
                confirmed.as_deref().ok_or(Error::StaleResponse)?,
                songs,
            )?;
        }
        let submit_add = first || matches!(command, Command::AddCreated {});
        if submit_add && add.is_none() {
            if confirmed.is_some() {
                check()?;
                let result = if source.history() {
                    service
                        .guarded_collect_history(
                            &source.token,
                            HistoryCollectDestination::Created(&attempt.create_id),
                            songs,
                            &attempt.add_id,
                            &check,
                        )
                        .await
                } else {
                    service
                        .guarded_collect_created_download(
                            &source.token,
                            &attempt.create_id,
                            songs,
                            &attempt.add_id,
                            &check,
                        )
                        .await
                };
                add = Some(match result {
                    Ok(record) => record,
                    Err(error) => service
                        .operation_for(&source.token, &attempt.add_id)?
                        .ok_or(error)?,
                });
            } else if !first {
                return Err(Error::StaleResponse);
            }
        } else if readback && add.as_ref().is_some_and(|r| r.phase.pending()) {
            add = Some(
                service
                    .reconcile_guarded_collect(
                        &source.token,
                        &attempt.add_id,
                        confirmed.as_deref().ok_or(Error::StaleResponse)?,
                        songs,
                        false,
                        &check,
                    )
                    .await?,
            );
        }
        if let Some(record) = &add {
            attempt.check_add(
                record,
                confirmed.as_deref().ok_or(Error::StaleResponse)?,
                songs,
            )?;
        }
        check()?;
        let value = source.session.with_token(&source.token, |uid| {
            let can_add = add.is_none() && confirmed.is_some();
            Ok(DownloadCollectCreation {
                receipt: receipt.into(),
                create: playlists::view(create, uid)?,
                add: add.map(|r| playlists::view(r, uid)).transpose()?,
                can_add,
            })
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
