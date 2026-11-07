//! FavouriteModal's explicit new destination, retaining the prepared group.
use super::{
    Attempt, Destination, MusicClient, Page, PlaylistCommand, PlaylistMutation, PlaylistPhase,
    PlaylistService, Shelf, VoiceSongCollectResult, collect_result, stale_credentials, uuid,
};
use crate::{
    Error, Result,
    protocol::api::{PlaylistAddPermission, playlist_name},
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongCollectCreation {
    pub epoch: String,
    pub play_id: String,
    pub selection_receipt: String,
    pub create_operation_id: String,
    pub create_phase: PlaylistPhase,
    pub playlist_id: Option<String>,
    pub can_add: bool,
    pub add_failed: bool,
    pub add: Option<VoiceSongCollectResult>,
}

#[derive(Clone)]
pub(super) struct Intent {
    name: String,
    private: bool,
    create: String,
    add: String,
}

impl Intent {
    pub(super) fn operation_ids(&self) -> [&str; 2] {
        [&self.create, &self.add]
    }

    fn validate(&self, record: &PlaylistMutation) -> Result<()> {
        if record.operation_id != self.create
            || record.system_liked_group
            || !matches!(&record.command, PlaylistCommand::Create { name, private }
                if name == &self.name && *private == self.private)
        {
            return Err(Error::State(
                "creation journal belongs to a different collection",
            ));
        }
        Ok(())
    }
}

struct Lease<'a> {
    shelf: &'a Shelf,
    receipt: &'a str,
    intent: Intent,
    songs: Arc<[String]>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.shelf.release(self.receipt);
    }
}

impl Shelf {
    fn lease_creation<'a>(
        &'a self,
        receipt: &'a str,
        choice: Option<(&str, bool)>,
    ) -> Result<Lease<'a>> {
        self.with_selection(receipt, |active| {
            if active.busy {
                return Err(Error::MutationBusy);
            }
            if active.attempt.is_some() {
                return Err(Error::StaleResponse);
            }
            if let Some((name, private)) = choice {
                let intent = active.creation.get_or_insert_with(|| Intent {
                    name: name.into(),
                    private,
                    create: uuid(),
                    add: uuid(),
                });
                if intent.name != name || intent.private != private {
                    return Err(Error::StaleResponse);
                }
            }
            let intent = active.creation.clone().ok_or(Error::StaleResponse)?;
            active.busy = true;
            Ok(Lease {
                shelf: self,
                receipt,
                intent,
                songs: active.songs.clone(),
            })
        })
    }

    fn cancel_prepared(&self, receipt: &str) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active = slot
            .as_ref()
            .filter(|active| active.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        if active.busy {
            return Err(Error::MutationBusy);
        }
        if active.attempt.is_some() || active.creation.is_some() {
            return Err(Error::StaleResponse);
        }
        *slot = None;
        Ok(())
    }
}

impl Lease<'_> {
    fn creation(&self, service: &PlaylistService, page: &Page) -> Result<Option<PlaylistMutation>> {
        let record = service.operation_for(&page.token, &self.intent.create)?;
        if let Some(record) = &record {
            self.intent.validate(record)?;
        }
        Ok(record)
    }

    fn add_record(
        &self,
        service: &PlaylistService,
        page: &Page,
    ) -> Result<Option<PlaylistMutation>> {
        service.operation_for(&page.token, &self.intent.add)
    }

    async fn add_once(
        &self,
        service: &PlaylistService,
        page: &Page,
        create: &PlaylistMutation,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.intent.validate(create)?;
        create
            .confirmed_created_target()
            .ok_or(Error::StaleResponse)?;
        check()?;
        if let Some(record) = self.add_record(service, page)? {
            return Ok(record);
        }
        service
            .guarded_collect_created_songs(
                &page.token,
                &self.intent.create,
                &self.songs,
                &self.intent.add,
                check,
            )
            .await
            .map_err(stale_credentials)
    }

    fn view(
        &self,
        page: &Page,
        create: PlaylistMutation,
        add: Option<PlaylistMutation>,
    ) -> Result<VoiceSongCollectCreation> {
        self.intent.validate(&create)?;
        let id = create.confirmed_created_target().map(|(id, _, _)| id);
        // Readback may prove membership written elsewhere even if this
        // operation got a definitive negative callback from the service.
        let add_failed = add.as_ref().is_some_and(|record| {
            matches!(
                record.phase,
                PlaylistPhase::Confirmed | PlaylistPhase::Rejected
            ) && record.checked_at.is_some()
                && record.error_class.as_deref() == Some("service")
                && record.codes.contains_key("main")
        });
        let add = add
            .map(|record| {
                let id = id.ok_or(Error::State("unverified collection creation"))?;
                collect_result(
                    page,
                    self.receipt,
                    &self.songs,
                    &Attempt {
                        destination: Destination::Owned(id.into()),
                        operation: self.intent.add.clone(),
                    },
                    record,
                )
            })
            .transpose()?;
        let playlist_id = id.map(str::to_owned);
        Ok(VoiceSongCollectCreation {
            epoch: page.token.generation.to_string(),
            play_id: page.play_id.clone(),
            selection_receipt: self.receipt.into(),
            create_operation_id: create.operation_id,
            create_phase: create.phase,
            can_add: playlist_id.is_some() && add.is_none(),
            playlist_id,
            add_failed,
            add,
        })
    }
}

impl MusicClient {
    fn voice_song_collect_creation_scope(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
    ) -> Result<(
        Arc<Page>,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        let (page, collect) = self.voice_song_collect_scope(epoch, play_id, card_receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let revision = page.revision.clone();
        let check = move || {
            if api
                .song_card_collection_create_scope_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            collect()
        };
        check()?;
        Ok((page, check))
    }

    /// Close the unsubmitted picker/form. This is a local-only invalidation;
    /// it neither interrupts a dispatched write nor deletes a journal entry.
    pub fn player_voice_songs_collect_cancel_prepared(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<()> {
        let (page, check) = self.voice_song_collect_scope(epoch, play_id, card_receipt)?;
        check()?;
        page.collect.cancel_prepared(selection_receipt)?;
        check()
    }

    /// Explicitly create a playlist for the prepared group. The native list
    /// must confirm the acknowledged ID before the initial ordered add.
    pub async fn player_voice_songs_collect_create(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        name: &str,
        private: bool,
    ) -> Result<VoiceSongCollectCreation> {
        let name = playlist_name(name)?;
        let (page, check) = self.voice_song_collect_creation_scope(epoch, play_id, card_receipt)?;
        let service = PlaylistService::new(self.session.clone())?;
        let lease = page
            .collect
            .lease_creation(selection_receipt, Some((&name, private)))?;
        let create = if let Some(create) = lease.creation(&service, &page)? {
            // Even after an acknowledged create, replaying the create action
            // cannot start an add that the original flow did not reach.
            let add = lease.add_record(&service, &page)?;
            check()?;
            return lease.view(&page, create, add);
        } else {
            service
                .guarded_create_collect_playlist(
                    &page.token,
                    &lease.intent.create,
                    &name,
                    private,
                    &check,
                )
                .await
                .map_err(stale_credentials)?
        };
        let add = if create.confirmed_created_target().is_some() {
            Some(lease.add_once(&service, &page, &create, &check).await?)
        } else {
            None
        };
        check()?;
        lease.view(&page, create, add)
    }

    /// Local status or readback only. It never creates a playlist or adds songs.
    pub async fn player_voice_songs_collect_creation_status(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        reconcile: bool,
    ) -> Result<VoiceSongCollectCreation> {
        let (page, check) = self.voice_song_collect_creation_scope(epoch, play_id, card_receipt)?;
        let service = PlaylistService::new(self.session.clone())?;
        let lease = page.collect.lease_creation(selection_receipt, None)?;
        let mut create = lease
            .creation(&service, &page)?
            .ok_or(Error::StaleResponse)?;
        if reconcile && create.phase.pending() {
            create = service
                .reconcile_guarded_create(&page.token, &lease.intent.create, &check)
                .await
                .map_err(stale_credentials)?;
            lease.intent.validate(&create)?;
        }
        let mut add = lease.add_record(&service, &page)?;
        if reconcile && add.as_ref().is_some_and(|record| record.phase.pending()) {
            let (id, _, _) = create
                .confirmed_created_target()
                .ok_or(Error::StaleResponse)?;
            add = Some(
                service
                    .reconcile_guarded_collect(
                        &page.token,
                        &lease.intent.add,
                        id,
                        &lease.songs,
                        false,
                        &check,
                    )
                    .await
                    .map_err(stale_credentials)?,
            );
        }
        check()?;
        lease.view(&page, create, add)
    }

    /// Explicit first add after readback of an acknowledged creation. It never
    /// repeats an existing add operation, whatever its journal phase.
    pub async fn player_voice_songs_collect_add_created(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<VoiceSongCollectCreation> {
        let (page, check) = self.voice_song_collect_creation_scope(epoch, play_id, card_receipt)?;
        let service = PlaylistService::new(self.session.clone())?;
        let lease = page.collect.lease_creation(selection_receipt, None)?;
        let create = lease
            .creation(&service, &page)?
            .ok_or(Error::StaleResponse)?;
        let add = lease.add_once(&service, &page, &create, &check).await?;
        check()?;
        lease.view(&page, create, Some(add))
    }
}
