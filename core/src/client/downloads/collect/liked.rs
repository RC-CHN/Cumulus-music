//! Explicit, native-verified system liked destination for a downloaded song.
use super::*;

pub(super) struct Choice {
    receipt: String,
    target: Option<(String, String)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadLikedDestination {
    pub receipt: String,
}

fn verify_origin(source: &Source, revision: &str) -> Result<()> {
    source.require()?;
    if source
        .api
        .download_system_liked_scope_revision()?
        .as_deref()
        != Some(revision)
    {
        return Err(Error::CredentialChanged);
    }
    Ok(())
}

impl MusicClient {
    pub async fn download_collect_liked_destination(
        &self,
        epoch: &str,
        receipt: &str,
    ) -> Result<AccountData<Option<DownloadLikedDestination>>> {
        let source = self.download_collection(epoch, receipt)?;
        if source.alternate_destination_songs().is_err() {
            return Ok(AccountData {
                epoch: epoch.into(),
                value: None,
            });
        }
        let slot = &self.downloads.inner.collect;
        let target_receipt = uuid::Uuid::new_v4().simple().to_string();
        with_visit(slot, &source, |visit| {
            if visit.busy || visit.attempt.is_some() || visit.creation.is_some() {
                return Err(Error::MutationBusy);
            }
            visit.liked = Some(Choice {
                receipt: target_receipt.clone(),
                target: None,
            });
            Ok(())
        })?;
        let Some(revision) = source.api.download_system_liked_scope_revision()? else {
            return Ok(AccountData {
                epoch: epoch.into(),
                value: None,
            });
        };
        let current = || {
            verify_origin(&source, &revision)?;
            with_visit(slot, &source, |visit| {
                if visit
                    .liked
                    .as_ref()
                    .is_none_or(|c| c.receipt != target_receipt)
                    || visit.busy
                    || visit.attempt.is_some()
                    || visit.creation.is_some()
                {
                    return Err(Error::StaleResponse);
                }
                Ok(())
            })
        };
        let service = PlaylistService::new(source.session.clone())?;
        let id = service
            .download_system_liked_destination(&source.token, &current)
            .await?;
        current()?;
        let value = with_visit(slot, &source, |visit| {
            let choice = visit
                .liked
                .as_mut()
                .filter(|c| c.receipt == target_receipt)
                .ok_or(Error::StaleResponse)?;
            if visit.busy || visit.attempt.is_some() || visit.creation.is_some() {
                return Err(Error::StaleResponse);
            }
            Ok(id.map(|id| {
                choice.target = Some((id, revision));
                DownloadLikedDestination {
                    receipt: target_receipt,
                }
            }))
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn download_collect_liked(
        &self,
        epoch: &str,
        receipt: &str,
        destination_receipt: &str,
        reconcile: bool,
    ) -> Result<AccountData<PlaylistMutationView>> {
        let source = self.download_collection(epoch, receipt)?;
        let songs = source.alternate_destination_songs()?;
        let slot = &self.downloads.inner.collect;
        let (attempt, revision) = with_visit(slot, &source, |visit| {
            if visit.busy {
                return Err(Error::MutationBusy);
            }
            if visit.creation.is_some() {
                return Err(Error::StaleResponse);
            }
            let (playlist, revision) = visit
                .liked
                .as_ref()
                .filter(|c| c.receipt == destination_receipt)
                .and_then(|c| c.target.clone())
                .ok_or(Error::StaleResponse)?;
            if let Some(attempt) = &visit.attempt {
                if !attempt.system_liked || attempt.playlist != playlist {
                    return Err(Error::StaleResponse);
                }
            } else if reconcile {
                return Err(Error::StaleResponse);
            } else {
                visit.attempt = Some(Attempt {
                    playlist,
                    operation: uuid::Uuid::new_v4().simple().to_string(),
                    system_liked: true,
                });
            }
            visit.busy = true;
            Ok((
                visit.attempt.clone().expect("system liked attempt"),
                revision,
            ))
        })?;
        let _lease = Lease {
            slot,
            source: source.clone(),
        };
        let check = || {
            verify_origin(&source, &revision)?;
            with_visit(slot, &source, |_| Ok(()))
        };
        let service = PlaylistService::new(source.session.clone())?;
        let previous = service.operation_for(&source.token, &attempt.operation)?;
        let record = match previous {
            Some(previous) if reconcile && previous.phase.pending() => {
                service
                    .reconcile_guarded_collect(
                        &source.token,
                        &attempt.operation,
                        &attempt.playlist,
                        songs,
                        true,
                        &check,
                    )
                    .await?
            }
            Some(previous) => previous,
            None if reconcile => return Err(Error::StaleResponse),
            None => {
                let outcome = if source.history() {
                    service
                        .guarded_collect_history(
                            &source.token,
                            HistoryCollectDestination::SystemLiked(&attempt.playlist),
                            songs,
                            &attempt.operation,
                            &check,
                        )
                        .await
                } else {
                    service
                        .guarded_collect_download_liked_group(
                            &source.token,
                            &attempt.playlist,
                            songs,
                            &attempt.operation,
                            &check,
                        )
                        .await
                };
                match outcome {
                    Ok(record) => record,
                    Err(error) => {
                        if let Some(record) =
                            service.operation_for(&source.token, &attempt.operation)?
                        {
                            record
                        } else {
                            with_visit(slot, &source, |visit| {
                                visit.attempt = None;
                                Ok(())
                            })?;
                            return Err(error);
                        }
                    }
                }
            }
        };
        check()?;
        if record.operation_id != attempt.operation
            || !record.system_liked_group
            || !matches!(&record.command, PlaylistCommand::Tracks { id, change: TrackChange::Add, ids, .. }
                if id == &attempt.playlist && ids.as_slice() == songs)
        {
            return Err(Error::StaleResponse);
        }
        let value = source
            .session
            .with_token(&source.token, |uid| playlists::view(record, uid))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
