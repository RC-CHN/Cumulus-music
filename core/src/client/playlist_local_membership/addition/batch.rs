//! Ordinary playlist BatchOperation → Favorite, with a separately owned
//! destination that already contains verified device playlist members.
use super::*;
use crate::{
    client::player::playlist_batch::selected_ids, playlist::PlaylistApi,
    protocol::api::GuardedPlaylistAdd,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaylistMixedBatchSource {
    playlist_id: String,
    revision: String,
    ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMixedBatchCollection {
    receipt: PlaylistMixedAddReceipt,
    current_selection: bool,
}

fn admit(rows: Vec<(String, CollectPrivilege)>) -> Result<Vec<String>> {
    let mut admitted: Vec<_> = rows
        .iter()
        .filter(|(_, privilege)| {
            matches!(
                privilege,
                CollectPrivilege::Missing | CollectPrivilege::Positive,
            )
        })
        .map(|(id, _)| id.clone())
        .collect();
    if admitted.is_empty() {
        // M/3085:768–783 checks the first selected row if all fail the
        // prefilter. The singleton filter retains defined, falsey zero.
        if let Some((id, CollectPrivilege::Falsey)) = rows.into_iter().next() {
            admitted.push(id);
        } else {
            return Err(Error::InvalidInput(
                "selected catalog songs cannot be collected",
            ));
        }
    }
    Ok(admitted)
}

impl MusicClient {
    pub async fn collect_playlist_batch_into_mixed(
        &self,
        epoch: &str,
        target: &str,
        source: PlaylistMixedBatchSource,
        operation: &str,
    ) -> Result<AccountData<Option<PlaylistMixedBatchCollection>>> {
        let token = self.playlist_token(epoch)?;
        let store = PlaylistLocalStore::new(self.session.clone())?;
        if !store.has_device_members(&token, target)? {
            // The existing ordinary picker owns its other destinations.
            return Ok(AccountData {
                epoch: epoch.into(),
                value: None,
            });
        }
        let lease = store.mixed_write_lease(&token)?;
        let existing = store
            .mixed_add_receipt(&token, target)?
            .filter(|receipt| receipt.phase.pending());
        let prepared = self
            .prepare_playlist_batch_catalog(&token, &store, source)
            .await;
        let (admitted, authority) = match prepared {
            Ok(source) => source,
            Err(error) => {
                self.session.check_generation(&token)?;
                // An earlier operation can still be checked after the source
                // list changes. Its result can never claim this new intent.
                if let Some(receipt) = existing {
                    return Ok(AccountData {
                        epoch: epoch.into(),
                        value: Some(PlaylistMixedBatchCollection {
                            receipt,
                            current_selection: false,
                        }),
                    });
                }
                return Err(error);
            }
        };
        let value = match store.mixed_add_target(&token, target, &admitted).await? {
            Some(PlaylistMixedAddTarget::Ready { revision, .. }) => {
                let receipt = self
                    .submit_catalog_collection(
                        &token,
                        &authority,
                        &store,
                        lease,
                        MixedPlaylistAddPlan {
                            id: target,
                            expected_revision: &revision,
                            ids: admitted,
                            operation,
                        },
                    )
                    .await?;
                PlaylistMixedBatchCollection {
                    receipt,
                    current_selection: true,
                }
            }
            Some(PlaylistMixedAddTarget::Pending {
                receipt,
                current_selection,
            }) => PlaylistMixedBatchCollection {
                receipt,
                current_selection,
            },
            None => return Err(Error::QueueChanged),
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: Some(value),
        })
    }

    async fn prepare_playlist_batch_catalog(
        &self,
        token: &SessionToken,
        store: &PlaylistLocalStore,
        source: PlaylistMixedBatchSource,
    ) -> Result<(Vec<String>, CatalogCollectionAuthority)> {
        if !(1..=GuardedPlaylistAdd::MAX_SONGS).contains(&source.ids.len())
            || source.ids.iter().collect::<HashSet<_>>().len() != source.ids.len()
        {
            return Err(Error::InvalidInput(
                "invalid mixed playlist batch selection",
            ));
        }
        if store.has_device_members(token, &source.playlist_id)? {
            return Err(Error::InvalidInput(
                "this batch requires an ordinary catalog playlist source",
            ));
        }
        self.session
            .run_for(token, |api, _| async move {
                let authority = CatalogCollectionAuthority::new(api.clone())?;
                let detail = PlaylistApi::new(api.clone())
                    .detail(&json!(source.playlist_id))
                    .await?;
                authority.require(self, token)?;
                let rows = detail["trackIds"]
                    .as_array()
                    .ok_or(Error::Response("missing complete playlist tracks"))?;
                if identifier(&detail["id"])? != source.playlist_id
                    || detail["specialType"].as_i64() != Some(0)
                    || rows.len() > 100_000
                    || (!detail["trackCount"].is_null()
                        && detail["trackCount"].as_u64() != Some(rows.len() as u64))
                    || rows.iter().any(|row| row["v"].as_i64() == Some(-1))
                {
                    return Err(Error::InvalidInput(
                        "this batch requires an ordinary catalog playlist source",
                    ));
                }
                let mut seen = HashSet::with_capacity(rows.len());
                for row in rows {
                    if !seen.insert(identifier(&row["id"])?) {
                        return Err(Error::Response("duplicate source playlist songs"));
                    }
                }
                let ids =
                    selected_ids(&source.playlist_id, &detail, &source.ids, &source.revision)?;
                // M/2997:518–523 uses M/120's c + default trialMode=-1 on
                // the ordinary route. Only rehydrate the selected catalog rows.
                let details = CatalogApi::new(api)
                    .songs_with_trial(&ids, 500, Some(-1))
                    .await?;
                authority.require(self, token)?;
                if !details.missing.is_empty() || ids.len() != details.songs.len() {
                    return Err(Error::InvalidInput(
                        "catalog song is unavailable for collection",
                    ));
                }
                let selected = ids
                    .into_iter()
                    .zip(details.songs)
                    .map(|(id, song)| {
                        let id = identifier(&id)?;
                        let track = song.ok_or(Error::InvalidInput(
                            "catalog song is unavailable for collection",
                        ))?;
                        let privilege =
                            catalog_collect_privilege(&id, &track, details.privileges.get(&id))?;
                        Ok((id, privilege))
                    })
                    .collect::<Result<_>>()?;
                Ok((admit(selected)?, authority))
            })
            .await
    }
}
