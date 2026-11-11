//! Destinations are tied to one chosen quality and one current-song offer.
use super::*;
use crate::{
    playlist::{PlaylistCommand, PlaylistMutation, PlaylistPhase, PlaylistService, TrackChange},
    protocol::api::playlist_name,
};

#[derive(Clone)]
pub(super) struct Attempt {
    destination: Destination,
    add: String,
    permit: String,
}

#[derive(Clone)]
enum Destination {
    Existing(String),
    New {
        name: String,
        private: bool,
        create: String,
    },
}

impl Destination {
    fn same(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Existing(a), Self::Existing(b)) => a == b,
            (
                Self::New {
                    name: a,
                    private: x,
                    ..
                },
                Self::New {
                    name: b,
                    private: y,
                    ..
                },
            ) => a == b && x == y,
            _ => false,
        }
    }
}

impl Choice {
    fn attempt(&self) -> Result<Attempt> {
        self.attempt.clone().ok_or(Error::StaleResponse)
    }

    fn begin_attempt(&mut self, destination: Destination) -> Result<(Attempt, bool)> {
        if let Some(existing) = &self.attempt {
            if !existing.destination.same(&destination) {
                return Err(Error::MutationUncertain);
            }
            return Ok((existing.clone(), false));
        }
        let attempt = Attempt {
            destination,
            add: receipt(),
            permit: receipt(),
        };
        self.attempt = Some(attempt.clone());
        Ok((attempt, true))
    }
}

impl Attempt {
    pub(super) fn pending(
        &self,
        service: &PlaylistService,
        token: &crate::account::SessionToken,
    ) -> Result<bool> {
        if let Destination::New { create, .. } = &self.destination
            && service
                .operation_for(token, create)?
                .is_some_and(|record| record.phase.pending())
        {
            return Ok(true);
        }
        Ok(service
            .operation_for(token, &self.add)?
            .is_some_and(|record| record.phase.pending()))
    }

    fn view(
        &self,
        record: PlaylistMutation,
        song: &str,
        playlist: &str,
    ) -> Result<CurrentSongDownloadPicker> {
        if record.operation_id != self.add
            || !matches!(&record.command, PlaylistCommand::Tracks { id, change: TrackChange::Add, ids, .. }
                if id == playlist && ids.len() == 1 && ids[0] == song)
        {
            return Err(Error::StaleResponse);
        }
        let (continued, add_failed) =
            record.download_picker_outcome(matches!(self.destination, Destination::Existing(_)));
        Ok(CurrentSongDownloadPicker {
            playlist_id: playlist.into(),
            operation_id: record.operation_id,
            phase: record.phase,
            add_failed,
            continuation: continued.then(|| self.permit.clone()),
        })
    }

    fn creation_target(&self, create: &PlaylistMutation) -> Result<Option<String>> {
        let Destination::New {
            name,
            private,
            create: expected,
        } = &self.destination
        else {
            return Err(Error::StaleResponse);
        };
        if &create.operation_id != expected
            || !matches!(&create.command, PlaylistCommand::Create { name: n, private: p } if n == name && p == private)
        {
            return Err(Error::StaleResponse);
        }
        Ok(create
            .confirmed_created_target()
            .map(|(id, _, _)| id.to_owned()))
    }

    fn creation_id(&self) -> Result<&str> {
        let Destination::New { create, .. } = &self.destination else {
            return Err(Error::StaleResponse);
        };
        Ok(create)
    }

    pub(super) fn check_continuation(
        &self,
        service: &PlaylistService,
        plan: &Plan,
        expected: &str,
    ) -> Result<()> {
        if expected != self.permit {
            return Err(Error::StaleResponse);
        }
        let token = &plan.authority.token;
        let playlist = match &self.destination {
            Destination::Existing(id) => id.clone(),
            Destination::New { create, .. } => {
                let record = service
                    .operation_for(token, create)?
                    .ok_or(Error::StaleResponse)?;
                self.creation_target(&record)?.ok_or(Error::StaleResponse)?
            }
        };
        let record = service
            .operation_for(token, &self.add)?
            .ok_or(Error::StaleResponse)?;
        let picker = self.view(record, &plan.authority.song, &playlist)?;
        if picker.continuation.as_deref() != Some(expected) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

fn existing_record(service: &PlaylistService, plan: &Plan, id: &str) -> Result<PlaylistMutation> {
    service
        .operation_for(&plan.authority.token, id)?
        .ok_or(Error::State(
            "playlist request was not recorded; reopen to choose a destination",
        ))
}

impl MusicClient {
    pub(super) async fn current_download_destination(
        &self,
        epoch: &str,
        plan: &Plan,
        command: CurrentSongDownloadCommand,
    ) -> Result<Reply> {
        use CurrentSongDownloadCommand as C;
        let (selection, scope) = match &command {
            C::Destinations { selection, .. }
            | C::ChoosePlaylist { selection, .. }
            | C::PickerStatus { selection, .. } => (selection, CredentialScope::Picker),
            C::CreatePlaylist { selection, .. }
            | C::CreationStatus { selection, .. }
            | C::AddCreatedPlaylist { selection } => (selection, CredentialScope::Create),
            _ => unreachable!("handled by the current-song command dispatcher"),
        };
        let choice = plan.state()?.choice(selection)?.proof.clone();
        let check = || plan.check_choice(&choice, scope);
        check()?;
        if choice.admission != Decision::PlaylistRequired || !plan.authority.collectable {
            return Err(Error::State(
                "this download does not require a playlist destination",
            ));
        }
        let service = PlaylistService::new(self.session.clone())?;
        let token = &plan.authority.token;
        let song = &plan.authority.song;
        match command {
            C::Destinations { sort, page, .. } => {
                let returned = self
                    .playlist_collect_page_scope(epoch, sort, page, false)
                    .await?;
                check()?;
                if returned.epoch != epoch {
                    return Err(Error::StaleResponse);
                }
                let can_create = plan.check_choice(&choice, CredentialScope::Create).is_ok();
                check()?;
                let mut state = plan.state()?;
                let active = state.choice_mut(&choice.receipt)?;
                let mut rows = Vec::with_capacity(returned.value.playlists.len());
                for playlist in returned.value.playlists {
                    let receipt = if let Some((_, value)) = active
                        .destinations
                        .iter()
                        .find(|(id, _)| id == &playlist.id)
                    {
                        value.clone()
                    } else {
                        if active.destinations.len() == 256 {
                            return Err(Error::State("playlist destination window is full"));
                        }
                        let value = receipt();
                        active
                            .destinations
                            .push((playlist.id.clone(), value.clone()));
                        value
                    };
                    rows.push(CurrentSongDownloadDestination { receipt, playlist });
                }
                Ok(Reply::Destinations {
                    rows,
                    next_page: returned.value.next_page,
                    can_create,
                })
            }
            C::ChoosePlaylist { destination, .. } => {
                let playlist = plan
                    .state()?
                    .choice(&choice.receipt)?
                    .destinations
                    .iter()
                    .find(|(_, grant)| grant == &destination)
                    .map(|(id, _)| id.clone())
                    .ok_or(Error::StaleResponse)?;
                self.ensure_current_picker_decision(epoch, plan).await?;
                check()?;
                let (attempt, first) = plan
                    .state()?
                    .choice_mut(&choice.receipt)?
                    .begin_attempt(Destination::Existing(playlist.clone()))?;
                let record = if first {
                    service
                        .guarded_add_song(token, &playlist, song, &attempt.add, &check)
                        .await?
                } else {
                    existing_record(&service, plan, &attempt.add)?
                };
                check()?;
                Ok(Reply::Picker {
                    picker: attempt.view(record, song, &playlist)?,
                })
            }
            C::PickerStatus { reconcile, .. } => {
                let attempt = plan.state()?.choice(&choice.receipt)?.attempt()?;
                let Destination::Existing(playlist) = &attempt.destination else {
                    return Err(Error::StaleResponse);
                };
                let mut record = existing_record(&service, plan, &attempt.add)?;
                if reconcile && record.phase.pending() {
                    record = service
                        .reconcile_guarded_add(token, &attempt.add, &check)
                        .await?;
                }
                check()?;
                Ok(Reply::Picker {
                    picker: attempt.view(record, song, playlist)?,
                })
            }
            C::CreatePlaylist { name, private, .. } => {
                let name = playlist_name(&name)?;
                self.ensure_current_picker_decision(epoch, plan).await?;
                check()?;
                let (attempt, first) =
                    plan.state()?
                        .choice_mut(&choice.receipt)?
                        .begin_attempt(Destination::New {
                            name: name.clone(),
                            private,
                            create: receipt(),
                        })?;
                let create_id = attempt.creation_id()?;
                let create = if first {
                    service
                        .guarded_create_song_card(token, create_id, &name, private, &check)
                        .await?
                } else {
                    existing_record(&service, plan, create_id)?
                };
                check()?;
                let playlist = attempt.creation_target(&create)?;
                let add = if first && playlist.is_some() {
                    // A different track/credential after the create ack
                    // cannot use that ack to send the second write.
                    self.ensure_current_picker_decision(epoch, plan).await?;
                    check()?;
                    Some(
                        service
                            .guarded_add_created_song(token, create_id, song, &attempt.add, &check)
                            .await?,
                    )
                } else {
                    service.operation_for(token, &attempt.add)?
                };
                check()?;
                creation_reply(&attempt, song, create, playlist, add)
            }
            C::CreationStatus { reconcile, .. } => {
                let attempt = plan.state()?.choice(&choice.receipt)?.attempt()?;
                let create_id = attempt.creation_id()?;
                let mut create = existing_record(&service, plan, create_id)?;
                if reconcile && create.phase.pending() {
                    create = service
                        .reconcile_guarded_create(token, create_id, &check)
                        .await?;
                }
                let playlist = attempt.creation_target(&create)?;
                let mut add = service.operation_for(token, &attempt.add)?;
                if reconcile && add.as_ref().is_some_and(|record| record.phase.pending()) {
                    add = Some(
                        service
                            .reconcile_guarded_add(token, &attempt.add, &check)
                            .await?,
                    );
                }
                check()?;
                creation_reply(&attempt, song, create, playlist, add)
            }
            C::AddCreatedPlaylist { .. } => {
                let attempt = plan.state()?.choice(&choice.receipt)?.attempt()?;
                let create_id = attempt.creation_id()?;
                let create = existing_record(&service, plan, create_id)?;
                let playlist = attempt
                    .creation_target(&create)?
                    .ok_or(Error::State("a direct create acknowledgement is required"))?;
                let add = if let Some(record) = service.operation_for(token, &attempt.add)? {
                    record
                } else {
                    self.ensure_current_picker_decision(epoch, plan).await?;
                    check()?;
                    service
                        .guarded_add_created_song(token, create_id, song, &attempt.add, &check)
                        .await?
                };
                check()?;
                creation_reply(&attempt, song, create, Some(playlist), Some(add))
            }
            _ => unreachable!("handled by the current-song command dispatcher"),
        }
    }

    async fn ensure_current_picker_decision(&self, epoch: &str, plan: &Plan) -> Result<()> {
        if self
            .current_download_decision(epoch, &plan.authority)
            .await?
            != Decision::PlaylistRequired
        {
            return Err(Error::State("playlist download admission changed"));
        }
        Ok(())
    }
}

fn creation_reply(
    attempt: &Attempt,
    song: &str,
    create: PlaylistMutation,
    playlist: Option<String>,
    add: Option<PlaylistMutation>,
) -> Result<Reply> {
    let can_resume_add =
        playlist.is_some() && create.phase == PlaylistPhase::Confirmed && add.is_none();
    let add = match (add, &playlist) {
        (Some(record), Some(id)) => Some(attempt.view(record, song, id)?),
        (None, _) => None,
        (Some(_), None) => return Err(Error::StaleResponse),
    };
    Ok(Reply::Creation {
        creation: CurrentSongDownloadCreation {
            create_operation_id: create.operation_id,
            create_phase: create.phase,
            playlist_id: playlist,
            add,
            can_resume_add,
        },
    })
}
