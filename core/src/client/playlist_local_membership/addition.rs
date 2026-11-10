mod batch;
pub use batch::{PlaylistMixedBatchCollection, PlaylistMixedBatchSource};

use super::{AccountData, MusicClient};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::CatalogApi,
    playback::QueueSnapshot,
    playlist::{
        MixedPlaylistAddPlan, MixedPlaylistWriteLease, PlaylistLocalStore, PlaylistMixedAddReceipt,
        PlaylistMixedAddTarget,
    },
    protocol::{
        api::{Api, identifier},
        wire::serialization::{js_truthy, trim_js},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

struct CatalogCollectionAuthority {
    api: Arc<dyn Api>,
    credentials: String,
}

impl CatalogCollectionAuthority {
    fn new(api: Arc<dyn Api>) -> Result<Self> {
        let credentials = api
            .mixed_playlist_add_scope_revision()?
            .ok_or(Error::CredentialChanged)?;
        Ok(Self { api, credentials })
    }
    fn require(&self, client: &MusicClient, token: &SessionToken) -> Result<()> {
        client.session.require_api(token, &self.api)?;
        if self.api.mixed_playlist_add_scope_revision()?.as_deref() != Some(&self.credentials) {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum CollectPrivilege {
    Missing,
    Falsey,
    Positive,
    Denied,
}

fn catalog_collect_privilege(
    song: &str,
    track: &Value,
    separate: Option<&Value>,
) -> Result<CollectPrivilege> {
    if identifier(&track["id"]).ok().as_deref() != Some(song)
        || [
            "voice",
            "voice_id",
            "voiceId",
            "djProgramId",
            "_cloud_public_id",
            "_cloud_catalog_id",
        ]
        .into_iter()
        .any(|key| js_truthy(&track[key]))
    {
        return Err(Error::InvalidInput("expected an ordinary catalog song"));
    }
    let mut observed = None;
    for row in [track.get("privilege"), separate]
        .into_iter()
        .flatten()
        .filter(|row| !row.is_null())
    {
        if !row.is_object()
            || (!row["id"].is_null() && identifier(&row["id"]).ok().as_deref() != Some(song))
        {
            return Err(Error::Response("invalid collection privilege identity"));
        }
        if ["cs", "cloudSong"]
            .into_iter()
            .any(|key| js_truthy(&row[key]))
        {
            return Err(Error::InvalidInput("expected an ordinary catalog song"));
        }
        // M/21:362 maps the wire subp value into M/3085's subPriv.
        // Missing differs from a present falsey value in its batch prefilter.
        for value in [row.get("subp"), row.get("subPriv")].into_iter().flatten() {
            let privilege = if !js_truthy(value) {
                CollectPrivilege::Falsey
            } else if value
                .as_f64()
                .or_else(|| trim_js(value.as_str()?).parse().ok())
                .is_some_and(|value| value.is_finite() && value > 0.0)
            {
                CollectPrivilege::Positive
            } else {
                CollectPrivilege::Denied
            };
            if observed.is_some_and(|previous| previous != privilege) {
                return Err(Error::Response("conflicting collection privileges"));
            }
            observed = Some(privilege);
        }
    }
    Ok(observed.unwrap_or(CollectPrivilege::Missing))
}

impl MusicClient {
    pub async fn playlist_mixed_add_target(
        &self,
        epoch: &str,
        id: &str,
        song: &str,
    ) -> Result<AccountData<Option<PlaylistMixedAddTarget>>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .mixed_add_target(&token, id, &[identifier(&json!(song))?])
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
        let source_token = &token;
        let (batch, authority) = self
            .session
            .run_for(&token, |api, _| async move {
                let authority = CatalogCollectionAuthority::new(api.clone())?;
                let batch = CatalogApi::new(api.clone())
                    .songs(&[json!(catalog_song)], 1)
                    .await?;
                authority.require(self, source_token)?;
                Ok((batch, authority))
            })
            .await?;
        let Some([Some(track)]) = batch.songs.get(..) else {
            return Err(Error::InvalidInput(
                "catalog song is unavailable for collection",
            ));
        };
        if catalog_collect_privilege(&song, track, batch.privileges.get(&song))?
            == CollectPrivilege::Denied
        {
            return Err(Error::InvalidInput("catalog song cannot be collected"));
        }

        let store = PlaylistLocalStore::new(self.session.clone())?;
        let lease = store.mixed_write_lease(&token)?;
        let value = self
            .submit_catalog_collection(
                &token,
                &authority,
                &store,
                lease,
                MixedPlaylistAddPlan {
                    id,
                    expected_revision,
                    ids: vec![song],
                    operation,
                },
            )
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    async fn submit_catalog_collection(
        &self,
        token: &SessionToken,
        authority: &CatalogCollectionAuthority,
        playlist_store: &PlaylistLocalStore,
        lease: MixedPlaylistWriteLease,
        plan: MixedPlaylistAddPlan<'_>,
    ) -> Result<PlaylistMixedAddReceipt> {
        // M/3085 reverses selectedData before computing its ids; M/3160
        // captures the first id before reversing. This is the last admitted
        // row in visible order, not the first row and not every queue match.
        let representative = plan
            .ids
            .last()
            .ok_or(Error::InvalidInput("empty collection"))?;
        let store = self.session.persistent_store()?;
        let persisted = || {
            self.session
                .with_token(token, |uid| store.get(uid, "playback", "queue"))
        };
        let (actor_revision, saved, queue) = if let Some(controller) = self.player.controller.get()
        {
            let (state, queue) = controller.queue_with_state(token).await?;
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
                    .any(|id| identifier(id).is_ok_and(|id| id == *representative))
        }) {
            return Err(Error::InvalidInput(
                "queued song requires its original collection context",
            ));
        }
        let current_source = || {
            authority.require(self, token)?;
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
        playlist_store
            .add_mixed(token, plan, &current_source, lease)
            .await
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
