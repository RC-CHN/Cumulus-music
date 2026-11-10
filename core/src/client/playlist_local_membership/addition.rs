use super::{AccountData, MusicClient};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    playback::QueueSnapshot,
    playlist::{PlaylistLocalStore, PlaylistMixedAddReceipt, PlaylistMixedAddTarget},
    protocol::{
        api::identifier,
        wire::serialization::{js_truthy, trim_js},
    },
};
use serde_json::json;

impl MusicClient {
    pub async fn playlist_mixed_add_target(
        &self,
        epoch: &str,
        id: &str,
        song: &str,
    ) -> Result<AccountData<Option<PlaylistMixedAddTarget>>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .mixed_add_target(&token, id, song)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    /// The ordinary one-song picker, only when M/3160 would take its bare
    /// request branch. Queue provenance and general mixed selections have
    /// separate upstream fields and must not fall through to this request.
    pub async fn add_catalog_song_to_mixed_playlist(
        &self,
        epoch: &str,
        id: &str,
        expected_revision: &str,
        song: &str,
        operation: &str,
    ) -> Result<AccountData<PlaylistMixedAddReceipt>> {
        let token = self.playlist_token(epoch)?;
        let song = identifier(&json!(song))?;
        let catalog_song = song.clone();
        // Refresh this specific track because the normal display DTO does not
        // carry the privilege that M/3085 uses to admit the singleton.
        let (batch, source_api, source_revision) = self
            .session
            .run_for(&token, |api, _| async move {
                let scope = api
                    .mixed_playlist_add_scope_revision()?
                    .ok_or(Error::CredentialChanged)?;
                let batch = CatalogApi::new(api.clone())
                    .songs(&[json!(catalog_song)], 1)
                    .await?;
                if api.mixed_playlist_add_scope_revision()?.as_deref() != Some(scope.as_str()) {
                    return Err(Error::CredentialChanged);
                }
                Ok((batch, api, scope))
            })
            .await?;
        let Some([Some(track)]) = batch.songs.get(..) else {
            return Err(Error::InvalidInput(
                "catalog song is unavailable for collection",
            ));
        };
        if !track["voice_id"].is_null()
            || !track["voice"].is_null()
            || !track["_cloud_public_id"].is_null()
            || !track["_cloud_catalog_id"].is_null()
        {
            return Err(Error::InvalidInput("expected an ordinary catalog song"));
        }
        for privilege in [track.get("privilege"), batch.privileges.get(&song)] {
            if let Some(row) = privilege.filter(|row| !row.is_null()) {
                if !row.is_object()
                    || (!row["id"].is_null()
                        && identifier(&row["id"]).ok().as_deref() != Some(song.as_str()))
                {
                    return Err(Error::Response("invalid collection privilege identity"));
                }
                // M/21 maps raw subp into the M/3085 singleton's subPriv.
                // A falsey field (including numeric zero) does not exclude it.
                for value in [&row["subp"], &row["subPriv"]] {
                    if js_truthy(value)
                        && !value
                            .as_f64()
                            .or_else(|| trim_js(value.as_str()?).parse().ok())
                            .is_some_and(|value| value.is_finite() && value > 0.0)
                    {
                        return Err(Error::InvalidInput("catalog song cannot be collected"));
                    }
                }
            }
        }

        let store = self.session.persistent_store()?;
        let persisted = || {
            self.session
                .with_token(&token, |uid| store.get(uid, "playback", "queue"))
        };
        let (actor_revision, saved, queue) = if let Some(controller) = self.player.controller.get()
        {
            let (state, queue) = controller.queue_with_state(&token).await?;
            (Some(state.queue_revision), None, queue)
        } else {
            // Collection need not initialize audio. Until the actor exists,
            // its authoritative starting queue is the same-account record.
            let saved = persisted()?;
            let queue = QueueSnapshot::load(saved.as_ref())?;
            (None, saved, queue)
        };
        if queue.items.iter().any(|entry| {
            entry.visible_in_queue()
                && [&entry.resource["song"]["id"], &entry.resource["id"]]
                    .into_iter()
                    .any(|id| identifier(id).is_ok_and(|id| id == song))
        }) {
            return Err(Error::InvalidInput(
                "queued song requires its original collection context",
            ));
        }
        let current_source = || {
            self.session.require_api(&token, &source_api)?;
            if source_api.mixed_playlist_add_scope_revision()?.as_deref()
                != Some(source_revision.as_str())
            {
                return Err(Error::CredentialChanged);
            }
            match (actor_revision, self.player.controller.get()) {
                (Some(expected), Some(controller))
                    if controller.snapshot().queue_revision == expected =>
                {
                    Ok(())
                }
                (None, None) if persisted()? == saved => Ok(()),
                _ => Err(Error::QueueChanged),
            }
        };
        let value = PlaylistLocalStore::new(self.session.clone())?
            .add_mixed(
                &token,
                id,
                expected_revision,
                vec![song],
                operation,
                &current_source,
            )
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub fn playlist_mixed_add_receipt(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<Option<PlaylistMixedAddReceipt>>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?.mixed_add_receipt(&token, id)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn reconcile_playlist_mixed_add(
        &self,
        epoch: &str,
        id: &str,
        operation_id: &str,
    ) -> Result<AccountData<PlaylistMixedAddReceipt>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .reconcile_mixed_add(&token, id, operation_id)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
