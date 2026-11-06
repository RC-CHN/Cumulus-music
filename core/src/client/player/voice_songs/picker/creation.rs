//! An explicit create form, then one add to the verified newly created ID.
use super::{
    Bound, Destination, Lease, MusicClient, Page, Target, VoiceSongDownloadPicker, result,
};
use crate::{
    Error, Result,
    client::dto::VoiceSongDownloadDecision,
    playlist::{PlaylistCommand, PlaylistMutation, PlaylistPhase, PlaylistService},
    protocol::api::{PlaylistAddPermission, playlist_name},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongDownloadCreation {
    pub epoch: String,
    pub play_id: String,
    pub track_id: String,
    pub create_operation_id: String,
    pub create_phase: PlaylistPhase,
    pub playlist_id: Option<String>,
    pub add: Option<VoiceSongDownloadPicker>,
}

fn creation_id<'a>(lease: &'a Lease<'_>) -> Result<&'a str> {
    lease
        .attempt
        .destination
        .create_operation()
        .ok_or(Error::StaleResponse)
}

fn observe_creation(lease: &mut Lease<'_>, record: &PlaylistMutation) -> Result<()> {
    let Destination::New {
        name,
        private,
        create,
        ..
    } = &lease.attempt.destination
    else {
        return Err(Error::StaleResponse);
    };
    if create != &record.operation_id
        || !matches!(&record.command, PlaylistCommand::Create {name:n, private:p} if n == name && p == private)
    {
        return Err(Error::State(
            "picker completion belongs to another creation",
        ));
    }
    if record.phase == PlaylistPhase::Confirmed && record.error_class.as_deref() != Some("service")
    {
        let id = record
            .playlist_id
            .as_deref()
            .ok_or(Error::State("verified playlist creation has no ID"))?;
        lease.bind_created(id)?;
    }
    Ok(())
}

fn view(
    page: &Page,
    lease: &Lease<'_>,
    create: PlaylistMutation,
    add: Option<PlaylistMutation>,
) -> Result<VoiceSongDownloadCreation> {
    let song = &page
        .rows
        .get(lease.attempt.index)
        .ok_or(Error::StaleResponse)?
        .track
        .id;
    Ok(VoiceSongDownloadCreation {
        epoch: page.token.generation.to_string(),
        play_id: page.play_id.clone(),
        track_id: song.clone(),
        create_operation_id: create.operation_id,
        create_phase: create.phase,
        playlist_id: lease.attempt.destination.playlist().map(str::to_owned),
        add: add
            .map(|record| result(page, &lease.attempt, record))
            .transpose()?,
    })
}

async fn add_once(
    service: &PlaylistService,
    page: &Page,
    lease: &Lease<'_>,
    check: &PlaylistAddPermission<'_>,
) -> Result<PlaylistMutation> {
    if let Some(existing) = service.operation_for(&page.token, &lease.attempt.operation)? {
        return Ok(existing);
    }
    let playlist = lease
        .attempt
        .destination
        .playlist()
        .ok_or(Error::State("verify the created playlist first"))?;
    let song = &page
        .rows
        .get(lease.attempt.index)
        .ok_or(Error::StaleResponse)?
        .track
        .id;
    check()?;
    service
        .guarded_add_song(&page.token, playlist, song, &lease.attempt.operation, check)
        .await
        .map_err(super::stale_credentials)
}

impl MusicClient {
    fn voice_song_create_scope(
        &self,
        target: Target<'_>,
    ) -> Result<(
        Bound,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        let (bound, picker_scope) = self.voice_song_picker_scope(target)?;
        let api = bound.page.api.upgrade().ok_or(Error::StaleResponse)?;
        let revision = bound.page.revision.clone();
        let check = move || {
            if api
                .song_card_playlist_create_scope_revision()
                .map_err(super::stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            picker_scope()
        };
        check()?;
        Ok((bound, check))
    }

    /// Explicit create submission. Privacy is mandatory and never inferred.
    pub async fn player_voice_song_download_create_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        index: usize,
        name: &str,
        private: bool,
    ) -> Result<VoiceSongDownloadCreation> {
        self.voice_song_create_playlist(
            Target::ordinary(epoch, play_id, card_receipt, index),
            name,
            private,
        )
        .await
    }

    pub(in crate::client::player::voice_songs) async fn voice_song_create_playlist(
        &self,
        target: Target<'_>,
        name: &str,
        private: bool,
    ) -> Result<VoiceSongDownloadCreation> {
        let (bound, check) = self.voice_song_create_scope(target)?;
        let page = &bound.page;
        let name = playlist_name(name)?;
        if !bound.row().playlist_eligible {
            return Err(Error::InvalidInput("invalid vinyl episode download row"));
        }
        let service = PlaylistService::new(self.session.clone())?;
        let (mut lease, repeated) = page.picker.choose(
            &service,
            &bound,
            Destination::New {
                name: name.clone(),
                private,
                create: uuid::Uuid::new_v4().simple().to_string(),
                playlist: None,
            },
        )?;
        let create = if repeated {
            service
                .operation_for(&page.token, creation_id(&lease)?)?
                .ok_or(Error::StaleResponse)?
        } else {
            let decision = self
                .voice_song_download_decision(target.epoch, page, bound.row(), true)
                .await?;
            if decision != VoiceSongDownloadDecision::PlaylistRequired {
                return Err(Error::State(
                    "this SongCard download does not require a playlist destination",
                ));
            }
            check()?;
            service
                .guarded_create_song_card(&page.token, creation_id(&lease)?, &name, private, &check)
                .await
                .map_err(super::stale_credentials)?
        };
        check()?;
        observe_creation(&mut lease, &create)?;
        let add = if !repeated
            && create.phase == PlaylistPhase::Confirmed
            && create.codes.get("main") == Some(&200)
        {
            // Normal official flow chains the initial add after the acknowledged
            // creation. An ambiguous creation/read-back needs explicit resume.
            Some(add_once(&service, page, &lease, &check).await?)
        } else {
            service.operation_for(&page.token, &lease.attempt.operation)?
        };
        check()?;
        view(page, &lease, create, add)
    }

    /// Read only, even if this read verifies an ambiguous create. The host
    /// must explicitly resume the not-yet-started add in that case.
    pub async fn player_voice_song_download_creation_status(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        index: usize,
        reconcile: bool,
    ) -> Result<VoiceSongDownloadCreation> {
        self.voice_song_creation_status(
            Target::ordinary(epoch, play_id, card_receipt, index),
            reconcile,
        )
        .await
    }

    pub(in crate::client::player::voice_songs) async fn voice_song_creation_status(
        &self,
        target: Target<'_>,
        reconcile: bool,
    ) -> Result<VoiceSongDownloadCreation> {
        let (bound, check) = self.voice_song_create_scope(target)?;
        let page = &bound.page;
        let mut lease = page.picker.get(&bound, None)?;
        let service = PlaylistService::new(self.session.clone())?;
        let id = creation_id(&lease)?;
        let create = if reconcile {
            service
                .reconcile_guarded_create(&page.token, id, &check)
                .await
                .map_err(super::stale_credentials)?
        } else {
            service
                .operation_for(&page.token, id)?
                .ok_or(Error::StaleResponse)?
        };
        observe_creation(&mut lease, &create)?;
        let mut add = service.operation_for(&page.token, &lease.attempt.operation)?;
        if reconcile && add.as_ref().is_some_and(|record| record.phase.pending()) {
            add = Some(
                service
                    .reconcile_guarded_add(&page.token, &lease.attempt.operation, &check)
                    .await
                    .map_err(super::stale_credentials)?,
            );
        }
        check()?;
        view(page, &lease, create, add)
    }

    /// Explicit continuation after recovery of a created playlist. If the add
    /// already has a journal entry, this never submits it again.
    pub async fn player_voice_song_download_add_created_playlist(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        index: usize,
    ) -> Result<VoiceSongDownloadCreation> {
        self.voice_song_add_created_playlist(Target::ordinary(epoch, play_id, card_receipt, index))
            .await
    }

    pub(in crate::client::player::voice_songs) async fn voice_song_add_created_playlist(
        &self,
        target: Target<'_>,
    ) -> Result<VoiceSongDownloadCreation> {
        let (bound, check) = self.voice_song_create_scope(target)?;
        let page = &bound.page;
        let mut lease = page.picker.get(&bound, None)?;
        let service = PlaylistService::new(self.session.clone())?;
        let create = service
            .operation_for(&page.token, creation_id(&lease)?)?
            .ok_or(Error::StaleResponse)?;
        observe_creation(&mut lease, &create)?;
        let add = add_once(&service, page, &lease, &check).await?;
        check()?;
        view(page, &lease, create, Some(add))
    }
}
